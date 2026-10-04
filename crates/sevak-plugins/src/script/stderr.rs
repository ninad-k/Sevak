//! A script's standard error, read with hard limits.
//!
//! Persistent scripts run for a long time and their stderr goes to Sevak's log.
//! Reading it line by line without a limit let a script that never prints a
//! newline (or prints endless output) grow Sevak's memory without bound, and a
//! line that was not valid UTF-8 stopped the reading, which could leave the
//! pipe full and the script blocked. This reader keeps a bounded amount, drops
//! the rest, and always drains the pipe.

use std::io::Read;

/// Longest line kept, in bytes; the rest of a longer line is dropped.
pub const MAX_LINE_BYTES: usize = 1024;
/// Lines kept per process.
pub const MAX_LINES: usize = 200;
/// Bytes kept per process, over all lines.
pub const MAX_TOTAL_BYTES: usize = 32 * 1024;

/// What `forward` did, for tests and the one-line "dropped" notice.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub lines: usize,
    pub kept_bytes: usize,
    pub dropped_bytes: u64,
}

/// Reads `stderr` to its end, calling `emit` with each kept line (lossy UTF-8,
/// without the line break, marked when cut) up to the limits. Memory use does
/// not depend on how much the script prints.
pub fn forward(stderr: impl Read, mut emit: impl FnMut(&str)) -> Report {
    forward_with(
        stderr,
        MAX_LINE_BYTES,
        MAX_LINES,
        MAX_TOTAL_BYTES,
        &mut emit,
    )
}

fn forward_with(
    mut stderr: impl Read,
    max_line: usize,
    max_lines: usize,
    max_total: usize,
    emit: &mut dyn FnMut(&str),
) -> Report {
    let mut report = Report::default();
    let mut notified = false;
    let mut line: Vec<u8> = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 4096];

    let mut finish = |line: &mut Vec<u8>, cut: &mut bool, report: &mut Report| {
        let keep = report.lines < max_lines && report.kept_bytes < max_total;
        if keep {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let mut text = String::from_utf8_lossy(line).into_owned();
            if *cut {
                text.push_str(" [line cut]");
            }
            report.lines += 1;
            report.kept_bytes += line.len();
            emit(&text);
        } else {
            report.dropped_bytes += line.len() as u64;
            if !notified {
                notified = true;
                emit("further stderr output is not logged");
            }
        }
        line.clear();
        *cut = false;
    };

    loop {
        let read = match stderr.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                finish(&mut line, &mut cut, &mut report);
            } else if line.len() < max_line {
                line.push(byte);
            } else {
                cut = true;
                report.dropped_bytes += 1;
            }
        }
    }
    if !line.is_empty() {
        finish(&mut line, &mut cut, &mut report);
    }
    report
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};

    use super::*;

    fn collect(input: &[u8]) -> (Vec<String>, Report) {
        let mut lines = Vec::new();
        let report = forward(Cursor::new(input.to_vec()), |line| {
            lines.push(line.to_owned())
        });
        (lines, report)
    }

    #[test]
    fn short_lines_come_through_unchanged() {
        let (lines, report) = collect(b"one\ntwo\r\nthree");
        assert_eq!(lines, ["one", "two", "three"]);
        assert_eq!(report.lines, 3);
        assert_eq!(report.dropped_bytes, 0);
    }

    #[test]
    fn a_huge_line_without_a_newline_is_cut_and_the_rest_dropped() {
        let big = 50 * 1024 * 1024u64;
        let mut lines = Vec::new();
        let report = forward(io::repeat(b'a').take(big), |line| {
            lines.push(line.to_owned());
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with(&"a".repeat(MAX_LINE_BYTES)));
        assert!(lines[0].ends_with("[line cut]"));
        assert!(lines[0].len() < MAX_LINE_BYTES + 32);
        assert_eq!(report.kept_bytes, MAX_LINE_BYTES);
        assert_eq!(report.dropped_bytes, big - MAX_LINE_BYTES as u64);
    }

    #[test]
    fn the_number_of_lines_and_total_bytes_are_capped_with_one_notice() {
        let mut input = Vec::new();
        for n in 0..(MAX_LINES * 3) {
            input.extend_from_slice(format!("line {n}\n").as_bytes());
        }
        let (lines, report) = collect(&input);
        assert_eq!(report.lines, MAX_LINES);
        assert_eq!(lines.len(), MAX_LINES + 1);
        assert_eq!(lines.last().unwrap(), "further stderr output is not logged");
        assert!(report.dropped_bytes > 0);

        // Long lines hit the byte budget before the line budget.
        let line = format!("{}\n", "x".repeat(MAX_LINE_BYTES - 1));
        let input = line.repeat(100);
        let (lines, report) = collect(input.as_bytes());
        assert!(
            report.kept_bytes < MAX_TOTAL_BYTES + MAX_LINE_BYTES,
            "{report:?}"
        );
        assert!(lines.len() < 40, "{}", lines.len());
        assert!(lines.last().unwrap().contains("not logged"));
    }

    #[test]
    fn invalid_utf8_does_not_stop_the_reading() {
        let mut input = b"before\n".to_vec();
        input.extend_from_slice(&[0xff, 0xfe, b'\n']);
        input.extend_from_slice(b"after\n");
        let (lines, _) = collect(&input);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "before");
        assert_eq!(lines[2], "after");
    }

    #[test]
    fn a_read_error_ends_the_stream_quietly() {
        struct Broken(bool);
        impl Read for Broken {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    Err(io::Error::other("closed"))
                } else {
                    self.0 = true;
                    buf[..3].copy_from_slice(b"hi\n");
                    Ok(3)
                }
            }
        }
        let mut lines = Vec::new();
        forward(Broken(false), |line| lines.push(line.to_owned()));
        assert_eq!(lines, ["hi"]);
    }
}
