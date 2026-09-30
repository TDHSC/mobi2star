//! Attribute-level rewrites with byte provenance, never DOM reserialization.
#![forbid(unsafe_code)]
pub mod css;
pub mod tokenizer;
use lexicon_core::{Document, Encoding, Entry, Error, Limits, Result, Span, StyleDelivery};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
};
use tokenizer::{Attribute, Tag, Token, Tokenizer};

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
/// A source `<style>` element. Inline payloads copy `element` verbatim; the
/// dictionary stylesheet file takes the `css` body.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StyleBlock {
    pub element: Span,
    pub css: Span,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Plan {
    pub edits: Vec<Edit>,
    pub links: Vec<InternalLink>,
    pub resource_links: Vec<ResourceLink>,
    pub styles: Vec<StyleBlock>,
    pub external_links: usize,
    /// Class of the wrapper that styled payloads are placed in.
    pub scope: String,
    /// The dictionary stylesheet: every `<style>` body in document order,
    /// scoped under `scope`. Empty when the book has no CSS.
    pub stylesheet: String,
}
impl Plan {
    /// Each distinct internal link as the key a reader looks up and the entry
    /// it points into. KOReader looks up everything after `bword://`,
    /// `#fragment` included, so every one needs its own alias.
    pub fn link_aliases(&self) -> BTreeSet<(&str, u64)> {
        self.links
            .iter()
            .filter_map(|link| {
                let key = link.href.strip_prefix("bword://")?;
                Some((key, link.target_entry))
            })
            .collect()
    }
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
    decode_entities(&encoding.decode(span.bytes(raw)?)?)
}
/// Decodes the character references in an attribute value.
pub fn decode_entities(value: &str) -> Result<String> {
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
/// The body of a `<style>` moves into the shared stylesheet, so only
/// attributes whose meaning survives that move are accepted:
/// - ones that merely identify or annotate the element (id, class, lang, dir,
///   nonce);
/// - a CSS `type`, empty or `text/css`;
/// - a `media` that applies on screen, empty or `all` or `screen`.
///
/// Anything else, such as `title` (alternate stylesheets), `disabled` or print
/// media, would change what the body does, so it is an error.
fn check_style_element(tag: &Tag, raw: &[u8], encoding: Encoding) -> Result<()> {
    for attr in &tag.attrs {
        let name = attr.name.as_str();
        if matches!(name, "id" | "class" | "lang" | "xml:lang" | "dir" | "nonce") {
            continue;
        }
        let value = match attr.value {
            Some(_) => attr_value(attr, raw, encoding)?.trim().to_ascii_lowercase(),
            None => String::new(),
        };
        let supported = match name {
            "type" => value.is_empty() || value == "text/css",
            "media" => matches!(value.as_str(), "" | "all" | "screen"),
            _ => false,
        };
        if !supported {
            return Err(Error::Unsupported(format!(
                "<style {name}=\"{value}\"> needs a stylesheet adapter"
            )));
        }
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
    let mut style_css = None;
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
                    style_css = Some(span);
                }
            }
            Token::Opaque(_) => {}
            Token::Tag(tag) => {
                if tag.closing {
                    if tag.name == "style" {
                        let start = style_start
                            .take()
                            .ok_or_else(|| Error::Malformed("unmatched style close".into()))?;
                        let element = Span {
                            start,
                            end: tag.span.end,
                        };
                        if boundaries
                            .range(element.start + 1..element.end)
                            .next()
                            .is_some()
                        {
                            return Err(Error::Incomplete(
                                "entry boundary splits a <style> element".into(),
                            ));
                        }
                        let empty = Span {
                            start: tag.span.start,
                            end: tag.span.start,
                        };
                        plan.styles.push(StyleBlock {
                            element,
                            css: style_css.take().unwrap_or(empty),
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
                    check_style_element(&tag, raw, doc.encoding)?;
                    style_start = Some(tag.span.start);
                    style_css = None;
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
    // Each body is scoped on its own, so it is a complete rule list: text left
    // over at the end of one body can never join the next body's selector.
    plan.scope = css::scope_class(&doc.namespace);
    for style in &plan.styles {
        let body = doc.encoding.decode(style.css.bytes(raw)?)?;
        let scoped = css::scope(&body, &plan.scope, css::Grammar::Open)?;
        if !scoped.is_empty() {
            plan.stylesheet.push_str(&scoped);
            plan.stylesheet.push('\n');
        }
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

pub fn render(
    doc: &Document,
    entry: &Entry,
    plan: &Plan,
    delivery: StyleDelivery,
) -> Result<String> {
    render_fragment(&doc.rawml, doc.encoding, entry.span, plan, delivery)
}
pub fn render_fragment(
    raw: &[u8],
    encoding: Encoding,
    span: Span,
    plan: &Plan,
    delivery: StyleDelivery,
) -> Result<String> {
    span.bytes(raw)?;
    let mut result = String::new();
    // Styled payloads sit in the wrapper the scoped stylesheet targets. A
    // <style> element inside the entry stays there as source bytes.
    let styled = !plan.stylesheet.is_empty();
    if styled {
        result.push_str(&delivery.references(&plan.stylesheet));
        result.push_str(&format!("<div class=\"{}\">", plan.scope));
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
    if styled {
        result.push_str("</div>");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lexicon_core::{EntryKind, Metadata};
    fn doc(raw: &str, entries: &[(usize, usize)]) -> Document {
        Document {
            namespace: "0".repeat(64),
            source: Vec::new(),
            rawml: raw.as_bytes().to_vec(),
            encoding: Encoding::Utf8,
            metadata: Metadata::default(),
            records: Vec::new(),
            entries: entries
                .iter()
                .enumerate()
                .map(|(i, &(start, end))| Entry {
                    id: i as u64,
                    headword: format!("w{i}"),
                    aliases: Vec::new(),
                    span: Span { start, end },
                    kind: EntryKind::Headword,
                })
                .collect(),
            resources: Vec::new(),
            source_headwords: entries.len(),
            source_aliases: 0,
            index_audit: Default::default(),
        }
    }
    fn plan_for(styles: &str) -> Result<(Document, Plan)> {
        let raw = format!("<html><head>{styles}</head><body><p>one</p></body></html>");
        let body = raw.find("<body>").unwrap();
        let doc = doc(&raw, &[(0, body), (body, raw.len())]);
        let plan = build(&doc, &Limits::default())?;
        Ok((doc, plan))
    }
    fn render_all(doc: &Document, plan: &Plan, delivery: StyleDelivery) -> String {
        let span = Span {
            start: 0,
            end: doc.rawml.len(),
        };
        render_fragment(&doc.rawml, doc.encoding, span, plan, delivery).unwrap()
    }
    #[test]
    fn style_bodies_form_the_scoped_stylesheet_in_document_order() {
        let (doc, plan) = plan_for(
            "<style type=\"text/css\">.a{color:red}</style><style media=\"screen\">body .b{x:\"}\"}</style><style></style>",
        )
        .unwrap();
        assert_eq!(plan.styles.len(), 3);
        assert_eq!(
            plan.styles[0].element.bytes(&doc.rawml).unwrap(),
            b"<style type=\"text/css\">.a{color:red}</style>"
        );
        let class = &plan.scope;
        assert_eq!(
            plan.stylesheet,
            format!(".{class} .a{{color:red}}\n.{class} .b{{x:\"}}\"}}\n")
        );
        // Styled payloads sit in the wrapper the stylesheet targets.
        let html = render_all(&doc, &plan, StyleDelivery::INLINE);
        assert!(html.starts_with(&format!(
            "<style>{}</style><div class=\"{class}\">",
            plan.stylesheet
        )));
        assert!(html.ends_with("</div>"));
    }
    #[test]
    fn leftover_text_in_one_body_never_joins_the_next_selector() {
        let (_, plan) = plan_for("<style>.x{a:b} h1</style><style>p{color:red}</style>").unwrap();
        let class = &plan.scope;
        assert_eq!(
            plan.stylesheet,
            format!(".{class} .x{{a:b}}\n.{class} p{{color:red}}\n")
        );
        assert!(!plan.stylesheet.contains("h1"));
    }
    #[test]
    fn blank_styles_give_no_stylesheet_link_or_wrapper() {
        let (doc, plan) = plan_for("<style>  \n </style><style></style>").unwrap();
        assert!(plan.stylesheet.is_empty());
        let both = StyleDelivery {
            link: true,
            inline: true,
        };
        assert_eq!(
            render_all(&doc, &plan, both).as_bytes(),
            doc.rawml.as_slice()
        );
    }
    #[test]
    fn meaning_neutral_style_attributes_are_accepted() {
        let (_, plan) = plan_for(
            "<style id=\"main\" class=\"c\" lang=\"en\" dir=\"ltr\" type=\"\" media=\"\">.a{b:c}</style>",
        )
        .unwrap();
        assert_eq!(plan.stylesheet, format!(".{} .a{{b:c}}\n", plan.scope));
    }
    #[test]
    fn styles_that_cannot_join_a_stylesheet_fail_closed() {
        for styles in [
            "<style media=\"print\">.a{}</style>",
            "<style title=\"alt\">.a{}</style>",
            "<style disabled=\"\">.a{}</style>",
            "<style type=\"text/less\">.a{}</style>",
            "<style>.a{}}</style>",
            "<style>.a{content:\"x}</style>",
            "<style>@layer base{p{}}</style>",
        ] {
            assert!(plan_for(styles).is_err(), "{styles}");
        }
        // An entry boundary between <style> and its body splits the element.
        let raw = "<style>.a{}</style><p>x</p>";
        assert!(build(&doc(raw, &[(0, 7), (7, raw.len())]), &Limits::default()).is_err());
    }
    #[test]
    fn entities_keep_nonascii_and_decode_positions() {
        assert_eq!(decode_entities("é&#233;&amp;中").unwrap(), "éé&中");
        assert!(decode_entities("&#0;").is_err());
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
                &Plan::default(),
                StyleDelivery::INLINE
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
            &plan,
            StyleDelivery::INLINE
        )
        .unwrap()
        .starts_with("中!<a"));
    }
}
