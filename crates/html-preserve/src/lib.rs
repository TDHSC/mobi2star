//! Attribute-level rewrites with byte provenance, never DOM reserialization.
#![forbid(unsafe_code)]
pub mod tokenizer;
use lexicon_core::{Document, Encoding, Entry, Error, Limits, Result, Span};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};
use tokenizer::{Attribute, Token, Tokenizer};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edit {
    pub span: Span,
    pub replacement: String,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InternalLink {
    pub attribute: Span,
    pub target_position: usize,
    pub target_entry: u64,
    pub href: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceLink {
    pub attribute: Span,
    pub recindex: u32,
    pub filename: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Plan {
    pub edits: Vec<Edit>,
    pub links: Vec<InternalLink>,
    pub resource_links: Vec<ResourceLink>,
    pub styles: Vec<Span>,
    pub external_links: usize,
}
#[derive(Clone)]
enum Target {
    Position(usize),
    Anchor(String),
}
struct Pending {
    attribute: Span,
    target: Target,
}

fn attr_value(a: &Attribute, raw: &[u8], encoding: Encoding) -> Result<String> {
    let span = a
        .value
        .ok_or_else(|| Error::Malformed(format!("attribute {} needs a value", a.name)))?;
    entities(&encoding.decode(span.bytes(raw)?)?)
}
fn entities(value: &str) -> Result<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        result.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        let Some(end) = rest.find(';').filter(|&n| n <= 32) else {
            result.push('&');
            continue;
        };
        let entity = &rest[..end];
        let ch = match entity {
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            "lt" => '<',
            "gt" => '>',
            "nbsp" => '\u{a0}',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                let n = u32::from_str_radix(&entity[2..], 16)
                    .map_err(|_| Error::Malformed("HTML hex entity".into()))?;
                char::from_u32(n)
                    .ok_or_else(|| Error::Malformed("invalid HTML codepoint".into()))?
            }
            _ if entity.starts_with('#') => {
                let n = entity[1..]
                    .parse::<u32>()
                    .map_err(|_| Error::Malformed("HTML numeric entity".into()))?;
                char::from_u32(n)
                    .ok_or_else(|| Error::Malformed("invalid HTML codepoint".into()))?
            }
            _ => {
                return Err(Error::Unsupported(format!(
                    "named entity &{entity}; in a routing attribute"
                )))
            }
        };
        if ch == '\0' {
            return Err(Error::Malformed("NUL entity in routing attribute".into()));
        }
        result.push(ch);
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    Ok(result)
}
fn check_css(text: &str) -> Result<()> {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if compact.contains("url(")
        || compact.contains("@import")
        || compact.contains('\\')
        || compact.contains("expression(")
        || compact.contains("behavior:")
        || compact.contains("-moz-binding")
    {
        return Err(Error::Unsupported(
            "CSS resources/escapes/active CSS need a semantic adapter; not stripped".into(),
        ));
    }
    Ok(())
}

pub fn build(doc: &Document, limits: &Limits) -> Result<Plan> {
    let raw = &doc.rawml;
    let mut plan = Plan::default();
    let mut anchors: BTreeMap<String, usize> = BTreeMap::new();
    let mut pending = Vec::new();
    let resources: BTreeMap<u32, _> = doc.resources.iter().map(|r| (r.recindex, r)).collect();
    let mut style_start = None;
    let boundaries: BTreeSet<usize> = doc
        .entries
        .iter()
        .flat_map(|e| [e.span.start, e.span.end])
        .collect();
    let mut tokens_seen = 0usize;
    for token in Tokenizer::new(raw) {
        let token = token?;
        tokens_seen += 1;
        if tokens_seen > limits.operations {
            return Err(Error::Limit("HTML token count".into()));
        }
        let span = token.span();
        if span.len() > 1 && boundaries.range(span.start + 1..span.end).next().is_some() {
            return Err(Error::Incomplete(format!(
                "entry boundary splits an HTML construct at {}..{}",
                span.start, span.end
            )));
        }
        match token {
            Token::Raw { name, span } => {
                if name == "style" {
                    check_css(&doc.encoding.decode(span.bytes(raw)?)?)?;
                }
            }
            Token::Opaque(_) => {}
            Token::Tag(tag) => {
                if tag.closing {
                    if tag.name == "style" {
                        let start = style_start
                            .take()
                            .ok_or_else(|| Error::Malformed("unmatched style close".into()))?;
                        plan.styles.push(Span {
                            start,
                            end: tag.span.end,
                        });
                    }
                    continue;
                }
                if [
                    "script", "iframe", "object", "embed", "audio", "video", "source", "svg",
                    "math", "canvas", "form", "input", "link", "base",
                ]
                .contains(&tag.name.as_str())
                {
                    return Err(Error::Unsupported(format!(
                        "HTML element <{}> has no validated StarDict adapter",
                        tag.name
                    )));
                }
                if tag.name == "meta"
                    && tag.attr("http-equiv").is_some_and(|a| {
                        attr_value(a, raw, doc.encoding)
                            .map(|v| v.eq_ignore_ascii_case("refresh"))
                            .unwrap_or(true)
                    })
                {
                    return Err(Error::Unsupported(
                        "HTML refresh/navigation directives".into(),
                    ));
                }
                if tag.name == "style" {
                    style_start = Some(tag.span.start);
                }
                for attr in &tag.attrs {
                    if attr.name.starts_with("on")
                        || ["srcset", "background", "hirecindex", "lorecindex"]
                            .contains(&attr.name.as_str())
                    {
                        return Err(Error::Unsupported(format!(
                            "HTML attribute {} cannot be silently removed",
                            attr.name
                        )));
                    }
                    if attr.name == "style" {
                        check_css(&attr_value(attr, raw, doc.encoding)?)?;
                    }
                    if attr.name == "id" || (tag.name == "a" && attr.name == "name") {
                        let value = attr_value(attr, raw, doc.encoding)?;
                        if value.starts_with("mobi2star-pos-") {
                            return Err(Error::Incomplete(
                                "source anchor collides with generated namespace".into(),
                            ));
                        }
                        if let Some(previous) = anchors.insert(value.clone(), tag.span.start) {
                            if previous != tag.span.start {
                                return Err(Error::Incomplete(format!(
                                    "duplicate source anchor {value:?}"
                                )));
                            }
                        }
                    }
                }
                if let Some(attr) = tag.attr("filepos") {
                    if tag.name != "a" || tag.attr("href").is_some() {
                        return Err(Error::Unsupported(
                            "filepos on a non-anchor or with a competing href".into(),
                        ));
                    }
                    let target = attr_value(attr, raw, doc.encoding)?
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| {
                            Error::Malformed("filepos must be a decimal byte offset".into())
                        })?;
                    pending.push(Pending {
                        attribute: attr.span,
                        target: Target::Position(target),
                    });
                }
                if let Some(attr) = tag.attr("href") {
                    let href = attr_value(attr, raw, doc.encoding)?;
                    if let Some(id) = href.strip_prefix('#') {
                        pending.push(Pending {
                            attribute: attr.span,
                            target: Target::Anchor(id.to_owned()),
                        });
                    } else if ["https://", "http://", "mailto:"]
                        .iter()
                        .any(|s| href.to_ascii_lowercase().starts_with(s))
                    {
                        plan.external_links += 1;
                    } else {
                        return Err(Error::Unsupported(format!(
                            "unresolved/nonportable href {href:?}"
                        )));
                    }
                }
                if let Some(attr) = tag.attr("recindex") {
                    if tag.name != "img" || tag.attr("src").is_some() {
                        return Err(Error::Unsupported(
                            "recindex requires an image without a competing src".into(),
                        ));
                    }
                    let number = attr_value(attr, raw, doc.encoding)?
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| Error::Malformed("invalid image recindex".into()))?;
                    let resource = resources.get(&number).ok_or_else(|| {
                        Error::Incomplete(format!(
                            "referenced resource {number} missing or not decoded"
                        ))
                    })?;
                    plan.edits.push(Edit {
                        span: attr.span,
                        replacement: format!("src=\"{}\"", resource.filename),
                        reason: "resource_reference".into(),
                    });
                    plan.resource_links.push(ResourceLink {
                        attribute: attr.span,
                        recindex: number,
                        filename: resource.filename.clone(),
                    });
                }
                if let Some(attr) = tag.attr("src") {
                    let value = attr_value(attr, raw, doc.encoding)?;
                    if tag.name != "img"
                        || ![
                            "data:image/png;base64,",
                            "data:image/jpeg;base64,",
                            "data:image/gif;base64,",
                        ]
                        .iter()
                        .any(|s| value.starts_with(s))
                    {
                        return Err(Error::Unsupported(format!("unresolved/remote resource src {value:?}; no network fetches are performed")));
                    }
                }
            }
        }
    }
    if style_start.is_some() {
        return Err(Error::Malformed("unclosed style element".into()));
    }
    let mut resolved = Vec::new();
    let mut positions = BTreeSet::new();
    for link in pending {
        let target = match link.target {
            Target::Position(p) => p,
            Target::Anchor(id) => *anchors
                .get(&id)
                .ok_or_else(|| Error::Incomplete(format!("unresolved anchor #{id}")))?,
        };
        if target >= raw.len() {
            return Err(Error::Incomplete(format!(
                "link target {target} outside text"
            )));
        }
        positions.insert(target);
        resolved.push((link.attribute, target));
    }
    // Validate insertion positions in a separate linear scan, not by guessing a nearby tag.
    for token in Tokenizer::new(raw) {
        let token = token?;
        let span = token.span();
        if (span.len() > 1 && positions.range(span.start + 1..span.end).next().is_some())
            || (matches!(token, Token::Raw { .. }) && positions.contains(&span.start))
        {
            return Err(Error::Incomplete(format!(
                "link points inside HTML syntax/raw-text at byte {}",
                span.start
            )));
        }
    }
    let owners = owners_at(&doc.entries, &positions)?;
    for &position in &positions {
        if doc.encoding == Encoding::Utf8 && raw.get(position).is_some_and(|b| b & 0xc0 == 0x80) {
            return Err(Error::Incomplete("link splits a UTF-8 character".into()));
        }
        plan.edits.push(Edit {
            span: Span {
                start: position,
                end: position,
            },
            replacement: format!("<a id=\"mobi2star-pos-{position}\"></a>"),
            reason: "exact_target_anchor".into(),
        });
    }
    for (attribute, target) in resolved {
        let id = owners[&target];
        let href = format!(
            "bword://{}#mobi2star-pos-{target}",
            lexicon_core::routing_key(&doc.namespace, id)
        );
        plan.edits.push(Edit {
            span: attribute,
            replacement: format!("href=\"{href}\""),
            reason: "internal_reference".into(),
        });
        plan.links.push(InternalLink {
            attribute,
            target_position: target,
            target_entry: id,
            href,
        });
    }
    plan.edits.sort_by_key(|e| (e.span.start, e.span.end));
    for pair in plan.edits.windows(2) {
        if pair[0].span.end > pair[1].span.start {
            return Err(Error::Incomplete("overlapping HTML edits".into()));
        }
    }
    Ok(plan)
}

