//! XML validation plus exact byte spans. Bytes are never DOM-reserialized.
use crate::{model::*, uri};
use html_preserve::tokenizer::{Tag, Token, Tokenizer};
use lexicon_core::{validate_word, Error, Limits, Result, Span};
use quick_xml::{events::{BytesStart, Event}, Reader};
use std::collections::BTreeMap;

pub fn entities(value: &str) -> Result<String> {
    quick_xml::escape::unescape(value).map(|v| v.into_owned()).map_err(|e| Error::Malformed(format!("character reference: {e}")))
}
pub fn utf8(bytes: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Malformed(format!("UTF-8: {e}")))?;
    if text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) { return Err(Error::Malformed("forbidden control character in source text".into())); }
    Ok(text)
}
pub fn xml_attrs(e: &BytesStart<'_>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for a in e.attributes() {
        let a = a.map_err(|err| Error::Malformed(format!("XML attribute: {err}")))?;
        let key = utf8(a.key.as_ref())?.to_owned();
        let raw = utf8(a.value.as_ref())?;
        let value = entities(&raw.replace("\r\n", " ").replace(['\r','\n','\t'], " "))?;
        if out.insert(key, value).is_some() { return Err(Error::Malformed("duplicate XML attribute".into())); }
    }
    Ok(out)
}
fn source_tag(raw: &[u8], before: usize, after: usize) -> Result<Tag> {
    let part = raw.get(before..after).ok_or_else(|| Error::Malformed("XML event span".into()))?;
    let Token::Tag(mut tag) = Tokenizer::new(part).next().ok_or_else(|| Error::Malformed("missing XML tag span".into()))?? else {
        return Err(Error::Malformed("XML event/span disagreement".into()));
    };
    tag.span.start += before; tag.span.end += before;
    for a in &mut tag.attrs {
        a.span.start += before; a.span.end += before;
        if let Some(v) = a.value.as_mut() { v.start += before; v.end += before; }
    }
    if tag.span.end != after { return Err(Error::Malformed("XML event end offset mismatch".into())); }
    Ok(tag)
}
#[derive(Clone)]
struct Frame { name: String, start: Span, entry: Option<usize>, orth: Option<usize> }
pub struct SourceParser;
impl SourceParser {
    pub fn parse(book: &mut SourceBook, file: &str, raw: &[u8], limits: &Limits, budget: &mut usize) -> Result<Page> {
        utf8(raw)?;
        // quick-xml skips a UTF-8 BOM and then reports positions relative to the
        // bytes after it. Parse past the BOM ourselves and add its length back so
        // every span indexes the original file.
        let bom = if raw.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
        let mut reader = Reader::from_reader(&raw[bom..]);
        reader.config_mut().check_end_names = true;
        let mut page = Page { file: file.into(), title: String::new(), body: Span { start: 0, end: 0 },
            body_tag: Span { start: 0, end: 0 }, head_end: 0, entries: Vec::new(), ids: BTreeMap::new(),
            links: Vec::new(), images: Vec::new(), stylesheets: Vec::new() };
        let mut stack: Vec<Frame> = Vec::new(); let mut roots = 0;
        loop {
            *budget = budget.checked_sub(1).ok_or_else(|| Error::Limit("XML event budget".into()))?;
            let before = bom + reader.buffer_position() as usize;
            let event = reader.read_event().map_err(|e| Error::Malformed(format!("{file} at byte {before}: {e}")))?;
            let after = bom + reader.buffer_position() as usize;
            match &event {
                Event::Start(e) | Event::Empty(e) => {
                    let name = utf8(e.name().as_ref())?.to_owned();
                    if name != name.to_ascii_lowercase() { return Err(Error::Unsupported("mixed-case XHTML element requires a renderer adapter".into())); }
                    let attrs = xml_attrs(e)?;
                    let tag = source_tag(raw,before,after)?;
                    let mut frame = Frame { name: name.clone(), start: tag.span, entry: None, orth: None };
                    if stack.is_empty() {
                        if name != "html" || roots != 0 { return Err(Error::Malformed("XHTML requires one html root".into())); }
                        roots += 1;
                    }
                    if stack.len() >= 256 { return Err(Error::Limit("XML nesting depth".into())); }
                    if matches!(name.as_str(), "script" | "iframe" | "object" | "embed" | "audio" | "video" | "svg" | "math" | "base" | "form") {
                        return Err(Error::Unsupported(format!("XHTML {name} needs a content adapter")));
                    }
                    if name.starts_with("idx:") && !matches!(name.as_str(), "idx:entry" | "idx:orth" | "idx:infl" | "idx:iform") {
                        return Err(Error::Unsupported(format!("dictionary element {name}")));
                    }
                    if attrs.keys().any(|k| k.to_ascii_lowercase().starts_with("on") || matches!(k.as_str(),"srcset"|"background"|"xml:base")) {
                        return Err(Error::Unsupported("active handler or unadapted resource attribute".into()));
                    }
                    if let Some(style) = attrs.get("style") { check_inline_style(style)?; }
                    if name == "style" { return Err(Error::Unsupported("inline style element requires per-page cascade adapter".into())); }
                    if name == "meta" && attrs.get("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh")) {
                        return Err(Error::Unsupported("HTML meta refresh".into()));
                    }
                    if name == "body" {
                        if page.body.start != 0 { return Err(Error::Malformed("multiple body elements".into())); }
                        page.body.start = after; page.body_tag = tag.span;
                    }
                    if name == "idx:entry" {
                        if page.body.start == 0 || page.body.end != 0 { return Err(Error::Malformed("dictionary entry outside body".into())); }
                        if book.entries.len() >= limits.entries { return Err(Error::Limit("source definition count".into())); }
                        let id = book.entries.len();
                        let ancestors = stack.iter().filter(|f| !matches!(f.name.as_str(),"html"|"head"|"body"|"idx:entry"))
                            .map(|f| Ancestor { name: f.name.clone(), start_tag: f.start }).collect();
                        book.entries.push(Definition { id, file: file.into(), span: Span { start:tag.span.start,end:0 }, orths:Vec::new(), ancestors });
                        page.entries.push(id); frame.entry = Some(id);
                    }
                    let current_entry = frame.entry.or_else(|| stack.iter().rev().find_map(|f| f.entry));
                    if name == "idx:orth" {
                        let entry_id = current_entry.ok_or_else(|| Error::Malformed("orphan idx:orth".into()))?;
                        if book.orths.len() >= limits.entries { return Err(Error::Limit("source headword count".into())); }
                        let value = required(&attrs,"value")?.to_owned(); validate_word(&value)?;
                        let id = book.orths.len();
                        book.orths.push(Orth { id, entry_id, value, span:Span { start:tag.span.start,end:0 }, forms:Vec::new() });
                        book.entries[entry_id].orths.push(id); frame.orth = Some(id);
                    }
                    if name == "idx:iform" {
                        let orth_id = stack.iter().rev().find_map(|f| f.orth).ok_or_else(|| Error::Malformed("orphan idx:iform".into()))?;
                        let orth = &book.orths[orth_id];
                        if Some(orth.entry_id) != current_entry { return Err(Error::Malformed("inflection crosses definition boundary".into())); }
                        if book.forms.len() >= limits.aliases { return Err(Error::Limit("source inflection count".into())); }
                        let value = required(&attrs,"value")?.to_owned(); validate_word(&value)?;
                        let id = book.forms.len();
                        book.forms.push(Inflection { id, entry_id:orth.entry_id, orth_id, value, attributes:attrs.clone(), position:tag.span.start });
                        book.orths[orth_id].forms.push(id);
                    }
                    if let Some(id) = attrs.get("id") { insert_anchor(&mut page,id,tag.span.start,current_entry)?; }
                    if name == "a" {
                        if let Some(id) = attrs.get("name") {
                            if attrs.get("id") != Some(id) { insert_anchor(&mut page,id,tag.span.start,current_entry)?; }
                        }
                        if let Some(url) = attrs.get("href") { page.links.push(reference(&tag,"href",url,file)?); }
                    }
                    if name == "img" { page.images.push(reference(&tag,"src",required(&attrs,"src")?,file)?); }
                    if name == "link" {
                        if attrs.get("rel").map(String::as_str) != Some("stylesheet") { return Err(Error::Unsupported("link relation needs adapter".into())); }
                        let target = uri::resolve(file,required(&attrs,"href")?)?.ok_or_else(|| Error::Unsupported("remote CSS".into()))?;
                        if !target.anchor.is_empty() { return Err(Error::Unsupported("stylesheet fragment".into())); }
                        page.stylesheets.push(target.file);
                    }
                    stack.push(frame);
                    if matches!(&event, Event::Empty(_)) { close(book,&mut page,&mut stack,&name,after,after,limits)?; }
                }
                Event::End(e) => {
                    let name = utf8(e.name().as_ref())?.to_owned();
                    close(book,&mut page,&mut stack,&name,before,after,limits)?;
                }
                Event::Text(e) => {
                    let text = entities(utf8(e.as_ref())?)?;
                    if stack.iter().any(|f| f.name == "title") { page.title.push_str(&text); }
                    if stack.is_empty() && !text.trim_matches('\u{feff}').trim().is_empty() { return Err(Error::Malformed("text outside html root".into())); }
                }
                Event::GeneralRef(e) => {
                    let text = entities(&format!("&{};",utf8(e.as_ref())?))?;
                    if stack.iter().any(|f| f.name == "title") { page.title.push_str(&text); }
                    if stack.is_empty() { return Err(Error::Malformed("entity outside root".into())); }
                }
                Event::CData(e) => {
                    if stack.is_empty() { return Err(Error::Malformed("CDATA outside root".into())); }
                    if stack.iter().any(|f| f.name == "title") { page.title.push_str(utf8(e.as_ref())?); }
                }
                Event::DocType(e) => {
                    if e.as_ref().contains(&b'[') { return Err(Error::Unsupported("DTD internal subset/external entities".into())); }
                }
                Event::Decl(e) => {
                    if let Some(encoding) = e.encoding() {
                        let encoding = encoding.map_err(|e| Error::Malformed(format!("XML encoding: {e}")))?;
                        if !encoding.eq_ignore_ascii_case(b"utf-8") && !encoding.eq_ignore_ascii_case(b"us-ascii") { return Err(Error::Unsupported("source XML encoding declaration".into())); }
                    }
                }
                Event::PI(_) => return Err(Error::Unsupported("XML processing instruction".into())),
                Event::Comment(_) => {},
                Event::Eof => break,
            }
        }
        if !stack.is_empty() || roots != 1 || page.body.start == 0 || page.body.end < page.body.start || page.head_end == 0 {
            return Err(Error::Malformed(format!("incomplete XHTML document {file}")));
        }
        Ok(page)
    }
}
fn required<'a>(attrs: &'a BTreeMap<String,String>, key: &str) -> Result<&'a str> {
    attrs.get(key).map(String::as_str).ok_or_else(|| Error::Malformed(format!("required attribute {key}")))
}
fn reference(tag: &Tag, attribute: &str, url: &str, file: &str) -> Result<Reference> {
    let value = tag.attr(attribute).and_then(|a| a.value).ok_or_else(|| Error::Malformed(format!("missing reference span {attribute}")))?;
    Ok(Reference { value, url:url.into(), target:uri::resolve(file,url)? })
}
fn insert_anchor(page:&mut Page,id:&str,position:usize,entry_id:Option<usize>) -> Result<()> {
    if id.is_empty() || id.chars().any(char::is_control) || page.ids.insert(id.into(),Anchor{position,entry_id}).is_some() {
        return Err(Error::Malformed(format!("empty/duplicate/invalid anchor in {}: {id:?}",page.file)));
    }
    Ok(())
}
fn close(book:&mut SourceBook,page:&mut Page,stack:&mut Vec<Frame>,name:&str,before:usize,after:usize,limits:&Limits) -> Result<()> {
    let frame = stack.pop().ok_or_else(|| Error::Malformed("unmatched end tag".into()))?;
    if frame.name != name { return Err(Error::Malformed("XML element nesting mismatch".into())); }
    if let Some(id) = frame.orth { book.orths[id].span.end = after; }
    if let Some(id) = frame.entry {
        let entry = &mut book.entries[id]; entry.span.end = after;
        if entry.orths.is_empty() { return Err(Error::Incomplete(format!("definition {id} has no headwords"))); }
        if entry.span.len() > limits.entry_bytes { return Err(Error::Limit(format!("definition {id} byte budget"))); }
    }
    if name == "body" { page.body.end = before; }
    if name == "head" { page.head_end = before; }
    Ok(())
}
pub fn check_inline_style(style:&str) -> Result<()> {
    let folded = style.to_ascii_lowercase();
    if folded.contains(['\\','<']) || ["url(","@import","expression(","-moz-binding","behavior:"].iter().any(|s| folded.contains(s)) {
        return Err(Error::Unsupported("CSS resource/executable declaration requires adapter".into()));
    }
    Ok(())
}
/// One-pass extraction of visible text for the compiled/source comparison.
/// Concatenate text exactly, then normalize only Unicode whitespace.
pub fn plain_text(raw:&[u8]) -> Result<String> {
    utf8(raw)?;
    let mut output=String::new(); let mut cursor=0;
    for token in Tokenizer::new(raw) {
        let token=token?; let span=token.span();
        if cursor < span.start { output.push_str(&entities(utf8(&raw[cursor..span.start])?)?); }
        if let Token::Raw { name, span } = &token {
            if !matches!(name.as_str(),"script"|"style") { output.push_str(&entities(utf8(span.bytes(raw)?)?)?); }
        }
        cursor=span.end;
    }
    if cursor < raw.len() { output.push_str(&entities(utf8(&raw[cursor..])?)?); }
    Ok(output.split_whitespace().collect::<Vec<_>>().join(" "))
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn entity_and_text_normalization() {
        assert_eq!(plain_text("<p>caf&eacute; <b>A</b>&nbsp; B</p><!-- hidden -->".as_bytes()).unwrap(),"café A B");
        assert!(entities("&unknown;").is_err());
    }
    #[test] fn reference_span_preserves_quotes() {
        let raw=b"<a href='x.xhtml#id'>"; let tag=source_tag(raw,0,raw.len()).unwrap();
        assert_eq!(tag.attr("href").unwrap().value.unwrap().bytes(raw).unwrap(),b"x.xhtml#id");
    }
}

