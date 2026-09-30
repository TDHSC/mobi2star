//! The page's preview: a converted dictionary opened from its zip, shown
//! as the chosen reader shows it. Plain Rust with JSON in and out, tested
//! natively; the WebAssembly bindings only forward to it.
use crate::Failure;
use lexicon_core::{Limits, TargetReader};
use reader_view::{koreader, App, Dictionary};

/// A dictionary opened for previewing in the readers of one choice.
pub struct Session {
    dictionary: Dictionary,
    apps: &'static [App],
}

fn failure(error: lexicon_core::Error) -> Failure {
    Failure {
        code: error.code(),
        message: error.to_string(),
    }
}

fn app(name: &str) -> Result<App, Failure> {
    serde_json::from_value(serde_json::Value::String(name.into())).map_err(|_| Failure {
        code: "OPTIONS",
        message: format!("unknown reader {name:?}"),
    })
}

fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

impl Session {
    /// Opens the zip a conversion produced, for the readers of `reader`
    /// (a `--reader` value).
    pub fn open(zip: Vec<u8>, reader: &str) -> Result<Self, Failure> {
        let target: TargetReader = serde_json::from_value(serde_json::Value::String(reader.into()))
            .map_err(|_| Failure {
                code: "OPTIONS",
                message: format!("unknown reader choice {reader:?}"),
            })?;
        let dictionary = Dictionary::from_zip(zip, &Limits::browser()).map_err(failure)?;
        Ok(Self {
            dictionary,
            apps: App::for_target(target),
        })
    }
    /// `{"bookname", "apps", "entries"}`.
    pub fn info(&self) -> String {
        serde_json::json!({
            "bookname": self.dictionary.bookname(),
            "apps": self.apps,
            "entries": self.dictionary.index().entries.len(),
        })
        .to_string()
    }
    fn app(&self, name: &str) -> Result<App, Failure> {
        let app = app(name)?;
        if self.apps.contains(&app) {
            Ok(app)
        } else {
            Err(Failure {
                code: "OPTIONS",
                message: format!("{name} is not previewed for this dictionary"),
            })
        }
    }
    /// A lookup in reader `app`, as a JSON `Outcome`.
    pub fn search(&self, app: &str, word: &str) -> Result<String, Failure> {
        let outcome =
            reader_view::search(&self.dictionary, self.app(app)?, word).map_err(failure)?;
        Ok(json(&outcome))
    }
    /// A followed link in reader `app`, as a JSON `Outcome`.
    pub fn follow(&self, app: &str, href: &str, current: usize) -> Result<String, Failure> {
        let outcome = reader_view::follow(&self.dictionary, self.app(app)?, href, current)
            .map_err(failure)?;
        Ok(json(&outcome))
    }
    /// Search suggestions for `prefix`, as a JSON list.
    pub fn suggest(&self, prefix: &str, limit: usize) -> String {
        json(&self.dictionary.suggest(prefix, limit))
    }
    /// A headword about `fraction` (0 to 1) of the way through the index,
    /// skipping internal keys, for "show me something".
    pub fn headword_at(&self, fraction: f64) -> Option<String> {
        let entries = &self.dictionary.index().entries;
        let start = ((fraction.clamp(0.0, 1.0) * entries.len() as f64) as usize).min(entries.len());
        entries[start..]
            .iter()
            .chain(&entries[..start])
            .map(|entry| &entry.word)
            .find(|word| !reader_view::is_internal_key(word))
            .cloned()
    }
}

/// KOReader's text box for a preset screen, as JSON `{width, height, em}`.
pub fn koreader_geometry(screen: &str, font_size: u32) -> Option<String> {
    let screen = koreader::SCREENS.iter().find(|s| s.id == screen)?;
    Some(json(&koreader::Geometry::for_screen(
        screen.width,
        screen.height,
        font_size.clamp(8, 32),
    )))
}

/// The font files KOReader ships for its dictionary popup, the fallback
/// last, as a JSON list.
pub fn koreader_fonts() -> String {
    let mut fonts: Vec<&str> = koreader::NOTO_SANS.to_vec();
    fonts.push(koreader::FALLBACK_FONT);
    json(&fonts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mobi2star::{Backend, OutputOptions};

    const SRCS: &[u8] = include_bytes!("../../../tests/fixtures/srcs.mobi");

    fn session(reader: &str) -> Session {
        let archive = mobi2star::convert_dictionary_zip(
            SRCS.to_vec(),
            &Limits::browser(),
            OutputOptions::default(),
            Backend::Auto,
            &mut |_| {},
        )
        .unwrap();
        Session::open(archive.zip, reader).unwrap()
    }

    #[test]
    fn a_session_answers_in_json() {
        let s = session("universal");
        let info: serde_json::Value = serde_json::from_str(&s.info()).unwrap();
        assert_eq!(info["bookname"], "Original Rust SRCS fixture");
        assert_eq!(info["apps"].as_array().unwrap().len(), 5);
        let found: serde_json::Value =
            serde_json::from_str(&s.search("koreader", "run").unwrap()).unwrap();
        assert_eq!(found["outcome"], "view");
        assert_eq!(found["results"].as_array().unwrap().len(), 2);
        let missing: serde_json::Value =
            serde_json::from_str(&s.search("readest", "fly").unwrap()).unwrap();
        assert_eq!(
            missing,
            serde_json::json!({"outcome": "not-found", "word": "fly"})
        );
        let stay: serde_json::Value =
            serde_json::from_str(&s.follow("readest", "bword://x", 0).unwrap()).unwrap();
        assert_eq!(
            stay,
            serde_json::json!({"outcome": "stay", "reason": "not-followed"})
        );
        assert_eq!(s.suggest("r", 5), "[\"run\",\"runs\"]");
        assert!(s
            .headword_at(0.99)
            .is_some_and(|w| !reader_view::is_internal_key(&w)));
    }

    #[test]
    fn only_the_choices_readers_are_previewed() {
        let s = session("readest");
        assert_eq!(s.search("koreader", "run").unwrap_err().code, "OPTIONS");
        assert_eq!(s.search("kindle", "run").unwrap_err().code, "OPTIONS");
        assert!(s.search("readest", "run").is_ok());
        assert_eq!(
            Session::open(b"x".to_vec(), "koreader").err().unwrap().code,
            "MALFORMED"
        );
        assert_eq!(
            Session::open(Vec::new(), "nope").err().unwrap().code,
            "OPTIONS"
        );
    }

    #[test]
    fn koreader_helpers() {
        let geometry: serde_json::Value =
            serde_json::from_str(&koreader_geometry("6in-300ppi", 20).unwrap()).unwrap();
        assert_eq!(
            geometry,
            serde_json::json!({"width": 854, "height": 517, "em": 36})
        );
        assert!(koreader_geometry("nope", 20).is_none());
        assert!(koreader_fonts().ends_with("\"NotoSansCJKsc-Regular.otf\"]"));
    }
}
