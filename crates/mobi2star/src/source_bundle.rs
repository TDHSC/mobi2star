//! Deterministic SRCS bundle production. The verifier rebuilds from the archived
//! MOBI and compares the actual artifacts, in addition to an independent IDX reader.
use crate::{
    bundle::collect_files,
    dictionary::{check_written, DictionaryBuilder, WrittenDictionary},
    manifest::{self, check_header, TOOL},
    transaction::{sync_tree, Transaction},
    tree::{DiskTree, Tree},
    FileDigest, OutputOptions, Profile, Stage,
};
use lexicon_core::{
    checked_member, hash_file, read_bounded, sha256, Error, LabelLanguage, Limits, Resource,
    Result, Span, TargetReader,
};
use mobi_reader::{container::TextRecord, Container};
use serde::{Deserialize, Serialize};
use srcs_reader::{Crosscheck, SourceArchive, SourceBook};
use srcs_render::{browser, Plan, Rendered};
use stardict_io::{ParsedDictionary, Payload};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(crate) const BACKEND: &str = "srcs-rust";
pub(crate) const SCHEMA: u32 = 3;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceReport {
    pub schema: u32,
    pub backend: String,
    pub implemented_content_checks_passed: bool,
    pub rendering_status: String,
    pub layout_profile: String,
    pub source_sha256: String,
    pub source_archive_sha256: String,
    pub source_headwords: usize,
    pub source_aliases: usize,
    pub definitions: usize,
    pub chapters: usize,
    pub supplement_entries: usize,
    pub output_entries: usize,
    pub output_synonyms: usize,
    pub source_body_bytes: usize,
    pub covered_source_body_bytes: usize,
    pub rawml_bytes: usize,
    pub source_files: usize,
    pub source_images: usize,
    pub compiled_images: usize,
    pub internal_links: usize,
    pub external_links: usize,
    pub source_image_references: usize,
    pub classified_pdb_records: usize,
    pub skipped_entries: usize,
    pub offset_bits: u8,
    pub reader: TargetReader,
    pub verification_scope: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema: u32,
    backend: String,
    tool: String,
    version: String,
    source_sha256: String,
    offset_bits: u8,
    /// Language of generated keys, galleries and viewer text; verification regenerates with it.
    labels: LabelLanguage,
    /// Reader the payloads' stylesheet references were made for.
    reader: TargetReader,
    files: Vec<FileDigest>,
}
#[derive(Serialize)]
struct ArticleAudit {
    kind: String,
    id: usize,
    source_file: String,
    source_span: Span,
    source_sha256: String,
    payload: Payload,
    payload_sha256: String,
    prefix_bytes: usize,
    fragment_bytes: usize,
    suffix_bytes: usize,
}
#[derive(Serialize)]
struct ImageAudit {
    file: String,
    origin: String,
    bytes: usize,
    sha256: String,
    width: u32,
    height: u32,
}

fn article_audit(
    kind: &str,
    id: usize,
    file: &str,
    span: Span,
    raw: &[u8],
    rendered: &Rendered,
    payload: Payload,
) -> Result<ArticleAudit> {
    Ok(ArticleAudit {
        kind: kind.into(),
        id,
        source_file: file.into(),
        source_span: span,
        source_sha256: sha256(span.bytes(raw)?),
        payload,
        payload_sha256: sha256(&rendered.bytes),
        prefix_bytes: rendered.prefix_bytes,
        fragment_bytes: rendered.fragment_bytes,
        suffix_bytes: rendered.suffix_bytes,
    })
}
fn gallery(title: &str, items: &[(String, String)]) -> Vec<u8> {
    let mut out = format!("<h1>{}</h1>", srcs_render::escape(title));
    for (label, path) in items {
        out.push_str(&format!(
            "<figure><figcaption>{}</figcaption><img src=\"{}\" alt=\"{}\"></figure>",
            srcs_render::escape(label),
            srcs_render::escape(&srcs_reader::uri::percent_encode(path, true)),
            srcs_render::escape(label)
        ));
    }
    out.into_bytes()
}
fn chapter_word(id: usize, title: &str, labels: LabelLanguage) -> String {
    // A human-readable key stays below StarDict's 256-byte bound. Full title and
    // original filename remain in the chapter, browser and audit metadata.
    let prefix = labels.chapter_key_prefix(id + 1);
    let mut out = prefix;
    for ch in title.chars() {
        if ch.is_control() {
            continue;
        }
        if out.len() + ch.len_utf8() >= 250 {
            break;
        }
        out.push(ch);
    }
    out
}

