use crate::{
    bundle::{collect_files, report, RecordAudit, WrittenEntry, SCHEMA},
    manifest::{self, check_header},
    Manifest, Profile, Report,
};
use lexicon_core::{
    checked_member, hash_file, read_bounded, sha256, Document, Error, Limits, Metadata, Resource,
    Result, StyleDelivery,
};
use serde::de::DeserializeOwned;
use stardict_io::{compare_synonyms, compare_words, IndexEntry, ParsedDictionary, Synonym};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

fn json<T: DeserializeOwned>(root: &Path, name: &str, cap: usize) -> Result<T> {
    Ok(serde_json::from_slice(&read_bounded(
        &checked_member(root, name)?,
        cap,
    )?)?)
}
/// Each possible stylesheet path must hold exactly the source-derived CSS
/// when the delivery calls for it, and must be absent otherwise.
fn check_stylesheets(root: &Path, css: &str, style: StyleDelivery, limits: &Limits) -> Result<()> {
    let expected: BTreeMap<String, &[u8]> = stardict_io::stylesheet_files(css, style)
        .into_iter()
        .collect();
    for path in stardict_io::stylesheet_paths() {
        match expected.get(&path) {
            Some(bytes) => ensure(
                read_bounded(&checked_member(root, &path)?, limits.text_bytes)? == *bytes,
                &format!("{path} is not the source-derived stylesheet"),
            )?,
            None => ensure(
                !root.join(&path).exists(),
                &format!("unexpected stylesheet {path}"),
            )?,
        }
    }
    Ok(())
}
fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Verify(message.into()))
    }
}
fn next_line(reader: &mut impl BufRead, cap: usize) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(Error::Verify("unterminated entries JSONL line".into()))
            };
        }
        let end = available.iter().position(|&b| b == b'\n').map(|n| n + 1);
        let take = end.unwrap_or(available.len());
        if take > cap.saturating_sub(line.len()) {
            return Err(Error::Limit("JSONL record byte budget".into()));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}

/// Checks a compiled dictionary against its source Document, independently
/// of how it was written: entry count and order, every synonym and internal
/// key, each provenance row's byte range and hashes, and every payload
/// against a replay of the source-preserving edits. `next_row` yields the
/// provenance rows in StarDict order; `payload` reads one entry's HTML.
pub(crate) fn check_entries(
    doc: &Document,
    plan: &html_preserve::Plan,
    parsed: &ParsedDictionary,
    style: StyleDelivery,
    next_row: &mut dyn FnMut() -> Result<Option<WrittenEntry>>,
    payload: &mut dyn FnMut(&IndexEntry) -> Result<String>,
) -> Result<()> {
    let mut ordered: Vec<&lexicon_core::Entry> = doc.entries.iter().collect();
    ordered.sort_by(|a, b| compare_words(&a.headword, &b.headword).then(a.id.cmp(&b.id)));
    ensure(
        ordered.len() == parsed.entries.len(),
        "source/output entry count mismatch",
    )?;
    let ordinals: BTreeMap<u64, u32> = ordered
        .iter()
        .enumerate()
        .map(|(n, e)| (e.id, n as u32))
        .collect();
    let mut expected_synonyms = Vec::new();
    for entry in &ordered {
        for alias in &entry.aliases {
            expected_synonyms.push(Synonym {
                word: alias.word.clone(),
                target: ordinals[&entry.id],
            });
        }
        expected_synonyms.push(Synonym {
            word: entry.internal_key(&doc.namespace),
            target: ordinals[&entry.id],
        });
    }
    expected_synonyms.sort_by(compare_synonyms);
    ensure(
        expected_synonyms == parsed.synonyms,
        "missing, changed, duplicated or misrouted synonym/inflection",
    )?;
    for (ordinal, (expected, actual)) in ordered.iter().zip(&parsed.entries).enumerate() {
        let row = next_row()?.ok_or_else(|| Error::Verify("missing entry provenance".into()))?;
        ensure(
            row.ordinal as usize == ordinal && &row.entry == *expected,
            "source entry/span/alias provenance mismatch",
        )?;
        ensure(
            actual.word == expected.headword
                && row.offset == actual.offset
                && row.size == actual.size,
            "entry key or byte range mismatch",
        )?;
        ensure(
            row.source_sha256 == sha256(expected.span.bytes(&doc.rawml)?),
            "source entry content hash mismatch",
        )?;
        let html = payload(actual)?;
        ensure(
            sha256(html.as_bytes()) == row.rendered_sha256,
            "rendered entry content hash mismatch",
        )?;
        let replay = html_preserve::render(doc, expected, plan, style)?;
        ensure(
            html == replay,
            "DICT differs from a replay of source-preserving edits",
        )?;
    }
    ensure(next_row()?.is_none(), "extra entry provenance rows")
}

