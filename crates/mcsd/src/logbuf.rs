//! Console capture: an in-memory ring buffer (instant backfill for the
//! dashboard) plus size-rotated log files on disk.

use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    /// Server stdout.
    Out,
    /// Server stderr.
    Err,
    /// Messages from mcsd itself (starting, stopping, downloads...).
    Sys,
    /// Commands typed by a user.
    In,
}

impl Stream {
    fn tag(self) -> &'static str {
        match self {
            Stream::Out => "out",
            Stream::Err => "err",
            Stream::Sys => "mcsd",
            Stream::In => "in",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct LogLine {
    pub seq: u64,
    /// Unix epoch milliseconds.
    pub ts: i64,
    pub stream: Stream,
    pub text: String,
}

pub struct LogBuffer {
    lines: VecDeque<LogLine>,
    cap: usize,
    next_seq: u64,
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(cap),
            cap,
            next_seq: 1,
        }
    }

    pub fn push(&mut self, stream: Stream, text: String) -> LogLine {
        let line = LogLine {
            seq: self.next_seq,
            ts: chrono::Utc::now().timestamp_millis(),
            stream,
            text,
        };
        self.next_seq += 1;
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line.clone());
        line
    }

    pub fn tail(&self, n: usize) -> Vec<LogLine> {
        let skip = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(skip).cloned().collect()
    }

    pub fn after(&self, seq: u64, limit: usize) -> Vec<LogLine> {
        self.lines
            .iter()
            .filter(|l| l.seq > seq)
            .take(limit)
            .cloned()
            .collect()
    }
}

/// Append-only log file rotated by size: console.log, console.log.1, ...
pub struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    keep: usize,
}

impl RotatingFile {
    pub fn new(path: PathBuf, max_bytes: u64, keep: usize) -> Self {
        Self {
            path,
            file: None,
            size: 0,
            max_bytes,
            keep,
        }
    }

    pub fn write(&mut self, line: &LogLine) {
        // Logging must never take the daemon down: swallow IO errors.
        let _ = self.try_write(line);
    }

    fn try_write(&mut self, line: &LogLine) -> std::io::Result<()> {
        if self.file.is_none() {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let f = OpenOptions::new().create(true).append(true).open(&self.path)?;
            self.size = f.metadata()?.len();
            self.file = Some(f);
        }
        let ts = chrono::DateTime::from_timestamp_millis(line.ts)
            .unwrap_or_default()
            .with_timezone(&chrono::Local);
        let entry = format!("{} [{}] {}\n", ts.format("%Y-%m-%d %H:%M:%S"), line.stream.tag(), line.text);
        if let Some(f) = self.file.as_mut() {
            f.write_all(entry.as_bytes())?;
        }
        self.size += entry.len() as u64;
        if self.size >= self.max_bytes {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file = None; // close before renaming (required on Windows)
        let numbered = |i: usize| PathBuf::from(format!("{}.{i}", self.path.display()));
        let _ = std::fs::remove_file(numbered(self.keep));
        for i in (1..self.keep).rev() {
            let from = numbered(i);
            if from.exists() {
                std::fs::rename(&from, numbered(i + 1))?;
            }
        }
        std::fs::rename(&self.path, numbered(1))?;
        self.size = 0;
        Ok(())
    }
}

/// Strip ANSI escape sequences and trailing CR from a raw console line.
pub fn clean(raw: &str) -> String {
    static ANSI: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07").unwrap());
    ANSI.replace_all(raw.trim_end_matches(['\r', '\n']), "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_caps_and_tails() {
        let mut b = LogBuffer::new(3);
        for i in 0..5 {
            b.push(Stream::Out, format!("l{i}"));
        }
        let t: Vec<_> = b.tail(10).into_iter().map(|l| l.text).collect();
        assert_eq!(t, ["l2", "l3", "l4"]);
        assert_eq!(b.after(4, 10).len(), 1);
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(clean("\x1b[32mhello\x1b[0m\r\n"), "hello");
    }

    #[test]
    fn rotates() {
        let dir = std::env::temp_dir().join(format!("mcsd-rot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut f = RotatingFile::new(dir.join("c.log"), 100, 2);
        let mut b = LogBuffer::new(10);
        for i in 0..20 {
            f.write(&b.push(Stream::Out, format!("line number {i}")));
        }
        assert!(dir.join("c.log.1").exists());
        assert!(dir.join("c.log.2").exists());
        assert!(!dir.join("c.log.3").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
