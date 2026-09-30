//! Shared streaming output primitives for all input adapters.
use crate::{compare_synonyms, compare_words, validate_key, IndexEntry, Synonym};
use lexicon_core::{Error, Limits, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Payload {
    pub offset: u64,
    pub size: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: u64,
    pub word: String,
    pub payload: Payload,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogAlias {
    pub word: String,
    pub target_id: u64,
}
#[derive(Debug)]
pub struct Catalog {
    pub index: Vec<IndexEntry>,
    pub synonyms: Vec<Synonym>,
    pub ordinals: BTreeMap<u64, u32>,
}
/// Streams HTML payloads into a `.dict` byte stream: a file on disk by
/// default, or any other writer (for example a zip entry).
pub struct PayloadWriter<W: Write = BufWriter<File>> {
    writer: W,
    bytes: u64,
    limit: u64,
    entry_limit: usize,
    offset_bits: u8,
}
fn create(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().create_new(true).write(true).open(path)?,
    ))
}
fn finish(mut writer: BufWriter<File>) -> Result<()> {
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}
fn write_file(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut file = create(&root.join(name))?;
    file.write_all(bytes)?;
    finish(file)
}
fn check_offset_bits(bits: u8) -> Result<()> {
    if matches!(bits, 32 | 64) {
        Ok(())
    } else {
        Err(Error::Unsupported(
            "StarDict offsets must be 32 or 64 bit".into(),
        ))
    }
}
impl PayloadWriter {
    /// Creates `dictionary.dict` in `root`, which must not exist yet.
    pub fn new(root: &Path, limits: &Limits, bits: u8) -> Result<Self> {
        check_offset_bits(bits)?;
        Self::with_writer(create(&root.join("dictionary.dict"))?, limits, bits)
    }
    /// Flushes and syncs the file; returns the `.dict` length.
    pub fn finish(self) -> Result<u64> {
        let bytes = self.bytes;
        finish(self.writer)?;
        Ok(bytes)
    }
}
impl<W: Write> PayloadWriter<W> {
    pub fn with_writer(writer: W, limits: &Limits, bits: u8) -> Result<Self> {
        check_offset_bits(bits)?;
        Ok(Self {
            writer,
            bytes: 0,
            limit: limits.output_bytes,
            entry_limit: limits.entry_bytes,
            offset_bits: bits,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<Payload> {
        if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
            return Err(Error::Incomplete(
                "StarDict payload must be nonempty UTF-8 HTML without NUL".into(),
            ));
        }
        if bytes.len() > self.entry_limit {
            return Err(Error::Limit("rendered article byte budget".into()));
        }
        let size = u32::try_from(bytes.len())
            .map_err(|_| Error::Limit("StarDict article length".into()))?;
        let end = self
            .bytes
            .checked_add(size as u64)
            .ok_or_else(|| Error::Limit("StarDict offset overflow".into()))?;
        if end > self.limit || self.offset_bits == 32 && end > u32::MAX as u64 {
            return Err(Error::Limit("StarDict output budget/offset width".into()));
        }
        let payload = Payload {
            offset: self.bytes,
            size,
        };
        self.writer.write_all(bytes)?;
        self.bytes = end;
        Ok(payload)
    }
    /// Flushes the writer and returns it with the `.dict` length.
    pub fn into_inner(mut self) -> Result<(W, u64)> {
        self.writer.flush()?;
        Ok((self.writer, self.bytes))
    }
}
/// The three index files of a dictionary, encoded in memory.
pub struct EncodedCatalog {
    pub idx: Vec<u8>,
    pub syn: Vec<u8>,
    pub ifo: Vec<u8>,
    pub catalog: Catalog,
}
/// Encodes `.idx`, `.syn` and `.ifo` for payloads already written.
/// Exact shared payload ranges are valid; partial overlaps and gaps are errors.
pub fn encode_catalog(
    title: &str,
    items: &[CatalogItem],
    aliases: &[CatalogAlias],
    bytes: u64,
    bits: u8,
    limits: &Limits,
) -> Result<EncodedCatalog> {
    if !matches!(bits, 32 | 64) {
        return Err(Error::Unsupported("StarDict offset width".into()));
    }
    if items.len() > limits.entries
        || u32::try_from(items.len()).is_err()
        || aliases.len() > limits.aliases
    {
        return Err(Error::Limit("StarDict catalog count".into()));
    }
    if title.contains(['\r', '\n', '\0']) {
        return Err(Error::Incomplete(
            "multiline/NUL StarDict book title".into(),
        ));
    }
    let mut ordered: Vec<&CatalogItem> = items.iter().collect();
    ordered.sort_by(|a, b| compare_words(&a.word, &b.word).then(a.id.cmp(&b.id)));
    let ordinals: BTreeMap<u64, u32> = ordered
        .iter()
        .enumerate()
        .map(|(i, x)| (x.id, i as u32))
        .collect();
    if ordinals.len() != items.len() {
        return Err(Error::Incomplete("catalog IDs must be unique".into()));
    }
    let mut ranges: Vec<Payload> = items.iter().map(|x| x.payload).collect();
    ranges.sort();
    ranges.dedup();
    let mut end = 0u64;
    for p in ranges {
        if p.offset != end {
            return Err(Error::Incomplete(
                "uncovered/partially overlapping StarDict payload ranges".into(),
            ));
        }
        end = end
            .checked_add(p.size as u64)
            .ok_or_else(|| Error::Limit("payload range overflow".into()))?;
    }
    if end != bytes {
        return Err(Error::Incomplete("unindexed payload bytes".into()));
    }
    let mut idx = Vec::new();
    let mut index = Vec::new();
    for item in ordered {
        validate_key(&item.word)?;
        idx.extend_from_slice(item.word.as_bytes());
        idx.push(0);
        if bits == 64 {
            idx.extend_from_slice(&item.payload.offset.to_be_bytes());
        } else {
            let offset = u32::try_from(item.payload.offset)
                .map_err(|_| Error::Limit("32-bit index offset".into()))?;
            idx.extend_from_slice(&offset.to_be_bytes());
        }
        idx.extend_from_slice(&item.payload.size.to_be_bytes());
        index.push(IndexEntry {
            word: item.word.clone(),
            offset: item.payload.offset,
            size: item.payload.size,
        });
    }
    let mut synonyms = Vec::new();
    for alias in aliases {
        validate_key(&alias.word)?;
        let target = *ordinals
            .get(&alias.target_id)
            .ok_or_else(|| Error::Incomplete("synonym target ID missing".into()))?;
        synonyms.push(Synonym {
            word: alias.word.clone(),
            target,
        });
    }
    synonyms.sort_by(compare_synonyms);
    let mut syn = Vec::new();
    for s in &synonyms {
        syn.extend_from_slice(s.word.as_bytes());
        syn.push(0);
        syn.extend_from_slice(&s.target.to_be_bytes());
    }
    let ifo = format!(
        "StarDict's dict ifo file\nversion=3.0.0\nbookname={title}\nwordcount={}\nsynwordcount={}\nidxfilesize={}\nidxoffsetbits={bits}\nsametypesequence=h\n",
        items.len(),
        synonyms.len(),
        idx.len()
    )
    .into_bytes();
    Ok(EncodedCatalog {
        idx,
        syn,
        ifo,
        catalog: Catalog {
            index,
            synonyms,
            ordinals,
        },
    })
}
/// Encodes the catalog and writes `dictionary.idx`, `.syn` and `.ifo` in `root`.
pub fn write_catalog(
    root: &Path,
    title: &str,
    items: &[CatalogItem],
    aliases: &[CatalogAlias],
    bytes: u64,
    bits: u8,
    limits: &Limits,
) -> Result<Catalog> {
    let encoded = encode_catalog(title, items, aliases, bytes, bits, limits)?;
    write_file(root, "dictionary.idx", &encoded.idx)?;
    write_file(root, "dictionary.syn", &encoded.syn)?;
    write_file(root, "dictionary.ifo", &encoded.ifo)?;
    Ok(encoded.catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_payloads_and_duplicate_headwords_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits::default();
        let mut out = PayloadWriter::new(dir.path(), &limits, 32).unwrap();
        let shared = out.append(b"<b>shared meaning</b>").unwrap();
        let separate = out.append(b"<b>another meaning</b>").unwrap();
        let bytes = out.finish().unwrap();
        let items = vec![
            CatalogItem {
                id: 0,
                word: "run".into(),
                payload: shared,
            },
            CatalogItem {
                id: 1,
                word: "running".into(),
                payload: shared,
            },
            CatalogItem {
                id: 2,
                word: "run".into(),
                payload: separate,
            },
        ];
        let aliases = vec![
            CatalogAlias {
                word: "runs".into(),
                target_id: 0,
            },
            CatalogAlias {
                word: "runs".into(),
                target_id: 2,
            },
        ];
        let catalog =
            write_catalog(dir.path(), "Test", &items, &aliases, bytes, 32, &limits).unwrap();
        let parsed = crate::open(dir.path(), &limits).unwrap();
        assert_eq!(catalog.index, parsed.entries);
        assert_eq!(catalog.synonyms, parsed.synonyms);
        assert_eq!(parsed.lookup("run").len(), 2);
        assert_eq!(parsed.lookup("runs").len(), 2);
        assert_eq!(
            parsed.entries[parsed.lookup("running")[0]].offset,
            shared.offset
        );
    }
    fn sample(writer: &mut PayloadWriter<impl Write>) -> (Vec<CatalogItem>, Vec<CatalogAlias>) {
        let shared = writer.append(b"<b>shared meaning</b>").unwrap();
        let separate = writer.append("<b>another \u{e9}</b>".as_bytes()).unwrap();
        let items = vec![
            CatalogItem {
                id: 0,
                word: "run".into(),
                payload: shared,
            },
            CatalogItem {
                id: 1,
                word: "running".into(),
                payload: shared,
            },
            CatalogItem {
                id: 2,
                word: "caf\u{e9}".into(),
                payload: separate,
            },
        ];
        let aliases = vec![CatalogAlias {
            word: "runs".into(),
            target_id: 0,
        }];
        (items, aliases)
    }
    #[test]
    fn memory_and_disk_output_are_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits::default();
        let mut disk = PayloadWriter::new(dir.path(), &limits, 64).unwrap();
        let (items, aliases) = sample(&mut disk);
        let bytes = disk.finish().unwrap();
        let catalog =
            write_catalog(dir.path(), "Test", &items, &aliases, bytes, 64, &limits).unwrap();

        let mut memory = PayloadWriter::with_writer(Vec::new(), &limits, 64).unwrap();
        sample(&mut memory);
        let (dict, length) = memory.into_inner().unwrap();
        let encoded = encode_catalog("Test", &items, &aliases, length, 64, &limits).unwrap();
        let file = |name: &str| std::fs::read(dir.path().join(name)).unwrap();
        assert_eq!(dict, file("dictionary.dict"));
        assert_eq!(encoded.idx, file("dictionary.idx"));
        assert_eq!(encoded.syn, file("dictionary.syn"));
        assert_eq!(encoded.ifo, file("dictionary.ifo"));
        assert_eq!(encoded.catalog.index, catalog.index);

        // The pure parser reads exactly what `open` reads from disk.
        let parsed =
            crate::parse(&encoded.ifo, &encoded.idx, &encoded.syn, length, &limits).unwrap();
        assert_eq!(parsed, crate::open(dir.path(), &limits).unwrap());
    }
    #[test]
    fn payload_check_reads_the_stream_once_and_catches_changes() {
        let limits = Limits::default();
        let mut writer = PayloadWriter::with_writer(Vec::new(), &limits, 32).unwrap();
        let written: Vec<(Payload, String)> = [&b"<i>one</i>"[..], b"<i>two</i>"]
            .iter()
            .map(|p| (writer.append(p).unwrap(), lexicon_core::sha256(p)))
            .collect();
        let (dict, _) = writer.into_inner().unwrap();
        let mut shared = written.clone();
        shared.push(written[0].clone());
        crate::check_payloads(dict.as_slice(), &shared).unwrap();
        let mut changed = dict.clone();
        changed[4] ^= 1;
        assert!(crate::check_payloads(changed.as_slice(), &written).is_err());
        let mut longer = dict.clone();
        longer.push(b'x');
        assert!(crate::check_payloads(longer.as_slice(), &written).is_err());
        assert!(crate::check_payloads(dict.as_slice(), &written[1..]).is_err());
    }
    #[test]
    fn partial_overlap_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits::default();
        let items = vec![
            CatalogItem {
                id: 0,
                word: "a".into(),
                payload: Payload { offset: 0, size: 4 },
            },
            CatalogItem {
                id: 1,
                word: "b".into(),
                payload: Payload { offset: 2, size: 4 },
            },
        ];
        assert!(write_catalog(dir.path(), "Test", &items, &[], 6, 32, &limits).is_err());
    }
    #[test]
    fn payload_budget_and_nul_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let limits = Limits {
            entry_bytes: 4,
            ..Limits::default()
        };
        let mut writer = PayloadWriter::new(dir.path(), &limits, 32).unwrap();
        assert!(writer.append(b"12345").is_err());
        assert!(writer.append(b"a\0b").is_err());
        assert!(writer.append(&[0xff]).is_err());
    }
}
