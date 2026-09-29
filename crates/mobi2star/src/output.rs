//! Choices that shape the bytes of a bundle. Manifests record them, and
//! verification regenerates with the recorded values.
use lexicon_core::{LabelLanguage, TargetReader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputOptions {
    /// StarDict index offset width: 32 (portable) or 64 (reader support required).
    pub offset_bits: u8,
    /// Language of the lookup keys and viewer text mobi2star generates itself.
    pub labels: LabelLanguage,
    /// Reader the dictionary is built for; decides how payloads reference the
    /// stylesheet.
    pub reader: TargetReader,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            offset_bits: 32,
            labels: LabelLanguage::default(),
            reader: TargetReader::default(),
        }
    }
}
