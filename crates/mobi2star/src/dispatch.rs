//! Backend selection is explicit and fail-closed: an SRCS conversion failure
//! propagates to the caller, preserving its diagnostics and rollback semantics.
use crate::OutputOptions;
use lexicon_core::{checked_member, read_bounded, Limits, Result};
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    Auto,
    Srcs,
    Compiled,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConversionReport {
    Source(crate::SourceReport),
    Compiled(crate::Report),
}
impl ConversionReport {
    pub fn source_headwords(&self) -> usize {
        match self {
            Self::Source(r) => r.source_headwords,
            Self::Compiled(r) => r.source_headwords,
        }
    }
    pub fn source_aliases(&self) -> usize {
        match self {
            Self::Source(r) => r.source_aliases,
            Self::Compiled(r) => r.source_aliases,
        }
    }
    pub fn supplement_entries(&self) -> usize {
        match self {
            Self::Source(r) => r.supplement_entries,
            Self::Compiled(r) => r.supplement_entries,
        }
    }
}
pub fn convert_with_backend(
    input: &Path,
    output: &Path,
    limits: &Limits,
    options: OutputOptions,
    backend: Backend,
) -> Result<(PathBuf, ConversionReport)> {
    let selected = match backend {
        Backend::Auto => {
            let bytes = read_bounded(input, limits.input_bytes)?;
            let mobi = mobi_reader::Container::open(&bytes, limits)?;
            if mobi.source_archive()?.is_some() {
                Backend::Srcs
            } else {
                Backend::Compiled
            }
        }
        other => other,
    };
    match selected {
        Backend::Srcs => {
            let (path, report) = crate::convert_source(input, output, limits, options)?;
            Ok((path, ConversionReport::Source(report)))
        }
        Backend::Compiled | Backend::Auto => {
            let (path, report) = crate::convert(input, output, limits, options)?;
            Ok((path, ConversionReport::Compiled(report)))
        }
    }
}
pub fn verify_bundle(
    root: &Path,
    source: Option<&Path>,
    limits: &Limits,
) -> Result<ConversionReport> {
    let bytes = read_bounded(&checked_member(root, "manifest.json")?, 16 * 1024 * 1024)?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    if manifest.get("backend").and_then(serde_json::Value::as_str)
        == Some(crate::source_bundle::BACKEND)
    {
        Ok(ConversionReport::Source(crate::verify_source(
            root, source, limits,
        )?))
    } else {
        Ok(ConversionReport::Compiled(crate::verify(
            root, source, limits,
        )?))
    }
}
pub fn dictionary_root(root: &Path) -> PathBuf {
    let nested = root.join("StarDict");
    if nested.is_dir() {
        nested
    } else {
        root.to_path_buf()
    }
}