/// Reopen hashes, parse actual StarDict files, then reconstruct source expectations.
/// Reusing the source parser catches omissions in output, not bugs shared by that parser.
pub fn verify(root: &Path, original_source: Option<&Path>, limits: &Limits) -> Result<Report> {
    if root.symlink_metadata()?.file_type().is_symlink() {
        return Err(Error::Verify("bundle root cannot be a symlink".into()));
    }
    let manifest_bytes = manifest::read(root, 32 * 1024 * 1024)?;
    check_header(&manifest_bytes, SCHEMA)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    let style = manifest.reader.style_delivery();
    ensure(manifest.files.len() <= 131072, "too many manifest members")?;
    let actual_names = collect_files(root)?;
    let declared_names: Vec<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
    ensure(
        actual_names == declared_names,
        "manifest member list is incomplete, duplicated, unsorted or contains extra files",
    )?;
    for member in &manifest.files {
        let path = checked_member(root, &member.path)?;
        let maximum = if member.path == "dictionary.dict" {
            limits.output_bytes
        } else {
            limits.input_bytes.max(limits.text_bytes) as u64
        };
        if member.bytes > maximum || path.metadata()?.len() > maximum {
            return Err(Error::Limit(format!(
                "bundle member {} exceeds byte budget",
                member.path
            )));
        }
        let (bytes, hash) = hash_file(&path)?;
        ensure(
            bytes == member.bytes && hash == member.sha256,
            &format!("size/SHA-256 mismatch: {}", member.path),
        )?;
    }
    if let Some(source) = original_source {
        if source.metadata()?.len() > limits.input_bytes as u64 {
            return Err(Error::Limit("original source byte budget".into()));
        }
        ensure(
            hash_file(source)?.1 == manifest.source_sha256,
            "supplied original is not this bundle's source",
        )?;
    }
    let source = read_bounded(
        &checked_member(root, "archive/source.mobi")?,
        limits.input_bytes,
    )?;
    ensure(
        sha256(&source) == manifest.source_sha256,
        "archived source fingerprint mismatch",
    )?;
    let doc = mobi_reader::read(source, limits, manifest.labels)?;
    ensure(
        sha256(&doc.rawml) == manifest.rawml_sha256,
        "redecoded source text fingerprint mismatch",
    )?;
    let archived_raw = read_bounded(
        &checked_member(root, "archive/rawml.bin")?,
        limits.text_bytes,
    )?;
    ensure(
        archived_raw == doc.rawml,
        "archived RAWML is not exact decompression of the source",
    )?;
    drop(archived_raw);
    let metadata: Metadata = json(root, "archive/metadata.json", limits.input_bytes)?;
    ensure(metadata == doc.metadata, "metadata archive mismatch")?;
    let index_audit: serde_json::Value = json(root, "archive/indexes.json", limits.input_bytes)?;
    ensure(
        index_audit == doc.index_audit,
        "source index audit mismatch",
    )?;
    let records: Vec<RecordAudit> = json(root, "archive/records.json", 32 * 1024 * 1024)?;
    ensure(
        records.len() == doc.records.len(),
        "PDB archive record count mismatch",
    )?;
    for (n, (&span, record)) in doc.records.iter().zip(&records).enumerate() {
        ensure(
            record.number == n
                && record.span == span
                && record.sha256 == sha256(span.bytes(&doc.source)?),
            "PDB record provenance mismatch",
        )?;
    }
    let resources: Vec<Resource> = json(root, "resources.json", 32 * 1024 * 1024)?;
    ensure(resources == doc.resources, "resource mapping mismatch")?;
    for resource in &resources {
        let path = checked_member(root, &format!("res/{}", resource.filename))?;
        let (bytes, hash) = hash_file(&path)?;
        ensure(
            bytes == resource.source_span.len() as u64
                && hash == sha256(resource.source_span.bytes(&doc.source)?),
            "resource bytes differ from source record",
        )?;
    }
    let saved_plan: html_preserve::Plan = json(root, "edits.json", limits.input_bytes)?;
    let plan = html_preserve::build(&doc, limits)?;
    ensure(
        plan == saved_plan,
        "HTML edit/link plan differs from source-derived plan",
    )?;
    check_stylesheets(root, &plan.stylesheet, style, limits)?;
    let parsed = stardict_io::open(root, limits)?;
    ensure(
        parsed.offset_bits == manifest.offset_bits,
        "offset width differs from manifest",
    )?;
    let mut provenance = BufReader::new(File::open(checked_member(root, "entries.jsonl")?)?);
    let mut payload_file = File::open(stardict_io::dictionary_file(root)?)?;
    check_entries(
        &doc,
        &plan,
        &parsed,
        style,
        &mut || {
            next_line(&mut provenance, limits.input_bytes)?
                .map(|line| Ok(serde_json::from_slice(&line)?))
                .transpose()
        },
        &mut |entry| stardict_io::read_payload(&mut payload_file, entry, limits.entry_bytes),
    )?;
    let saved_report: Report = json(root, "report.json", 1024 * 1024)?;
    let expected_report = report(
        &doc,
        &plan,
        parsed.offset_bits,
        manifest.reader,
        Profile::Bundle,
    );
    ensure(
        saved_report == expected_report,
        "report is not supported by source and output checks",
    )?;
    Ok(expected_report)
}
