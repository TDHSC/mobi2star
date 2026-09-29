//! How generated dictionary payloads reference the dictionary stylesheet.
//! Renderers decide what goes into each payload; stardict-io decides where
//! the stylesheet files live.

/// File name of the dictionary stylesheet. The companion copy sits next to
/// `dictionary.ifo` (KOReader); the linked copy sits in `res/` (GoldenDict).
pub const STYLESHEET_FILE: &str = "dictionary.css";

/// Per-payload reference to the linked stylesheet in `res/`. Self-closed so
/// that converters which copy entries into XHTML keep them well-formed.
pub const LINK_TAG: &str = "<link rel=\"stylesheet\" href=\"dictionary.css\"/>";

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
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_tag_names_the_stylesheet_file() {
        assert!(LINK_TAG.contains(&format!("href=\"{STYLESHEET_FILE}\"")));
        assert!(LINK_TAG.ends_with("/>"));
    }
}
