//! CSS scoping shared by both backends. Every selector is prefixed with a
//! payload wrapper class. A prefix can only narrow what a selector matches, so
//! dictionary CSS never styles anything outside its own payloads, whichever
//! reader shows several dictionaries on one page.
use lexicon_core::{Error, Result};

/// Which CSS a scoper accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grammar {
    /// Simple selectors and `@media` only. Anything else is an error, so its
    /// semantics can be added explicitly (publisher-source books).
    Bounded,
    /// Any selector; `@media`/`@supports` are scoped recursively, descriptor
    /// at-rules are kept verbatim, and input CSS error recovery is applied
    /// where CSS defines it (compiled books, whose CSS used to be copied as is).
    Open,
}

/// Wrapper class for a book's payloads, derived from its SHA-256 namespace.
pub fn scope_class(namespace: &str) -> String {
    format!("m2s_{}", namespace.chars().take(16).collect::<String>())
}

/// Scopes every rule of `css` under `.{class}`. A leading `body` selector
/// stands for the payload wrapper itself.
pub fn scope(css: &str, class: &str, grammar: Grammar) -> Result<String> {
    rules(&strip_comments(css, grammar)?, class, grammar, 0)
}

fn strip_comments(css: &str, grammar: Grammar) -> Result<String> {
    let bytes = css.as_bytes();
    let mut cleaned = Vec::new();
    let mut p = 0;
    let mut quote = None;
    while p < bytes.len() {
        let b = bytes[p];
        if let Some(q) = quote {
            cleaned.push(b);
            if b == q {
                quote = None;
            }
            p += 1;
            continue;
        }
        if b == b'\'' || b == b'"' {
            quote = Some(b);
            cleaned.push(b);
            p += 1;
            continue;
        }
        if bytes.get(p..p + 2) == Some(b"/*") {
            match bytes[p + 2..].windows(2).position(|w| w == b"*/") {
                Some(end) => p += end + 4,
                // CSS ends an unclosed comment at the end of the stylesheet.
                None if grammar == Grammar::Open => break,
                None => return Err(Error::Malformed("unclosed CSS comment".into())),
            }
        } else {
            cleaned.push(b);
            p += 1;
        }
    }
    if quote.is_some() {
        return Err(Error::Malformed("unclosed CSS string".into()));
    }
    String::from_utf8(cleaned).map_err(|_| Error::Malformed("CSS encoding".into()))
}

/// Index of the first `{` or `;` outside strings, parentheses and brackets.
fn prelude_end(b: &[u8], mut p: usize) -> Result<Option<usize>> {
    let (mut depth, mut quote) = (0usize, None);
    while p < b.len() {
        let c = b[p];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                b'\'' | b'"' => quote = Some(c),
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b'{' | b';' if depth == 0 => return Ok(Some(p)),
                b'}' if depth == 0 => return Err(Error::Malformed("unmatched `}` in CSS".into())),
                _ => {}
            },
        }
        p += 1;
    }
    Ok(None)
}

/// Index just after the `}` closing the block opened at `open`.
fn block_end(b: &[u8], open: usize, grammar: Grammar) -> Result<usize> {
    let (mut q, mut nesting, mut quote) = (open + 1, 1, None);
    while q < b.len() && nesting > 0 {
        let c = b[q];
        if let Some(mark) = quote {
            if c == mark {
                quote = None;
            }
        } else if c == b'\'' || c == b'"' {
            quote = Some(c);
        } else if c == b'{' {
            nesting += 1;
        } else if c == b'}' {
            nesting -= 1;
        }
        q += 1;
    }
    match nesting {
        0 => Ok(q),
        // CSS closes blocks left open at the end of the stylesheet.
        _ if grammar == Grammar::Open => Ok(b.len() + 1),
        _ => Err(Error::Malformed("unclosed CSS block".into())),
    }
}

fn at_keyword(prelude: &str) -> Option<String> {
    let name = prelude.strip_prefix('@')?;
    let end = name
        .find(|c: char| c.is_ascii_whitespace() || c == '(' || c == '{')
        .unwrap_or(name.len());
    Some(name[..end].to_ascii_lowercase())
}