/// Sweep-line interval ownership: O((entries + targets) log entries), including overlaps.
fn owners_at(entries: &[Entry], positions: &BTreeSet<usize>) -> Result<BTreeMap<usize, u64>> {
    let mut by_start: Vec<usize> = (0..entries.len()).collect();
    by_start.sort_by_key(|&i| entries[i].span.start);
    let mut ends = BinaryHeap::new();
    let mut active = BTreeSet::new();
    let mut p = 0;
    let mut result = BTreeMap::new();
    for &target in positions {
        while p < by_start.len() && entries[by_start[p]].span.start <= target {
            let i = by_start[p];
            let e = &entries[i];
            active.insert((e.span.len(), e.id, i));
            ends.push(Reverse((e.span.end, i)));
            p += 1;
        }
        while let Some(&Reverse((end, i))) = ends.peek() {
            if end > target {
                break;
            }
            ends.pop();
            active.remove(&(entries[i].span.len(), entries[i].id, i));
        }
        let &(_, id, _) = active
            .first()
            .ok_or_else(|| Error::Incomplete(format!("no entry owns link target {target}")))?;
        result.insert(target, id);
    }
    Ok(result)
}

pub fn render(doc: &Document, entry: &Entry, plan: &Plan) -> Result<String> {
    render_fragment(&doc.rawml, doc.encoding, entry.span, plan)
}
pub fn render_fragment(raw: &[u8], encoding: Encoding, span: Span, plan: &Plan) -> Result<String> {
    span.bytes(raw)?;
    let mut result = String::new();
    // Global style bytes are copied, never rewritten. This cannot certify renderer equivalence.
    for style in &plan.styles {
        if !(span.start <= style.start && style.end <= span.end) {
            result.push_str(&encoding.decode(style.bytes(raw)?)?);
        }
    }
    let first = plan.edits.partition_point(|e| e.span.start < span.start);
    let mut cursor = span.start;
    for edit in plan.edits[first..]
        .iter()
        .take_while(|e| e.span.start < span.end)
    {
        if edit.span.start < cursor || edit.span.end > span.end {
            return Err(Error::Incomplete("edit crosses entry boundary".into()));
        }
        result.push_str(&encoding.decode(&raw[cursor..edit.span.start])?);
        result.push_str(&edit.replacement);
        cursor = edit.span.end;
    }
    result.push_str(&encoding.decode(&raw[cursor..span.end])?);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entities_keep_nonascii_and_decode_positions() {
        assert_eq!(entities("é&#233;&amp;中").unwrap(), "éé&中");
        assert!(entities("&#0;").is_err());
    }
    #[test]
    fn untouched_bytes_are_not_reserialized() {
        let raw = b"<B class='A'>a &amp; b</B>";
        assert_eq!(
            render_fragment(
                raw,
                Encoding::Utf8,
                Span {
                    start: 0,
                    end: raw.len()
                },
                &Plan::default()
            )
            .unwrap()
            .as_bytes(),
            raw
        );
    }
    #[test]
    fn byte_based_insertion_after_multibyte_text() {
        let raw = "中<a filepos='0'>x</a>".as_bytes();
        let plan = Plan {
            edits: vec![Edit {
                span: Span { start: 3, end: 3 },
                replacement: "!".into(),
                reason: "test".into(),
            }],
            ..Plan::default()
        };
        assert!(render_fragment(
            raw,
            Encoding::Utf8,
            Span {
                start: 0,
                end: raw.len()
            },
            &plan
        )
        .unwrap()
        .starts_with("中!<a"));
    }
}
