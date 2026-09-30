use crate::{
    dictionary::{check_written, DictionaryBuilder, WrittenDictionary},
    manifest::TOOL,
    transaction::{sync_directory, Transaction},
    tree::{DiskTree, Tree},
    verify::check_entries,
    OutputOptions, Profile, Stage,
};
use html_preserve::Plan;
use lexicon_core::{
    hash_file, read_bounded, sha256, Document, Entry, EntryKind, Error, LabelLanguage, Limits,
    Result, Span, TargetReader,
};
use serde::{Deserialize, Serialize};
use stardict_io::compare_words;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDigest {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
/// Manifest schema numbers are unique across both bundle formats: compiled
/// 1 and 4, SRCS 2 and 3. Releases up to 0.3 told the formats apart by the
/// number alone (2 meant SRCS), so no number is ever reused for the other.
pub(crate) const SCHEMA: u32 = 4;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub tool: String,
    pub version: String,
    pub source_sha256: String,
    pub rawml_sha256: String,
    pub offset_bits: u8,
    /// Language of generated lookup keys; verification regenerates with it.
    pub labels: LabelLanguage,
    /// Reader the payloads' stylesheet references were made for.
    pub reader: TargetReader,
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
    pub reader: TargetReader,
    pub notes: Vec<String>,
    /// Absent from full bundles; see `Profile`.
    #[serde(default, skip_serializing_if = "Profile::is_bundle")]
    pub profile: Profile,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RecordAudit {
    pub number: usize,
    pub span: Span,
    pub sha256: String,
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
/// One line of `entries.jsonl`: where an entry's payload landed and what it
/// was rendered from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WrittenEntry {
    pub entry: Entry,
    pub ordinal: u32,
    pub offset: u64,
    pub size: u32,
    pub source_sha256: String,
    pub rendered_sha256: String,
}
/// The compiled dictionary at the bundle root and its provenance rows.
pub(crate) struct CompiledDictionary {
    pub written: WrittenDictionary,
    pub rows: Vec<WrittenEntry>,
}
/// The dictionary's title, with a fallback for books that have none.
fn title(document: &Document) -> &str {
    if document.metadata.title.is_empty() {
        "Converted MOBI dictionary"
    } else {
        &document.metadata.title
    }
}
/// Writes the stylesheet, image resources and every entry in StarDict order
/// under `dir`.
fn write_dictionary(
    tree: &mut impl Tree,
    document: &Document,
    plan: &Plan,
    dir: &str,
    limits: &Limits,
    options: OutputOptions,
    progress: &mut dyn FnMut(Stage),
) -> Result<CompiledDictionary> {
    let style = options.reader.style_delivery();
    for (path, bytes) in stardict_io::stylesheet_files(&plan.stylesheet, style) {
        tree.put(&format!("{dir}{path}"), bytes)?;
    }
    for resource in &document.resources {
        tree.put(
            &format!("{dir}res/{}", resource.filename),
            resource.source_span.bytes(&document.source)?,
        )?;
    }
    let mut ordered: Vec<&Entry> = document.entries.iter().collect();
    ordered.sort_by(|a, b| compare_words(&a.headword, &b.headword).then(a.id.cmp(&b.id)));
    let mut builder = DictionaryBuilder::start(
        tree,
        dir,
        limits,
        options.offset_bits,
        ordered.len(),
        progress,
    )?;
    let mut rows = Vec::new();
    for entry in ordered {
        let html = html_preserve::render(document, entry, plan, style)?;
        let payload = builder.append(html.as_bytes())?;
        builder.item(entry.id, entry.headword.clone(), payload);
        for alias in &entry.aliases {
            builder.alias(alias.word.clone(), entry.id);
        }
        builder.alias(entry.internal_key(&document.namespace), entry.id);
        rows.push(WrittenEntry {
            entry: entry.clone(),
            ordinal: 0,
            offset: payload.offset,
            size: payload.size,
            source_sha256: sha256(entry.span.bytes(&document.rawml)?),
            rendered_sha256: sha256(html.as_bytes()),
        });
    }
    let written = builder.finish(title(document), limits)?;
    for row in &mut rows {
        row.ordinal = written.encoded.catalog.ordinals[&row.entry.id];
    }
    Ok(CompiledDictionary { written, rows })
}
pub(crate) fn report(
    doc: &lexicon_core::Document,
    plan: &Plan,
    offset_bits: u8,
    reader: TargetReader,
    profile: Profile,
) -> Report {
    let mut report = Report { schema: SCHEMA, implemented_content_checks_passed: true, rendering_status: "unverified_reader_dependent".into(),
        source_headwords: doc.source_headwords, source_aliases: doc.source_aliases,
        supplement_entries: doc.entries.iter().filter(|e| e.kind == EntryKind::Supplement).count(),
        output_entries: doc.entries.len(), output_synonyms: doc.source_aliases + doc.entries.len(),
        source_rawml_bytes: doc.rawml.len(), covered_rawml_bytes: doc.rawml.len(), copied_resources: doc.resources.len(),
        resolved_resource_references: plan.resource_links.len(), resolved_internal_links: plan.links.len(),
        external_links_retained: plan.external_links, skipped_entries: 0, offset_bits, reader, profile,
        notes: vec![
            "Coverage measures the union of decompressed source byte ranges; it is not proof of universal MOBI semantic support.".into(),
            "All source spellings and explicit inflections are retained without Unicode normalization; reader lookup/folding may differ.".into(),
            "Internal links use stable entry aliases plus exact byte-position anchors; fragment navigation must be acceptance-tested in the target reader.".into(),
            "Images are copied byte-for-byte after signature recognition; image decoding and rendered appearance are not verified.".into(),
            "Source <style> bodies are scoped under the book's wrapper class to form dictionary.css; payloads sit in that wrapper and reference the stylesheet as the recorded reader needs (res/ link, inline copies or both). A <style> element inside an entry stays in that entry as source bytes. Container context and reader CSS can still change appearance.".into(),
        ] };
    if profile == Profile::Bundle {
        report.notes.push("The manifest detects accidental changes, not malicious replacement: it is not digitally signed.".into());
    }
    report
}

/// Builds only the dictionary, in the folder `folder` names from the book
/// title, and checks it as `verify` checks a bundle's dictionary: the
/// independent readback, then every entry against the source Document and
/// a replay of its edits, read from the written output.
pub(crate) fn build_dictionary(
    source: Vec<u8>,
    tree: &mut impl Tree,
    folder: impl FnOnce(&str) -> String,
    limits: &Limits,
    options: OutputOptions,
    progress: &mut dyn FnMut(Stage),
) -> Result<Report> {
    let document = mobi_reader::read(source, limits, options.labels)?;
    let plan = html_preserve::build(&document, limits)?;
    let dir = folder(title(&document));
    let compiled = write_dictionary(tree, &document, &plan, &dir, limits, options, progress)?;
    progress(Stage::Checking);
    let mut output = tree.readback()?;
    let parsed = check_written(&mut *output, &compiled.written, limits)?;
    let (mut dict, _) = output.open(&format!("{dir}dictionary.dict"))?;
    let mut position = 0;
    let mut rows = compiled.rows.into_iter();
    check_entries(
        &document,
        &plan,
        &parsed,
        options.reader.style_delivery(),
        &mut || Ok(rows.next()),
        &mut |entry| {
            stardict_io::read_next_payload(&mut dict, &mut position, entry, limits.entry_bytes)
        },
    )?;
    Ok(report(
        &document,
        &plan,
        options.offset_bits,
        options.reader,
        Profile::Stardict,
    ))
}

/// Return the final bundle path only after the verifier has reopened the staged files.
pub fn convert(
    input: &Path,
    output: &Path,
    limits: &Limits,
    options: OutputOptions,
) -> Result<(PathBuf, Report)> {
    let OutputOptions {
        offset_bits,
        labels,
        reader,
    } = options;
    let source = read_bounded(input, limits.input_bytes)?;
    let document = mobi_reader::read(source, limits, labels)?;
    let plan = html_preserve::build(&document, limits)?;
    let tx = Transaction::begin(output)?;
    let root = tx.path()?;
    fs::create_dir(root.join("archive"))?;
    fs::create_dir(root.join("res"))?;
    let mut tree = DiskTree::new(root, limits);
    let compiled = write_dictionary(
        &mut tree,
        &document,
        &plan,
        "",
        limits,
        options,
        &mut |_| {},
    )?;
    // No `check_written` here: `verify` below reads every file back and
    // checks it against the source, which covers the same ground.
    tree.put("archive/source.mobi", &document.source)?;
    tree.put("archive/rawml.bin", &document.rawml)?;
    tree.put_json("archive/metadata.json", &document.metadata)?;
    tree.put_json("archive/indexes.json", &document.index_audit)?;
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
    tree.put_json("archive/records.json", &records)?;
    tree.put_json("resources.json", &document.resources)?;
    tree.put_json("edits.json", &plan)?;
    let entries = tree.stream("entries.jsonl")?;
    for row in &compiled.rows {
        serde_json::to_writer(&mut *entries, row)?;
        entries.write_all(b"\n")?;
    }
    tree.end_stream()?;
    let report = report(&document, &plan, offset_bits, reader, Profile::Bundle);
    tree.put_json("report.json", &report)?;
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
        schema: SCHEMA,
        tool: TOOL.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        source_sha256: sha256(&document.source),
        rawml_sha256: sha256(&document.rawml),
        offset_bits,
        labels,
        reader,
        files,
    };
    tree.put_json("manifest.json", &manifest)?;
    sync_directory(&root.join("res"))?;
    sync_directory(&root.join("archive"))?;
    sync_directory(root)?;
    drop(compiled);
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
