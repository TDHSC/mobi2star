//! Independent native MOBI5/6 reader. No dependency on mobi2stardict,
//! KindleUnpack, libmobi, a shell, or an external unpacker.
#![forbid(unsafe_code)]
mod compression;
pub mod header;
pub mod index;
pub mod inflection;
pub mod pdb;
pub mod container;
pub use container::Container;

use header::Header;
use index::Index;
use lexicon_core::{Document, Encoding, Entry, EntryKind, Error, Limits, Resource, Result, Span, uncovered, validate_word};
use pdb::PalmDatabase;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub bytes: usize,
    pub records: usize,
    pub header: Header,
    pub header_gate_passed: bool,
    pub blocking_reason: Option<String>,
    pub note: &'static str,
}
pub fn inspect(source: &[u8]) -> Result<Inspection> {
    let pdb = PalmDatabase::parse(source)?;
    let header = Header::parse(pdb.record(source, 0)?)?;
    let failure = header.conversion_gate().err();
    Ok(Inspection { bytes: source.len(), records: pdb.records.len(), header,
        header_gate_passed: failure.is_none(), blocking_reason: failure.map(|e| e.to_string()),
        note: "Header preflight only, NOT a completeness or conversion guarantee. convert performs the full audit." })
}

pub fn read(source: Vec<u8>, limits: &Limits) -> Result<Document> {
    if source.len() > limits.input_bytes { return Err(Error::Limit("input size".into())); }
    let pdb = PalmDatabase::parse(&source)?;
    let h = Header::parse(pdb.record(&source, 0)?)?;
    h.conversion_gate()?;
    let encoding = Encoding::from_mobi(h.encoding_number)?;
    let (rawml, _) = container::decode_text(&pdb, &source, &h, limits)?;
    let orth = Index::parse(&pdb, &source, h.orth_index.ok_or_else(|| Error::Unsupported("no orth index".into()))?, limits.entries)?;
    orth.require_tags(&[1, 2, 42])?;
    if orth.rows.is_empty() { return Err(Error::Incomplete("empty headword index".into())); }
    let infl = h.infl_index.map(|n| Index::parse(&pdb, &source, n, limits.entries)).transpose()?;
    let is_new = orth.descriptors.iter().any(|d| d.tag == 42 && d.end == 0);
    let old = if let Some(index) = &infl {
        if is_new { index.require_tags(&[5, 26])?; None }
        else {
            if index.encoding != orth.encoding { return Err(Error::Unsupported("mixed encodings in old inflection index".into())); }
            Some(inflection::OldSuffixIndex::build(index, &pdb, &source)?)
        }
    } else { None };
    if is_new && infl.is_none() && orth.rows.iter().any(|r| r.tags.contains_key(&42)) {
        return Err(Error::Incomplete("headwords reference an absent inflection index".into()));
    }
    let mut known_lengths: BTreeMap<usize, usize> = BTreeMap::new();
    for row in &orth.rows {
        let start = row.scalar(1)? as usize;
        let len = row.scalar(2)? as usize;
        if len > 0 {
            if known_lengths.get(&start).is_some_and(|&old| old != len) {
                // Differing lengths at an identical start are valid only as explicit entries;
                // do not use that position to resolve a zero-length reference.
                known_lengths.insert(start, usize::MAX);
            } else { known_lengths.insert(start, len); }
        }
    }
    let mut entries = Vec::new();
    let mut source_aliases = 0usize;
    for (number, row) in orth.rows.iter().enumerate() {
        let headword = orth.encoding.decode(&row.label)?;
        validate_word(&headword)?;
        let start = row.scalar(1)? as usize;
        let declared_len = row.scalar(2)? as usize;
        let len = if declared_len == 0 {
            *known_lengths.get(&start).filter(|&&n| n != usize::MAX).ok_or_else(|| Error::Incomplete(format!("{headword:?} has zero length and no unambiguous exact-position target")))?
        } else { declared_len };
        let span = Span::new(start, len, rawml.len())?;
        let remaining = limits.aliases.saturating_sub(source_aliases);
        let aliases = if let Some(suffixes) = &old { suffixes.aliases(&row.label, remaining)? }
        else if let Some(index) = &infl { inflection::new_aliases(row, index, &pdb, &source, orth.encoding, remaining)? }
        else { Vec::new() };
        source_aliases += aliases.len();
        entries.push(Entry { id: number as u64, headword, aliases, span, kind: EntryKind::Headword });
    }
    let source_headwords = entries.len();
    let headwords: BTreeSet<String> = entries.iter().map(|e| e.headword.clone()).collect();
    for (i, span) in uncovered(rawml.len(), entries.iter().map(|e| e.span))?.into_iter().enumerate() {
        let headword = format!("〔原书补充内容 {:06}〕", i + 1);
        if headwords.contains(&headword) { return Err(Error::Incomplete("supplement key collision".into())); }
        entries.push(Entry { id: entries.len() as u64, headword, aliases: Vec::new(), span, kind: EntryKind::Supplement });
    }
    let mut resources = Vec::new();
    if let Some(first) = h.first_image {
        if first <= h.text_records || first > pdb.records.len() { return Err(Error::Malformed("first image record beyond PDB".into())); }
        for record in first..pdb.records.len() {
            let raw = pdb.record(&source, record)?;
            let identified = if raw.starts_with(b"\x89PNG\r\n\x1a\n") { Some(("png", "image/png")) }
                else if raw.starts_with(b"\xff\xd8\xff") { Some(("jpg", "image/jpeg")) }
                else if raw.starts_with(b"GIF87a") || raw.starts_with(b"GIF89a") { Some(("gif", "image/gif")) }
                else if raw.starts_with(b"BM") { Some(("bmp", "image/bmp")) }
                else { None };
            if let Some((ext, mime)) = identified {
                let recindex = u32::try_from(record - first + 1).map_err(|_| Error::Limit("resource id".into()))?;
                resources.push(Resource { recindex, pdb_record: record, source_span: pdb.records[record], filename: format!("mobi-{recindex:06}.{ext}"), media_type: mime.into() });
            }
            if raw.starts_with(b"FONT") || raw.starts_with(b"AUDI") || raw.starts_with(b"VIDE") {
                return Err(Error::Unsupported(format!("font/audio/video resource at record {record}; archive preservation alone is not usable conversion")));
            }
        }
    }
    let record_roles = record_roles(&pdb, &source, &h, &orth, infl.as_ref(), &resources)?;
    let index_audit = serde_json::json!({ "orth": orth, "infl": infl, "header": h, "record_roles": record_roles });
    let namespace = lexicon_core::sha256(&source);
    let document = Document { namespace, source, rawml, encoding, metadata: h.metadata, records: pdb.records,
        entries, resources, source_headwords, source_aliases, index_audit };
    document.validate(limits)?;
    Ok(document)
}