fn rules(text: &str, class: &str, grammar: Grammar, depth: usize) -> Result<String> {
    if depth > 16 {
        return Err(Error::Limit("CSS nesting".into()));
    }
    let b = text.as_bytes();
    let mut p = 0;
    let mut out = String::new();
    while p < b.len() {
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if p == b.len() {
            break;
        }
        if grammar == Grammar::Open {
            // HTML comment delimiters are ignored between CSS rules.
            if let Some(skip) = ["<!--", "-->"].iter().find(|t| text[p..].starts_with(**t)) {
                p += skip.len();
                continue;
            }
        }
        let Some(end) = prelude_end(b, p)? else {
            match grammar {
                Grammar::Bounded => {
                    return Err(Error::Unsupported(
                        "CSS statement requires an adapter".into(),
                    ))
                }
                // CSS drops a rule left incomplete at the end of the stylesheet.
                Grammar::Open => break,
            }
        };
        let prelude = text[p..end].trim();
        if b[end] == b';' {
            match (grammar, at_keyword(prelude).as_deref()) {
                // The stylesheet file is always UTF-8.
                (Grammar::Open, Some("charset")) => {}
                (Grammar::Open, Some("namespace")) => {
                    out.push_str(prelude);
                    out.push(';');
                }
                _ => {
                    return Err(Error::Unsupported(format!(
                        "CSS statement {prelude:?} requires an adapter"
                    )))
                }
            }
            p = end + 1;
            continue;
        }
        let close = block_end(b, end, grammar)?;
        let body = &text[end + 1..(close - 1).min(text.len())];
        // The bounded grammar keeps its original `@media ` spelling check.
        let nested = match grammar {
            Grammar::Bounded => prelude.starts_with("@media "),
            Grammar::Open => matches!(at_keyword(prelude).as_deref(), Some("media" | "supports")),
        };
        match (grammar, at_keyword(prelude).as_deref()) {
            _ if nested => {
                if grammar == Grammar::Bounded && prelude.contains(['<', '>', ';']) {
                    return Err(Error::Unsupported("CSS media prelude".into()));
                }
                out.push_str(prelude);
                out.push('{');
                out.push_str(&rules(body, class, grammar, depth + 1)?);
                out.push('}');
            }
            // Descriptor blocks select no elements, so there is nothing to scope.
            (
                Grammar::Open,
                Some(
                    "font-face" | "page" | "keyframes" | "-webkit-keyframes" | "-moz-keyframes"
                    | "counter-style",
                ),
            ) => {
                out.push_str(prelude);
                out.push('{');
                out.push_str(body);
                out.push('}');
            }
            (Grammar::Open, Some(_)) => {
                return Err(Error::Unsupported(format!(
                    "CSS at-rule {prelude:?} needs an adapter"
                )))
            }
            _ => {
                out.push_str(&selectors(prelude, class, grammar)?);
                out.push('{');
                out.push_str(body);
                out.push('}');
            }
        }
        p = close;
    }
    Ok(out)
}

fn selectors(prelude: &str, class: &str, grammar: Grammar) -> Result<String> {
    if grammar == Grammar::Bounded
        && (prelude.is_empty()
            || !prelude.bytes().all(|c| {
                c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || b"_.#,-".contains(&c)
            }))
    {
        return Err(Error::Unsupported(format!(
            "CSS selector {prelude:?} needs adapter"
        )));
    }
    let mut out = String::new();
    for (i, sel) in split_selectors(prelude).into_iter().enumerate() {
        let sel = sel.trim();
        if sel.is_empty() {
            return Err(Error::Malformed("empty CSS selector".into()));
        }
        if i > 0 {
            out.push(',');
        }
        out.push('.');
        out.push_str(class);
        match (grammar, root_relative(sel)) {
            (Grammar::Open, Some((combinator, rest))) => {
                if !rest.is_empty() {
                    out.push_str(combinator);
                    out.push_str(rest);
                }
            }
            _ if sel == "body" => {}
            _ => {
                out.push(' ');
                out.push_str(sel);
            }
        }
    }
    Ok(out)
}

/// Splits a selector list at commas outside strings, parentheses and brackets.
fn split_selectors(prelude: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth, mut quote) = (Vec::new(), 0, 0usize, None);
    for (i, c) in prelude.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(&prelude[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
    }
    parts.push(&prelude[start..]);
    parts
}

