//! A zip archive in memory: the browser's single download.
use super::{Budget, Counted, ReadBack, Tree};
use lexicon_core::{Error, Limits, Result};
use mobi_reader::container::image_type;
use std::{
    io::{self, Cursor, Read, Write},
    mem,
};
use zip::{write::SimpleFileOptions, CompressionMethod, DateTime, ZipArchive, ZipWriter};

type Buffer = Cursor<Vec<u8>>;

enum State {
    /// Accepting files; `streaming` while a stream is open.
    Writing {
        zip: Box<Counted<ZipWriter<Buffer>>>,
        streaming: bool,
    },
    /// Finished and reopened from its own bytes for the readback.
    Finished(ZipArchive<Buffer>),
    /// Finishing failed; nothing more can be done.
    Failed,
}

/// Files in a zip archive held in memory. Text is deflated; PNG, JPEG and
/// GIF data, already compressed, is stored. Every entry has the same fixed
/// timestamp and permissions, so the same input always gives the same bytes.
pub(crate) struct ZipTree {
    state: State,
    budget: Budget,
    /// Zip64 headers, needed only for entries of 4 GiB or more.
    large_files: bool,
}
impl ZipTree {
    pub fn new(limits: &Limits) -> Self {
        Self {
            state: State::Writing {
                zip: Box::new(Counted {
                    inner: ZipWriter::new(Cursor::new(Vec::new())),
                    bytes: 0,
                }),
                streaming: false,
            },
            budget: Budget::new(limits),
            large_files: limits.output_bytes > u64::from(u32::MAX),
        }
    }
    /// Starts an entry and returns the writer positioned in it.
    fn start(&mut self, path: &str, compress: bool) -> Result<&mut Counted<ZipWriter<Buffer>>> {
        srcs_reader::uri::validate_path(path)?;
        let (method, level) = if compress {
            (CompressionMethod::Deflated, Some(6))
        } else {
            (CompressionMethod::Stored, None)
        };
        let options = SimpleFileOptions::default()
            .compression_method(method)
            .compression_level(level)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644)
            .large_file(self.large_files);
        match &mut self.state {
            State::Writing {
                streaming: true, ..
            } => Err(Error::Incomplete("a stream is open".into())),
            State::Writing { zip, .. } => {
                zip.inner
                    .start_file(path, options)
                    .map_err(io::Error::from)?;
                Ok(zip)
            }
            State::Finished(_) | State::Failed => {
                Err(Error::Incomplete("the archive is finished".into()))
            }
        }
    }
    /// The archive's bytes, finishing it if the readback has not.
    pub fn into_bytes(mut self) -> Result<Vec<u8>> {
        self.finish()?;
        match self.state {
            State::Finished(archive) => Ok(archive.into_inner().into_inner()),
            _ => Err(Error::Incomplete("the archive is not finished".into())),
        }
    }
    /// Writes the central directory and reopens the archive from its bytes,
    /// so the readback sees exactly what gets downloaded.
    fn finish(&mut self) -> Result<()> {
        match self.state {
            State::Writing {
                streaming: true, ..
            } => return Err(Error::Incomplete("a stream is still open".into())),
            State::Writing { .. } => {}
            State::Finished(_) | State::Failed => return Ok(()),
        }
        if let State::Writing { zip, .. } = mem::replace(&mut self.state, State::Failed) {
            let bytes = zip.inner.finish().map_err(io::Error::from)?.into_inner();
            self.state =
                State::Finished(ZipArchive::new(Cursor::new(bytes)).map_err(io::Error::from)?);
        }
        Ok(())
    }
}
impl Tree for ZipTree {
    fn put(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        let compress = !matches!(image_type(bytes), Some(("png" | "jpg" | "gif", _)));
        self.budget.charge(bytes.len() as u64)?;
        self.start(path, compress)?.inner.write_all(bytes)?;
        Ok(())
    }
    fn stream(&mut self, path: &str) -> Result<&mut dyn Write> {
        self.start(path, true)?;
        match &mut self.state {
            State::Writing { zip, streaming } => {
                zip.bytes = 0;
                *streaming = true;
                Ok(zip)
            }
            _ => Err(Error::Incomplete("the archive is finished".into())),
        }
    }
    fn stream_writer(&mut self) -> Result<&mut dyn Write> {
        match &mut self.state {
            State::Writing {
                zip,
                streaming: true,
            } => Ok(zip),
            _ => Err(Error::Incomplete("no open stream".into())),
        }
    }
    fn end_stream(&mut self) -> Result<u64> {
        let bytes = match &mut self.state {
            State::Writing { zip, streaming } if *streaming => {
                *streaming = false;
                zip.bytes
            }
            _ => return Err(Error::Incomplete("no open stream".into())),
        };
        self.budget.charge(bytes)?;
        Ok(bytes)
    }
    fn used(&self) -> u64 {
        self.budget.used
    }
    fn readback(&mut self) -> Result<Box<dyn ReadBack + '_>> {
        self.finish()?;
        match &mut self.state {
            State::Finished(archive) => Ok(Box::new(ZipReadBack(archive))),
            _ => Err(Error::Incomplete("the archive is not finished".into())),
        }
    }
}

