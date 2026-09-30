//! Span-level edits of entry HTML shared by the readers. Rewrites keep
//! every byte they do not change; when the strict tokenizer rejects an
//! entry, it is left as it is.
use html_preserve::tokenizer::{Token, Tokenizer};
use lexicon_core::bytes::image_type;
use std::collections::BTreeSet;

/// Element ids and `<a name>` values: the places a `#fragment` can land.
pub fn anchors(html: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for token in Tokenizer::new(html.as_bytes()) {
        let Ok(Token::Tag(tag)) = token else {
            continue;
        };
        let names = tag
            .attrs
            .iter()
            .filter(|a| a.name == "id" || (tag.name == "a" && a.name == "name"));
        for attr in names {
            if let Some(value) = attr.value.and_then(|span| html.get(span.start..span.end)) {
                found
                    .insert(html_preserve::decode_entities(value).unwrap_or_else(|_| value.into()));
            }
        }
    }
    found
}

/// Replaces spans of `html`, which must be sorted and disjoint.
fn splice(html: &str, edits: Vec<(usize, usize, String)>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    for (start, end, replacement) in edits {
        out.push_str(&html[at..start]);
        out.push_str(&replacement);
        at = end;
    }
    out.push_str(&html[at..]);
    out
}

/// A `data:` URI for an image file, or `None` if it is not an image.
pub fn data_uri(bytes: &[u8]) -> Option<String> {
    let (_, media_type) = image_type(bytes)?;
    Some(format!("data:{media_type};base64,{}", base64(bytes)))
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

/// Rewrites every `<img src>` that `resolve` maps to an image file into a
/// `data:` URI of that file. `resolve` receives the attribute value with
/// character references decoded, and applies the reader's own rules.
pub fn inline_images<'a>(html: &str, resolve: impl Fn(&str) -> Option<&'a [u8]>) -> String {
    let mut edits = Vec::new();
    for token in Tokenizer::new(html.as_bytes()) {
        let Ok(token) = token else {
            return html.to_owned();
        };
        let Token::Tag(tag) = token else { continue };
        if tag.name != "img" || tag.closing {
            continue;
        }
        let Some(span) = tag.attr("src").and_then(|a| a.value) else {
            continue;
        };
        let Some(raw) = html.get(span.start..span.end) else {
            continue;
        };
        let src = html_preserve::decode_entities(raw).unwrap_or_else(|_| raw.into());
        if let Some(uri) = resolve(&src).and_then(data_uri) {
            edits.push((span.start, span.end, uri));
        }
    }
    splice(html, edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nxyz";

    #[test]
    fn anchors_are_ids_and_link_names() {
        let found =
            anchors(r#"<p id="a">x</p><a name="b"></a><div name="c"></div><a id="d&amp;e">"#);
        assert_eq!(found.into_iter().collect::<Vec<_>>(), ["a", "b", "d&e"]);
    }

    #[test]
    fn images_become_data_uris_when_resolved() {
        let html =
            r#"<p><img alt="x" src="res/a.png"/><img src='missing.gif'><IMG SRC="a%20b.png"></p>"#;
        let out = inline_images(html, |src| match src {
            "res/a.png" | "a%20b.png" => Some(PNG),
            _ => None,
        });
        let uri = data_uri(PNG).unwrap();
        assert_eq!(
            out,
            format!(r#"<p><img alt="x" src="{uri}"/><img src='missing.gif'><IMG SRC="{uri}"></p>"#)
        );
        assert!(uri.starts_with("data:image/png;base64,iVBORw0KGgp4eXo"));
        assert_eq!(
            inline_images("<img src=\"a", |_| Some(PNG)),
            "<img src=\"a",
            "unparsable stays"
        );
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }
}