/// The payload wrapper stands for the document root: strips a leading chain
/// of `html`/`body` joined by descendant or child combinators and returns the
/// combinator to put after the wrapper class plus the remaining selector.
/// Sibling combinators after the root would reach outside the wrapper, so
/// such selectors are not rewritten (they stay scoped and never match).
fn root_relative(sel: &str) -> Option<(&'static str, &str)> {
    let mut rest = sel;
    let mut combinator = None;
    loop {
        let word_end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        let word = &rest[..word_end];
        let after = &rest[word_end..];
        let root = word.eq_ignore_ascii_case("html") || word.eq_ignore_ascii_case("body");
        if !root
            || !(after.is_empty()
                || after.starts_with(|c: char| c.is_ascii_whitespace() || c == '>'))
        {
            break;
        }
        let trimmed = after.trim_start();
        (combinator, rest) = match trimmed.strip_prefix('>') {
            Some(child) => (Some(" > "), child.trim_start()),
            None => (Some(" "), trimmed),
        };
    }
    let combinator = combinator?;
    if rest.starts_with(['+', '~']) {
        return None;
    }
    Some((combinator, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Grammar::{Bounded, Open};
    #[test]
    fn bounded_selectors_scoped() {
        assert_eq!(
            scope(
                "body { color:red } .entry,b{font-weight:bold}",
                "dict",
                Bounded
            )
            .unwrap(),
            ".dict{ color:red }.dict .entry,.dict b{font-weight:bold}"
        );
        assert_eq!(
            scope("/*c*/@media screen {.a{color:red}}", "d", Bounded).unwrap(),
            "@media screen{.d .a{color:red}}"
        );
    }
    #[test]
    fn bounded_rejects_what_it_cannot_prove() {
        for s in [
            "p:hover{color:red}",
            "p{font:bad",
            "@charset 'x';",
            "p{} h1",
            "@font-face{font-family:x}",
            "/* open",
        ] {
            assert!(scope(s, "d", Bounded).is_err(), "{s}");
        }
    }
    #[test]
    fn open_scopes_any_selector() {
        assert_eq!(
            scope(
                "p:hover, a > b[title='x,y'], :is(i, u) ~ s{color:red}",
                "d",
                Open
            )
            .unwrap(),
            ".d p:hover,.d a > b[title='x,y'],.d :is(i, u) ~ s{color:red}"
        );
    }
    #[test]
    fn open_maps_the_document_root_to_the_wrapper() {
        let cases = [
            ("body{margin:0}", ".d{margin:0}"),
            ("html body p{x:y}", ".d p{x:y}"),
            ("html>body>p{x:y}", ".d > p{x:y}"),
            ("body > p, body{x:y}", ".d > p,.d{x:y}"),
            // Siblings of the root and qualified roots never match inside a payload.
            ("body + p{x:y}", ".d body + p{x:y}"),
            ("body.x p{x:y}", ".d body.x p{x:y}"),
            ("bodyx{x:y}", ".d bodyx{x:y}"),
        ];
        for (css, scoped) in cases {
            assert_eq!(scope(css, "d", Open).unwrap(), scoped, "{css}");
        }
    }
    #[test]
    fn open_applies_css_error_recovery() {
        let cases = [
            // A trailing incomplete rule is dropped instead of reaching the next body.
            (".x{a:b} h1", ".d .x{a:b}"),
            (".a{color:red", ".d .a{color:red}"),
            ("/* open .a{}", ""),
            ("<!-- .a{b:c} -->", ".d .a{b:c}"),
            ("@charset \"utf-8\"; .a{b:c}", ".d .a{b:c}"),
            (
                "@media (width > 5em){ body{b:c} }",
                "@media (width > 5em){.d{b:c}}",
            ),
            ("@font-face{font-family:x}", "@font-face{font-family:x}"),
        ];
        for (css, scoped) in cases {
            assert_eq!(scope(css, "d", Open).unwrap(), scoped, "{css}");
        }
        for css in [".a{}} .b{}", ".a{content:\"x}", "@layer x{p{}}", "@foo x;"] {
            assert!(scope(css, "d", Open).is_err(), "{css}");
        }
    }
    #[test]
    fn scope_class_uses_the_namespace_prefix() {
        assert_eq!(scope_class(&"ab".repeat(32)), "m2s_abababababababab");
    }
}
