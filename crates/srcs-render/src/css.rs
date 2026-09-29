//! Bounded, fail-closed CSS scoping for the source grammar supported here.
use lexicon_core::{Error, Result};
pub fn scope(css: &str, scope: &str) -> Result<String> {
    srcs_reader::markup::check_inline_style(css)?;
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
            let end = bytes[p + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .ok_or_else(|| Error::Malformed("unclosed CSS comment".into()))?;
            p += end + 4;
        } else {
            cleaned.push(b);
            p += 1;
        }
    }
    if quote.is_some() {
        return Err(Error::Malformed("unclosed CSS string".into()));
    }
    let text = String::from_utf8(cleaned).map_err(|_| Error::Malformed("CSS encoding".into()))?;
    rules(&text, scope, 0)
}
fn rules(text: &str, scope: &str, depth: usize) -> Result<String> {
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
        let open = b[p..]
            .iter()
            .position(|&c| c == b'{')
            .map(|i| p + i)
            .ok_or_else(|| Error::Unsupported("CSS statement requires an adapter".into()))?;
        let prelude = text[p..open].trim();
        let mut q = open + 1;
        let mut nesting = 1;
        let mut quote = None;
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
        if nesting != 0 {
            return Err(Error::Malformed("unclosed CSS block".into()));
        }
        let body = &text[open + 1..q - 1];
        if prelude.starts_with("@media ") {
            if prelude.contains(['<', '>', ';']) {
                return Err(Error::Unsupported("CSS media prelude".into()));
            }
            out.push_str(prelude);
            out.push('{');
            out.push_str(&rules(body, scope, depth + 1)?);
            out.push('}');
        } else {
            if prelude.is_empty()
                || !prelude.bytes().all(|c| {
                    c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || b"_.#,-".contains(&c)
                })
            {
                return Err(Error::Unsupported(format!(
                    "CSS selector {prelude:?} needs adapter"
                )));
            }
            for (i, sel) in prelude.split(',').enumerate() {
                let sel = sel.trim();
                if sel.is_empty() {
                    return Err(Error::Malformed("empty CSS selector".into()));
                }
                if i > 0 {
                    out.push(',');
                }
                out.push('.');
                out.push_str(scope);
                if sel != "body" {
                    out.push(' ');
                    out.push_str(sel);
                }
            }
            out.push('{');
            out.push_str(body);
            out.push('}');
        }
        p = q;
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_scoped() {
        assert_eq!(
            scope("body { color:red } .entry,b{font-weight:bold}", "dict").unwrap(),
            ".dict{ color:red }.dict .entry,.dict b{font-weight:bold}"
        );
    }
    #[test]
    fn media_and_comments() {
        assert_eq!(
            scope("/*c*/@media screen {.a{color:red}}", "d").unwrap(),
            "@media screen{.d .a{color:red}}"
        );
    }
    #[test]
    fn resources_and_unknown_grammar_fail() {
        for s in [
            "@import 'x';",
            "p{background:url(x)}",
            "p:hover{color:red}",
            "p{font:bad",
        ] {
            assert!(scope(s, "d").is_err());
        }
    }
}
