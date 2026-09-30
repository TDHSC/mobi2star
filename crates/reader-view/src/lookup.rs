//! Finding entries by word, in the ways readers do.
use crate::Dictionary;
use stardict_io::{compare_words, IndexEntry, Synonym};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Orders words the way `compare_words` does first: bytes, ASCII case
/// ignored. Words equal under it are contiguous in the index.
fn ascii_order(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|x| x.to_ascii_lowercase())
        .cmp(b.bytes().map(|x| x.to_ascii_lowercase()))
}

/// GoldenDict's folding: canonical decomposition without combining marks,
/// lowercase, and no whitespace or punctuation.
pub(crate) fn fold(word: &str) -> String {
    word.nfd()
        .filter(|&c| !is_combining_mark(c) && c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `word` is a lookup key the converter adds for links, which
/// nobody types: `lexicon_core::routing_key` and `srcs_render`'s entry and
/// page routes, with or without a `#fragment`.
pub fn is_internal_key(word: &str) -> bool {
    let word = word.split('#').next().unwrap_or_default();
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    if let Some(rest) = word.strip_prefix("__mobi2star_") {
        return rest
            .split_once("_entry_")
            .is_some_and(|(namespace, id)| hex(namespace, 64) && hex(id, 16));
    }
    if let Some(rest) = word.strip_prefix("m2s-") {
        return rest.split_once('-').is_some_and(|(namespace, id)| {
            hex(namespace, 64)
                && id.len() > 1
                && matches!(id.as_bytes()[0], b'e' | b'p')
                && id[1..].bytes().all(|b| b.is_ascii_digit())
        });
    }
    false
}

/// The first entry for each lowercase headword, and the target of the
/// first synonym for each lowercase synonym, in index order.
#[derive(Debug, Default)]
pub(crate) struct Lowercase {
    entries: BTreeMap<String, usize>,
    synonyms: BTreeMap<String, usize>,
}

impl Dictionary {
    /// Entries whose headword or synonym is exactly `word`.
    pub fn lookup_exact(&self, word: &str) -> Vec<usize> {
        self.index().lookup(word)
    }

    /// Entries whose headword or synonym is `word`, ignoring ASCII case.
    pub fn lookup_ascii_case(&self, word: &str) -> Vec<usize> {
        let index = self.index();
        let mut hits = BTreeSet::new();
        let first = index
            .entries
            .partition_point(|e: &IndexEntry| ascii_order(&e.word, word).is_lt());
        hits.extend(
            (first..index.entries.len())
                .take_while(|&n| index.entries[n].word.eq_ignore_ascii_case(word)),
        );
        let first = index
            .synonyms
            .partition_point(|s: &Synonym| ascii_order(&s.word, word).is_lt());
        hits.extend(
            index.synonyms[first..]
                .iter()
                .take_while(|s| s.word.eq_ignore_ascii_case(word))
                .map(|s| s.target as usize),
        );
        hits.into_iter().collect()
    }

    /// Entries whose headword or synonym folds to the same key as `word`,
    /// as in GoldenDict's index. The folded index is built on first use and,
    /// like GoldenDict's, includes internal keys: following a link looks
    /// its route up through it.
    pub fn lookup_folded(&self, word: &str) -> Vec<usize> {
        let folded = self.folded.get_or_init(|| {
            let index = self.index();
            let mut map: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
            for (n, entry) in index.entries.iter().enumerate() {
                map.entry(fold(&entry.word)).or_default().insert(n);
            }
            for synonym in &index.synonyms {
                map.entry(fold(&synonym.word))
                    .or_default()
                    .insert(synonym.target as usize);
            }
            map.into_iter()
                .map(|(key, hits)| (key, hits.into_iter().collect()))
                .collect()
        });
        folded.get(&fold(word)).cloned().unwrap_or_default()
    }

    /// The first entry whose headword equals `word` ignoring case, else the
    /// target of the first synonym that does, as Readest looks words up.
    /// The lowercase index is built on first use.
    pub fn lookup_lowercase_first(&self, word: &str) -> Option<usize> {
        let lowercase = self.lowercase.get_or_init(|| {
            let index = self.index();
            let mut lowercase = Lowercase::default();
            for (n, entry) in index.entries.iter().enumerate() {
                lowercase
                    .entries
                    .entry(entry.word.to_lowercase())
                    .or_insert(n);
            }
            for synonym in &index.synonyms {
                lowercase
                    .synonyms
                    .entry(synonym.word.to_lowercase())
                    .or_insert(synonym.target as usize);
            }
            lowercase
        });
        let lower = word.to_lowercase();
        lowercase
            .entries
            .get(&lower)
            .or_else(|| lowercase.synonyms.get(&lower))
            .copied()
    }

    /// The synonyms that lead to `entry`, in `.syn` order.
    pub fn synonyms_of(&self, entry: usize) -> impl Iterator<Item = &Synonym> {
        let synonyms = &self.index().synonyms;
        let by_target = self.synonyms_by_target.get_or_init(|| {
            let mut order: Vec<usize> = (0..synonyms.len()).collect();
            order.sort_by_key(|&n| synonyms[n].target);
            order
        });
        let first = by_target.partition_point(|&n| (synonyms[n].target as usize) < entry);
        by_target[first..]
            .iter()
            .map(|&n| &synonyms[n])
            .take_while(move |s| s.target as usize == entry)
    }

    /// Up to `limit` distinct headwords and synonyms that start with
    /// `prefix`, ignoring ASCII case, in index order. Internal keys are left
    /// out: they are for links, not people.
    pub fn suggest(&self, prefix: &str, limit: usize) -> Vec<String> {
        let index = self.index();
        let starts = |w: &str| {
            w.len() >= prefix.len()
                && w.is_char_boundary(prefix.len())
                && w[..prefix.len()].eq_ignore_ascii_case(prefix)
        };
        let mut words: Vec<&str> = Vec::new();
        let first = index
            .entries
            .partition_point(|e| ascii_order(&e.word, prefix).is_lt());
        words.extend(
            index.entries[first..]
                .iter()
                .map(|e| e.word.as_str())
                .take_while(|w| starts(w))
                .filter(|w| !is_internal_key(w))
                .take(limit),
        );
        let first = index
            .synonyms
            .partition_point(|s| ascii_order(&s.word, prefix).is_lt());
        words.extend(
            index.synonyms[first..]
                .iter()
                .map(|s| s.word.as_str())
                .take_while(|w| starts(w))
                .filter(|w| !is_internal_key(w))
                .take(limit),
        );
        words.sort_by(|a, b| compare_words(a, b));
        words.dedup();
        words.truncate(limit);
        words.into_iter().map(str::to_owned).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::tests::build;

    fn sample() -> Dictionary {
        build(
            &[
                (1, "Run", "<b>Run</b>"),
                (2, "run", "<b>run</b>"),
                (3, "café", "<b>café</b>"),
                (4, "cat", "<b>cat</b>"),
            ],
            &[
                ("runs", 2),
                ("RAN", 2),
                (&lexicon_core::routing_key(&"a".repeat(64), 7), 4),
            ],
            None,
            &[],
        )
    }
    fn words(d: &Dictionary, hits: Vec<usize>) -> Vec<String> {
        hits.iter()
            .map(|&n| d.index().entries[n].word.clone())
            .collect()
    }

    #[test]
    fn the_lookups_differ_as_readers_do() {
        let d = sample();
        assert_eq!(words(&d, d.lookup_exact("run")), ["run"]);
        assert_eq!(words(&d, d.lookup_ascii_case("RUN")), ["Run", "run"]);
        assert_eq!(words(&d, d.lookup_ascii_case("ran")), ["run"]);
        assert!(d.lookup_ascii_case("CAFÉ").is_empty(), "not ASCII");
        assert_eq!(words(&d, d.lookup_folded("Cafe")), ["café"]);
        assert_eq!(words(&d, d.lookup_folded(" c-a-f-e! ")), ["café"]);
        assert!(d.lookup_folded("dog").is_empty());
        let route = lexicon_core::routing_key(&"a".repeat(64), 7);
        assert_eq!(
            words(&d, d.lookup_folded(&route)),
            ["cat"],
            "links need routes"
        );
    }

    #[test]
    fn lowercase_lookups_take_the_first_match() {
        let d = sample();
        let first = |w: &str| {
            d.lookup_lowercase_first(w)
                .map(|n| d.index().entries[n].word.clone())
        };
        assert_eq!(first("RUN").as_deref(), Some("Run"), "index order");
        assert_eq!(first("CAFÉ").as_deref(), Some("café"), "not only ASCII");
        assert_eq!(first("ran").as_deref(), Some("run"), "through a synonym");
        assert_eq!(first("dog"), None);
    }

    #[test]
    fn synonyms_are_found_by_their_target() {
        let d = sample();
        let run = d.lookup_exact("run")[0];
        let words: Vec<&str> = d.synonyms_of(run).map(|s| s.word.as_str()).collect();
        assert_eq!(words, ["RAN", "runs"]);
        assert_eq!(d.synonyms_of(d.lookup_exact("Run")[0]).count(), 0);
    }

    #[test]
    fn suggestions_hide_internal_keys() {
        let d = sample();
        assert_eq!(d.suggest("r", 10), ["RAN", "Run", "run", "runs"]);
        assert_eq!(d.suggest("r", 2).len(), 2);
        assert!(d.suggest("_", 10).is_empty());
        assert_eq!(d.suggest("ca", 10), ["café", "cat"]);
    }

    #[test]
    fn internal_keys_are_the_converters_route_formats() {
        let namespace = "0123456789abcdef".repeat(4);
        for key in [
            lexicon_core::routing_key(&namespace, 42),
            srcs_render::entry_route(&namespace, 7),
            srcs_render::page_route(&namespace, 0),
            format!(
                "{}#mobi2star-pos-9",
                lexicon_core::routing_key(&namespace, 1)
            ),
            format!("{}#frag", srcs_render::entry_route(&namespace, 3)),
        ] {
            assert!(is_internal_key(&key), "{key}");
        }
        for word in ["m2s-short-e1", "__mobi2star_", "run", "m2s", "m2s-x-e1"] {
            assert!(!is_internal_key(word), "{word}");
        }
    }
}
