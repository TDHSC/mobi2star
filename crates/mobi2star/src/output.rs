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

/// What a conversion produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Profile {
    /// The full bundle: dictionary, audit files, report and manifest; verifiable later
    #[default]
    Bundle,
    /// Only StarDict/ and report.json, checked while converting; no manifest, so `verify` cannot check it later
    Stardict,
}

/// Progress of a conversion, reported by count, never by time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "stage", rename_all = "kebab-case")]
pub enum Stage {
    /// Reading and cross-checking the source.
    Parsing,
    /// Rendering and writing payloads.
    Rendering { done: usize, total: usize },
    /// Writing the index files.
    Writing,
    /// Reading the output back and checking it.
    Checking,
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

#[cfg(test)]
mod tests {
    use clap::ValueEnum;
    use lexicon_core::{LabelLanguage, TargetReader};
    #[test]
    fn cli_values_are_the_manifest_spellings() {
        assert_eq!(TargetReader::value_variants(), TargetReader::ALL);
        for reader in TargetReader::ALL {
            let cli = reader.to_possible_value().unwrap();
            assert_eq!(serde_json::to_value(reader).unwrap(), cli.get_name());
        }
        assert_eq!(LabelLanguage::value_variants(), LabelLanguage::ALL);
        for labels in LabelLanguage::value_variants() {
            let cli = labels.to_possible_value().unwrap();
            assert_eq!(serde_json::to_value(labels).unwrap(), cli.get_name());
        }
    }
}
