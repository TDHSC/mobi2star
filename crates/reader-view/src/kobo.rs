//! A Kobo dictionary made from the StarDict folder with PyGlossary, drawn
//! by the page's browser. The HTML is exactly what PyGlossary writes; how
//! a Kobo device draws it, and what it does with links, is unknown.
//!
//! Ported from PyGlossary 5.4.2 (GPL-3.0-or-later):
//! - pyglossary/plugins/stardict/reader.py: synonyms become alternate terms;
//! - pyglossary/entry.py: `strip`, `_stripTrailingBR`, `stripFullHtml`;
//! - pyglossary/plugins/ebook_kobo/writer.py: `get_prefix`, `fix_defi`,
//!   `normalize_headword` and the `<w>` blocks of `write_groups`.
//!
//! PyGlossary tells scripts apart by Unicode character names; the ranges
//! below cover the same characters for the scripts it names.
use crate::{App, Dictionary, Match, Outcome, Stay, View};
use lexicon_core::Result;
use std::collections::BTreeMap;

fn is_han(c: char) -> bool {
    matches!(u32::from(c), 0x2E80..=0x2EFF | 0x31C0..=0x31EF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3134F)
}
fn is_kana(c: char) -> bool {
    matches!(u32::from(c), 0x3041..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF)
}
fn is_cyrillic(c: char) -> bool {
    matches!(u32::from(c), 0x0400..=0x052F | 0x1C80..=0x1C8F | 0xA640..=0xA69F)
        || matches!(c, '\u{fe2e}' | '\u{fe2f}' | '\u{1d2b}' | '\u{1d78}')
}
/// jaconv's `hira2kata`.
fn to_katakana(text: &str) -> String {
    text.chars()
        .map(|c| match u32::from(c) {
            n @ (0x3041..=0x3096 | 0x309D..=0x309E) => char::from_u32(n + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// `get_prefix`: the group file a term is written to.
fn prefix(word: &str) -> String {
    let head: String = word.chars().take(2).collect();
    let mut wo: Vec<char> = head.trim().to_lowercase().chars().collect();
    if wo.is_empty() || wo[0] == '\0' {
        return "11".into();
    }
    if wo.len() > 1 && wo[1] == '\0' {
        wo.truncate(1);
    }
    if is_han(wo[0]) {
        return wo[0].to_string();
    }
    let text: String = wo.iter().collect();
    if is_kana(wo[0]) {
        return to_katakana(&text);
    }
    if is_cyrillic(wo[0]) {
        return text;
    }
    if !wo.iter().all(|c| c.is_alphabetic()) {
        return "11".into();
    }
    format!("{text:a<2}")
}

/// `strip`, then `stripFullHtml` (a no-op for fragments), then `fix_defi`.
fn definition(payload: &str) -> String {
    let mut defi = payload.trim();
    while let Some(rest) = defi
        .strip_suffix("<br>")
        .or_else(|| defi.strip_suffix("<BR>"))
    {
        defi = rest;
    }
    replace_images(&strip_full_html(defi))
}

fn strip_full_html(defi: &str) -> String {
    let mut text = defi;
    if let Some(rest) = text.strip_prefix("<!DOCTYPE html>") {
        text = rest.trim();
        if !text.starts_with("<html") {
            return defi.into();
        }
    } else if !text.starts_with("<html>") {
        return defi.into();
    }
    let Some(body) = text.find("<body") else {
        return defi.into();
    };
    let text = &text[body + 5..];
    let Some(open) = text.find('>') else {
        return defi.into();
    };
    let text = &text[open + 1..];
    match text.find("</body") {
        Some(close) => text[..close].into(),
        None => defi.into(),
    }
}

/// `re.sub('<img src="([^<>"]*?)"( [^<>]*?)?>', "[Image: \\1]", defi)`.
fn replace_images(defi: &str) -> String {
    const OPEN: &str = "<img src=\"";
    let mut out = String::with_capacity(defi.len());
    let mut rest = defi;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        let src_end = after.find(['<', '>', '"']);
        let matched = src_end
            .filter(|&end| after[end..].starts_with('"'))
            .and_then(|end| {
                let tail = &after[end + 1..];
                if let Some(tail) = tail.strip_prefix('>') {
                    return Some((end, tail));
                }
                let spaced = tail.strip_prefix(' ')?;
                let close = spaced.find(['<', '>'])?;
                spaced[close..]
                    .starts_with('>')
                    .then(|| (end, &spaced[close + 1..]))
            });
        match matched {
            Some((end, tail)) => {
                out.push_str(&rest[..at]);
                out.push_str(&format!("[Image: {}]", &after[..end]));
                rest = tail;
            }
            None => {
                out.push_str(&rest[..at + 1]);
                rest = &rest[at + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One `<w>` block as `write_groups` writes it.
struct Block {
    name: String,
    headword: String,
    variants: Vec<String>,
}

/// The blocks PyGlossary writes for one entry: its terms (the headword,
/// then its synonyms in .syn order) grouped by prefix, keyed by prefix.
fn blocks(dictionary: &Dictionary, entry: usize) -> Result<BTreeMap<String, Block>> {
    let index = dictionary.index();
    let main = dictionary.entry(entry)?.word.clone();
    let terms = std::iter::once(main.clone()).chain(
        index
            .synonyms
            .iter()
            .filter(|s| s.target as usize == entry)
            .map(|s| s.word.clone()),
    );
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for term in terms {
        let key = prefix(&term);
        match groups.iter_mut().find(|(p, _)| *p == key) {
            Some((_, words)) => words.push(term),
            None => groups.push((key, vec![term])),
        }
    }
    Ok(groups
        .into_iter()
        .map(|(key, words)| {
            let mut words = words.into_iter();
            let first = words.next().unwrap_or_default();
            let headword = if first == main {
                first
            } else {
                format!("{main}, {first}")
            };
            let name = headword.rsplit(", ").next().unwrap_or_default();
            let name = if name.chars().any(is_kana) {
                to_katakana(name)
            } else {
                name.into()
            };
            let variants = words.map(|v| v.trim().to_lowercase()).collect();
            (
                key,
                Block {
                    name,
                    headword,
                    variants,
                },
            )
        })
        .collect())
}

/// The blocks a Kobo shows for `word`: in `word`'s group file, those named
/// `word` or listing it as a variant, in dictionary order.
pub fn search(dictionary: &Dictionary, word: &str) -> Result<Outcome> {
    let key = prefix(word);
    let lower = word.trim().to_lowercase();
    let mut candidates = dictionary.lookup_exact(word);
    for hit in dictionary.lookup_ascii_case(word) {
        if !candidates.contains(&hit) {
            candidates.push(hit);
        }
    }
    candidates.sort_unstable();
    let mut results = Vec::new();
    let mut html = String::new();
    for entry in candidates {
        let Some(block) = blocks(dictionary, entry)?.remove(&key) else {
            continue;
        };
        if block.name != word && !block.variants.contains(&lower) {
            continue;
        }
        let variants: String = block
            .variants
            .iter()
            .map(|v| format!("<variant name=\"{v}\"/>"))
            .collect();
        html.push_str(&format!(
            "<w><a name=\"{}\" /><div><b>{}</b><var>{variants}</var><br/>{}</div></w>\n",
            block.name,
            block.headword,
            definition(&dictionary.payload(entry)?)
        ));
        results.push(Match {
            headword: block.headword,
            entry,
        });
    }
    if results.is_empty() {
        return Ok(Outcome::NotFound { word: word.into() });
    }
    Ok(Outcome::View(View {
        app: App::KoboPyglossary,
        query: word.into(),
        results,
        documents: vec![format!("<html>\n{html}</html>")],
        scroll_to: None,
    }))
}

/// What a Kobo does with a tapped link in a dictionary is not known.
pub fn follow() -> Outcome {
    Outcome::Stay {
        reason: Stay::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::tests::build;

    #[test]
    fn prefixes_follow_pyglossary() {
        for (word, expected) in [
            ("Run", "ru"),
            ("a", "aa"),
            ("x1", "11"),
            ("__mobi2star_x", "11"),
            ("m2s-abc", "11"),
            ("中文", "中"),
            ("ひら", "ヒラ"),
            ("Жук", "жу"),
            ("", "11"),
        ] {
            assert_eq!(prefix(word), expected, "{word}");
        }
    }

    #[test]
    fn images_become_text_only_when_src_comes_first() {
        assert_eq!(
            replace_images(
                r#"<img src="a.png"><img src="b.gif" alt="x"><img alt="y" src="c.png"><img src="d.png"/>"#
            ),
            r#"[Image: a.png][Image: b.gif]<img alt="y" src="c.png"><img src="d.png"/>"#
        );
    }

    #[test]
    fn definitions_are_trimmed_like_pyglossary() {
        assert_eq!(definition("  <p>x</p><br><BR> "), "<p>x</p>");
        assert_eq!(
            definition("<!DOCTYPE html> <html><body class=\"x\"><p>y</p></body></html>"),
            "<p>y</p>"
        );
    }

    #[test]
    fn synonyms_in_another_prefix_duplicate_the_entry() {
        let d = build(
            &[(1, "run", "<p>move fast</p>")],
            &[("runs", 1), ("ran", 1), ("__mobi2star_route", 1)],
            None,
            &[],
        );
        let groups = blocks(&d, 0).unwrap();
        assert_eq!(groups["ru"].variants, ["runs"]);
        assert_eq!(groups["ra"].headword, "run, ran");
        assert_eq!(groups["ra"].name, "ran");
        assert_eq!(
            groups["11"].name, "__mobi2star_route",
            "the internal key is its own block"
        );
        let Outcome::View(view) = search(&d, "ran").unwrap() else {
            panic!()
        };
        assert_eq!(
            view.documents[0],
            "<html>\n<w><a name=\"ran\" /><div><b>run, ran</b><var></var><br/><p>move fast</p></div></w>\n</html>"
        );
        assert!(
            matches!(search(&d, "runs").unwrap(), Outcome::View(v) if v.results[0].headword == "run")
        );
    }
}
