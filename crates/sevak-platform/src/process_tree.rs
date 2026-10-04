//! Ending a process together with everything it started.
//!
//! `Child::kill` only ends the process Sevak started. A script that ran
//! `sh -c "..."` or `py -3` left its own children behind on a timeout, still
//! holding the output pipes, so a hung plugin kept its helpers alive and the
//! reader threads never saw the end of the stream.
//!
//! * Windows: the child is put into a Job Object created with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. [`ProcessTree::kill`] terminates the
//!   whole job, and because the operating system closes the job's handle when
//!   Sevak exits (or crashes), the processes die with Sevak. The child runs for
//!   an instant before it is assigned, so a process it starts in that moment
//!   escapes the job; scripts do not do that in practice.
//! * Unix: the child is started as the leader of a new process group
//!   ([`ProcessTree::prepare`]) and [`ProcessTree::kill`] signals the group.
//!   Children that start their own session or group (daemons) escape. The group
//!   id is the child's pid, so **call `kill` only while the child has not been
//!   reaped**; once it is reaped the pid may belong to someone else.
//!
//! When the child ends on its own, call [`ProcessTree::release`]: whatever it
//! left running stays running, as it did before (a script that starts an
//! editor must not have the editor killed when it returns). Dropping the tree
//! without releasing it ends the processes on Windows (it closes the job).

use std::process::{Child, Command};

/// A started process and its descendants. See the module notes.
pub struct ProcessTree {
    inner: imp::Tree,
}

impl ProcessTree {
    /// Call on the command **before** spawning it.
    pub fn prepare(command: &mut Command) {
        imp::prepare(command);
    }

    /// Starts tracking `child`, which was spawned from a command that went
    /// through [`ProcessTree::prepare`]. If the operating system refuses (a
    /// restrictive job already contains Sevak), the tree tracks only the
    /// child and [`ProcessTree::kill`] ends nothing more than `Child::kill`
    /// would; the caller kills the child itself as well.
    pub fn adopt(child: &Child) -> Self {
        Self {
            inner: imp::adopt(child),
        }
    }

    /// Ends the child and every process it started. Safe to call twice.
    pub fn kill(&self) {
        self.inner.kill();
    }

    /// The child ended by itself: stop tracking, and leave what it started
    /// running.
    pub fn release(self) {
        self.inner.release();
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub struct Tree {
        pgid: Option<i32>,
    }

    pub fn prepare(command: &mut Command) {
        // Its own process group, with the child as leader.
        command.process_group(0);
    }

    pub fn adopt(child: &Child) -> Tree {
        Tree {
            pgid: i32::try_from(child.id()).ok().filter(|pid| *pid > 1),
        }
    }

    impl Tree {
        pub fn kill(&self) {
            if let Some(pgid) = self.pgid {
                // SAFETY: plain system call; a negative pid addresses the group.
                // The caller guarantees the leader has not been reaped, so the
                // group id still belongs to this child's group.
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }
        }

        pub fn release(self) {}
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct Job(HANDLE);

    // SAFETY: a job handle may be used from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Drop for Job {
        fn drop(&mut self) {
            // With KILL_ON_JOB_CLOSE still set this ends the processes in it.
            // SAFETY: the handle is ours and closed once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    pub struct Tree {
        job: Option<Job>,
    }

    pub fn prepare(_command: &mut Command) {}

    fn set_limits(job: HANDLE, flags: JOB_OBJECT_LIMIT) -> windows::core::Result<()> {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = flags;
        // SAFETY: `info` is a valid, initialised structure of the size given.
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
    }

    pub fn adopt(child: &Child) -> Tree {
        // SAFETY: no security attributes and no name.
        let Ok(handle) = (unsafe { CreateJobObjectW(None, PCWSTR::null()) }) else {
            return Tree { job: None };
        };
        let job = Job(handle);
        if let Err(err) = set_limits(job.0, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE) {
            tracing::debug!(%err, "could not configure a job object");
            return Tree { job: None };
        }
        // SAFETY: both handles are valid; the child's stays open while `child` lives.
        if let Err(err) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) }
        {
            tracing::debug!(%err, "could not put a script process into a job object");
            return Tree { job: None };
        }
        Tree { job: Some(job) }
    }

