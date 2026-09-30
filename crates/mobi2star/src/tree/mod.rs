//! Where conversion output goes. The conversion stages write through the
//! `Tree` trait, so the same code fills a directory on disk (CLI) or a zip
//! archive in memory (browser).
mod archive;
mod disk;
pub(crate) use archive::{folder_name, ZipTree};
pub(crate) use disk::DiskTree;

use lexicon_core::{read_limited, Error, Limits, Result};
use serde::Serialize;
use std::io::{Read, Write};

/// A write-only file tree. Paths are relative and `/`-separated; each file
/// is created once. At most one stream is open at a time, and nothing else
/// is written while it is, which matches archives that write one entry
/// after another.
pub(crate) trait Tree {
    /// Writes a whole file.
    fn put(&mut self, path: &str, bytes: &[u8]) -> Result<()>;
    /// Opens a file to be written in pieces, for outputs too large to hold.
    fn stream(&mut self, path: &str) -> Result<&mut dyn Write>;
    /// The stream opened by `stream`, while it is open.
    fn stream_writer(&mut self) -> Result<&mut dyn Write>;
    /// Closes the open stream; returns its length.
    fn end_stream(&mut self) -> Result<u64>;
    /// Bytes written so far, counted against the output budget.
    fn used(&self) -> u64;
    /// Reads back what has been written, to check it. A tree that can only
    /// be read once complete (an archive) is finished by this call and
    /// accepts no further files.
    fn readback(&mut self) -> Result<Box<dyn ReadBack + '_>>;
    /// Writes `value` as pretty JSON with a trailing newline.
    fn put_json<T: Serialize + ?Sized>(&mut self, path: &str, value: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.put(path, &bytes)
    }
}

/// Reads finished output back for checking.
pub(crate) trait ReadBack {
    /// A reader over `path` and its length.
    fn open(&mut self, path: &str) -> Result<(Box<dyn Read + '_>, u64)>;
    /// All of `path`, refusing more than `limit` bytes.
    fn read(&mut self, path: &str, limit: usize) -> Result<Vec<u8>> {
        let (reader, length) = self.open(path)?;
        read_limited(reader, length, limit, &path)
    }
}

/// Aggregate output budget shared by every tree.
struct Budget {
    used: u64,
    limit: u64,
}
impl Budget {
    fn new(limits: &Limits) -> Self {
        Self {
            used: 0,
            limit: limits.output_bytes,
        }
    }
    fn charge(&mut self, bytes: u64) -> Result<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .ok_or_else(|| Error::Limit("bundle byte overflow".into()))?;
        if self.used > self.limit {
            return Err(Error::Limit("aggregate bundle byte budget".into()));
        }
        Ok(())
    }
}

/// Counts the bytes that pass through a writer.
struct Counted<W> {
    inner: W,
    bytes: u64,
}
impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
