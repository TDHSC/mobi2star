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
pub mod goldendict_ng;
pub mod html;
pub mod koreader;
mod lookup;
pub mod url;
mod view;

pub use app::{App, Engine, Facts, Fidelity, StyleSource};
pub use dictionary::Dictionary;
pub use lookup::is_internal_key;
pub use view::{Match, Outcome, Stay, View};