    impl Tree {
        pub fn kill(&self) {
            if let Some(job) = &self.job {
                // SAFETY: the handle is valid for the life of `job`.
                let _ = unsafe { TerminateJobObject(job.0, 1) };
            }
        }

        pub fn release(self) {
            if let Some(job) = &self.job {
                // Nothing to kill when the handle closes (below, on drop).
                let _ = set_limits(job.0, JOB_OBJECT_LIMIT(0));
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::process::{Child, Command};

    pub struct Tree;

    pub fn prepare(_command: &mut Command) {}

    pub fn adopt(_child: &Child) -> Tree {
        Tree
    }

    impl Tree {
        pub fn kill(&self) {}
        pub fn release(self) {}
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        /// The parent has started its grandchild.
        Started,
        /// Every holder of the output pipe is gone.
        Closed,
    }

    /// A program that starts a grandchild which lives for several seconds and
    /// shares the parent's standard output, then prints `started`.
    fn parent_with_grandchild() -> Command {
        if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args([
                "/c",
                "start /b ping -n 9 127.0.0.1 & echo started & ping -n 9 127.0.0.1",
            ]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 8 & echo started; wait"]);
            command
        }
    }

    struct Watched {
        child: Child,
        tree: Option<ProcessTree>,
        events: mpsc::Receiver<Event>,
    }

    /// Spawns the parent, waits until it says its grandchild is running, and
    /// reports (through `events`) when the output pipe finally closes.
    fn spawn_watched(track: bool) -> Watched {
        let mut command = parent_with_grandchild();
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        ProcessTree::prepare(&mut command);
        let mut child = command.spawn().expect("spawn the test program");
        let tree = track.then(|| ProcessTree::adopt(&child));
        let mut stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        thread::spawn(move || {
            let mut seen = Vec::new();
            let mut announced = false;
            let mut chunk = [0u8; 256];
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&chunk[..n]);
                if !announced && String::from_utf8_lossy(&seen).contains("started") {
                    announced = true;
                    let _ = tx.send(Event::Started);
                }
            }
            let _ = tx.send(Event::Closed);
        });
        let mut watched = Watched {
            child,
            tree,
            events,
        };
        assert_eq!(
            watched.next(Duration::from_secs(20)),
            Some(Event::Started),
            "the test program never started its grandchild"
        );
        watched
    }

    impl Watched {
        /// The next event within `limit`, polling in short steps.
        fn next(&mut self, limit: Duration) -> Option<Event> {
            let deadline = Instant::now() + limit;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return None;
                }
                if let Ok(event) = self
                    .events
                    .recv_timeout(left.min(Duration::from_millis(50)))
                {
                    return Some(event);
                }
            }
        }

        fn kill_child_only(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn killing_the_tree_ends_the_grandchild_too() {
        let mut watched = spawn_watched(true);
        // The child is not reaped yet, as `kill`'s contract requires.
        watched.tree.as_ref().unwrap().kill();
        watched.kill_child_only();
        assert_eq!(
            watched.next(Duration::from_secs(5)),
            Some(Event::Closed),
            "the grandchild still holds the pipe"
        );
    }

    #[test]
    fn killing_only_the_child_leaves_the_grandchild_which_is_why_the_tree_exists() {
        let mut watched = spawn_watched(false);
        watched.kill_child_only();
        assert_eq!(
            watched.next(Duration::from_millis(600)),
            None,
            "expected the grandchild to outlive its parent"
        );
    }

    #[test]
    fn a_released_tree_leaves_what_the_child_started() {
        let mut watched = spawn_watched(true);
        watched.tree.take().unwrap().release();
        watched.kill_child_only();
        assert_eq!(
            watched.next(Duration::from_millis(600)),
            None,
            "release must not end the grandchild"
        );
    }

    #[cfg(windows)]
    #[test]
    fn dropping_the_tree_ends_the_processes_on_windows() {
        let mut watched = spawn_watched(true);
        drop(watched.tree.take());
        let _ = watched.child.wait();
        assert_eq!(watched.next(Duration::from_secs(5)), Some(Event::Closed));
    }
}
