//! Where conversion output goes. The conversion stages write through the
//! `Tree` trait, so the same code fills a directory on disk (CLI) or a zip
//! archive in memory (browser).
use lexicon_core::{Error, Limits, Result};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

/// A write-only file tree. Paths are relative and `/`-separated; each file
/// is created once. At most one stream is open at a time, which matches
/// archives that write one entry after another.
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
    /// Writes `value` as pretty JSON with a trailing newline.
    fn put_json<T: Serialize + ?Sized>(&mut self, path: &str, value: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.put(path, &bytes)
    }
}

/// Aggregate output budget shared by every tree.
pub(crate) struct Budget {
    used: u64,
    limit: u64,
}
impl Budget {
    pub fn new(limits: &Limits) -> Self {
        Self {
            used: 0,
            limit: limits.output_bytes,
        }
    }
    pub fn charge(&mut self, bytes: u64) -> Result<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .ok_or_else(|| Error::Limit("bundle byte overflow".into()))?;
        if self.used > self.limit {
            return Err(Error::Limit("aggregate bundle byte budget".into()));
        }
        Ok(())
    }
    pub fn used(&self) -> u64 {
        self.used
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

/// Files under a directory, typically a private transaction stage. Every
/// file is created exclusively and synced; parents are created as needed.
pub(crate) struct DiskTree<'a> {
    root: &'a Path,
    budget: Budget,
    stream: Option<Counted<BufWriter<File>>>,
}
impl<'a> DiskTree<'a> {
    pub fn new(root: &'a Path, limits: &Limits) -> Self {
        Self {
            root,
            budget: Budget::new(limits),
            stream: None,
        }
    }
    fn create(&self, path: &str) -> Result<File> {
        srcs_reader::uri::validate_path(path)?;
        let target = self.root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?)
    }
}
impl Tree for DiskTree<'_> {
    fn put(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        self.budget.charge(bytes.len() as u64)?;
        let mut file = self.create(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
    fn stream(&mut self, path: &str) -> Result<&mut dyn Write> {
        if self.stream.is_some() {
            return Err(Error::Incomplete("a stream is already open".into()));
        }
        let file = self.create(path)?;
        Ok(self.stream.insert(Counted {
            inner: BufWriter::new(file),
            bytes: 0,
        }))
    }
    fn stream_writer(&mut self) -> Result<&mut dyn Write> {
        match self.stream.as_mut() {
            Some(stream) => Ok(stream),
            None => Err(Error::Incomplete("no open stream".into())),
        }
    }
    fn end_stream(&mut self) -> Result<u64> {
        let mut stream = self
            .stream
            .take()
            .ok_or_else(|| Error::Incomplete("no open stream".into()))?;
        stream.flush()?;
        stream.inner.get_ref().sync_all()?;
        self.budget.charge(stream.bytes)?;
        Ok(stream.bytes)
    }
    fn used(&self) -> u64 {
        self.budget.used()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_tree_counts_every_byte_against_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits {
            output_bytes: 10,
            ..Limits::default()
        };
        let mut tree = DiskTree::new(dir.path(), &limits);
        tree.put("a/b.txt", b"abc").unwrap();
        write!(tree.stream("s.txt").unwrap(), "12345").unwrap();
        assert!(tree.stream("t.txt").is_err(), "one stream at a time");
        assert_eq!(tree.end_stream().unwrap(), 5);
        tree.put("c.txt", b"xy").unwrap();
        assert_eq!(tree.used(), 10);
        assert!(tree.put("d.txt", b"z").is_err(), "aggregate budget");
        assert_eq!(fs::read(dir.path().join("s.txt")).unwrap(), b"12345");
        assert_eq!(fs::read(dir.path().join("a/b.txt")).unwrap(), b"abc");
    }
    #[test]
    fn disk_tree_creates_each_file_once_at_a_validated_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut tree = DiskTree::new(dir.path(), &Limits::default());
        tree.put("x.txt", b"1").unwrap();
        assert!(tree.put("x.txt", b"2").is_err());
        assert!(tree.put("../escape", b"").is_err());
        assert_eq!(fs::read(dir.path().join("x.txt")).unwrap(), b"1");
    }
}
