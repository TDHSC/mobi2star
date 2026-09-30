//! What a lookup or a followed link leads to. Everything user-facing is a
//! code; the page translates it.
use crate::App;
use serde::Serialize;

/// One matched entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Match {
    /// The index headword, which may differ from the query (an inflection
    /// or a synonym leads to its headword).
    pub headword: String,
    /// Ordinal of the entry in the `.idx`.
    pub entry: usize,
}

/// A reader showing the results of one lookup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct View {
    pub app: App,
    /// What was looked up.
    pub query: String,
    /// The matches, in the reader's order.
    pub results: Vec<Match>,
    /// The documents the engine draws: one per result for readers that
    /// show one result at a time (KOReader), otherwise one for all.
    pub documents: Vec<String>,
    /// An element id to scroll to once drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_to: Option<String>,
}

/// Why following a link leaves the reader where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stay {
    /// The link's target is in the entry already shown, and the reader
    /// does not scroll to it (KOReader).
    AnchorInEntry,
    /// A web link; readers hand it to the browser, which a preview must not.
    ExternalLink,
    /// The reader does not follow dictionary links (Readest).
    NotFollowed,
    /// What the reader does with the link is not known.
    Unknown,
}
impl Stay {
    pub const ALL: &'static [Self] = &[
        Self::AnchorInEntry,
        Self::ExternalLink,
        Self::NotFollowed,
        Self::Unknown,
    ];
}

/// The result of a search or a followed link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    View(View),
    /// Stay on the current view and scroll to an element id.
    Scroll {
        id: String,
    },
    Stay {
        reason: Stay,
    },
    /// The reader finds nothing for `word`.
    NotFound {
        word: String,
    },
}