/// Entries of a finished archive, read back. Reading an entry to its end
/// also checks its CRC-32.
struct ZipReadBack<'a>(&'a mut ZipArchive<Buffer>);
impl ReadBack for ZipReadBack<'_> {
    fn open(&mut self, path: &str) -> Result<(Box<dyn Read + '_>, u64)> {
        let file = self.0.by_name(path).map_err(io::Error::from)?;
        let length = file.size();
        Ok((Box::new(file), length))
    }
}

/// A folder name made from a dictionary title that is valid on Windows,
/// macOS and Linux: no reserved characters or device names, no leading or
/// trailing dots or spaces, and at most 120 bytes.
pub(crate) fn folder_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut name = String::new();
    for word in cleaned.split_whitespace() {
        let separator = usize::from(!name.is_empty());
        if name.len() + separator + word.len() > 120 {
            // A single overlong word is cut at a character boundary.
            if name.is_empty() {
                name.extend(word.chars().scan(0, |bytes, c| {
                    *bytes += c.len_utf8();
                    (*bytes <= 120).then_some(c)
                }));
            }
            break;
        }
        if separator == 1 {
            name.push(' ');
        }
        name.push_str(word);
    }
    let name = name.trim_matches(['.', ' ']);
    // Windows device names, with or without an extension. Compared as
    // bytes, so a multi-byte character is never split.
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let reserved = match stem.as_bytes() {
        device @ [_, _, _] => [b"CON", b"PRN", b"AUX", b"NUL"]
            .iter()
            .any(|name| device.eq_ignore_ascii_case(*name)),
        [a, b, c, b'1'..=b'9'] => [b"COM", b"LPT"]
            .iter()
            .any(|name| [*a, *b, *c].eq_ignore_ascii_case(*name)),
        _ => false,
    };
    match name {
        "" => "Dictionary".into(),
        _ if reserved => format!("_{name}"),
        _ => name.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really";

    fn entries(bytes: Vec<u8>) -> Vec<(String, CompressionMethod, Vec<u8>)> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..archive.len())
            .map(|i| {
                let mut file = archive.by_index(i).unwrap();
                let mut data = Vec::new();
                file.read_to_end(&mut data).unwrap();
                (file.name().to_owned(), file.compression(), data)
            })
            .collect()
    }
    fn sample(tree: &mut ZipTree) {
        tree.put("d/a.txt", b"hello hello hello").unwrap();
        tree.put("d/res/i.png", PNG).unwrap();
        write!(tree.stream("d/s.txt").unwrap(), "12").unwrap();
        write!(tree.stream_writer().unwrap(), "345").unwrap();
        assert_eq!(tree.end_stream().unwrap(), 5);
    }
    #[test]
    fn zip_tree_writes_entries_in_order_and_stores_images() {
        let mut tree = ZipTree::new(&Limits::default());
        sample(&mut tree);
        let names: Vec<_> = entries(tree.into_bytes().unwrap())
            .into_iter()
            .map(|(name, method, data)| (name, method, data.len()))
            .collect();
        assert_eq!(
            names,
            [
                ("d/a.txt".into(), CompressionMethod::Deflated, 17),
                ("d/res/i.png".into(), CompressionMethod::Stored, PNG.len()),
                ("d/s.txt".into(), CompressionMethod::Deflated, 5),
            ]
        );
    }
    #[test]
    fn zip_tree_bytes_are_deterministic() {
        let build = || {
            let mut tree = ZipTree::new(&Limits::default());
            sample(&mut tree);
            tree.into_bytes().unwrap()
        };
        assert_eq!(build(), build());
    }
    #[test]
    fn zip_tree_follows_the_tree_rules() {
        let limits = Limits {
            output_bytes: 10,
            ..Limits::default()
        };
        let mut tree = ZipTree::new(&limits);
        tree.put("a.txt", b"abc").unwrap();
        assert!(tree.put("a.txt", b"").is_err(), "each file once");
        assert!(tree.put("../x", b"").is_err(), "validated paths");
        write!(tree.stream("s.txt").unwrap(), "12345").unwrap();
        assert!(tree.stream("t.txt").is_err(), "one stream at a time");
        assert!(
            tree.put("u.txt", b"").is_err(),
            "nothing else while streaming"
        );
        assert!(tree.readback().is_err(), "no readback while streaming");
        tree.end_stream().unwrap();
        tree.put("c.txt", b"xy").unwrap();
        assert_eq!(tree.used(), 10);
        assert!(tree.put("d.txt", b"z").is_err(), "aggregate budget");
    }
    #[test]
    fn zip_readback_reads_the_finished_archive() {
        let mut tree = ZipTree::new(&Limits::default());
        sample(&mut tree);
        {
            let mut output = tree.readback().unwrap();
            assert_eq!(output.read("d/s.txt", 5).unwrap(), b"12345");
            assert!(output.read("d/s.txt", 4).is_err(), "readback limit");
            assert!(output.read("missing", 5).is_err());
        }
        assert!(
            tree.put("late.txt", b"").is_err(),
            "finished by the readback"
        );
        assert_eq!(entries(tree.into_bytes().unwrap()).len(), 3);
    }
    #[test]
    fn folder_names_are_portable() {
        for (title, expected) in [
            ("Collins COBUILD", "Collins COBUILD"),
            ("  A/B: C*?  ", "A B C"),
            ("...hidden. ", "hidden"),
            ("", "Dictionary"),
            ("\u{7}", "Dictionary"),
            ("con", "_con"),
            ("LPT1.dict", "_LPT1.dict"),
            ("COM0", "COM0"),
            ("Console", "Console"),
            ("牛津高阶英汉双解词典", "牛津高阶英汉双解词典"),
            // Four bytes whose fourth is inside a character.
            ("a中", "a中"),
            ("abé", "abé"),
            ("😀", "😀"),
            ("lpt9.x", "_lpt9.x"),
        ] {
            assert_eq!(folder_name(title), expected, "{title:?}");
        }
        let long = folder_name(&"词".repeat(100));
        assert!(long.len() <= 120 && long.chars().all(|c| c == '词'));
        let words = folder_name(&"word ".repeat(100));
        assert!(words.len() <= 120 && words.ends_with("word"));
        for title in ["a", "LPT9", " x. ", &"é".repeat(300)] {
            srcs_reader::uri::validate_path(&folder_name(title)).unwrap();
        }
    }
    #[test]
    fn every_short_title_gives_a_valid_folder_name() {
        // Characters of every UTF-8 width, plus the ones the rules treat specially.
        let alphabet = ['a', 'é', '中', '😀', '.', ' ', '1', ':'];
        let mut layer = vec![String::new()];
        for _ in 0..4 {
            layer = layer
                .iter()
                .flat_map(|title| alphabet.iter().map(move |c| format!("{title}{c}")))
                .collect();
            for title in &layer {
                srcs_reader::uri::validate_path(&folder_name(title)).unwrap();
            }
        }
    }
}