/// A parsed, cross-checked and planned publisher-source book.
struct Prepared<'s> {
    source: &'s [u8],
    namespace: String,
    book: SourceBook,
    plan: Plan,
    resources: Vec<Resource>,
    /// Report counts about the compiled text, kept after that text is dropped.
    rawml_bytes: usize,
    classified_pdb_records: usize,
}
/// Compiled-side facts that only the full bundle's Audit/ records.
struct AuditInputs<'s> {
    mobi: Container<'s>,
    archive: &'s [u8],
    rawml: Vec<u8>,
    text_records: Vec<TextRecord>,
    cross: Crosscheck,
}
/// What the dictionary stage produced for the checks, audits and report.
struct DictionaryFacts {
    written: WrittenDictionary,
    articles: Vec<ArticleAudit>,
    images: Vec<ImageAudit>,
}
/// Where the full bundle keeps the dictionary.
pub(crate) const DICTIONARY_DIR: &str = "StarDict/";

/// Parses the embedded publisher source, cross-checks it against the
/// compiled MOBI and plans the rendering.
fn prepare<'s>(source: &'s [u8], limits: &Limits) -> Result<(Prepared<'s>, AuditInputs<'s>)> {
    let mobi = Container::open(source, limits)?;
    let (src_record, archive) = mobi
        .source_archive()?
        .ok_or_else(|| Error::Unsupported("MOBI has no embedded SRCS source archive".into()))?;
    let book = SourceBook::parse(SourceArchive::read(archive, src_record, limits)?, limits)?;
    let (rawml, text_records) = mobi.rawml(limits)?;
    let cross = srcs_reader::crosscheck(&mobi, &book, &rawml, limits)?;
    let namespace = sha256(source);
    let plan = Plan::build(&book, &namespace)?;
    let resources = mobi.resources()?;
    let prepared = Prepared {
        source,
        namespace,
        book,
        plan,
        resources,
        rawml_bytes: rawml.len(),
        classified_pdb_records: cross.record_roles.len(),
    };
    let audit = AuditInputs {
        mobi,
        archive,
        rawml,
        text_records,
        cross,
    };
    Ok((prepared, audit))
}

