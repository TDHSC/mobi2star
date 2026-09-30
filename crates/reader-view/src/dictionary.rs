//! A converted StarDict dictionary held in memory.
use lexicon_core::{Error, Limits, Result};
use stardict_io::{IndexEntry, ParsedDictionary};
use std::{cell::OnceCell, collections::BTreeMap, io::Cursor};

/// The files a reader sees: the parsed index, the payloads, the stylesheet
/// next to the `.ifo`, and the `res/` folder.
#[derive(Debug)]
pub struct Dictionary {
    parsed: ParsedDictionary,
    dict: Vec<u8>,
    companion_css: Option<String>,
    /// Files under `res/`, by their path inside it.
    resources: BTreeMap<String, Vec<u8>>,
    entry_limit: usize,
    /// Lookup indexes built on first use; see `lookup`.
    pub(crate) folded: OnceCell<BTreeMap<String, Vec<usize>>>,
    pub(crate) lowercase: OnceCell<crate::lookup::Lowercase>,
    pub(crate) synonyms_by_target: OnceCell<Vec<usize>>,
}

impl Dictionary {
    /// Parses the index files with the independent StarDict reader.
    pub fn from_parts(
        ifo: &[u8],
        idx: &[u8],
        syn: &[u8],
        dict: Vec<u8>,
        companion_css: Option<String>,
        resources: BTreeMap<String, Vec<u8>>,
        limits: &Limits,
    ) -> Result<Self> {
        let parsed = stardict_io::parse(ifo, idx, syn, dict.len() as u64, limits)?;
        Ok(Self {
            parsed,
            dict,
            companion_css,
            resources,
            entry_limit: limits.entry_bytes,
            folded: OnceCell::new(),
            lowercase: OnceCell::new(),
            synonyms_by_target: OnceCell::new(),
        })
    }
    pub fn bookname(&self) -> &str {
        &self.parsed.bookname
    }
    pub fn index(&self) -> &ParsedDictionary {
        &self.parsed
    }
    pub fn entry(&self, ordinal: usize) -> Result<&IndexEntry> {
        self.parsed
            .entries
            .get(ordinal)
            .ok_or_else(|| Error::Verify(format!("no index entry {ordinal}")))
    }
    /// The HTML payload of the entry at `ordinal`.
    pub fn payload(&self, ordinal: usize) -> Result<String> {
        let entry = self.entry(ordinal)?;
        stardict_io::read_payload(&mut Cursor::new(&self.dict), entry, self.entry_limit)
    }
    /// The `.css` next to the `.ifo`, if the dictionary has one.
    pub fn companion_css(&self) -> Option<&str> {
        self.companion_css.as_deref()
    }
    /// A file in `res/`, by its path inside `res/`.
    pub fn resource(&self, path: &str) -> Option<&[u8]> {
        self.resources.get(path).map(Vec::as_slice)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use stardict_io::{CatalogAlias, CatalogItem, PayloadWriter};

    /// A dictionary built in memory: `entries` are (id, headword, html),
    /// `aliases` are (word, id).
    pub fn build(
        entries: &[(u64, &str, &str)],
        aliases: &[(&str, u64)],
        companion_css: Option<&str>,
        resources: &[(&str, &[u8])],
    ) -> Dictionary {
        let limits = Limits::default();
        let mut writer = PayloadWriter::with_writer(Vec::new(), &limits, 32).unwrap();
        let items: Vec<CatalogItem> = entries
            .iter()
            .map(|&(id, word, html)| CatalogItem {
                id,
                word: word.into(),
                payload: writer.append(html.as_bytes()).unwrap(),
            })
            .collect();
        let aliases: Vec<CatalogAlias> = aliases
            .iter()
            .map(|&(word, target_id)| CatalogAlias {
                word: word.into(),
                target_id,
            })
            .collect();
        let (dict, length) = writer.into_inner().unwrap();
        let encoded =
            stardict_io::encode_catalog("Test book", &items, &aliases, length, 32, &limits)
                .unwrap();
        Dictionary::from_parts(
            &encoded.ifo,
            &encoded.idx,
            &encoded.syn,
            dict,
            companion_css.map(str::to_owned),
            resources
                .iter()
                .map(|&(path, bytes)| (path.to_owned(), bytes.to_vec()))
                .collect(),
            &limits,
        )
        .unwrap()
    }

    #[test]
    fn reads_payloads_and_files() {
        let dictionary = build(
            &[(1, "run", "<b>run</b>"), (2, "cat", "<i>cat</i>")],
            &[("runs", 1)],
            Some(".x{}"),
            &[("a.png", b"PNG")],
        );
        assert_eq!(dictionary.bookname(), "Test book");
        let run = dictionary.index().lookup("runs");
        assert_eq!(dictionary.payload(run[0]).unwrap(), "<b>run</b>");
        assert_eq!(dictionary.companion_css(), Some(".x{}"));
        assert_eq!(dictionary.resource("a.png"), Some(&b"PNG"[..]));
        assert!(dictionary.payload(9).is_err());
    }
}
