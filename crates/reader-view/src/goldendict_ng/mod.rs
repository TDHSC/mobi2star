//! GoldenDict-ng's article view for a StarDict HTML dictionary, drawn by
//! the page's browser (GoldenDict-ng uses Chromium through Qt WebEngine).
//!
//! Ported from GoldenDict-ng (GPL-3.0-or-later) at commit b16a94a:
//! - src/dict/stardict.cc: `handleResource` (the `sdct_h` wrapper, res/
//!   URLs for `<img>` and `<link>`, article links) and the article list
//!   with `sdct_headwords` and `Utils::Html::getHtmlCleaner`;
//! - src/article_maker.cc: the per-dictionary `<article>` markup;
//! - src/stylesheets/article-style.css: the rules that shape an article;
//! - src/articleview.cc and scripts/gd-custom.js: following links.
mod isolate_css;

pub use isolate_css::isolate_css;

use crate::{
    html::{escape, inline_images},
    url::{percent_decode, resource_path},
    App, Dictionary, Match, Outcome, Stay, View,
};
use html_preserve::tokenizer::{Token, Tokenizer};
use lexicon_core::{sha256, Result};

/// The rules of article-style.css that shape an article; the rest styles
/// GoldenDict's own chrome or print.
const ARTICLE_STYLE: &str = r#"html { height: 100%; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen-Sans, Ubuntu, Cantarell, "Helvetica Neue", Arial, "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol", sans-serif; }
gd-dict-header, .gddictname { display: flow-root; margin-top: 0.5em; margin-bottom: 0.5em; font-weight: bold; font-size: 0.8em; padding: 0.3em; padding-left: 0.5em; background: #def; user-select: none; }
.gdarticlebody { clear: both; }
.gdfromprefix { display: none; }
.gdarticle { display: flow-root; padding-top: 1px; margin-top: -8px; margin-bottom: 8px; font-style: normal; }
"#;

/// Closes tags an article may have left open (`getHtmlCleaner`).
const CLEANER: &str =
    "</font></font></font></font></font></font></font></font></font></font></font></font>
                     </b></b></b></b></b></b></b></b>
                     </i></i></i></i></i></i></i></i>
                     </a></a></a></a></a></a></a></a>";

/// GoldenDict shows at most this many matches from one dictionary.
const MAX_MATCHES: usize = 10;

/// The res/ stylesheets an entry links to, in order.
fn linked_stylesheets(html: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in Tokenizer::new(html.as_bytes()) {
        let Ok(Token::Tag(tag)) = token else {
            continue;
        };
        if tag.name != "link" || tag.closing {
            continue;
        }
        let Some(raw) = tag
            .attr("href")
            .and_then(|a| a.value)
            .and_then(|span| html.get(span.start..span.end))
        else {
            continue;
        };
        let href = html_preserve::decode_entities(raw).unwrap_or_else(|_| raw.into());
        paths.extend(resource_path(&href));
    }
    paths
}

/// The page GoldenDict-ng shows for `matches` from this dictionary.
pub fn document(dictionary: &Dictionary, matches: &[Match]) -> Result<String> {
    let id = &sha256(dictionary.bookname().as_bytes())[..32];
    let mut styles = Vec::new();
    let mut articles = String::new();
    for found in matches {
        let payload = dictionary.payload(found.entry)?;
        for path in linked_stylesheets(&payload) {
            if let Some(css) = dictionary.resource(&path) {
                let css = isolate_css(&String::from_utf8_lossy(css), id);
                if !styles.contains(&css) {
                    styles.push(css);
                }
            }
        }
        let payload = inline_images(&payload, |src| {
            resource_path(src).and_then(|path| dictionary.resource(&path))
        });
        articles.push_str(&format!(
            "<h3 class=\"sdct_headwords\">{}</h3><div class=\"sdct_h\">{payload}</div>{CLEANER}",
            found.headword
        ));
    }
    let styles: String = styles
        .iter()
        .map(|css| format!("<style>{css}</style>"))
        .collect();
    Ok(format!(
        "<!DOCTYPE html><html><head><style>{ARTICLE_STYLE}</style>{styles}</head><body>\
         <article class=\"gdarticle\" id=\"gdfrom-{id}\">\
         <gd-dict-header class=\"gddictname\"><span class=\"gdfromprefix\">From </span>\
         <span class=\"gddicttitle\">{}</span></gd-dict-header>\
         <section class=\"gdarticlebody\" id=\"gd-{id}\">{articles}</section></article>\
         <div style=\"clear:both;\"></div></body></html>",
        escape(dictionary.bookname())
    ))
}

/// A lookup through GoldenDict's folded index: case-insensitive matches
/// of the headword first, then the other folded matches, at most ten.
pub fn search(dictionary: &Dictionary, word: &str) -> Result<Outcome> {
    view(dictionary, word, None)
}

fn view(dictionary: &Dictionary, word: &str, scroll_to: Option<String>) -> Result<Outcome> {
    let lower = word.to_lowercase();
    let mut main = Vec::new();
    let mut alternates = Vec::new();
    for entry in dictionary.lookup_folded(word) {
        let headword = dictionary.entry(entry)?.word.clone();
        let target = if headword.to_lowercase() == lower {
            &mut main
        } else {
            &mut alternates
        };
        target.push(Match { headword, entry });
    }
    main.append(&mut alternates);
    main.truncate(MAX_MATCHES);
    if main.is_empty() {
        return Ok(Outcome::NotFound { word: word.into() });
    }
    let document = document(dictionary, &main)?;
    Ok(Outcome::View(View {
        app: App::GoldendictNg,
        query: word.into(),
        results: main,
        documents: vec![document],
        scroll_to,
    }))
}

/// Following a link as GoldenDict-ng does, given its `href` as written in
/// the entry. `#x` scrolls within the page; `bword://word` and relative
/// words look `word` up, URL-decoded; `word#x` looks up `word` and then
/// scrolls to `x`; anything with a scheme is external.
pub fn follow(dictionary: &Dictionary, href: &str) -> Result<Outcome> {
    if let Some(id) = href.strip_prefix('#') {
        return Ok(Outcome::Scroll {
            id: percent_decode(id),
        });
    }
    let link = href.strip_prefix("bword://").unwrap_or(href);
    if link.contains(':') {
        return Ok(Outcome::Stay {
            reason: Stay::ExternalLink,
        });
    }
    match link.find('#') {
        Some(at) if at > 0 => view(
            dictionary,
            &percent_decode(&link[..at]),
            Some(percent_decode(&link[at + 1..])),
        ),
        _ => view(dictionary, &percent_decode(link), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::tests::build;

    fn sample() -> Dictionary {
        build(
            &[
                (
                    1,
                    "café",
                    r#"<link rel="stylesheet" href="dictionary.css" style="display:none"/><p>coffee <a href="bword://tea#t2">tea</a></p><img src="source/a.png">"#,
                ),
                (2, "Cafe", "<p>other</p>"),
                (3, "tea", r#"<p id="t1">one</p><p id="t2">two</p>"#),
            ],
            &[],
            None,
            &[
                ("dictionary.css", b"body{margin:0} .m2s p{color:red}"),
                ("source/a.png", b"\x89PNG\r\n\x1a\n."),
            ],
        )
    }

    #[test]
    fn the_page_is_goldendicts() {
        let d = sample();
        let Outcome::View(view) = search(&d, "cafe").unwrap() else {
            panic!()
        };
        let words: Vec<_> = view.results.iter().map(|m| m.headword.as_str()).collect();
        assert_eq!(words, ["Cafe", "café"], "case-insensitive match first");
        let html = &view.documents[0];
        let id = &sha256(b"Test book")[..32];
        assert!(html.contains(&format!("<style>#gd-{id}, #gd-{id} gd-section-body {{margin:0}}#gd-{id} .m2s p {{color:red}}</style>")), "{html}");
        assert!(html.contains(&format!("<section class=\"gdarticlebody\" id=\"gd-{id}\"><h3 class=\"sdct_headwords\">Cafe</h3><div class=\"sdct_h\"><p>other</p></div></font>")));
        assert!(html.contains("<img src=\"data:image/png;base64,"));
        assert_eq!(
            html.matches("<style>#gd-").count(),
            1,
            "one copy of the linked sheet"
        );
    }

    #[test]
    fn links_look_up_then_scroll() {
        let d = sample();
        let Outcome::View(view) = follow(&d, "bword://tea#t2").unwrap() else {
            panic!()
        };
        assert_eq!(view.results[0].headword, "tea");
        assert_eq!(view.scroll_to.as_deref(), Some("t2"));
        assert_eq!(
            follow(&d, "#t1").unwrap(),
            Outcome::Scroll { id: "t1".into() }
        );
        assert_eq!(
            follow(&d, "http://x.org").unwrap(),
            Outcome::Stay {
                reason: Stay::ExternalLink
            }
        );
        assert!(
            matches!(follow(&d, "bword://caf%C3%A9").unwrap(), Outcome::View(v) if v.results.len() == 2)
        );
        assert!(matches!(
            follow(&d, "bword://nothing").unwrap(),
            Outcome::NotFound { .. }
        ));
    }
}
