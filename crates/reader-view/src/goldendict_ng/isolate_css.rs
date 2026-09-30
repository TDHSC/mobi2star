//! GoldenDict-ng's `isolateCSS`, ported literally, quirks included: text
//! before an at-rule is dropped, selectors are split on every comma, and
//! `body X` becomes `#gd-ID gd-section-body X`, which matches nothing
//! because StarDict articles have no `gd-section-body` element.
//!
//! Ported from GoldenDict-ng (GPL-3.0-or-later) at commit b16a94a:
//! src/dict/dictionary.cc, `Class::isolateCSS` and `findMatchingBracket`.
//! All delimiters are ASCII, so byte offsets stand in for Qt's UTF-16 ones.

/// Prefixes every selector in `css` with `#gd-{id}`.
pub fn isolate_css(css: &str, id: &str) -> String {
    if css.is_empty() {
        return String::new();
    }
    let css = remove_comments(css);
    let id_selector = format!("#gd-{id}");
    isolate(&css, &id_selector)
}

/// `css.remove(QRegularExpression(R"(\/\*.*?\*\/)", DotMatchesEverything))`.
fn remove_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        match rest[start + 2..].find("*/") {
            Some(end) => {
                out.push_str(&rest[..start]);
                rest = &rest[start + 2 + end + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn isolate(css: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(css.len() * 2);
    let mut pos = 0;
    let len = css.len();
    while pos < len {
        let open_brace = css[pos..].find('{').map(|i| pos + i);
        let at_rule = css[pos..].find('@').map(|i| pos + i);
        match (at_rule, open_brace) {
            (Some(at), brace) if brace.is_none_or(|b| at < b) => {
                let semicolon = css[at..].find(';').map(|i| at + i);
                let brace = css[at..].find('{').map(|i| at + i);
                match (brace, semicolon) {
                    (Some(brace), semicolon) if semicolon.is_none_or(|s| brace < s) => {
                        let Some(matching) = find_matching_bracket(css, brace) else {
                            result.push_str(&css[at..]);
                            break;
                        };
                        let header = &css[at..=brace];
                        result.push_str(header);
                        let sub = &css[brace + 1..matching];
                        let lower = header.to_lowercase();
                        if ["@media", "@supports", "@container", "@layer"]
                            .iter()
                            .any(|rule| lower.contains(rule))
                        {
                            result.push_str(&isolate(&remove_comments(sub), prefix));
                        } else {
                            result.push_str(sub);
                        }
                        result.push('}');
                        pos = matching + 1;
                    }
                    (_, Some(semicolon)) => {
                        result.push_str(&css[at..=semicolon]);
                        pos = semicolon + 1;
                    }
                    _ => {
                        result.push_str(&css[at..]);
                        break;
                    }
                }
            }
            (_, Some(open)) => {
                result.push_str(&isolate_selectors(&css[pos..open], prefix));
                result.push_str(" {");
                match find_matching_bracket(css, open) {
                    Some(matching) => {
                        result.push_str(&css[open + 1..=matching]);
                        pos = matching + 1;
                    }
                    None => {
                        result.push_str(&css[open + 1..]);
                        break;
                    }
                }
            }
            _ => {
                result.push_str(&css[pos..]);
                break;
            }
        }
    }
    result
}

fn isolate_selectors(part: &str, id_selector: &str) -> String {
    const LEADERS: &[u8] = b" \t\r\n,>~+(";
    const FOLLOWERS: &[u8] = b" \t\r\n.#[:>~+,)";
    let mut isolated = Vec::new();
    for selector in part.split(',') {
        let mut s = selector.trim().to_owned();
        if s.is_empty() {
            continue;
        }
        for (tag, replacement) in [
            ("body", "gd-section-body"),
            ("html", "gd-section-html"),
            (":root", "gd-section-html"),
        ] {
            let mut p = 0;
            while let Some(found) = find_ignore_ascii_case(&s, tag, p) {
                let bytes = s.as_bytes();
                let leader = found == 0 || LEADERS.contains(&bytes[found - 1]);
                let end = found + tag.len();
                let follower = end == s.len() || FOLLOWERS.contains(&bytes[end]);
                if leader && follower {
                    s.replace_range(found..end, replacement);
                    p = found + replacement.len();
                } else {
                    p = end;
                }
            }
        }
        if s.starts_with(id_selector) {
            isolated.push(s);
        } else {
            if s == "gd-section-body" || s == "gd-section-html" {
                isolated.push(id_selector.to_owned());
            }
            isolated.push(format!("{id_selector} {s}"));
        }
    }
    isolated.join(", ")
}

/// `QString::indexOf(needle, from, Qt::CaseInsensitive)` for an ASCII needle.
fn find_ignore_ascii_case(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let bytes = haystack.as_bytes();
    (from..=bytes.len().checked_sub(needle.len())?)
        .find(|&i| bytes[i..i + needle.len()].eq_ignore_ascii_case(needle.as_bytes()))
}

/// The `}` closing the `{` at `start`, counting nesting and skipping
/// backslash-escaped characters.
fn find_matching_bracket(css: &str, start: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let mut depth = 1;
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::isolate_css;

    fn check(css: &str, expected: &str) {
        assert_eq!(isolate_css(css, "X"), expected, "{css}");
    }

    #[test]
    fn selectors_get_the_article_prefix() {
        check(".a{color:red}", "#gd-X .a {color:red}");
        check("p, .b > i{x:y}", "#gd-X p, #gd-X .b > i {x:y}");
        check("#gd-X p{}", "#gd-X p {}");
    }

    #[test]
    fn root_selectors_apply_to_the_article_and_compounds_match_nothing() {
        check("body{margin:0}", "#gd-X, #gd-X gd-section-body {margin:0}");
        check("HTML{}", "#gd-X, #gd-X gd-section-html {}");
        check(":root{}", "#gd-X, #gd-X gd-section-html {}");
        check("body .x{}", "#gd-X gd-section-body .x {}");
        check(".tbody{} bodyish{}", "#gd-X .tbody {}#gd-X bodyish {}");
    }

    #[test]
    fn at_rules_recurse_or_pass_through() {
        check("@media screen{p{a:b}}", "@media screen{#gd-X p {a:b}}");
        check(
            "@supports (x:y){@media print{b{}}}",
            "@supports (x:y){@media print{#gd-X b {}}}",
        );
        check("@font-face{font-family:F}", "@font-face{font-family:F}");
        check("@import \"a.css\";p{}", "@import \"a.css\";#gd-X p {}");
        check(".a{}  @media x{.b{}}", "#gd-X .a {}@media x{#gd-X .b {}}");
    }

    #[test]
    fn comments_escapes_and_unclosed_blocks() {
        check("/* c */.a{}/* d */", "#gd-X .a {}");
        check(
            ".a{content:\"\\}\"} .b{}",
            "#gd-X .a {content:\"\\}\"}#gd-X .b {}",
        );
        check(".a{color:red", "#gd-X .a {color:red");
        check("", "");
    }
}
