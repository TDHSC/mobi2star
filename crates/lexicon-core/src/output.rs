//! How generated dictionary payloads reference the dictionary stylesheet.
//! Renderers decide what goes into each payload; stardict-io decides where
//! the stylesheet files live.
use serde::{Deserialize, Serialize};

/// The reader a dictionary is built for. Readers differ in how they load a
/// dictionary stylesheet; see docs/READERS.md for the evidence behind each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetReader {
    /// Loads the `.ifo`-named stylesheet; ignores `<style>` inside entries.
    #[default]
    Koreader,
    /// GoldenDict / GoldenDict-ng desktop: loads linked `res/` stylesheets,
    /// scoped to the dictionary.
    Goldendict,
    /// Applies only stylesheets inside each entry.
    GoldendictMobile,
    /// Applies only stylesheets inside each entry.
    Readest,
    /// For conversion to Kobo with PyGlossary or penelope, which keep only
    /// stylesheets inside each entry.
    Kobo,
    /// Link and inline copy, for mixed or untested readers.
    Universal,
}

impl TargetReader {
    pub const ALL: [Self; 6] = [
        Self::Koreader,
        Self::Goldendict,
        Self::GoldendictMobile,
        Self::Readest,
        Self::Kobo,
        Self::Universal,
    ];
    pub fn style_delivery(self) -> StyleDelivery {
        match self {
            Self::Koreader | Self::Goldendict => StyleDelivery {
                link: true,
                inline: false,
            },
            Self::GoldendictMobile | Self::Readest | Self::Kobo => StyleDelivery::INLINE,
            Self::Universal => StyleDelivery {
                link: true,
                inline: true,
            },
        }
    }
}

/// File name of the dictionary stylesheet. The companion copy sits next to
/// `dictionary.ifo` (KOReader); the linked copy sits in `res/` (GoldenDict).
pub const STYLESHEET_FILE: &str = "dictionary.css";

/// Per-payload reference to the linked stylesheet in `res/`.
/// - Self-closed, so converters that copy entries into XHTML keep them
///   well-formed.
/// - Hidden inline: MuPDF (KOReader) does not hide `link` by default, and an
///   unhidden one in the body adds an empty box that stops the first block's
///   top margin from collapsing.
pub const LINK_TAG: &str =
    "<link rel=\"stylesheet\" href=\"dictionary.css\" style=\"display:none\"/>";

/// Which stylesheet references each payload carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleDelivery {
    /// Start the payload with [`LINK_TAG`].
    pub link: bool,
    /// Start the payload with its own `<style>` copy.
    pub inline: bool,
}

impl StyleDelivery {
    /// Every payload carries its own copy and nothing else.
    pub const INLINE: Self = Self {
        link: false,
        inline: true,
    };
    /// Stylesheet references a payload starts with when `css` applies to it:
    /// the link, then an inline copy. CSS that is empty needs none.
    pub fn references(self, css: &str) -> String {
        let mut out = String::new();
        if css.is_empty() {
            return out;
        }
        if self.link {
            out.push_str(LINK_TAG);
        }
        if self.inline {
            out.push_str("<style>");
            out.push_str(css);
            out.push_str("</style>");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readers_map_to_their_delivery() {
        let delivery = |r: TargetReader| {
            let d = r.style_delivery();
            (d.link, d.inline)
        };
        assert_eq!(TargetReader::default(), TargetReader::Koreader);
        assert_eq!(delivery(TargetReader::Koreader), (true, false));
        assert_eq!(delivery(TargetReader::Goldendict), (true, false));
        for inline_only in [
            TargetReader::GoldendictMobile,
            TargetReader::Readest,
            TargetReader::Kobo,
        ] {
            assert_eq!(delivery(inline_only), (false, true));
        }
        assert_eq!(delivery(TargetReader::Universal), (true, true));
    }
    #[test]
    fn manifest_spelling_is_kebab_case() {
        let names: Vec<String> = TargetReader::ALL
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "\"koreader\"",
                "\"goldendict\"",
                "\"goldendict-mobile\"",
                "\"readest\"",
                "\"kobo\"",
                "\"universal\""
            ]
        );
    }
    #[test]
    fn references_follow_the_delivery() {
        let both = StyleDelivery {
            link: true,
            inline: true,
        };
        assert_eq!(
            StyleDelivery::INLINE.references("p{}"),
            "<style>p{}</style>"
        );
        assert_eq!(
            both.references("p{}"),
            format!("{LINK_TAG}<style>p{{}}</style>")
        );
        assert_eq!(both.references(""), "");
    }
    #[test]
    fn link_tag_names_the_stylesheet_file() {
        assert!(LINK_TAG.contains(&format!("href=\"{STYLESHEET_FILE}\"")));
        assert!(LINK_TAG.contains("style=\"display:none\"") && LINK_TAG.ends_with("/>"));
    }
}
