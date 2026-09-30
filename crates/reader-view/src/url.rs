//! The lenient URL decoding readers apply to links and image paths. The
//! converter's strict decoder rejects malformed input; readers do not.

/// Decodes `%XX` escapes. Malformed escapes and `+` are kept as they are;
/// bytes that do not form UTF-8 become U+FFFD.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The path under `res/` that a web-engine reader resolves a URL in an
/// entry to: percent-decoded, without a leading slash. `data:`, `http:`,
/// `https:` and `ftp:` URLs are not resources (GoldenDict-ng's
/// `handleResource`).
pub fn resource_path(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if ["data:", "http:", "https:", "ftp:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return None;
    }
    Some(percent_decode(url).trim_start_matches('/').to_owned())
}

/// MuPDF's `fz_cleanname`: drops empty and `.` segments and resolves `..`
/// where it can. KOReader's image paths and relative links pass through it.
pub fn clean_path(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    match (rooted, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoding_is_lenient() {
        assert_eq!(percent_decode("a%20b%zz%2"), "a b%zz%2");
        assert_eq!(percent_decode("caf%C3%A9+x"), "café+x");
        assert_eq!(percent_decode("%FF"), "\u{fffd}");
        assert_eq!(percent_decode("%41"), "A");
    }
    #[test]
    fn resources_are_decoded_paths_without_a_scheme() {
        assert_eq!(resource_path("/a%20b.png").as_deref(), Some("a b.png"));
        assert_eq!(resource_path("HTTPS://x/a.png"), None);
        assert_eq!(resource_path("data:image/png;base64,AA"), None);
    }
    #[test]
    fn paths_are_cleaned_like_mupdf() {
        assert_eq!(clean_path("./a//b/./c"), "a/b/c");
        assert_eq!(clean_path("a/../../b"), "../b");
        assert_eq!(clean_path("/../a"), "/a");
        assert_eq!(clean_path("./"), ".");
    }
}
