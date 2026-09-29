//! A span tokenizer, NOT a browser DOM parser. It never serializes source HTML.
//! Unsupported/malformed constructs are errors rather than recovery heuristics.
use lexicon_core::{Error, Result, Span};

#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub span: Span,
    pub value: Option<Span>,
}
#[derive(Clone, Debug)]
pub struct Tag {
    pub name: String,
    pub closing: bool,
    pub span: Span,
    pub attrs: Vec<Attribute>,
}
impl Tag {
    pub fn attr(&self, name: &str) -> Option<&Attribute> {
        self.attrs.iter().find(|a| a.name == name)
    }
}
#[derive(Clone, Debug)]
pub enum Token {
    Tag(Tag),
    Opaque(Span),
    Raw { name: String, span: Span },
}
impl Token {
    pub fn span(&self) -> Span {
        match self {
            Self::Tag(t) => t.span,
            Self::Opaque(s) | Self::Raw { span: s, .. } => *s,
        }
    }
}
pub struct Tokenizer<'a> {
    bytes: &'a [u8],
    cursor: usize,
    raw: Option<String>,
    done: bool,
}
impl<'a> Tokenizer<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            raw: None,
            done: false,
        }
    }
    fn token(&mut self) -> Result<Option<Token>> {
        let b = self.bytes;
        if let Some(name) = self.raw.take() {
            let needle = format!("</{name}");
            let end = (self.cursor..b.len())
                .find(|&i| {
                    b.get(i..i + needle.len())
                        .is_some_and(|s| s.eq_ignore_ascii_case(needle.as_bytes()))
                        && b.get(i + needle.len())
                            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>')
                })
                .ok_or_else(|| Error::Malformed(format!("unclosed raw-text element {name}")))?;
            let span = Span {
                start: self.cursor,
                end,
            };
            self.cursor = end;
            if !span.is_empty() {
                return Ok(Some(Token::Raw { name, span }));
            }
        }
        while self.cursor < b.len() {
            let start = self.cursor;
            if b[start] != b'<' {
                self.cursor += 1;
                continue;
            }
            if b[start..].starts_with(b"<!--") {
                let relative = b[start + 4..]
                    .windows(3)
                    .position(|w| w == b"-->")
                    .ok_or_else(|| Error::Malformed("unclosed HTML comment".into()))?;
                self.cursor = start + 4 + relative + 3;
                return Ok(Some(Token::Opaque(Span {
                    start,
                    end: self.cursor,
                })));
            }
            let mut p = start + 1;
            if b.get(p) == Some(&b'!') || b.get(p) == Some(&b'?') {
                let mut quote = None;
                while p < b.len() {
                    let c = b[p];
                    if let Some(q) = quote {
                        if c == q {
                            quote = None;
                        }
                    } else if c == b'\'' || c == b'"' {
                        quote = Some(c);
                    } else if c == b'[' {
                        return Err(Error::Unsupported("DTD internal subsets/CDATA".into()));
                    } else if c == b'>' {
                        self.cursor = p + 1;
                        return Ok(Some(Token::Opaque(Span {
                            start,
                            end: self.cursor,
                        })));
                    }
                    p += 1;
                }
                return Err(Error::Malformed("unclosed declaration".into()));
            }
            let closing = b.get(p) == Some(&b'/');
            if closing {
                p += 1;
            }
            if !b.get(p).is_some_and(u8::is_ascii_alphabetic) {
                self.cursor += 1;
                continue;
            }
            let name_start = p;
            while b
                .get(p)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, b':' | b'-' | b'_'))
            {
                p += 1;
            }
            let name = ascii_name(&b[name_start..p])?;
            let mut attrs = Vec::new();
            let mut self_closing = false;
            loop {
                while b.get(p).is_some_and(u8::is_ascii_whitespace) {
                    p += 1;
                }
                match b.get(p) {
                    Some(b'>') => {
                        p += 1;
                        break;
                    }
                    Some(b'/') if b.get(p + 1) == Some(&b'>') => {
                        p += 2;
                        self_closing = true;
                        break;
                    }
                    None => return Err(Error::Malformed("unclosed HTML tag".into())),
                    _ => {}
                }
                if closing {
                    return Err(Error::Malformed("attributes on closing tag".into()));
                }
                let astart = p;
                while b.get(p).is_some_and(|c| {
                    !c.is_ascii_whitespace()
                        && !matches!(*c, b'=' | b'>' | b'/' | b'\'' | b'"' | b'<')
                }) {
                    p += 1;
                }
                if astart == p {
                    return Err(Error::Malformed(format!(
                        "invalid HTML attribute at byte {p}"
                    )));
                }
                let aname = ascii_name(&b[astart..p])?;
                if attrs.iter().any(|a: &Attribute| a.name == aname) {
                    return Err(Error::Malformed(format!("duplicate attribute {aname}")));
                }
                let name_end = p;
                while b.get(p).is_some_and(u8::is_ascii_whitespace) {
                    p += 1;
                }
                let mut value = None;
                let attr_end;
                if b.get(p) == Some(&b'=') {
                    p += 1;
                    while b.get(p).is_some_and(u8::is_ascii_whitespace) {
                        p += 1;
                    }
                    if let Some(q) = b.get(p).copied().filter(|&q| q == b'\'' || q == b'"') {
                        p += 1;
                        let vstart = p;
                        while b.get(p).is_some_and(|&c| c != q) {
                            p += 1;
                        }
                        if p == b.len() {
                            return Err(Error::Malformed("unclosed quoted attribute".into()));
                        }
                        value = Some(Span {
                            start: vstart,
                            end: p,
                        });
                        p += 1;
                    } else {
                        let vstart = p;
                        while b
                            .get(p)
                            .is_some_and(|c| !c.is_ascii_whitespace() && *c != b'>')
                        {
                            if matches!(b[p], b'<' | b'\'' | b'"' | b'=' | b'`') {
                                return Err(Error::Malformed("invalid unquoted attribute".into()));
                            }
                            p += 1;
                        }
                        if p == vstart {
                            return Err(Error::Malformed("empty unquoted attribute".into()));
                        }
                        value = Some(Span {
                            start: vstart,
                            end: p,
                        });
                    }
                    attr_end = p;
                } else {
                    attr_end = name_end;
                }
                attrs.push(Attribute {
                    name: aname,
                    span: Span {
                        start: astart,
                        end: attr_end,
                    },
                    value,
                });
            }
            self.cursor = p;
            if !closing
                && !self_closing
                && matches!(name.as_str(), "style" | "script" | "title" | "textarea")
            {
                self.raw = Some(name.clone());
            }
            return Ok(Some(Token::Tag(Tag {
                name,
                closing,
                span: Span { start, end: p },
                attrs,
            })));
        }
        Ok(None)
    }
}
fn ascii_name(data: &[u8]) -> Result<String> {
    if !data.is_ascii() {
        return Err(Error::Unsupported(
            "non-ASCII HTML tag/attribute name".into(),
        ));
    }
    Ok(data
        .iter()
        .map(|b| char::from(b.to_ascii_lowercase()))
        .collect())
}
impl Iterator for Tokenizer<'_> {
    type Item = Result<Token>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.token() {
            Ok(Some(t)) => Some(Ok(t)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_angles_and_comments() {
        let bytes = b"<!-- <a filepos='99'> --><A title='a > b' filepos=12>word</A>";
        let tokens = Tokenizer::new(bytes).collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(tokens.len(), 3);
        let Token::Tag(tag) = &tokens[1] else {
            panic!("expected tag")
        };
        assert_eq!(tag.name, "a");
        assert_eq!(
            tag.attr("filepos")
                .unwrap()
                .value
                .unwrap()
                .bytes(bytes)
                .unwrap(),
            b"12"
        );
    }
    #[test]
    fn raw_style_is_not_parsed_as_html() {
        let tokens = Tokenizer::new(b"<style>.x::after{content:'<a>'}</style>")
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert!(matches!(&tokens[1], Token::Raw { name, .. } if name == "style"));
    }
    #[test]
    fn malformed_input_fails() {
        assert!(Tokenizer::new(b"<img src='x>")
            .collect::<Result<Vec<_>>>()
            .is_err());
        assert!(Tokenizer::new(b"<a href='a' href='b'>")
            .collect::<Result<Vec<_>>>()
            .is_err());
    }
}
