//! This parser reads emitted files independently of the writer's in-memory model.
use crate::{compare_synonyms, compare_words, validate_key, IndexEntry, Payload, Synonym};
use lexicon_core::{
    bytes::{be32, be64},
    checked_member, read_bounded, sha256, Error, Limits, Result,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedDictionary {
    pub entries: Vec<IndexEntry>,
    pub synonyms: Vec<Synonym>,
    pub offset_bits: u8,
    pub dictionary_bytes: u64,
}
impl ParsedDictionary {
    /// Exact source spelling. Return every distinct destination, not just the first homograph.
    pub fn lookup(&self, word: &str) -> Vec<usize> {
        let mut targets = BTreeSet::new();
        let first = self
            .entries
            .partition_point(|e| compare_words(&e.word, word).is_lt());
        for (i, e) in self
            .entries
            .iter()
            .enumerate()
            .skip(first)
            .take_while(|(_, e)| e.word == word)
        {
            let _ = e;
            targets.insert(i);
        }
        let first_syn = self
            .synonyms
            .partition_point(|s| compare_words(&s.word, word).is_lt());
        for s in self.synonyms[first_syn..]
            .iter()
            .take_while(|s| s.word == word)
        {
            targets.insert(s.target as usize);
        }
        targets.into_iter().collect()
    }
}
fn number(fields: &BTreeMap<&str, &str>, name: &str) -> Result<u64> {
    fields
        .get(name)
        .ok_or_else(|| Error::Verify(format!("IFO lacks {name}")))?
        .parse()
        .map_err(|_| Error::Verify(format!("invalid IFO {name}")))
}
fn word(data: &[u8], at: &mut usize) -> Result<String> {
    let rest = data
        .get(*at..)
        .ok_or_else(|| Error::Verify("index cursor out of range".into()))?;
    let length = rest
        .iter()
        .take(256)
        .position(|&b| b == 0)
        .ok_or_else(|| Error::Verify("unterminated/oversized index key".into()))?;
    let text = std::str::from_utf8(&rest[..length])
        .map_err(|_| Error::Verify("index key is not UTF-8".into()))?
        .to_owned();
    validate_key(&text)?;
    *at += length + 1;
    Ok(text)
}
/// Path of `dictionary.dict` in `root`.
pub fn dictionary_file(root: &Path) -> Result<PathBuf> {
    checked_member(root, "dictionary.dict")
}
/// Reads the dictionary files in `root` and parses them with [`parse`].
pub fn open(root: &Path, limits: &Limits) -> Result<ParsedDictionary> {
    let ifo = read_bounded(&checked_member(root, "dictionary.ifo")?, 65536)?;
    let idx = read_bounded(&checked_member(root, "dictionary.idx")?, limits.input_bytes)?;
    let syn = read_bounded(&checked_member(root, "dictionary.syn")?, limits.input_bytes)?;
    let dictionary_bytes = dictionary_file(root)?.metadata()?.len();
    parse(&ifo, &idx, &syn, dictionary_bytes, limits)
}
/// Parses dictionary files independently of the writer's in-memory model and
/// checks ordering, counts, ordinals and that the `.dict` of length
/// `dictionary_bytes` is covered exactly.
pub fn parse(
    ifo_bytes: &[u8],
    bytes: &[u8],
    syn_bytes: &[u8],
    dictionary_bytes: u64,
    limits: &Limits,
) -> Result<ParsedDictionary> {
    if ifo_bytes.len() > 65536 {
        return Err(Error::Limit("IFO byte budget".into()));
    }
    let ifo = std::str::from_utf8(ifo_bytes).map_err(|_| Error::Verify("IFO encoding".into()))?;
    let mut lines = ifo.lines();
    if lines.next() != Some("StarDict's dict ifo file") {
        return Err(Error::Verify("IFO magic".into()));
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| Error::Verify("malformed IFO line".into()))?;
        if fields.insert(key, value).is_some() {
            return Err(Error::Verify("duplicate IFO key".into()));
        }
    }
    if fields.get("version") != Some(&"3.0.0") || fields.get("sametypesequence") != Some(&"h") {
        return Err(Error::Unsupported(
            "verifier expects StarDict 3.0.0 with HTML payloads".into(),
        ));
    }
    let bits = number(&fields, "idxoffsetbits")?;
    if !matches!(bits, 32 | 64) {
        return Err(Error::Verify("invalid index offset width".into()));
    }
    let count = number(&fields, "wordcount")?;
    let syn_count = number(&fields, "synwordcount")?;
    if count > limits.entries as u64 || syn_count > (limits.aliases as u64 + limits.entries as u64)
    {
        return Err(Error::Limit("output index count".into()));
    }
    if bytes.len() > limits.input_bytes || syn_bytes.len() > limits.input_bytes {
        return Err(Error::Limit("index file byte budget".into()));
    }
    if bytes.len() as u64 != number(&fields, "idxfilesize")? {
        return Err(Error::Verify("IFO/IDX byte count mismatch".into()));
    }
    if dictionary_bytes > limits.output_bytes {
        return Err(Error::Limit("DICT byte budget".into()));
    }
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if entries.len() >= limits.entries {
            return Err(Error::Limit("IDX entries".into()));
        }
        let key = word(bytes, &mut at)?;
        let offset = if bits == 64 {
            let n = be64(bytes, at)?;
            at += 8;
            n
        } else {
            let n = be32(bytes, at)?;
            at += 4;
            u64::from(n)
        };
        let size = be32(bytes, at)?;
        at += 4;
        if size == 0 || size as usize > limits.entry_bytes {
            return Err(Error::Verify("invalid payload size".into()));
        }
        if offset
            .checked_add(u64::from(size))
            .is_none_or(|end| end > dictionary_bytes)
        {
            return Err(Error::Verify("IDX points outside DICT".into()));
        }
        if entries
            .last()
            .is_some_and(|previous| compare_words(&previous.word, &key).is_gt())
        {
            return Err(Error::Verify("IDX not sorted according to StarDict".into()));
        }
        entries.push(IndexEntry {
            word: key,
            offset,
            size,
        });
    }
    if entries.len() as u64 != count {
        return Err(Error::Verify("IFO/IDX word count mismatch".into()));
    }
    // Account for every distinct physical payload range. Exact sharing is valid.
    let mut by_offset: Vec<&IndexEntry> = entries.iter().collect();
    by_offset.sort_by_key(|e| (e.offset, e.size));
    by_offset.dedup_by(|a, b| a.offset == b.offset && a.size == b.size);
    let mut end = 0u64;
    for e in by_offset {
        if e.offset != end {
            return Err(Error::Verify("overlapping or uncovered DICT bytes".into()));
        }
        end += u64::from(e.size);
    }
    if end != dictionary_bytes {
        return Err(Error::Verify("trailing unindexed DICT bytes".into()));
    }
    let mut synonyms: Vec<Synonym> = Vec::new();
    at = 0;
    while at < syn_bytes.len() {
        if synonyms.len() as u64 >= syn_count {
            return Err(Error::Verify("too many SYN records".into()));
        }
        let key = word(syn_bytes, &mut at)?;
        let target = be32(syn_bytes, at)?;
        at += 4;
        if target as usize >= entries.len() {
            return Err(Error::Verify("SYN ordinal out of range".into()));
        }
        let s = Synonym { word: key, target };
        if synonyms
            .last()
            .is_some_and(|previous| compare_synonyms(previous, &s).is_gt())
        {
            return Err(Error::Verify("SYN not sorted".into()));
        }
        synonyms.push(s);
    }
    if synonyms.len() as u64 != syn_count {
        return Err(Error::Verify("IFO/SYN count mismatch".into()));
    }
    Ok(ParsedDictionary {
        entries,
        synonyms,
        offset_bits: bits as u8,
        dictionary_bytes,
    })
}
/// Reads one payload from a `.dict` file or any other seekable source.
pub fn read_payload<R: Read + Seek>(
    file: &mut R,
    entry: &IndexEntry,
    limit: usize,
) -> Result<String> {
    if entry.size as usize > limit {
        return Err(Error::Limit("payload allocation".into()));
    }
    file.seek(SeekFrom::Start(entry.offset))?;
    let mut bytes = vec![0; entry.size as usize];
    file.read_exact(&mut bytes)?;
    let text =
        String::from_utf8(bytes).map_err(|_| Error::Verify("DICT payload is not UTF-8".into()))?;
    if text.contains('\0') {
        return Err(Error::Verify("NUL in HTML payload".into()));
    }
    Ok(text)
}
/// Reads a `.dict` stream front to back and checks every payload against the
/// SHA-256 recorded when it was written. `written` lists (range, digest)
/// pairs; exact shared ranges may repeat. The ranges must tile the stream
/// from offset 0, as [`parse`] requires, and the stream must end after them.
pub fn check_payloads(mut dict: impl Read, written: &[(Payload, String)]) -> Result<()> {
    let mut ranges: Vec<&(Payload, String)> = written.iter().collect();
    ranges.sort_by_key(|(payload, _)| *payload);
    ranges.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let mut offset = 0u64;
    let mut buffer = Vec::new();
    for (payload, digest) in ranges {
        if payload.offset != offset {
            return Err(Error::Verify("payload ranges do not tile the DICT".into()));
        }
        buffer.resize(payload.size as usize, 0);
        dict.read_exact(&mut buffer)?;
        if sha256(&buffer) != *digest {
            return Err(Error::Verify(
                "DICT payload differs from what was written".into(),
            ));
        }
        offset += u64::from(payload.size);
    }
    if dict.read(&mut [0u8])? != 0 {
        return Err(Error::Verify("trailing unindexed DICT bytes".into()));
    }
    Ok(())
}
