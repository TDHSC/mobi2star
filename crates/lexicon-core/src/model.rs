use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
impl Span {
    pub fn new(start: usize, len: usize, bound: usize) -> Result<Self> {
        let end = start
            .checked_add(len)
            .ok_or_else(|| Error::Malformed("span overflow".into()))?;
        if start > end || end > bound {
            return Err(Error::Malformed(format!(
                "span {start}..{end} exceeds {bound}"
            )));
        }
        Ok(Self { start, end })
    }
    pub fn len(self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
    pub fn contains(self, position: usize) -> bool {
        self.start <= position && position < self.end
    }
    pub fn bytes(self, data: &[u8]) -> Result<&[u8]> {
        data.get(self.start..self.end)
            .ok_or_else(|| Error::Malformed("invalid stored span".into()))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Utf8,
    Windows1252,
}
impl Encoding {
    pub fn from_mobi(value: u32) -> Result<Self> {
        match value {
            65001 => Ok(Self::Utf8),
            1252 => Ok(Self::Windows1252),
            _ => Err(Error::Unsupported(format!("MOBI encoding {value}"))),
        }
    }
    pub fn decode(self, bytes: &[u8]) -> Result<String> {
        let value = match self {
            Self::Utf8 => std::str::from_utf8(bytes).map(str::to_owned).map_err(|e| {
                Error::Malformed(format!(
                    "invalid UTF-8 at relative byte {}",
                    e.valid_up_to()
                ))
            })?,
            Self::Windows1252 => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling_and_without_replacement(bytes)
                .ok_or_else(|| Error::Malformed("undecodable Windows-1252".into()))?
                .into_owned(),
        };
        if value.contains('\0') {
            return Err(Error::Incomplete(
                "NUL in text; refusing silent truncation".into(),
            ));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub input_bytes: usize,
    pub output_bytes: u64,
    pub text_bytes: usize,
    pub entry_bytes: usize,
    pub entries: usize,
    pub aliases: usize,
    pub operations: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1024 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024 * 1024,
            text_bytes: 512 * 1024 * 1024,
            entry_bytes: 32 * 1024 * 1024,
            entries: 2_000_000,
            aliases: 8_000_000,
            operations: 100_000_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Alias {
    pub word: String,
    pub group: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Headword,
    Supplement,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub headword: String,
    pub aliases: Vec<Alias>,
    pub span: Span,
    pub kind: EntryKind,
}
impl Entry {
    pub fn internal_key(&self, namespace: &str) -> String {
        routing_key(namespace, self.id)
    }
}
/// Source identity prevents cross-dictionary internal alias collisions.
pub fn routing_key(namespace: &str, id: u64) -> String {
    format!("__mobi2star_{namespace}_entry_{id:016x}")
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Resource {
    /// 1-based MOBI recindex, not the PDB record number.
    pub recindex: u32,
    pub pdb_record: usize,
    pub source_span: Span,
    pub filename: String,
    pub media_type: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub input_language: u32,
    pub output_language: u32,
    /// All EXTH records are retained, including unknown metadata types.
    pub exth: Vec<ExthRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExthRecord {
    pub kind: u32,
    pub data_hex: String,
}
#[derive(Debug)]
pub struct Document {
    pub namespace: String,
    pub source: Vec<u8>,
    pub rawml: Vec<u8>,
    pub encoding: Encoding,
    pub metadata: Metadata,
    pub records: Vec<Span>,
    pub entries: Vec<Entry>,
    pub resources: Vec<Resource>,
    pub source_headwords: usize,
    pub source_aliases: usize,
    /// Parsed source index/tag data, separate from the transformed model.
    pub index_audit: serde_json::Value,
}

/// Coverage uses the union of ranges, so nested/subentries do not inflate it.
pub fn uncovered(bound: usize, spans: impl IntoIterator<Item = Span>) -> Result<Vec<Span>> {
    let mut ranges: Vec<Span> = spans.into_iter().collect();
    ranges.sort();
    let mut cursor = 0;
    let mut gaps = Vec::new();
    for s in ranges {
        if s.start > s.end || s.end > bound {
            return Err(Error::Incomplete("out-of-range content span".into()));
        }
        if cursor < s.start {
            gaps.push(Span {
                start: cursor,
                end: s.start,
            });
        }
        cursor = cursor.max(s.end);
    }
    if cursor < bound {
        gaps.push(Span {
            start: cursor,
            end: bound,
        });
    }
    Ok(gaps)
}

pub fn validate_word(word: &str) -> Result<()> {
    if word.is_empty() || word.len() >= 256 || word.chars().any(char::is_control) {
        return Err(Error::Incomplete(format!(
            "StarDict key cannot represent this word without changes: {word:?}"
        )));
    }
    if word.starts_with("__mobi2star_") {
        return Err(Error::Incomplete(
            "source key collides with reserved routing namespace".into(),
        ));
    }
    Ok(())
}

impl Document {
    pub fn validate(&self, limits: &Limits) -> Result<()> {
        if self.namespace != crate::sha256(&self.source) {
            return Err(Error::Incomplete(
                "dictionary routing namespace is not its source fingerprint".into(),
            ));
        }
        if self.rawml.len() > limits.text_bytes
            || self.source.len() > limits.input_bytes
            || self.entries.len() > limits.entries
        {
            return Err(Error::Limit("document exceeds configured bounds".into()));
        }
        let mut ids = BTreeSet::new();
        let mut main = 0;
        let mut aliases = 0usize;
        for e in &self.entries {
            if !ids.insert(e.id) {
                return Err(Error::Incomplete("duplicate entry identifier".into()));
            }
            validate_word(&e.headword)?;
            let body = e.span.bytes(&self.rawml)?;
            if body.is_empty() || body.len() > limits.entry_bytes {
                return Err(Error::Limit(format!(
                    "entry {} has invalid size {}",
                    e.id,
                    body.len()
                )));
            }
            self.encoding.decode(body)?;
            if e.kind == EntryKind::Headword {
                main += 1;
            }
            for a in &e.aliases {
                validate_word(&a.word)?;
            }
            aliases = aliases
                .checked_add(e.aliases.len())
                .ok_or_else(|| Error::Limit("alias overflow".into()))?;
        }
        if aliases > limits.aliases {
            return Err(Error::Limit("alias count".into()));
        }
        if main != self.source_headwords || aliases != self.source_aliases {
            return Err(Error::Incomplete(
                "headword/inflection count does not match source model".into(),
            ));
        }
        if !uncovered(self.rawml.len(), self.entries.iter().map(|e| e.span))?.is_empty() {
            return Err(Error::Incomplete(
                "some decompressed text has no output entry".into(),
            ));
        }
        for r in &self.resources {
            r.source_span.bytes(&self.source)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn union_handles_nested_ranges() {
        let gaps = uncovered(
            30,
            [
                Span { start: 5, end: 20 },
                Span { start: 8, end: 12 },
                Span { start: 15, end: 25 },
            ],
        )
        .unwrap();
        assert_eq!(
            gaps,
            vec![Span { start: 0, end: 5 }, Span { start: 25, end: 30 }]
        );
    }
    #[test]
    fn encoding_is_never_lossy() {
        assert!(Encoding::Utf8.decode(&[0xff]).is_err());
        assert_eq!(Encoding::Windows1252.decode(&[0x80]).unwrap(), "€");
        assert!(Encoding::Utf8.decode(b"a\0b").is_err());
    }
    #[test]
    fn internal_keys_are_namespaced_by_source() {
        let a = routing_key(&crate::sha256(b"book A"), 0);
        let b = routing_key(&crate::sha256(b"book B"), 0);
        assert_ne!(a, b);
        assert!(a.len() < 256);
    }
    #[test]
    fn never_truncate_keys() {
        assert!(validate_word(&"中".repeat(86)).is_err());
        assert!(validate_word("café").is_ok());
    }
}
