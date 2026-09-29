//! Where a dictionary keeps its stylesheet, relative to `dictionary.ifo`.
//! KOReader loads the companion file with the `.ifo` basename; payloads that
//! carry `LINK_TAG` load the copy in `res/` (GoldenDict).
use lexicon_core::{StyleDelivery, STYLESHEET_FILE};

/// Every path a stylesheet file may occupy, whether or not it is written.
pub fn stylesheet_paths() -> [String; 2] {
    [STYLESHEET_FILE.to_owned(), format!("res/{STYLESHEET_FILE}")]
}

/// Stylesheet files to write for `css`: always the companion file, plus the
/// `res/` copy when payloads link to it. A dictionary without CSS gets none.
pub fn stylesheet_files(css: &str, delivery: StyleDelivery) -> Vec<(String, &[u8])> {
    if css.is_empty() {
        return Vec::new();
    }
    let [companion, linked] = stylesheet_paths();
    let mut files = vec![(companion, css.as_bytes())];
    if delivery.link {
        files.push((linked, css.as_bytes()));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn companion_shares_the_ifo_basename() {
        // write_catalog and open use "dictionary.ifo".
        assert_eq!(
            Path::new(STYLESHEET_FILE).file_stem(),
            Some("dictionary".as_ref())
        );
    }
    #[test]
    fn files_follow_the_delivery() {
        let paths = |d| {
            stylesheet_files("p{}", d)
                .into_iter()
                .map(|(path, bytes)| {
                    assert_eq!(bytes, b"p{}");
                    path
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(StyleDelivery::INLINE), ["dictionary.css"]);
        let link = StyleDelivery {
            link: true,
            inline: false,
        };
        assert_eq!(paths(link), ["dictionary.css", "res/dictionary.css"]);
        assert!(stylesheet_files("", link).is_empty());
    }
}