/// Writes the StarDict dictionary under `dir`: stylesheet and image
/// resources first, then every definition, chapter and image gallery.
fn write_dictionary(
    tree: &mut impl Tree,
    prepared: &Prepared,
    dir: &str,
    limits: &Limits,
    options: OutputOptions,
    progress: &mut dyn FnMut(Stage),
) -> Result<DictionaryFacts> {
    let Prepared {
        source,
        namespace,
        book,
        plan,
        resources,
        ..
    } = prepared;
    let text = options.labels.text();
    let style = options.reader.style_delivery();
    for (path, bytes) in stardict_io::stylesheet_files(&plan.stylesheet(), style) {
        tree.put(&format!("{dir}{path}"), bytes)?;
    }
    let mut images = Vec::new();
    for (file, bytes) in &book.files {
        if !book.pages.contains_key(file) {
            tree.put(&format!("{dir}res/source/{file}"), bytes)?;
        }
        if mobi_reader::container::image_type(bytes).is_some() {
            let (width, height) = srcs_reader::validate_image(bytes, limits.text_bytes)?;
            images.push(ImageAudit {
                file: file.clone(),
                origin: "source_zip".into(),
                bytes: bytes.len(),
                sha256: sha256(bytes),
                width,
                height,
            });
        }
    }
    for resource in resources {
        let bytes = resource.source_span.bytes(source)?;
        let (width, height) = srcs_reader::validate_image(bytes, limits.text_bytes)?;
        tree.put(&format!("{dir}res/compiled/{}", resource.filename), bytes)?;
        images.push(ImageAudit {
            file: resource.filename.clone(),
            origin: format!("pdb_record_{}", resource.pdb_record),
            bytes: bytes.len(),
            sha256: sha256(bytes),
            width,
            height,
        });
    }
    let gallery_items = |source_zip: bool, dir: &str| -> Vec<(String, String)> {
        images
            .iter()
            .filter(|image| (image.origin == "source_zip") == source_zip)
            .map(|image| (image.file.clone(), format!("{dir}/{}", image.file)))
            .collect()
    };
    let source_gallery = gallery_items(true, "source");
    let compiled_gallery = gallery_items(false, "compiled");

    // Definitions, chapters and the two galleries.
    let total = book.entries.len() + plan.page_ids.len() + 2;
    let mut builder =
        DictionaryBuilder::start(tree, dir, limits, options.offset_bits, total, progress)?;
    let mut articles = Vec::new();
    for entry in &book.entries {
        let rendered = plan.definition(book, entry, style)?;
        let payload = builder.append(&rendered.bytes)?;
        articles.push(article_audit(
            "definition",
            entry.id,
            &entry.file,
            entry.span,
            &book.files[&entry.file],
            &rendered,
            payload,
        )?);
        for &id in &entry.orths {
            builder.item(id as u64, book.orths[id].value.clone(), payload);
        }
        let first = *entry
            .orths
            .first()
            .ok_or_else(|| Error::Incomplete("definition has no headword".into()))?;
        builder.alias(srcs_render::entry_route(namespace, entry.id), first as u64);
    }
    for form in &book.forms {
        builder.alias(form.value.clone(), form.orth_id as u64);
    }
    let source_words: BTreeSet<&str> = book
        .orths
        .iter()
        .map(|o| o.value.as_str())
        .chain(book.forms.iter().map(|f| f.value.as_str()))
        .collect();
    // KOReader passes the entire bword URI suffix, including #fragment, to sdcv.
    // Add those exact keys while retaining the ordinary route aliases for other readers.
    let mut fragment_aliases = BTreeSet::new();
    for link in &plan.links {
        if link.target.anchor.is_empty() {
            continue;
        }
        let word = format!(
            "{}#{}",
            link.route,
            srcs_reader::uri::percent_encode(&link.target.anchor, false)
        );
        if source_words.contains(word.as_str()) {
            return Err(Error::Incomplete(
                "fragment route collides with source word".into(),
            ));
        }
        let target_id = if let Some(id) = link.target_entry {
            book.entries[id].orths[0] as u64
        } else {
            (book.orths.len() + plan.page_ids[&link.target.file]) as u64
        };
        if fragment_aliases.insert((word.clone(), target_id)) {
            builder.alias(word, target_id);
        }
    }
    for (file, &id) in &plan.page_ids {
        let page = &book.pages[file];
        let rendered = plan.chapter(book, page, style)?;
        let payload = builder.append(&rendered.bytes)?;
        articles.push(article_audit(
            "chapter",
            id,
            file,
            page.body,
            &book.files[file],
            &rendered,
            payload,
        )?);
        let word = chapter_word(id, &page.title, options.labels);
        if source_words.contains(word.as_str()) {
            return Err(Error::Incomplete(
                "chapter key collides with source word".into(),
            ));
        }
        let target_id = (book.orths.len() + id) as u64;
        builder.item(target_id, word, payload);
        builder.alias(srcs_render::page_route(namespace, id), target_id);
    }
    for (i, (title, images)) in [
        (text.source_images, &source_gallery),
        (text.compiled_images, &compiled_gallery),
    ]
    .into_iter()
    .enumerate()
    {
        if source_words.contains(title) {
            return Err(Error::Incomplete(
                "gallery key collides with source word".into(),
            ));
        }
        let original_gallery = gallery(title, images);
        let rendered_gallery = match plan.profile_style_set() {
            Some(set) => {
                let mut html = format!(
                    "{}<div class=\"m2s-readable\">",
                    plan.style_prefix(set, style)
                )
                .into_bytes();
                html.extend_from_slice(&original_gallery);
                html.extend_from_slice(b"</div>");
                html
            }
            None => original_gallery,
        };
        let payload = builder.append(&rendered_gallery)?;
        let id = (book.orths.len() + book.pages.len() + i) as u64;
        builder.item(id, title.into(), payload);
    }
    let written = builder.finish(&book.package.title, options.offset_bits, limits)?;
    Ok(DictionaryFacts {
        written,
        articles,
        images,
    })
}

