//! Readest's dictionary popup for a StarDict HTML dictionary, drawn by the
//! page's browser (Readest runs in a system web view).
//!
//! Ported from Readest (AGPL-3.0) at commit 2ce0522:
//! - apps/readest-app/src/services/dictionaries/providers/starDictProvider.ts:
//!   `renderEntry` and the lookup (index, then synonyms; one entry);
//! - components/.../DictionaryResultsView.tsx: the result card;
//! - src/styles/globals.css: the `[data-dict-content]` sizes.
//!
//! It also uses the parts of Tailwind CSS 4.3.3 preflight.css (MIT,
//! Copyright Tailwind Labs, Inc.) that reach an entry, in the `base` layer,
//! so that an entry's own unlayered `<style>` wins as it does in Readest.
use crate::{App, Dictionary, Match, Outcome, Stay, View};
use lexicon_core::Result;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

const STYLE: &str = r#"@layer theme, base, components, utilities;
@layer base {
  *, ::after, ::before { box-sizing: border-box; margin: 0; padding: 0; border: 0 solid; }
  html { line-height: 1.5; -webkit-text-size-adjust: 100%; tab-size: 4; font-family: 'Inter', ui-sans-serif, system-ui, sans-serif; }
  hr { height: 0; color: inherit; border-top-width: 1px; }
  abbr:where([title]) { text-decoration: underline dotted; }
  h1, h2, h3, h4, h5, h6 { font-size: inherit; font-weight: inherit; }
  a { color: inherit; text-decoration: inherit; }
  b, strong { font-weight: bolder; }
  small { font-size: 80%; }
  sub, sup { font-size: 75%; line-height: 0; position: relative; vertical-align: baseline; }
  sub { bottom: -0.25em; }
  sup { top: -0.5em; }
  table { text-indent: 0; border-color: inherit; border-collapse: collapse; }
  ol, ul, menu { list-style: none; }
  img, svg, video, canvas, audio, iframe, embed, object { display: block; vertical-align: middle; }
  img, video { max-width: 100%; height: auto; }
  [hidden]:where(:not([hidden='until-found'])) { display: none !important; }
  body { color: #171717; background: #e0e0e0; padding: 1rem; }
}
@layer utilities {
  .font-sans { font-family: 'Inter', ui-sans-serif, system-ui, sans-serif; }
  .font-bold { font-weight: 700; }
  .mt-2 { margin-top: 0.5rem; }
  .text-xs { font-size: 0.75rem; line-height: calc(1 / 0.75); }
  .text-sm { font-size: 0.875rem; line-height: calc(1.25 / 0.875); }
  .text-lg { font-size: 1.125rem; line-height: calc(1.75 / 1.125); }
  [data-dict-content] { font-size: calc(var(--dict-font-scale, 1) * 1em); }
  [data-dict-content] .text-xs { font-size: 0.75em; }
  [data-dict-content] .text-sm { font-size: 0.875em; }
  [data-dict-content] .text-lg { font-size: 1.125em; }
  .source { margin-top: 0.5rem; padding-bottom: 0.5rem; opacity: 0.6; }
}
"#;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The words Readest tries, in order: as typed, lowercase, capitalised,
/// uppercase, NFC, NFD, and with combining marks removed.
fn candidates(word: &str) -> Vec<String> {
    let mut capitalised = String::new();
    let mut chars = word.chars();
    if let Some(first) = chars.next() {
        capitalised.extend(first.to_uppercase());
        capitalised.push_str(&chars.as_str().to_lowercase());
    }
    let mut list: Vec<String> = vec![
        word.into(),
        word.to_lowercase(),
        capitalised,
        word.to_uppercase(),
        word.nfc().collect(),
        word.nfd().collect(),
        word.nfd().filter(|&c| !is_combining_mark(c)).collect(),
    ];
    let mut seen = std::collections::BTreeSet::new();
    list.retain(|w| !w.is_empty() && seen.insert(w.clone()));
    list
}

/// Readest's lookup: for the first candidate that matches, the first
/// index entry equal to it ignoring case, else the first synonym. One
/// entry per dictionary.
fn lookup(dictionary: &Dictionary, word: &str) -> Option<usize> {
    let index = dictionary.index();
    candidates(word).iter().find_map(|candidate| {
        let lower = candidate.to_lowercase();
        index
            .entries
            .iter()
            .position(|e| e.word.to_lowercase() == lower)
            .or_else(|| {
                index
                    .synonyms
                    .iter()
                    .find(|s| s.word.to_lowercase() == lower)
                    .map(|s| s.target as usize)
            })
    })
}

/// The popup Readest shows: the query as title, the entry's card with its
/// headword, and the dictionary's name. `res/` never reaches Readest, so
/// images stay unresolved.
pub fn document(dictionary: &Dictionary, query: &str, found: &Match) -> Result<String> {
    Ok(format!(
        "<!DOCTYPE html><html><head><style>{STYLE}</style></head><body>\
         <p class=\"font-bold\">{}</p>\
         <div data-dict-content=\"\" style=\"--dict-font-scale:1\" class=\"font-sans\">\
         <h1 class=\"text-lg font-bold\">{}</h1><div class=\"mt-2 text-sm\">{}</div></div>\
         <div class=\"source\"><span class=\"text-xs\">{}</span></div></body></html>",
        escape(query),
        escape(&found.headword),
        dictionary.payload(found.entry)?,
        escape(dictionary.bookname()),
    ))
}

pub fn search(dictionary: &Dictionary, word: &str) -> Result<Outcome> {
    let Some(entry) = lookup(dictionary, word) else {
        return Ok(Outcome::NotFound { word: word.into() });
    };
    let found = Match {
        headword: dictionary.entry(entry)?.word.clone(),
        entry,
    };
    let document = document(dictionary, word, &found)?;
    Ok(Outcome::View(View {
        app: App::Readest,
        query: word.into(),
        results: vec![found],
        documents: vec![document],
        scroll_to: None,
    }))
}

/// Readest's StarDict provider handles no links: `#x` is an ordinary
/// in-page jump, web links open the system browser, and anything else,
/// `bword://` included, goes nowhere.
pub fn follow(href: &str) -> Outcome {
    if let Some(id) = href.strip_prefix('#') {
        return Outcome::Scroll { id: id.into() };
    }
    let lower = href.to_ascii_lowercase();
    let reason = if lower.starts_with("http://") || lower.starts_with("https://") {
        Stay::ExternalLink
    } else {
        Stay::NotFollowed
    };
    Outcome::Stay { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::tests::build;

    #[test]
    fn one_entry_found_through_readests_candidates() {
        let d = build(
            &[
                (1, "eclair", "<p>cake</p>"),
                (2, "run", "<p>a</p>"),
                (3, "run", "<p>b</p>"),
            ],
            &[("runs", 2)],
            None,
            &[],
        );
        let Outcome::View(view) = search(&d, "RUNS").unwrap() else {
            panic!()
        };
        assert_eq!(view.results.len(), 1);
        assert_eq!(view.results[0].headword, "run");
        assert!(view.documents[0].contains("<p class=\"font-bold\">RUNS</p>"));
        assert!(view.documents[0].contains(
            "<h1 class=\"text-lg font-bold\">run</h1><div class=\"mt-2 text-sm\"><p>a</p></div>"
        ));
        // Readest strips marks from the query, not from headwords.
        assert!(matches!(search(&d, "Éclair").unwrap(), Outcome::View(_)));
        assert!(matches!(
            search(&d, "walk").unwrap(),
            Outcome::NotFound { .. }
        ));
    }

    #[test]
    fn links_are_not_followed() {
        assert_eq!(
            follow("bword://x"),
            Outcome::Stay {
                reason: Stay::NotFollowed
            }
        );
        assert_eq!(
            follow("https://x"),
            Outcome::Stay {
                reason: Stay::ExternalLink
            }
        );
        assert_eq!(follow("#a"), Outcome::Scroll { id: "a".into() });
    }
}
