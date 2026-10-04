//! A tiny HTTP server on the loopback interface, for the assistant's tests.
//!
//! It stands in for OpenAI, Anthropic and Ollama: no test ever contacts a real
//! provider. It answers every request with the same canned [`Reply`] and
//! records what it was sent, so a test can check the URL, the headers and the
//! JSON a provider builds, and how the client copes with slow, huge or broken
//! replies.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the server answers.
#[derive(Clone)]
pub struct Reply {
    status: u16,
    body: Vec<u8>,
    content_type: &'static str,
    delay: Duration,
    /// No `Content-Length`: the body ends when the connection closes.
    unsized_body: bool,
    /// Promise the whole body, send half of it, then go quiet.
    stall: bool,
    location: Option<String>,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            content_type: "application/json",
            delay: Duration::ZERO,
            unsized_body: false,
            stall: false,
            location: None,
        }
    }

    pub fn redirect(to: &str) -> Self {
        Self {
            location: Some(to.to_owned()),
            ..Self::json(302, "")
        }
    }

    /// Waits this long before answering.
    #[must_use]
    pub fn after(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Sends no length, so a client must stop reading by itself.
    #[must_use]
    pub fn chunked(mut self) -> Self {
        self.unsized_body = true;
        self
    }

    /// Sends the headers and half the body, then nothing.
    #[must_use]
    pub fn stall_body(mut self) -> Self {
        self.stall = true;
        self
    }
}

/// One request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The request line and the headers, as sent.
    pub head: String,
    pub body: Vec<u8>,
}

impl Recorded {
    /// The value of header `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }

    /// The path of the request line.
    pub fn path(&self) -> &str {
        self.head.split_whitespace().nth(1).unwrap_or_default()
    }

    /// The body as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub struct TestServer {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
}

impl TestServer {
    pub fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind a loopback port");
        let addr = listener.local_addr().expect("local address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen_in, stop_in) = (Arc::clone(&seen), Arc::clone(&stop));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_in.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (reply, seen) = (reply.clone(), Arc::clone(&seen_in));
                std::thread::spawn(move || serve(stream, &reply, &seen));
            }
        });
        Self { addr, seen, stop }
    }

    /// `http://127.0.0.1:<port>` followed by `path`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The base address, without a path.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.seen.lock().expect("requests").clone()
    }

    /// A loopback port nothing listens on.
    pub fn unused_port() -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        listener.local_addr().expect("addr").port()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop so the thread ends.
        let _ = TcpStream::connect(self.addr);
    }
}

fn serve(mut stream: TcpStream, reply: &Reply, seen: &Mutex<Vec<Recorded>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    seen.lock().expect("requests").push(request);
    std::thread::sleep(reply.delay);

    let reason = match reply.status {
        200 => "OK",
        302 => "Found",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nConnection: close\r\n",
        reply.status, reply.content_type
    );
    if let Some(location) = &reply.location {
        head.push_str(&format!("Location: {location}\r\n"));
    }
    if !reply.unsized_body {
        head.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    if reply.stall {
        let _ = stream.write_all(&reply.body[..reply.body.len() / 2]);
        let _ = stream.flush();
        std::thread::sleep(Duration::from_secs(5));
        return;
    }
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let headers: HashMap<String, String> = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(Recorded { head, body })
}