/// Every generated internal route must resolve to exactly its intended payload.
fn check_routes(
    parsed: &ParsedDictionary,
    prepared: &Prepared,
    articles: &[ArticleAudit],
) -> Result<()> {
    let Prepared {
        namespace,
        book,
        plan,
        ..
    } = prepared;
    for entry in &book.entries {
        let targets = parsed.lookup(&srcs_render::entry_route(namespace, entry.id));
        if targets.len() != 1
            || parsed.entries[targets[0]].offset != articles[entry.id].payload.offset
        {
            return Err(Error::Verify("internal definition route readback".into()));
        }
    }
    for (file, &id) in &plan.page_ids {
        let targets = parsed.lookup(&srcs_render::page_route(namespace, id));
        let expected = articles
            .get(book.entries.len() + id)
            .ok_or_else(|| Error::Verify("chapter audit ordinal".into()))?;
        if targets.len() != 1
            || parsed.entries[targets[0]].offset != expected.payload.offset
            || expected.source_file != *file
        {
            return Err(Error::Verify("internal chapter route readback".into()));
        }
    }
    for link in &plan.links {
        let key = if link.target.anchor.is_empty() {
            link.route.clone()
        } else {
            format!(
                "{}#{}",
                link.route,
                srcs_reader::uri::percent_encode(&link.target.anchor, false)
            )
        };
        let expected = link
            .target_entry
            .unwrap_or(book.entries.len() + plan.page_ids[&link.target.file]);
        let hits = parsed.lookup(&key);
        if hits.len() != 1 || parsed.entries[hits[0]].offset != articles[expected].payload.offset {
            return Err(Error::Verify("reader exact fragment route readback".into()));
        }
    }
    Ok(())
}