#[cfg(test)]mod structural_tests{
    use super::*;
    fn parse(raw:&str)->Result<(SourceBook,Page)>{
        let mut book=SourceBook{files:BTreeMap::new(),archive_sha256:String::new(),archive_record:0,pages:BTreeMap::new(),entries:Vec::new(),orths:Vec::new(),forms:Vec::new(),package:Package::default()};
        let limits=Limits::default();let mut budget=limits.operations;
        let page=SourceParser::parse(&mut book,"a.xhtml",raw.as_bytes(),&limits,&mut budget)?;Ok((book,page))
    }
    #[test]fn utf8_bom_and_entities_have_exact_spans(){
        let raw="\u{feff}<?xml version=\"1.0\"?><html><head><title>字典</title></head><body>前言<idx:entry><idx:orth value=\"caf&eacute;\"/><b>café</b></idx:entry>附录</body></html>";
        let (book,page)=parse(raw).unwrap();assert_eq!(book.orths[0].value,"café");
        assert_eq!(book.entries[0].span.bytes(raw.as_bytes()).unwrap(),"<idx:entry><idx:orth value=\"caf&eacute;\"/><b>café</b></idx:entry>".as_bytes());
        assert!(utf8(page.body.bytes(raw.as_bytes()).unwrap()).unwrap().starts_with("前言"));
    }
    #[test]fn malformed_active_and_duplicate_anchor_inputs_fail(){
        for middle in ["<div><b></div>","<script>bad()</script>","<div onclick=\"bad()\"/>","<a id=\"x\"/><a id=\"x\"/>","<idx:orth value=\"orphan\"/>"]{
            let raw=format!("<html><head><title>T</title></head><body>{middle}</body></html>");assert!(parse(&raw).is_err(),"{middle}");
        }
    }
    #[test]fn dtd_subset_is_rejected(){assert!(parse("<!DOCTYPE html [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><html><head></head><body></body></html>").is_err());}
    #[test]fn excessive_depth_is_an_error(){let raw=format!("<html><head></head><body>{}{}</body></html>","<div>".repeat(300),"</div>".repeat(300));assert!(parse(&raw).is_err());}
}
