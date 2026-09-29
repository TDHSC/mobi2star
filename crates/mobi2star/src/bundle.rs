use crate::transaction::{sync_directory, Transaction};
use html_preserve::Plan;
use lexicon_core::{
    hash_file, read_bounded, sha256, EntryKind, Error, LabelLanguage, Limits, Result, Span,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDigest {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub tool: String,
    pub version: String,
    pub source_sha256: String,
    pub rawml_sha256: String,
    /// Language of generated lookup keys; verification regenerates with it.
    pub labels: LabelLanguage,
    pub files: Vec<FileDigest>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Report {
    pub schema: u32,
    /// This attests only to the implemented checks, not all source-format semantics.
    pub implemented_content_checks_passed: bool,
    pub rendering_status: String,
    pub source_headwords: usize,
    pub source_aliases: usize,
    pub supplement_entries: usize,
    pub output_entries: usize,
    pub output_synonyms: usize,
    pub source_rawml_bytes: usize,
    pub covered_rawml_bytes: usize,
    pub copied_resources: usize,
    pub resolved_resource_references: usize,
    pub resolved_internal_links: usize,
    pub external_links_retained: usize,
    pub skipped_entries: usize,
    pub offset_bits: u8,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RecordAudit {
    pub number: usize,
    pub span: Span,
    pub sha256: String,
}

pub(crate) fn write_bytes(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(crate) fn write_json<T: Serialize + ?Sized>(root: &Path, name: &str, value: &T) -> Result<()> {
    let mut out = serde_json::to_vec_pretty(value)?;
    out.push(b'\n');
    write_bytes(root, name, &out)
}
pub(crate) fn collect_files(root: &Path) -> Result<Vec<String>> {
    fn visit(root: &Path, at: &Path, result: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(at)? {
            let entry = entry?;
            let typ = entry.file_type()?;
            if typ.is_symlink() {
                return Err(Error::Verify("symlink in bundle tree".into()));
            }
            if typ.is_dir() {
                visit(root, &entry.path(), result)?;
            } else if typ.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::Verify("bundle path escaped".into()))?;
                let name = relative
                    .to_str()
                    .ok_or_else(|| Error::Verify("non-UTF8 bundle member name".into()))?
                    .replace('\\', "/");
                if name != "manifest.json" {
                    result.push(name);
                }
                if result.len() > 131072 {
                    return Err(Error::Limit("bundle file count".into()));
                }
            } else {
                return Err(Error::Verify("nonregular bundle member".into()));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}
pub(crate) fn report(doc: &lexicon_core::Document, plan: &Plan, offset_bits: u8) -> Report {
    Report { schema: 1, implemented_content_checks_passed: true, rendering_status: "unverified_reader_dependent".into(),
        source_headwords: doc.source_headwords, source_aliases: doc.source_aliases,
        supplement_entries: doc.entries.iter().filter(|e| e.kind == EntryKind::Supplement).count(),
        output_entries: doc.entries.len(), output_synonyms: doc.source_aliases + doc.entries.len(),
        source_rawml_bytes: doc.rawml.len(), covered_rawml_bytes: doc.rawml.len(), copied_resources: doc.resources.len(),
        resolved_resource_references: plan.resource_links.len(), resolved_internal_links: plan.links.len(),
        external_links_retained: plan.external_links, skipped_entries: 0, offset_bits,
        notes: vec![
            "Coverage measures the union of decompressed source byte ranges; it is not proof of universal MOBI semantic support.".into(),
            "All source spellings and explicit inflections are retained without Unicode normalization; reader lookup/folding may differ.".into(),
            "Internal links use stable entry aliases plus exact byte-position anchors; fragment navigation must be acceptance-tested in the target reader.".into(),
            "Images are copied byte-for-byte after signature recognition; image decoding and rendered appearance are not verified.".into(),
            "Global style blocks are copied, but inherited container context, embedded book structure and reader CSS can change appearance.".into(),
            "The manifest detects accidental changes, not malicious replacement: it is not digitally signed.".into(),
        ] }
}

/// Return the final bundle path only after the verifier has reopened the staged files.
pub fn convert(
    input: &Path,
    output: &Path,
    limits: &Limits,
    offset_bits: u8,
    labels: LabelLanguage,
) -> Result<(PathBuf, Report)> {
    let source = read_bounded(input, limits.input_bytes)?;
    let document = mobi_reader::read(source, limits, labels)?;
    let plan = html_preserve::build(&document, limits)?;
    let tx = Transaction::begin(output)?;
    let root = tx.path()?;
    fs::create_dir(root.join("archive"))?;
    fs::create_dir(root.join("res"))?;
    let written = stardict_io::write(root, &document, limits, offset_bits, |entry| {
        html_preserve::render(&document, entry, &plan)
    })?;
    write_bytes(root, "archive/source.mobi", &document.source)?;
    write_bytes(root, "archive/rawml.bin", &document.rawml)?;
    write_json(root, "archive/metadata.json", &document.metadata)?;
    write_json(root, "archive/indexes.json", &document.index_audit)?;
    let records: Vec<RecordAudit> = document
        .records
        .iter()
        .enumerate()
        .map(|(number, &span)| {
            Ok(RecordAudit {
                number,
                span,
                sha256: sha256(span.bytes(&document.source)?),
            })
        })
        .collect::<Result<_>>()?;
    write_json(root, "archive/records.json", &records)?;
    write_json(root, "resources.json", &document.resources)?;
    write_json(root, "edits.json", &plan)?;
    for resource in &document.resources {
        write_bytes(
            root,
            &format!("res/{}", resource.filename),
            resource.source_span.bytes(&document.source)?,
        )?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("entries.jsonl"))?;
    let mut entries = BufWriter::new(file);
    for row in &written.entries {
        serde_json::to_writer(&mut entries, row)?;
        entries.write_all(b"\n")?;
    }
    entries.flush()?;
    entries.get_ref().sync_all()?;
    drop(entries);
    let report = report(&document, &plan, offset_bits);
    write_json(root, "report.json", &report)?;
    let files = collect_files(root)?
        .into_iter()
        .map(|path| {
            let (bytes, sha256) = hash_file(&root.join(&path))?;
            Ok(FileDigest {
                path,
                bytes,
                sha256,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = Manifest {
        schema: 1,
        tool: "mobi2star".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        source_sha256: sha256(&document.source),
        rawml_sha256: sha256(&document.rawml),
        labels,
        files,
    };
    write_json(root, "manifest.json", &manifest)?;
    sync_directory(&root.join("res"))?;
    sync_directory(&root.join("archive"))?;
    sync_directory(root)?;
    drop(written);
    drop(plan);
    drop(document);
    // No circular 'writer said success' trust: read the disk bundle and source again.
    let verified = crate::verify(root, None, limits)?;
    if report != verified {
        return Err(Error::Verify(
            "report changed during staging verification".into(),
        ));
    }
    let destination = tx.commit()?;
    Ok((destination, verified))
}
