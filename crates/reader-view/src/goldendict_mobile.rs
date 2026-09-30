//! GoldenDict Mobile on Android, drawn by the page's browser (it uses the
//! Android WebView). Its source is not public: user reports say only a
//! `<style>` inside an entry works and linked res/ stylesheets do not.
//! Nothing else is established, so this adds no rules of its own: entries
//! appear in a plain page, and links are not followed.
use crate::{
    html::inline_images, url::percent_decode, App, Dictionary, Match, Outcome, Stay, View,
};
use lexicon_core::Result;

pub fn document(dictionary: &Dictionary, matches: &[Match]) -> Result<String> {
    let mut body = String::new();
    for found in matches {
        let payload = dictionary.payload(found.entry)?;
        body.push_str(&inline_images(&payload, |src| {
            dictionary.resource(percent_decode(src).trim_start_matches('/'))
        }));
    }
    Ok(format!(
        "<!DOCTYPE html><html><head></head><body>{body}</body></html>"
    ))
}

/// Exact matches, else matches ignoring ASCII case.
pub fn search(dictionary: &Dictionary, word: &str) -> Result<Outcome> {
    let mut hits = dictionary.lookup_exact(word);
    if hits.is_empty() {
        hits = dictionary.lookup_ascii_case(word);
    }
    if hits.is_empty() {
        return Ok(Outcome::NotFound { word: word.into() });
    }
    let results = hits
        .into_iter()
        .map(|entry| {
            Ok(Match {
                headword: dictionary.entry(entry)?.word.clone(),
                entry,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let document = document(dictionary, &results)?;
    Ok(Outcome::View(View {
        app: App::GoldendictMobile,
        query: word.into(),
        results,
        documents: vec![document],
        scroll_to: None,
    }))
}

/// How GoldenDict Mobile follows links is not known.
pub fn follow() -> Outcome {
    Outcome::Stay {
        reason: Stay::Unknown,
    }
}
