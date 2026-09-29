//! Package paths use URL/POSIX rules on every host; never OS path joining.
use crate::model::Target;
use lexicon_core::{Error, Result};
use unicode_normalization::UnicodeNormalization;

pub fn validate_path(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 4096
        || name.split('/').count() > 64
        || name.starts_with('/')
        || name.contains(['\\', ':'])
        || name.chars().any(char::is_control)
    {
        return Err(Error::Malformed(format!("unsafe package path {name:?}")));
    }
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) {
            return Err(Error::Malformed(format!(
                "unsafe package path component {part:?}"
            )));
        }
    }
    Ok(())
}
/// Conservative collision key for common macOS case/normalization-insensitive volumes.
pub fn collision_key(path: &str) -> String {
    path.nfd().flat_map(char::to_lowercase).collect()
}
pub fn percent_decode(text: &str) -> Result<String> {
    let input = text.as_bytes();
    let mut out = Vec::new();
    let mut p = 0;
    while p < input.len() {
        if input[p] == b'%' {
            let pair = input
                .get(p + 1..p + 3)
                .ok_or_else(|| Error::Malformed("truncated URL escape".into()))?;
            let a = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| Error::Malformed("URL escape digit".into()))?;
            let b = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| Error::Malformed("URL escape digit".into()))?;
            out.push((16 * a + b) as u8);
            p += 3;
        } else {
            out.push(input[p]);
            p += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::Malformed("URL is not UTF-8".into()))
}
pub fn percent_encode(text: &str, slash: bool) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') || slash && b == b'/'
        {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
pub fn resolve(file: &str, url: &str) -> Result<Option<Target>> {
    if url.trim() != url || url.chars().any(char::is_control) || url.contains('\\') {
        return Err(Error::Malformed(format!("unsafe URL {url:?}")));
    }
    if url.starts_with("//") {
        return Err(Error::Unsupported("scheme-relative remote URL".into()));
    }
    if let Some((scheme, _)) = url.split_once(':') {
        if !scheme.contains(['/', '#', '?']) {
            return match scheme.to_ascii_lowercase().as_str() {
                "https" | "http" | "mailto" => Ok(None),
                _ => Err(Error::Unsupported(format!("URL scheme {scheme}"))),
            };
        }
    }
    let (path, anchor) = url.split_once('#').unwrap_or((url, ""));
    if path.contains('?') {
        return Err(Error::Unsupported("local URL query".into()));
    }
    let path = percent_decode(path)?;
    let anchor = percent_decode(anchor)?;
    if anchor.chars().any(char::is_control) {
        return Err(Error::Malformed("control character in fragment".into()));
    }
    if path.is_empty() {
        return Ok(Some(Target {
            file: file.into(),
            anchor,
        }));
    }
    if path.starts_with('/') || path.contains(['\\', ':']) {
        return Err(Error::Malformed("absolute/ambiguous local URL".into()));
    }
    let mut parts: Vec<&str> = file
        .rsplit_once('/')
        .map(|(d, _)| d.split('/').collect())
        .unwrap_or_default();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::Malformed("local URL escapes package".into()));
                }
            }
            _ => parts.push(part),
        }
    }
    let result = parts.join("/");
    validate_path(&result)?;
    Ok(Some(Target {
        file: result,
        anchor,
    }))
}
pub fn relative(from: &str, to: &str) -> String {
    let parent = from.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let a: Vec<&str> = parent.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = to.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = vec![".."; a.len() - common];
    out.extend_from_slice(&b[common..]);
    out.join("/")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_inside_package() {
        assert_eq!(
            resolve("OEBPS/a.xhtml", "images/a%20b.png")
                .unwrap()
                .unwrap()
                .file,
            "OEBPS/images/a b.png"
        );
    }
    #[test]
    fn rejects_escape_and_active_schemes() {
        for url in [
            "../../evil",
            "%2fetc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
            "x%00.png",
            "bad%FF",
            "//host/file",
            "%2e%2e/%2e%2e/file",
        ] {
            assert!(resolve("OEBPS/a.xhtml", url).is_err(), "{url}");
        }
    }
    #[test]
    fn anchor_and_relative() {
        assert_eq!(
            resolve("x/a.xhtml", "#caf%C3%A9").unwrap().unwrap().anchor,
            "café"
        );
        assert_eq!(relative("x/y/a.html", "x/z/b.html"), "../z/b.html");
    }
    #[test]
    fn unicode_collision() {
        assert_eq!(collision_key("É.png"), collision_key("e\u{301}.PNG"));
    }
    #[test]
    fn percent_roundtrip() {
        let s = "café <x>/字";
        assert_eq!(percent_decode(&percent_encode(s, true)).unwrap(), s);
    }
}
