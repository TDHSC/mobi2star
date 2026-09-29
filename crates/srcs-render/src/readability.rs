//! Collins publisher-profile presentation. All boundaries are between siblings or
//! inside character-data; source tags, spelling, IPA, links and lookup facts stay
//! available for byte replay. Source <br> elements at these boundaries become
//! inert spans retaining their attributes, avoiding MuPDF's extra blank lines.
use crate::Edit;
use html_preserve::tokenizer::{Tag, Token, Tokenizer};
use lexicon_core::{Error, Result, Span};
use serde::{Deserialize, Serialize};
use srcs_reader::markup::{entities, utf8};
use std::collections::BTreeMap;

pub const CSS: &str = include_str!("readable.css");
pub const PROFILE: &str = "collins-readable-v2";

/// Selection is explicit and conservative: arbitrary books keep source layout.
/// `title` is the OPF title, which in the retail Kindle edition is
/// "COBUILD Advanced Learner's Dictionary" without "Collins"; the stylesheet
/// signature is what identifies the publisher's markup.
pub fn applies(title: &str, stylesheets: &[&str]) -> bool {
    title.contains("COBUILD")
        && stylesheets.iter().any(|css| {
            css.contains("amzn-mobi")
                && [".hw", ".hwtxt", ".entry", "span.ex"]
                    .iter()
                    .all(|part| css.contains(part))
        })
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub headers: usize,
    pub senses: usize,
    pub examples: usize,
    pub runons: usize,
    pub paragraphs: usize,
    pub source_breaks: usize,
    pub records: usize,
    /// Headword-level POS labels included in the header; numbered sense labels
    /// retain their original ownership.
    pub header_pos: usize,
    /// Structural source markers retained in hidden presentation spans.
    pub hidden_markers: usize,
}
#[derive(Clone, Debug)]
enum Part {
    Text(Span),
    Element(usize),
    Opaque(Span),
}
#[derive(Clone, Debug)]
struct Node {
    tag: Tag,
    close: usize,
    end: usize,
    parts: Vec<Part>,
}
impl Node {
    fn class(&self, raw: &[u8], class: &str) -> Result<bool> {
        Ok(self
            .tag
            .attr("class")
            .and_then(|a| a.value)
            .map(|s| {
                Ok::<_, Error>(
                    entities(utf8(s.bytes(raw)?)?)?
                        .split_whitespace()
                        .any(|c| c == class),
                )
            })
            .transpose()?
            .unwrap_or(false))
    }
}
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "div"
            | "p"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "ul"
            | "ol"
            | "blockquote"
            | "figure"
            | "idx:entry"
    )
}
fn parse(raw: &[u8]) -> Result<Vec<Node>> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cursor = 0;
    for token in Tokenizer::new(raw) {
        let token = token?;
        let span = token.span();
        if let Some(&id) = stack.last() {
            if cursor < span.start {
                nodes[id].parts.push(Part::Text(Span {
                    start: cursor,
                    end: span.start,
                }));
            }
        }
        cursor = span.end;
        match token {
            Token::Tag(tag) if tag.closing => {
                let id = stack
                    .pop()
                    .ok_or_else(|| Error::Malformed("layout: unmatched closing tag".into()))?;
                if nodes[id].tag.name != tag.name {
                    return Err(Error::Malformed("layout: mismatched tag".into()));
                }
                nodes[id].close = span.start;
                nodes[id].end = span.end;
            }
            Token::Tag(tag) => {
                let empty = is_void(&tag.name) || span.bytes(raw)?.ends_with(b"/>");
                let id = nodes.len();
                nodes.push(Node {
                    tag,
                    close: span.end,
                    end: span.end,
                    parts: Vec::new(),
                });
                if let Some(&parent) = stack.last() {
                    nodes[parent].parts.push(Part::Element(id));
                }
                if !empty {
                    if stack.len() >= 256 {
                        return Err(Error::Limit("layout nesting".into()));
                    }
                    stack.push(id);
                }
            }
            Token::Opaque(s) => {
                if let Some(&id) = stack.last() {
                    nodes[id].parts.push(Part::Opaque(s));
                }
            }
            Token::Raw { span: s, .. } => {
                if let Some(&id) = stack.last() {
                    nodes[id].parts.push(Part::Text(s));
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err(Error::Malformed("layout: unclosed tag".into()));
    }
    Ok(nodes)
}
fn bounds(part: &Part, nodes: &[Node]) -> Span {
    match part {
        Part::Text(s) | Part::Opaque(s) => *s,
        Part::Element(id) => Span {
            start: nodes[*id].tag.span.start,
            end: nodes[*id].end,
        },
    }
}
fn text(part: &Part, nodes: &[Node], raw: &[u8]) -> Result<String> {
    match part {
        Part::Opaque(_) => Ok(String::new()),
        Part::Text(s) => entities(utf8(s.bytes(raw)?)?),
        Part::Element(id) => {
            let mut out = String::new();
            for p in &nodes[*id].parts {
                out.push_str(&text(p, nodes, raw)?);
            }
            Ok(out)
        }
    }
}
#[derive(Default)]
struct Batch {
    end: usize,
    replacement: String,
    reasons: Vec<String>,
}
type Batches = BTreeMap<usize, Batch>;
fn insert(out: &mut Batches, at: usize, value: &str, reason: &str) {
    let batch = out.entry(at).or_insert_with(|| Batch {
        end: at,
        ..Batch::default()
    });
    batch.replacement.push_str(value);
    batch.reasons.push(reason.into());
}
/// A close-tag must precede a paragraph boundary at the same source offset.
fn prepend(out: &mut Batches, at: usize, value: &str, reason: &str) {
    let batch = out.entry(at).or_insert_with(|| Batch {
        end: at,
        ..Batch::default()
    });
    batch.replacement.insert_str(0, value);
    batch.reasons.insert(0, reason.into());
}

/// Publisher POS inventory. Usage / register labels (e.g. APPROVAL, INFORMAL)
/// keep their original semantic location. A spelling variant uses the same rule.
fn is_pos_label(value: &str) -> bool {
    let value = value.trim().replace(['\u{2010}', '\u{2011}'], "-");
    matches!(
        value.as_str(),
        "ADJ"
            | "ADJ-GRADED"
            | "ADV"
            | "ADV-GRADED"
            | "VERB"
            | "V"
            | "N"
            | "NOUN"
            | "N-COUNT"
            | "N-UNCOUNT"
            | "N-SING"
            | "N-PLURAL"
            | "N-VAR"
            | "N-PROPER"
            | "N-MASS"
            | "PRON"
            | "PRON-SING"
            | "PRON-PLURAL"
            | "DET"
            | "CONJ"
            | "COLOUR"
            | "COLOR"
            | "PREP"
            | "PHRASE"
            | "EXCLAM"
            | "NUM"
            | "ORD"
            | "ORDINAL"
            | "QUANT"
            | "PREFIX"
            | "SUFFIX"
            | "AUX"
            | "MODAL"
            | "MODAL VERB"
            | "LINK VERB"
            | "PHRASAL VERB"
    )
}

/// Recognize only the marker at an already identified structural boundary.
/// Return its original bytes plus separator whitespace; preserve every byte.
fn structural_marker(raw: &[u8], start: usize, limit: usize) -> Result<Option<Span>> {
    let value = utf8(&raw[start..limit])?;
    let markers = ["□", "●", "&#9633;", "&#9679;", "&#x25a1;", "&#x25cf;"];
    let Some(mark) = markers.iter().find(|mark| {
        value
            .get(..mark.len())
            .is_some_and(|v| v.eq_ignore_ascii_case(mark))
    }) else {
        return Ok(None);
    };
    let mut len = mark.len();
    loop {
        let rest = &value[len..];
        if let Some(c) = rest.chars().next().filter(|c| c.is_whitespace()) {
            len += c.len_utf8();
            continue;
        }
        if let Some(entity) = ["&nbsp;", "&#160;", "&#xa0;"].iter().find(|entity| {
            rest.get(..entity.len())
                .is_some_and(|v| v.eq_ignore_ascii_case(entity))
        }) {
            len += entity.len();
            continue;
        }
        break;
    }
    Ok(Some(Span {
        start,
        end: start + len,
    }))
}

fn add_class(out: &mut Batches, node: &Node, raw: &[u8], class: &str) -> Result<()> {
    if let Some(value) = node.tag.attr("class").and_then(|a| a.value) {
        insert(out, value.end, &format!(" {class}"), "layout_class");
    } else {
        let mut at = node.tag.span.end - 1;
        while raw[at - 1].is_ascii_whitespace() {
            at -= 1;
        }
        if raw[at - 1] == b'/' {
            at -= 1;
        }
        insert(out, at, &format!(" class=\"{class}\""), "layout_class");
    }
    Ok(())
}
fn break_to_span(out: &mut Batches, node: &Node, raw: &[u8]) -> Result<()> {
    let mut attrs = utf8(&raw[node.tag.span.start + 3..node.tag.span.end - 1])?
        .trim_end()
        .to_owned();
    if attrs.ends_with('/') {
        attrs.pop();
    }
    if let Some(value) = node.tag.attr("class").and_then(|a| a.value) {
        attrs.insert_str(value.end - node.tag.span.start - 3, " m2s-source-break");
    } else {
        attrs.push_str(" class=\"m2s-source-break\"");
    }
    insert(
        out,
        node.tag.span.start,
        &format!("<span{attrs} data-m2s-break=\"1\"></span>"),
        "layout_break_to_span",
    );
    out.get_mut(&node.tag.span.start).expect("inserted").end = node.end;
    Ok(())
}
fn marker_boundaries(value: &str, start: usize, out: &mut BTreeMap<usize, &'static str>) {
    // Match character-data only. Equivalent numeric references keep their bytes.
    for (offset, _) in value.char_indices() {
        let suffix = &value[offset..];
        for (mark, kind) in [
            ("□", "example"),
            ("●", "runon"),
            ("&#9633;", "example"),
            ("&#9679;", "runon"),
            ("&#x25a1;", "example"),
            ("&#x25cf;", "runon"),
        ] {
            if suffix
                .get(..mark.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(mark))
            {
                let tail = &suffix[mark.len()..];
                if tail.chars().next().is_some_and(char::is_whitespace)
                    || ["&nbsp;", "&#160;", "&#xa0;"].iter().any(|s| {
                        tail.get(..s.len())
                            .is_some_and(|p| p.eq_ignore_ascii_case(s))
                    })
                {
                    out.insert(start + offset, kind);
                }
            }
        }
    }
}
/// Layout-only source edits plus audit counts. All other source bytes replay intact.
pub fn edits(raw: &[u8], browser_body_class: bool) -> Result<(Vec<Edit>, Counts)> {
    utf8(raw)?;
    let nodes = parse(raw)?;
    let mut out = Batches::new();
    let mut counts = Counts::default();
    for node in &nodes {
        if node.class(raw, "m2s-readable")? || node.class(raw, "m2s-record")? {
            return Err(Error::Incomplete(
                "layout already adapted or reserved class collision".into(),
            ));
        }
    }
    for node in &nodes {
        if node.tag.name == "idx:entry" {
            add_class(&mut out, node, raw, "m2s-record")?;
            counts.records += 1;
        }
        if browser_body_class && node.tag.name == "body" {
            add_class(&mut out, node, raw, "m2s-readable")?;
        }
        let target = node.tag.name == "td"
            || (node.tag.name == "div"
                && (node.class(raw, "entry")? || node.class(raw, "entrysect")?));
        if !target {
            if node.tag.name == "idx:entry" {
                for (i, part) in node.parts.iter().enumerate() {
                    let Part::Element(id) = part else {
                        continue;
                    };
                    if nodes[*id].tag.name != "br" {
                        continue;
                    }
                    let mut neighbors = Vec::new();
                    for step in [-1isize, 1] {
                        let mut j = i as isize + step;
                        let mut block = false;
                        while j >= 0 && (j as usize) < node.parts.len() {
                            let p = &node.parts[j as usize];
                            let ignore = match p {
                                Part::Opaque(_) => true,
                                Part::Text(_) => text(p, &nodes, raw)?.trim().is_empty(),
                                Part::Element(n) => {
                                    nodes[*n].tag.name == "br"
                                        || (nodes[*n].tag.name == "a"
                                            && text(p, &nodes, raw)?.trim().is_empty())
                                }
                            };
                            if ignore {
                                j += step;
                                continue;
                            }
                            if let Part::Element(n) = p {
                                block = is_block(&nodes[*n].tag.name);
                            }
                            break;
                        }
                        neighbors.push(block);
                    }
                    if neighbors.iter().all(|v| *v) {
                        break_to_span(&mut out, &nodes[*id], raw)?;
                        counts.source_breaks += 1;
                    }
                }
            }
            continue;
        }
        if node.parts.is_empty() {
            continue;
        }
        let mut cuts: BTreeMap<usize, &'static str> =
            BTreeMap::from([(node.tag.span.end, "paragraph")]);
        let mut barriers: Vec<Span> = Vec::new();
        let mut hw = false;
        let mut header_end = None;
        let mut header_pos: Option<Span> = None;
        for (i, part) in node.parts.iter().enumerate() {
            match part {
                Part::Text(s) => marker_boundaries(utf8(s.bytes(raw)?)?, s.start, &mut cuts),
                Part::Opaque(_) => {}
                Part::Element(id) => {
                    let child = &nodes[*id];
                    let name = child.tag.name.as_str();
                    if child.class(raw, "hw")? {
                        hw = true;
                    }
                    if hw
                        && header_end.is_none()
                        && (matches!(name, "small" | "br") || is_block(name))
                    {
                        // Include a direct, unbracketed POS in the headword line.
                        // Numbered senses, block boundaries and grammar labels
                        // keep their existing scope.
                        let mut bracketed = false;
                        for previous in node.parts[..i].iter().rev() {
                            if let Part::Text(_) = previous {
                                let value = text(previous, &nodes, raw)?;
                                if value.trim().is_empty() {
                                    continue;
                                }
                                bracketed = value.trim_end().ends_with('[');
                            }
                            if !matches!(previous, Part::Opaque(_)) {
                                break;
                            }
                        }
                        if name == "small" && !bracketed && is_pos_label(&text(part, &nodes, raw)?)
                        {
                            header_end = Some(child.end);
                            header_pos = Some(bounds(part, &nodes));
                        } else {
                            header_end = Some(child.tag.span.start);
                        }
                    }
                    if name == "br" || is_block(name) {
                        let span = bounds(part, &nodes);
                        barriers.push(span);
                        cuts.insert(span.start, "barrier");
                        cuts.insert(span.end, "paragraph");
                        if name == "br" {
                            break_to_span(&mut out, child, raw)?;
                            counts.source_breaks += 1;
                        }
                    }
                    if name == "b" {
                        let value = text(part, &nodes, raw)?;
                        let value = value.trim();
                        if !value.is_empty()
                            && value.len() <= 3
                            && value.bytes().all(|c| c.is_ascii_digit())
                        {
                            let mut prev = None;
                            for p in node.parts[..i].iter().rev() {
                                let ignore = match p {
                                    Part::Opaque(_) => true,
                                    Part::Text(_) => text(p, &nodes, raw)?.trim().is_empty(),
                                    Part::Element(n) => nodes[*n].tag.name == "a",
                                };
                                if !ignore {
                                    prev = Some(p);
                                    break;
                                }
                            }
                            let boundary = match prev {
                                None => true,
                                Some(Part::Element(n)) => {
                                    nodes[*n].tag.name == "br" || is_block(&nodes[*n].tag.name)
                                }
                                _ => false,
                            };
                            if boundary {
                                cuts.insert(child.tag.span.start, "sense");
                            }
                        }
                    }
                }
            }
        }
        if hw {
            let mut end = cuts
                .iter()
                .filter(|(p, k)| {
                    **p > node.tag.span.end
                        && matches!(**k, "sense" | "example" | "runon" | "barrier")
                })
                .map(|(p, _)| *p)
                .min()
                .unwrap_or(node.close);
            if let Some(at) = header_end {
                end = end.min(at);
            }
            if end > node.tag.span.end {
                cuts.insert(node.tag.span.end, "header");
                cuts.entry(end).or_insert("sense");
                if header_pos.is_some_and(|p| p.end > end) {
                    header_pos = None;
                }
            } else {
                header_pos = None;
            }
        }
        cuts.insert(node.close, "end");
        let cuts: Vec<_> = cuts.into_iter().collect();
        for pair in cuts.windows(2) {
            let ((start, kind), (end, _)) = (pair[0], pair[1]);
            if end <= start
                || kind == "barrier"
                || barriers.iter().any(|s| s.start <= start && end <= s.end)
            {
                continue;
            }
            let mut visible = false;
            for part in &node.parts {
                let span = bounds(part, &nodes);
                let value = if matches!(part, Part::Text(_)) && span.start < end && span.end > start
                {
                    entities(utf8(&raw[start.max(span.start)..end.min(span.end)])?)?
                } else if span.start >= start && span.end <= end {
                    text(part, &nodes, raw)?
                } else {
                    String::new()
                };
                if !value.trim().is_empty() {
                    visible = true;
                    break;
                }
            }
            if !visible {
                continue;
            }
            insert(
                &mut out,
                start,
                &format!("<div class=\"m2s-{kind}\">"),
                &format!("layout_open_{kind}"),
            );
            insert(&mut out, end, "</div>", &format!("layout_close_{kind}"));
            match kind {
                "header" => counts.headers += 1,
                "sense" => counts.senses += 1,
                "example" => counts.examples += 1,
                "runon" => counts.runons += 1,
                _ => counts.paragraphs += 1,
            }
        }
        // Inline closes precede block closes at identical source offsets.
        if let Some(pos) = header_pos {
            insert(
                &mut out,
                pos.start,
                "<span class=\"m2s-pos\">",
                "layout_header_pos_open",
            );
            prepend(&mut out, pos.end, "</span>", "layout_header_pos_close");
            counts.header_pos += 1;
        }
        for pair in cuts.windows(2) {
            let ((start, kind), (end, _)) = (pair[0], pair[1]);
            if !matches!(kind, "example" | "runon") || end <= start {
                continue;
            }
            if let Some(marker) = structural_marker(raw, start, end)? {
                insert(
                    &mut out,
                    marker.start,
                    &format!("<span class=\"m2s-{kind}-marker\">"),
                    "layout_decoration_open",
                );
                prepend(&mut out, marker.end, "</span>", "layout_decoration_close");
                counts.hidden_markers += 1;
            }
        }
    }
    let edits = out
        .into_iter()
        .map(|(start, b)| Edit {
            span: Span { start, end: b.end },
            replacement: b.replacement,
            reason: b.reasons.join("+"),
        })
        .collect::<Vec<_>>();
    for pair in edits.windows(2) {
        if pair[0].span.end > pair[1].span.start {
            return Err(Error::Incomplete("layout edits overlap".into()));
        }
    }
    Ok((edits, counts))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn apply(s: &str) -> (String, Counts) {
        let (e, c) = edits(s.as_bytes(), false).unwrap();
        (
            String::from_utf8(
                crate::replay(
                    s.as_bytes(),
                    &e,
                    Span {
                        start: 0,
                        end: s.len(),
                    },
                )
                .unwrap(),
            )
            .unwrap(),
            c,
        )
    }
    #[test]
    fn split_headword_stays_in_one_header() {
        let (h,c)=apply("<idx:entry><div class=\"entry\"><span class=\"hw\">read</span><span class=\"hwn\">|</span><span class=\"hw\">able</span> /riːd/ <small>ADJ</small> useful □ <span class=\"ex\">An example.</span> ● <span class=\"hwtxt\">readably</span> <small>ADV</small> □ <span class=\"ex\">Another.</span></div></idx:entry>");
        assert_eq!((c.headers, c.senses, c.examples, c.runons), (1, 1, 2, 1));
        assert!(h.contains("class=\"m2s-record\""));
        assert!(h.contains("<div class=\"m2s-example\"><span class=\"m2s-example-marker\">□"));
        assert!(h.contains("<span class=\"hwn\">|</span>"));
    }
    #[test]
    fn grammar_prefix_stays_with_example() {
        let (h,c)=apply("<div class='entry'><span class='hw'>read</span><br id='s1'/><b>1</b> gloss □ [<small>V</small> n] <span class='ex'>Read it.</span><br/><b>2</b> next</div>");
        assert_eq!((c.senses, c.examples, c.source_breaks), (2, 1, 2));
        assert!(h.contains("<div class=\"m2s-example\"><span class=\"m2s-example-marker\">□ </span>[<small>V</small> n]"));
        assert!(h.contains("id='s1'"));
        assert!(!h.contains("<br"));
    }
    #[test]
    fn quoted_symbols_and_prose_numbers_are_inline() {
        let (h,c)=apply("<table><tr><td title='□ ●'>Meaning <b>1</b> and <b>2</b>. <span class='ex'>A □ box.</span></td></tr></table>");
        assert_eq!((c.examples, c.runons, c.senses), (0, 0, 0));
        assert!(h.contains("title='□ ●'"));
    }
    #[test]
    fn opaque_comments_are_preserved() {
        let s="<div class='entry'><!-- □ example --><span class='hw'>item</span> <small>N</small> text</div>";
        let (h, c) = apply(s);
        assert!(h.contains("<!-- □ example -->"));
        assert_eq!(c.examples, 0);
    }
    #[test]
    fn entity_markers_are_preserved() {
        let (h, c) =
            apply("<div class='entry'>gloss &#x25A1;&#160;<span class='ex'>Test.</span></div>");
        assert_eq!(c.examples, 1);
        assert!(h.contains("&#x25A1;&#160;"));
    }
    #[test]
    fn nested_table_is_a_structural_barrier() {
        let (h,_)=apply("<div class='entry'><span class='hw'>word</span><table class='greybox'><tr><td>Note.</td></tr></table><b>1</b> definition □ <span class='ex'>Example.</span></div>");
        parse(h.as_bytes()).unwrap();
        assert!(h.contains("<table class='greybox'>"));
    }
    #[test]
    fn unknown_books_keep_source_profile() {
        assert!(!applies(
            "Other dictionary",
            &["amzn-mobi .hw .hwtxt .entry span.ex"]
        ));
        assert!(applies(
            "Collins COBUILD Advanced Learner’s Dictionary",
            &["amzn-mobi .hw .hwtxt .entry span.ex"]
        ));
        // OPF title of the retail Kindle edition: no "Collins", ASCII apostrophe.
        assert!(applies(
            "COBUILD Advanced Learner's Dictionary",
            &["amzn-mobi .hw .hwtxt .entry span.ex"]
        ));
        assert!(!applies(
            "COBUILD Advanced Learner's Dictionary",
            &["amzn-mobi .hw .entry"]
        ));
    }
    #[test]
    fn css_has_reader_portable_semantics() {
        assert!(!CSS.contains("@media"));
        assert!(!CSS.contains("rem;"));
        assert!(CSS.contains(".st { text-decoration: line-through; }"));
        assert!(CSS.contains("max-width: 100%"));
    }
    #[test]
    fn transformed_input_fails_instead_of_double_wrapping() {
        let (h, _) = apply("<idx:entry><div class='entry'>word</div></idx:entry>");
        assert!(edits(h.as_bytes(), false).is_err());
    }
    #[test]
    fn malformed_input_rejected() {
        assert!(edits(b"<div><span></div>", false).is_err());
    }
    #[test]
    fn block_separated_breaks_are_inert() {
        let (h,c)=apply("<idx:entry><div>Menu</div><br id='gap'/><idx:entry><div class='entry'>Sense</div></idx:entry></idx:entry>");
        assert_eq!(c.source_breaks, 1);
        assert!(!h.contains("<br"));
        assert!(h.contains("id='gap'"));
        parse(h.as_bytes()).unwrap();
    }
    #[test]
    fn long_unbroken_runs_have_css_fallback() {
        assert!(CSS.contains("overflow-wrap: break-word"));
    }
    #[test]
    fn shared_release_golden_examples() {
        for (input, expected) in [
            (
                include_str!("../../../tests/fixtures/readability/headword.input.html"),
                include_str!("../../../tests/fixtures/readability/headword.expected.html"),
            ),
            (
                include_str!("../../../tests/fixtures/readability/menu.input.html"),
                include_str!("../../../tests/fixtures/readability/menu.expected.html"),
            ),
            (
                include_str!("../../../tests/fixtures/readability/usage.input.html"),
                include_str!("../../../tests/fixtures/readability/usage.expected.html"),
            ),
        ] {
            let (actual, _) = apply(input);
            assert_eq!(actual, expected);
            parse(actual.as_bytes()).unwrap();
        }
    }
}

#[cfg(test)]
mod v2_regressions {
    use super::*;
    fn render(input: &str) -> (String, Counts) {
        let (edits, counts) = edits(input.as_bytes(), false).unwrap();
        let bytes = crate::replay(
            input.as_bytes(),
            &edits,
            Span {
                start: 0,
                end: input.len(),
            },
        )
        .unwrap();
        let output = String::from_utf8(bytes).unwrap();
        parse(output.as_bytes()).unwrap();
        (output, counts)
    }
    #[test]
    fn direct_adjective_pos_joins_headword() {
        let (html, counts) = render("<div class='entry'><span class='hw'>steady</span> /stedi/ <small id='pos'>ADJ</small> Calm.</div>");
        assert_eq!(counts.header_pos, 1);
        assert!(html.contains("<span class=\"m2s-pos\"><small id='pos'>ADJ</small></span></div><div class=\"m2s-sense\"> Calm."));
    }
    #[test]
    fn all_sibling_headers_are_processed() {
        let input = "<idx:entry><div class='entry'><span class='hw'>one</span> <small>ADJ</small> First.</div><div class='entry'><span class='hw'>two</span> <small>ADV</small> Second.</div></idx:entry>";
        let (_, counts) = render(input);
        assert_eq!(counts.header_pos, 2);
    }
    #[test]
    fn register_is_not_a_pos() {
        let (html, counts) = render("<div class='entry'><span class='hw'>word</span> <small>INFORMAL</small> Description.</div>");
        assert_eq!(counts.header_pos, 0);
        assert!(html.contains("<div class=\"m2s-sense\"><small>INFORMAL</small>"));
    }
    #[test]
    fn numbered_senses_keep_their_own_pos() {
        let (html, counts) = render("<div class='entry'><span class='hw'>record</span><br/><b>1</b> <small>N-COUNT</small> A file.<br/><b>2</b> <small>VERB</small> Store a sound.</div>");
        assert_eq!(counts.header_pos, 0);
        assert!(html.contains("<b>1</b> <small>N-COUNT</small>"));
        assert!(html.contains("<b>2</b> <small>VERB</small>"));
    }
    #[test]
    fn decoration_wrapping_preserves_source_bytes() {
        let (html, counts) = render("<div class='entry'>Gloss □ <span class='ex'>Example.</span> ● <span class='hwtxt'>derived</span> <small>ADV</small></div>");
        assert_eq!(counts.hidden_markers, 2);
        assert!(html.contains("<span class=\"m2s-example-marker\">□ </span>"));
        assert!(html.contains("<span class=\"m2s-runon-marker\">● </span>"));
    }
    #[test]
    fn numeric_marker_entities_use_same_rule() {
        let (html, counts) = render("<div class='entry'>Gloss &#x25A1;&#160;<span class='ex'>Example.</span> &#9679;&nbsp;<span class='hwtxt'>derived</span></div>");
        assert_eq!(counts.hidden_markers, 2);
        assert!(html.contains("<span class=\"m2s-example-marker\">&#x25A1;&#160;</span>"));
        assert!(html.contains("<span class=\"m2s-runon-marker\">&#9679;&nbsp;</span>"));
    }
    #[test]
    fn quoted_marker_inside_example_is_kept_visible() {
        let (html, counts) = render("<div class='entry'>Gloss □ <span class='ex'>Choose the □ box and the ● sign.</span></div>");
        assert_eq!(counts.hidden_markers, 1);
        assert!(html.contains("<span class='ex'>Choose the □ box and the ● sign.</span>"));
    }
    #[test]
    fn marker_rules_are_in_the_shared_stylesheet() {
        assert!(CSS.contains(".m2s-example-marker { display: none; }"));
        assert!(CSS.contains(".m2s-runon-marker { display: none; }"));
        assert_eq!(PROFILE, "collins-readable-v2");
    }
    #[test]
    fn hyphenated_pos_spelling_is_recognized() {
        for label in ["N-COUNT", "N‑COUNT", "N‐COUNT"] {
            assert!(is_pos_label(label));
        }
    }
    #[test]
    fn explicit_grammar_brackets_stay_with_definition() {
        let (_, counts) = render(
            "<div class='entry'><span class='hw'>word</span> [<small>ADJ</small> n] Usage.</div>",
        );
        assert_eq!(counts.header_pos, 0);
    }
}
