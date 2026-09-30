//! Backend selection is explicit and fail-closed: an SRCS conversion failure
//! propagates to the caller, preserving its diagnostics and rollback semantics.
use crate::{
    source_bundle::DICTIONARY_DIR,
    transaction::{sync_tree, Transaction},
    tree::{folder_name, DiskTree, Tree, ZipTree},
    OutputOptions, Stage,
};
use lexicon_core::{read_bounded, Error, Limits, Result};
use std::path::{Path, PathBuf};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Backend {
    /// The embedded publisher source when the MOBI has one, else the compiled text
    #[default]
    Auto,
    /// The embedded publisher source (SRCS record)
    Srcs,
    /// The compiled MOBI text
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
impl Backend {
    /// The backend to run: `Auto` becomes the one the source calls for.
    /// `source` is called only for `Auto`.
    fn resolve<S: AsRef<[u8]>>(
        self,
        limits: &Limits,
        source: impl FnOnce() -> Result<S>,
    ) -> Result<Backend> {
        if self != Backend::Auto {
            return Ok(self);
        }
        let source = source()?;
        let mobi = mobi_reader::Container::open(source.as_ref(), limits)?;
        Ok(if mobi.source_archive()?.is_some() {
            Backend::Srcs
        } else {
            Backend::Compiled
        })
    }
}
/// Converts to the full bundle, verified and published as `OUTPUT/bundle`.
pub fn convert_with_backend(
    input: &Path,
    output: &Path,
    limits: &Limits,
    options: OutputOptions,
    backend: Backend,
) -> Result<(PathBuf, ConversionReport)> {
    match backend.resolve(limits, || read_bounded(input, limits.input_bytes))? {
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
/// Builds only the dictionary into `tree`, in the folder `folder` names
/// from the book title (with a trailing `/`). Shared by the CLI's
/// dictionary-only profile and the zip archive.
pub(crate) fn build_dictionary(
    source: Vec<u8>,
    tree: &mut impl Tree,
    folder: impl FnOnce(&str) -> String,
    limits: &Limits,
    options: OutputOptions,
    backend: Backend,
    progress: &mut dyn FnMut(Stage),
) -> Result<ConversionReport> {
    progress(Stage::Parsing);
    Ok(match backend.resolve(limits, || Ok(source.as_slice()))? {
        Backend::Srcs => ConversionReport::Source(crate::source_bundle::build_dictionary(
            &source, tree, folder, limits, options, progress,
        )?),
        Backend::Compiled | Backend::Auto => ConversionReport::Compiled(
            crate::bundle::build_dictionary(source, tree, folder, limits, options, progress)?,
        ),
    })
}
/// Converts to the dictionary-only profile and publishes `OUTPUT/bundle`
/// holding `StarDict/` and `report.json`. It runs the checks that happen
/// during conversion, but writes no manifest, so `verify` cannot check it
/// later.
pub fn convert_dictionary(
    input: &Path,
    output: &Path,
    limits: &Limits,
    options: OutputOptions,
    backend: Backend,
    progress: &mut dyn FnMut(Stage),
) -> Result<(PathBuf, ConversionReport)> {
    let source = read_bounded(input, limits.input_bytes)?;
    let tx = Transaction::begin(output)?;
    let root = tx.path()?;
    let mut tree = DiskTree::new(root, limits);
    let report = build_dictionary(
        source,
        &mut tree,
        |_| DICTIONARY_DIR.into(),
        limits,
        options,
        backend,
        progress,
    )?;
    tree.put_json("report.json", &report)?;
    sync_tree(root)?;
    Ok((tx.commit()?, report))
}
/// A dictionary packed as a zip archive.
#[derive(Clone, Debug)]
pub struct DictionaryArchive {
    /// The archive: one folder holding the StarDict files.
    pub zip: Vec<u8>,
    /// That folder's name, made from the dictionary title.
    pub folder: String,
    pub report: ConversionReport,
}
/// Converts `source` to the dictionary-only profile in memory and packs it
/// as a zip archive, read back and checked like `convert_dictionary`'s
/// output. It uses no filesystem and no clock, so it runs in WebAssembly.
pub fn convert_dictionary_zip(
    source: Vec<u8>,
    limits: &Limits,
    options: OutputOptions,
    backend: Backend,
    progress: &mut dyn FnMut(Stage),
) -> Result<DictionaryArchive> {
    if source.len() > limits.input_bytes {
        return Err(Error::Limit(format!(
            "input exceeds {} bytes",
            limits.input_bytes
        )));
    }
    let mut tree = ZipTree::new(limits);
    let mut folder = String::new();
    let report = build_dictionary(
        source,
        &mut tree,
        |title| {
            folder = folder_name(title);
            format!("{folder}/")
        },
        limits,
        options,
        backend,
        progress,
    )?;
    Ok(DictionaryArchive {
        zip: tree.into_bytes()?,
        folder,
        report,
    })
}
pub fn verify_bundle(
    root: &Path,
    source: Option<&Path>,
    limits: &Limits,
) -> Result<ConversionReport> {
    let bytes = crate::manifest::read(root, 16 * 1024 * 1024)?;
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