/// The full bundle's Audit/, Source/ and Browser/ trees.
fn write_extras(
    tree: &mut impl Tree,
    prepared: &Prepared,
    audit: &AuditInputs,
    facts: &DictionaryFacts,
    labels: LabelLanguage,
) -> Result<()> {
    let Prepared {
        source,
        book,
        plan,
        resources,
        ..
    } = prepared;
    let text = labels.text();
    tree.put("Audit/original.mobi", source)?;
    tree.put("Audit/embedded-source.zip", audit.archive)?;
    tree.put("Audit/rawml.bin", &audit.rawml)?;
    tree.put_json("Audit/container-header.json", &audit.mobi.header)?;
    tree.put_json("Audit/text-records.json", &audit.text_records)?;
    tree.put_json("Audit/compiled-crosscheck.json", &audit.cross)?;
    tree.put_json("Audit/source-definitions.json", &book.entries)?;
    tree.put_json("Audit/source-headwords.json", &book.orths)?;
    tree.put_json("Audit/source-inflections.json", &book.forms)?;
    tree.put_json("Audit/source-pages.json", &book.pages)?;
    tree.put_json("Audit/package.json", &book.package)?;
    tree.put_json("Audit/render-plan.json", plan)?;
    let pdb_records=audit.mobi.pdb.records.iter().enumerate().map(|(n,span)|Ok(serde_json::json!({"number":n,"span":span,"role":audit.cross.record_roles[n],"sha256":sha256(span.bytes(source)?)}))).collect::<Result<Vec<_>>>()?;
    tree.put_json("Audit/pdb-records.json", &pdb_records)?;
    for (file, bytes) in &book.files {
        tree.put(&format!("Source/{file}"), bytes)?;
        if let Some(page) = book.pages.get(file) {
            tree.put(
                &format!("Browser/{}", srcs_render::browser_path(file)),
                &plan.browser_page(book, page)?,
            )?;
        } else {
            tree.put(&format!("Browser/content/{file}"), bytes)?;
        }
    }
    for resource in resources {
        tree.put(
            &format!("Browser/compiled/{}", resource.filename),
            resource.source_span.bytes(source)?,
        )?;
    }
    tree.put_json("Audit/images.json", &facts.images)?;
    tree.put("Browser/index.html", &browser::index(book, plan, labels))?;
    tree.put("Browser/lookup-data.js", &browser::lookup_data(book)?)?;
    tree.put("Browser/viewer.css", browser::CSS.as_bytes())?;
    tree.put("Browser/viewer.js", browser::JS.as_bytes())?;
    let browser_gallery: Vec<(String, String)> = facts
        .images
        .iter()
        .map(|image| {
            let dir = if image.origin == "source_zip" {
                "content"
            } else {
                "compiled"
            };
            (image.file.clone(), format!("{dir}/{}", image.file))
        })
        .collect();
    let mut all_images=format!("<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body class=\"m2s-readable\">",
        text.html_lang,srcs_render::escape(text.all_images),srcs_render::readability::CSS).into_bytes();
    all_images.extend(gallery(text.all_images, &browser_gallery));
    all_images.extend_from_slice(b"</body></html>");
    tree.put("Browser/images.html", &all_images)?;
    tree.put_json("Audit/articles.json", &facts.articles)?;
    tree.put_json("Audit/catalog-items.json", &facts.written.items)?;
    tree.put_json("Audit/catalog-aliases.json", &facts.written.aliases)?;
    Ok(())
}

fn report(
    prepared: &Prepared,
    facts: &DictionaryFacts,
    options: OutputOptions,
    profile: Profile,
) -> SourceReport {
    let Prepared {
        namespace,
        book,
        plan,
        resources,
        rawml_bytes,
        classified_pdb_records,
        ..
    } = prepared;
    let catalog = &facts.written.encoded.catalog;
    let source_images = facts
        .images
        .iter()
        .filter(|i| i.origin == "source_zip")
        .count();
    SourceReport {
        schema: SCHEMA,
        backend: BACKEND.into(),
        implemented_content_checks_passed: true,
        rendering_status: "unverified_reader_dependent".into(),
        layout_profile: plan.layout_profile.clone(),
        source_sha256: namespace.clone(),
        source_archive_sha256: book.archive_sha256.clone(),
        source_headwords: book.orths.len(),
        source_aliases: book.forms.len(),
        definitions: book.entries.len(),
        chapters: book.pages.len(),
        supplement_entries: book.pages.len() + 2,
        output_entries: catalog.index.len(),
        output_synonyms: catalog.synonyms.len(),
        source_body_bytes: book.source_body_bytes(),
        covered_source_body_bytes: book.source_body_bytes(),
        rawml_bytes: *rawml_bytes,
        source_files: book.files.len(),
        source_images,
        compiled_images: resources.len(),
        internal_links: plan.links.len(),
        external_links: book
            .pages
            .values()
            .map(|p| p.links.iter().filter(|l| l.target.is_none()).count())
            .sum(),
        source_image_references: book.pages.values().map(|p| p.images.len()).sum(),
        classified_pdb_records: *classified_pdb_records,
        skipped_entries: 0,
        offset_bits: options.offset_bits,
        reader: options.reader,
        verification_scope: verification_scope(profile),
    }
}
/// The checks each profile runs. Only the full bundle keeps every source
/// file and is regenerated before publication.
fn verification_scope(profile: Profile) -> Vec<String> {
    let full = profile == Profile::Bundle;
    let mut scope = vec![
        "SRCS/compiled headword and inflection multisets with ownership and multiplicity",
        "Every definition's whitespace-normalized visible text agrees with compiled MOBI",
        if full {
            "Every source chapter body is rendered by byte-preserving edits; all ZIP files retained exactly"
        } else {
            "Every source chapter body is rendered by byte-preserving edits"
        },
        "All raster images decoded and retained; source link targets resolved",
        "Actual StarDict index, synonyms and shared payload ranges independently read back",
    ];
    if full {
        scope.push("Complete bundle regenerated from original MOBI before atomic publication");
    }
    scope.push("Display/layout equivalence and reading-system behavior require reader acceptance");
    scope.into_iter().map(String::from).collect()
}

