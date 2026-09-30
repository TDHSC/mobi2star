//! Output files in a directory, typically a private transaction stage.
use super::{Budget, Counted, ReadBack, Tree};
use lexicon_core::{checked_member, Error, Limits, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

/// Files under a directory. Every file is created exclusively and synced;
/// parents are created as needed.
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
        if self.stream.is_some() {
            return Err(Error::Incomplete("a stream is open".into()));
        }
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
        self.budget.used
    }
    fn readback(&mut self) -> Result<Box<dyn ReadBack + '_>> {
        if self.stream.is_some() {
            return Err(Error::Incomplete("a stream is still open".into()));
        }
        Ok(Box::new(DirReadBack(self.root)))
    }
}

/// Output files in a directory, read back.
struct DirReadBack<'a>(&'a Path);
impl ReadBack for DirReadBack<'_> {
    fn open(&mut self, path: &str) -> Result<(Box<dyn Read + '_>, u64)> {
        let file = File::open(checked_member(self.0, path)?)?;
        let length = file.metadata()?.len();
        Ok((Box::new(BufReader::new(file)), length))
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
        assert!(
            tree.put("u.txt", b"").is_err(),
            "nothing else while streaming"
        );
        assert_eq!(tree.end_stream().unwrap(), 5);
        tree.put("c.txt", b"xy").unwrap();
        assert_eq!(tree.used(), 10);
        assert!(tree.put("d.txt", b"z").is_err(), "aggregate budget");
        assert_eq!(fs::read(dir.path().join("s.txt")).unwrap(), b"12345");
        assert_eq!(fs::read(dir.path().join("a/b.txt")).unwrap(), b"abc");
        let mut output = tree.readback().unwrap();
        assert_eq!(output.read("a/b.txt", 3).unwrap(), b"abc");
        assert!(output.read("s.txt", 4).is_err(), "readback limit");
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
