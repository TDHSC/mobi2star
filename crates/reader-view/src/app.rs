//! The readers a preview emulates, and the facts about each that generic
//! code and the page need. How each reader composes, looks up and follows
//! links lives in its own module.
use lexicon_core::TargetReader;
use serde::{Deserialize, Serialize};

/// A reader application the preview can emulate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum App {
    Koreader,
    GoldendictNg,
    GoldendictMobile,
    Readest,
    /// A Kobo dictionary made from the StarDict folder with PyGlossary.
    KoboPyglossary,
}

/// What lays out and draws the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    /// MuPDF, as in KOReader.
    Mupdf,
    /// A web engine: the page's own browser.
    Web,
}

/// Where a reader takes a dictionary's styles from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StyleSource {
    /// The `.css` next to the `.ifo`, with the same basename.
    Companion,
    /// A stylesheet in `res/` that the entry links to.
    Linked,
    /// A `<style>` element inside the entry.
    Inline,
}

/// How well established the emulation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fidelity {
    /// The reader's own engine family and rules, both from its source.
    SameEngine,
    /// The reader's rules from its source, drawn by the page's browser.
    SameRules,
    /// Rules from user reports; the reader's source is not available.
    Reported,
    /// The conversion tool's rules from its source; the device's engine is
    /// unknown.
    EngineUnknown,
}

/// What generic code and the page need to know about a reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub engine: Engine,
    /// Style sources the reader applies, in cascade order.
    pub styles: &'static [StyleSource],
    /// Whether files in `res/` (images) reach the entry.
    pub resources: bool,
    pub fidelity: Fidelity,
}

impl App {
    pub const ALL: &'static [Self] = &[
        Self::Koreader,
        Self::GoldendictNg,
        Self::GoldendictMobile,
        Self::Readest,
        Self::KoboPyglossary,
    ];

    pub fn facts(self) -> Facts {
        use StyleSource::*;
        match self {
            Self::Koreader => Facts {
                engine: Engine::Mupdf,
                styles: &[Companion],
                resources: true,
                fidelity: Fidelity::SameEngine,
            },
            Self::GoldendictNg => Facts {
                engine: Engine::Web,
                styles: &[Linked, Inline],
                resources: true,
                fidelity: Fidelity::SameRules,
            },
            Self::GoldendictMobile => Facts {
                engine: Engine::Web,
                styles: &[Inline],
                resources: true,
                fidelity: Fidelity::Reported,
            },
            Self::Readest => Facts {
                engine: Engine::Web,
                styles: &[Inline],
                resources: false,
                fidelity: Fidelity::SameRules,
            },
            Self::KoboPyglossary => Facts {
                engine: Engine::Web,
                styles: &[Inline],
                resources: false,
                fidelity: Fidelity::EngineUnknown,
            },
        }
    }

    /// The readers a `--reader` choice is made for; `universal` is for all.
    pub fn for_target(target: TargetReader) -> &'static [Self] {
        match target {
            TargetReader::Koreader => &[Self::Koreader],
            TargetReader::Goldendict => &[Self::GoldendictNg],
            TargetReader::GoldendictMobile => &[Self::GoldendictMobile],
            TargetReader::Readest => &[Self::Readest],
            TargetReader::Kobo => &[Self::KoboPyglossary],
            TargetReader::Universal => Self::ALL,
        }
    }

    /// Whether a dictionary written with `target`'s stylesheet delivery
    /// gets its styles to this reader. The companion file is always written.
    pub fn is_styled_by(self, target: TargetReader) -> bool {
        let delivery = target.style_delivery();
        self.facts().styles.iter().any(|source| match source {
            StyleSource::Companion => true,
            StyleSource::Linked => delivery.link,
            StyleSource::Inline => delivery.inline,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/READERS.md's "Use" column as an invariant: every reader a choice
    /// is made for, and the extra readers the README says it also serves,
    /// receives the dictionary's styles.
    #[test]
    fn every_choice_styles_the_readers_it_is_for() {
        for &target in TargetReader::ALL {
            for &app in App::for_target(target) {
                assert!(
                    app.is_styled_by(target),
                    "{target:?} leaves {app:?} unstyled"
                );
            }
        }
        // "koreader: also works for GoldenDict desktop"
        assert!(App::GoldendictNg.is_styled_by(TargetReader::Koreader));
        // Link-only output leaves the inline-only readers bare.
        assert!(!App::Readest.is_styled_by(TargetReader::Koreader));
        assert!(!App::KoboPyglossary.is_styled_by(TargetReader::Goldendict));
    }

    #[test]
    fn names_are_stable() {
        let names: Vec<String> = App::ALL
            .iter()
            .map(|app| {
                serde_json::to_value(app)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            names,
            [
                "koreader",
                "goldendict-ng",
                "goldendict-mobile",
                "readest",
                "kobo-pyglossary"
            ]
        );
    }
}
