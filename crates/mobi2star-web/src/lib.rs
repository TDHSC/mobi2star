//! The browser page's converter: `mobi2star::convert_dictionary_zip` with
//! the page's choices, browser limits and a summary to show. Everything
//! here builds and tests natively; the WebAssembly bindings are a thin
//! layer over it.
use lexicon_core::{LabelLanguage, Limits, TargetReader};
use mobi2star::{Backend, ConversionReport, OutputOptions, Stage};
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
mod bindings;

/// What the page lets the user choose. The backend is detected and the
/// offset width stays at the portable 32 bits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choices {
    pub reader: TargetReader,
    pub labels: LabelLanguage,
}

/// A finished conversion, ready to download.
#[derive(Debug)]
pub struct Converted {
    pub zip: Vec<u8>,
    pub file_name: String,
    /// JSON for the page; see `Summary`.
    pub summary: String,
}

#[derive(Serialize)]
struct Summary<'a> {
    version: &'static str,
    folder: &'a str,
    file_name: &'a str,
    zip_bytes: usize,
    report: &'a ConversionReport,
}

/// Why a conversion failed: a stable code the page translates, and the
/// converter's English detail.
#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: &'static str,
    pub message: String,
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The largest file the page accepts, checked before reading it.
pub fn max_input_bytes() -> usize {
    Limits::browser().input_bytes
}

/// The values the page offers, as JSON: `{"readers": [..], "labels": [..]}`.
pub fn choices() -> String {
    serde_json::json!({
        "readers": TargetReader::ALL,
        "labels": LabelLanguage::ALL,
    })
    .to_string()
}

/// Converts `source` to a zip of its StarDict folder. `choices` is the
/// page's JSON; `progress` receives each `Stage`.
pub fn convert(
    source: Vec<u8>,
    choices: &str,
    progress: &mut dyn FnMut(Stage),
) -> Result<Converted, Failure> {
    let choices: Choices = serde_json::from_str(choices).map_err(|error| Failure {
        code: "OPTIONS",
        message: error.to_string(),
    })?;
    let options = OutputOptions {
        labels: choices.labels,
        reader: choices.reader,
        ..OutputOptions::default()
    };
    let archive = mobi2star::convert_dictionary_zip(
        source,
        &Limits::browser(),
        options,
        Backend::Auto,
        progress,
    )
    .map_err(|error| Failure {
        code: error.code(),
        message: error.to_string(),
    })?;
    let file_name = format!("{} (StarDict).zip", archive.folder);
    let summary = serde_json::to_string(&Summary {
        version: version(),
        folder: &archive.folder,
        file_name: &file_name,
        zip_bytes: archive.zip.len(),
        report: &archive.report,
    })
    .map_err(|error| Failure {
        code: "INTERNAL",
        message: error.to_string(),
    })?;
    Ok(Converted {
        zip: archive.zip,
        file_name,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const HUFF: &[u8] = include_bytes!("../../../tests/fixtures/huff.mobi");

    #[test]
    fn converts_with_the_pages_choices() {
        let mut stages = Vec::new();
        let converted = convert(
            HUFF.to_vec(),
            r#"{"reader":"readest","labels":"zh"}"#,
            &mut |stage| stages.push(stage),
        )
        .unwrap();
        let summary: serde_json::Value = serde_json::from_str(&converted.summary).unwrap();
        assert_eq!(summary["file_name"], converted.file_name.as_str());
        assert_eq!(summary["zip_bytes"], converted.zip.len());
        assert_eq!(summary["report"]["reader"], "readest");
        assert_eq!(summary["report"]["offset_bits"], 32);
        assert_eq!(summary["version"], version());
        assert!(converted.zip.starts_with(b"PK\x03\x04"));
        assert_eq!(stages.first(), Some(&Stage::Parsing));
        assert_eq!(stages.last(), Some(&Stage::Checking));
    }
    #[test]
    fn failures_carry_a_code() {
        let options = r#"{"reader":"koreader","labels":"en"}"#;
        let failure = convert(b"not a mobi".to_vec(), options, &mut |_| {}).unwrap_err();
        assert_eq!(failure.code, "MALFORMED");
        for bad in [
            r#"{"reader":"kindle","labels":"en"}"#,
            r#"{"reader":"koreader"}"#,
        ] {
            let failure = convert(HUFF.to_vec(), bad, &mut |_| {}).unwrap_err();
            assert_eq!(failure.code, "OPTIONS");
        }
    }
    #[test]
    fn choices_list_every_value() {
        let choices: serde_json::Value = serde_json::from_str(&choices()).unwrap();
        assert_eq!(
            choices["readers"],
            serde_json::to_value(TargetReader::ALL).unwrap()
        );
        assert_eq!(choices["labels"], serde_json::json!(["en", "zh"]));
    }
}
