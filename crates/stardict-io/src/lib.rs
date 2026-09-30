//! StarDict writer and separately implemented structural reader.
//! Sorting is bytewise ASCII-folded, then bytewise original, never Unicode folding.
#![forbid(unsafe_code)]
mod catalog;
mod reader;
mod style;
mod writer;
pub use catalog::{
    encode_catalog, write_catalog, Catalog, CatalogAlias, CatalogItem, EncodedCatalog, Payload,
    PayloadWriter,
};
use lexicon_core::{Error, Result};
pub use reader::{check_payloads, dictionary_file, open, parse, read_payload, ParsedDictionary};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
pub use style::{stylesheet_files, stylesheet_paths};
pub use writer::{write, WrittenDictionary, WrittenEntry};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexEntry {
    pub word: String,
    pub offset: u64,
    pub size: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Synonym {
    pub word: String,
    pub target: u32,
}

pub fn compare_words(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|x| x.to_ascii_lowercase())
        .cmp(b.bytes().map(|x| x.to_ascii_lowercase()))
        .then_with(|| a.as_bytes().cmp(b.as_bytes()))
}
pub fn compare_synonyms(a: &Synonym, b: &Synonym) -> Ordering {
    compare_words(&a.word, &b.word).then(a.target.cmp(&b.target))
}
pub fn validate_key(word: &str) -> Result<()> {
    if word.is_empty() || word.len() >= 256 || word.chars().any(char::is_control) {
        return Err(Error::Verify(format!("invalid StarDict key {word:?}")));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_sort_is_not_unicode_casefold() {
        let mut words = vec!["ä", "Z", "apple", "Apple", "Ä", "z"];
        words.sort_by(|a, b| compare_words(a, b));
        assert_eq!(words, ["Apple", "apple", "Z", "z", "Ä", "ä"]);
    }
    #[test]
    fn distinct_homographs_survive() {
        let a = Synonym {
            word: "axes".into(),
            target: 0,
        };
        let b = Synonym {
            word: "axes".into(),
            target: 1,
        };
        assert_eq!(compare_synonyms(&a, &b), Ordering::Less);
    }
}