const README: &[u8] = b"mobi2star native Rust source bundle\n\nStarDict/: import the whole directory including res/.\nBrowser/index.html: offline viewer (JavaScript runs only in your browser).\nSource/: byte-exact original publisher-source files.\nAudit/: original MOBI, embedded ZIP, decompressed compiled text and provenance.\n\nContent checks are scoped in report.json. Reader rendering remains unverified.\nVerify with: mobi2star verify BUNDLE --source ORIGINAL.mobi --json\n";

/// Hashes every file under `root` into the manifest, within the budget.
fn write_manifest(
    tree: &mut impl Tree,
    root: &Path,
    namespace: String,
    options: OutputOptions,
    limits: &Limits,
) -> Result<()> {
    let mut total = 0u64;
    let files = collect_files(root)?
        .into_iter()
        .map(|path| {
            let (bytes, sha256) = hash_file(&root.join(&path))?;
            total = total
                .checked_add(bytes)
                .ok_or_else(|| Error::Limit("bundle byte overflow".into()))?;
            if total > limits.output_bytes {
                return Err(Error::Limit("aggregate bundle byte budget".into()));
            }
            Ok(FileDigest {
                path,
                bytes,
                sha256,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = SourceManifest {
        schema: SCHEMA,
        backend: BACKEND.into(),
        tool: TOOL.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        source_sha256: namespace,
        offset_bits: options.offset_bits,
        labels: options.labels,
        reader: options.reader,
        files,
    };
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    if total
        .checked_add(encoded.len() as u64 + 1)
        .is_none_or(|n| n > limits.output_bytes)
    {
        return Err(Error::Limit("aggregate bundle with manifest".into()));
    }
    tree.put_json("manifest.json", &manifest)
}

/// Builds the full bundle in `root`: dictionary, readback checks, audit
/// trees, report and manifest.
fn build(
    source: &[u8],
    root: &Path,
    limits: &Limits,
    options: OutputOptions,
) -> Result<SourceReport> {
    let (prepared, audit) = prepare(source, limits)?;
    let mut tree = DiskTree::new(root, limits);
    let facts = write_dictionary(
        &mut tree,
        &prepared,
        DICTIONARY_DIR,
        limits,
        options,
        &mut |_| {},
    )?;
    let parsed = check_written(&mut *tree.readback()?, &facts.written, limits)?;
    check_routes(&parsed, &prepared, &facts.articles)?;
    write_extras(&mut tree, &prepared, &audit, &facts, options.labels)?;
    let report = report(&prepared, &facts, options, Profile::Bundle);
    tree.put_json("report.json", &report)?;
    tree.put("README.txt", README)?;
    write_manifest(&mut tree, root, prepared.namespace.clone(), options, limits)?;
    sync_tree(root)?;
    Ok(report)
}

/// Builds only the dictionary, in the folder `folder` names from the book
/// title, with the checks that run during conversion: the independent
/// readback and every internal route. The compiled-side audit inputs are
/// dropped as soon as the cross-check has passed.
pub(crate) fn build_dictionary(
    source: &[u8],
    tree: &mut impl Tree,
    folder: impl FnOnce(&str) -> String,
    limits: &Limits,
    options: OutputOptions,
    progress: &mut dyn FnMut(Stage),
) -> Result<SourceReport> {
    let (prepared, audit) = prepare(source, limits)?;
    drop(audit);
    let dir = folder(&prepared.book.package.title);
    let facts = write_dictionary(tree, &prepared, &dir, limits, options, progress)?;
    progress(Stage::Checking);
    let parsed = check_written(&mut *tree.readback()?, &facts.written, limits)?;
    check_routes(&parsed, &prepared, &facts.articles)?;
    Ok(report(&prepared, &facts, options, Profile::Stardict))
}

pub fn convert_source(
    input: &Path,
    output: &Path,
    limits: &Limits,
    options: OutputOptions,
) -> Result<(PathBuf, SourceReport)> {
    let source = read_bounded(input, limits.input_bytes)?;
    let tx = Transaction::begin(output)?;
    let report = build(&source, tx.path()?, limits, options)?;
    drop(source);
    let verified = verify_source(tx.path()?, Some(input), limits)?;
    if report != verified {
        return Err(Error::Verify(
            "source report changed during staging verification".into(),
        ));
    }
    Ok((tx.commit()?, verified))
}

pub fn verify_source(
    root: &Path,
    original: Option<&Path>,
    limits: &Limits,
) -> Result<SourceReport> {
    let manifest_bytes = manifest::read(root, 16 * 1024 * 1024)?;
    check_header(&manifest_bytes, SCHEMA)?;
    let manifest: SourceManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.backend != BACKEND || !matches!(manifest.offset_bits, 32 | 64) {
        return Err(Error::Verify("source manifest profile".into()));
    }
    let actual = collect_files(root)?;
    if actual
        != manifest
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    {
        return Err(Error::Verify(
            "manifest does not describe exact bundle membership".into(),
        ));
    }
    let mut total = manifest_bytes.len() as u64;
    for item in &manifest.files {
        let path = checked_member(root, &item.path)?;
        let metadata = path.metadata()?;
        total = total
            .checked_add(metadata.len())
            .ok_or_else(|| Error::Limit("verification byte overflow".into()))?;
        if total > limits.output_bytes {
            return Err(Error::Limit("verification bundle byte budget".into()));
        }
        if metadata.len() != item.bytes {
            return Err(Error::Verify(format!("file size mismatch: {}", item.path)));
        }
        let (bytes, hash) = hash_file(&path)?;
        if bytes != item.bytes || hash != item.sha256 {
            return Err(Error::Verify(format!(
                "file digest mismatch: {}",
                item.path
            )));
        }
    }
    let source = read_bounded(
        &checked_member(root, "Audit/original.mobi")?,
        limits.input_bytes,
    )?;
    if sha256(&source) != manifest.source_sha256 {
        return Err(Error::Verify("archived source identity".into()));
    }
    if let Some(path) = original {
        let (bytes, hash) = hash_file(path)?;
        if bytes != source.len() as u64 || hash != manifest.source_sha256 {
            return Err(Error::Verify(
                "supplied original differs from archived MOBI".into(),
            ));
        }
    }
    let parsed = stardict_io::open(&root.join("StarDict"), limits)?;
    if parsed.offset_bits != manifest.offset_bits {
        return Err(Error::Verify("offset profile differs from manifest".into()));
    }
    let stage = tempfile::Builder::new()
        .prefix("mobi2star-verify-")
        .tempdir()?;
    let expected = build(
        &source,
        stage.path(),
        limits,
        OutputOptions {
            offset_bits: manifest.offset_bits,
            labels: manifest.labels,
            reader: manifest.reader,
        },
    )?;
    let regenerated: SourceManifest = serde_json::from_slice(&read_bounded(
        &stage.path().join("manifest.json"),
        16 * 1024 * 1024,
    )?)?;
    if manifest.files != regenerated.files {
        return Err(Error::Verify(
            "regeneration differs: bundle is not the deterministic source-derived output".into(),
        ));
    }
    let report: SourceReport = serde_json::from_slice(&read_bounded(
        &checked_member(root, "report.json")?,
        1024 * 1024,
    )?)?;
    if report != expected {
        return Err(Error::Verify("audit report is not source-derived".into()));
    }
    Ok(expected)
}
