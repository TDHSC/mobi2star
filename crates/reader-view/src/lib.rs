//! How dictionary readers display a StarDict dictionary, for previews.
//!
//! Each reader module reproduces one reader's own rules, taken from its
//! source code: which stylesheets it applies, how it wraps an entry, how it
//! looks words up and how it follows links. Rendering happens elsewhere
//! (MuPDF, or a browser engine); this crate produces what they are given.
//!
//! Licensed AGPL-3.0-or-later, unlike the rest of the workspace, because it
//! ports behaviour from GoldenDict-ng (GPL-3.0) and KOReader (AGPL-3.0). The
//! converter never depends on it.
mod app;
mod archive;
mod dictionary;
pub mod goldendict_mobile;
pub mod goldendict_ng;
pub mod html;
pub mod kobo;
pub mod koreader;
mod lookup;
pub mod readest;
pub mod url;
mod view;

pub use app::{App, Engine, Facts, Fidelity, StyleSource};
pub use dictionary::Dictionary;
pub use lookup::is_internal_key;
pub use view::{Match, Outcome, Stay, View};

use lexicon_core::Result;

/// Looks `word` up the way `app` does.
pub fn search(dictionary: &Dictionary, app: App, word: &str) -> Result<Outcome> {
    match app {
        App::Koreader => koreader::search(dictionary, word),
        App::GoldendictNg => goldendict_ng::search(dictionary, word),
        App::GoldendictMobile => goldendict_mobile::search(dictionary, word),
        App::Readest => readest::search(dictionary, word),
        App::KoboPyglossary => kobo::search(dictionary, word),
    }
}

/// Follows a link the way `app` does. `href` is what the reader receives:
/// for KOReader the URI MuPDF reports, otherwise the `href` attribute.
/// `current` is the entry being shown.
pub fn follow(dictionary: &Dictionary, app: App, href: &str, current: usize) -> Result<Outcome> {
    match app {
        App::Koreader => koreader::follow(dictionary, href, current),
        App::GoldendictNg => goldendict_ng::follow(dictionary, href),
        App::GoldendictMobile => Ok(goldendict_mobile::follow()),
        App::Readest => Ok(readest::follow(href)),
        App::KoboPyglossary => Ok(kobo::follow()),
    }
}