/// All non-text records must have a known role; archival retention is not a
/// license to silently treat an unknown content record as expendable metadata.
fn record_roles(pdb: &PalmDatabase, source: &[u8], header: &Header, orth: &Index,
    infl: Option<&Index>, resources: &[Resource]) -> Result<Vec<String>> {
    let mut roles = vec![String::new(); pdb.records.len()];
    roles[0] = "header_and_metadata".into();
    for role in roles.iter_mut().take(header.text_records + 1).skip(1) { *role = "compressed_text".into(); }
    for index in std::iter::once(orth).chain(infl) {
        let count = lexicon_core::bytes::be32(pdb.record(source, index.meta_record)?, 24)? as usize;
        for (n, role) in roles.iter_mut().enumerate().skip(index.meta_record).take(count + 1) {
            if !role.is_empty() { return Err(Error::Malformed(format!("overlapping record roles at {n}"))); }
            *role = "dictionary_index".into();
        }
        for &n in &index.cncx_records {
            if !roles[n].is_empty() { return Err(Error::Malformed("CNCX overlaps another record role".into())); }
            roles[n] = "index_strings".into();
        }
    }
    if header.compression == 17480 {
        let start = header.huff_start.ok_or_else(|| Error::Malformed("missing HUFF record start".into()))?;
        for (n, role) in roles.iter_mut().enumerate().skip(start).take(header.huff_count) {
            if !role.is_empty() { return Err(Error::Malformed(format!("HUFF overlaps record {n}"))); }
            *role = "huffman_dictionary".into();
        }
    }
    for resource in resources {
        let role = &mut roles[resource.pdb_record];
        if !role.is_empty() { return Err(Error::Malformed("image overlaps another record role".into())); }
        *role = "image_resource".into();
    }
    for (n, role) in roles.iter_mut().enumerate() {
        if role.is_empty() {
            let bytes = pdb.record(source, n)?;
            if bytes.starts_with(b"FLIS") || bytes.starts_with(b"FCIS") || bytes == b"\xe9\x8e\x0d\x0a" {
                *role = "known_container_auxiliary".into();
            } else {
                return Err(Error::Unsupported(format!("unclassified PDB record {n}; add a format adapter rather than dropping it")));
            }
        }
    }
    Ok(roles)
}
