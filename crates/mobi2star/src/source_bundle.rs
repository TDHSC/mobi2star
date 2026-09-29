//! Deterministic SRCS bundle production. The verifier rebuilds from the archived
//! MOBI and compares the actual artifacts, in addition to an independent IDX reader.
use crate::{
    bundle::{collect_files, write_bytes, write_json},
    transaction::{sync_directory, Transaction},
    FileDigest, OutputOptions,
};
use lexicon_core::{
    checked_member, hash_file, read_bounded, sha256, Error, LabelLanguage, Limits, Result, Span,
    StyleDelivery,
};
use mobi_reader::Container;
use serde::{Deserialize, Serialize};
use srcs_reader::{SourceArchive, SourceBook};
use srcs_render::{browser, Plan, Rendered};
use stardict_io::{CatalogAlias, CatalogItem, Payload, PayloadWriter};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
};

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

/// Every application write goes to a private transaction tree; aggregate disk
/// bytes are checked while emitting files, then again including StarDict files.
struct Sink<'a> {
    root: &'a Path,
    used: u64,
    limit: u64,
}
impl<'a> Sink<'a> {
    fn new(root: &'a Path, limits: &Limits) -> Self {
        Self {
            root,
            used: 0,
            limit: limits.output_bytes,
        }
    }
    fn bytes(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        srcs_reader::uri::validate_path(name)?;
        self.used = self
            .used
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| Error::Limit("bundle byte overflow".into()))?;
        if self.used > self.limit {
            return Err(Error::Limit("aggregate bundle byte budget".into()));
        }
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_bytes(self.root, name, bytes)
    }
    fn json<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.bytes(name, &bytes)
    }
    fn account_dictionary(&mut self, bytes: u64) -> Result<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .ok_or_else(|| Error::Limit("bundle byte overflow".into()))?;
        if self.used > self.limit {
            return Err(Error::Limit("aggregate bundle byte budget".into()));
        }
        Ok(())
    }
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

fn build(
    source: &[u8],
    root: &Path,
    limits: &Limits,
    options: OutputOptions,
) -> Result<SourceReport> {
    let OutputOptions {
        offset_bits: bits,
        labels,
    } = options;
    let text = labels.text();
    // Every payload still carries its own stylesheet copy.
    let style = StyleDelivery::INLINE;
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
    let mut sink = Sink::new(root, limits);
    if plan.layout_profile == srcs_render::readability::PROFILE {
        sink.bytes(
            "StarDict/dictionary.css",
            srcs_render::readability::CSS.as_bytes(),
        )?;
    }
    sink.bytes("Audit/original.mobi", source)?;
    sink.bytes("Audit/embedded-source.zip", archive)?;
    sink.bytes("Audit/rawml.bin", &rawml)?;
    sink.json("Audit/container-header.json", &mobi.header)?;
    sink.json("Audit/text-records.json", &text_records)?;
    sink.json("Audit/compiled-crosscheck.json", &cross)?;
    sink.json("Audit/source-definitions.json", &book.entries)?;
    sink.json("Audit/source-headwords.json", &book.orths)?;
    sink.json("Audit/source-inflections.json", &book.forms)?;
    sink.json("Audit/source-pages.json", &book.pages)?;
    sink.json("Audit/package.json", &book.package)?;
    sink.json("Audit/render-plan.json", &plan)?;
    let pdb_records=mobi.pdb.records.iter().enumerate().map(|(n,span)|Ok(serde_json::json!({"number":n,"span":span,"role":cross.record_roles[n],"sha256":sha256(span.bytes(source)?)}))).collect::<Result<Vec<_>>>()?;
    sink.json("Audit/pdb-records.json", &pdb_records)?;
    let mut image_audit = Vec::new();
    let mut source_gallery = Vec::new();
    let mut compiled_gallery = Vec::new();
    let mut browser_gallery = Vec::new();
    for (file, bytes) in &book.files {
        sink.bytes(&format!("Source/{file}"), bytes)?;
        if let Some(page) = book.pages.get(file) {
            sink.bytes(
                &format!("Browser/{}", srcs_render::browser_path(file)),
                &plan.browser_page(&book, page)?,
            )?;
        } else {
            sink.bytes(&format!("Browser/content/{file}"), bytes)?;
        }
        if !book.pages.contains_key(file) {
            sink.bytes(&format!("StarDict/res/source/{file}"), bytes)?;
        }
        if mobi_reader::container::image_type(bytes).is_some() {
            let (width, height) = srcs_reader::validate_image(bytes, limits.text_bytes)?;
            image_audit.push(ImageAudit {
                file: file.clone(),
                origin: "source_zip".into(),
                bytes: bytes.len(),
                sha256: sha256(bytes),
                width,
                height,
            });
            source_gallery.push((file.clone(), format!("source/{file}")));
            browser_gallery.push((file.clone(), format!("content/{file}")));
        }
    }
    for resource in &resources {
        let bytes = resource.source_span.bytes(source)?;
        let (width, height) = srcs_reader::validate_image(bytes, limits.text_bytes)?;
        sink.bytes(
            &format!("StarDict/res/compiled/{}", resource.filename),
            bytes,
        )?;
        sink.bytes(&format!("Browser/compiled/{}", resource.filename), bytes)?;
        image_audit.push(ImageAudit {
            file: resource.filename.clone(),
            origin: format!("pdb_record_{}", resource.pdb_record),
            bytes: bytes.len(),
            sha256: sha256(bytes),
            width,
            height,
        });
        compiled_gallery.push((
            resource.filename.clone(),
            format!("compiled/{}", resource.filename),
        ));
        browser_gallery.push((
            resource.filename.clone(),
            format!("compiled/{}", resource.filename),
        ));
    }
    sink.json("Audit/images.json", &image_audit)?;
    sink.bytes("Browser/index.html", &browser::index(&book, &plan, labels))?;
    sink.bytes("Browser/lookup-data.js", &browser::lookup_data(&book)?)?;
    sink.bytes("Browser/viewer.css", browser::CSS.as_bytes())?;
    sink.bytes("Browser/viewer.js", browser::JS.as_bytes())?;
    let mut all_images=format!("<!doctype html><html lang=\"{}\"><head><meta charset=\"utf-8\"><title>{}</title><style>{}</style></head><body class=\"m2s-readable\">",
        text.html_lang,srcs_render::escape(text.all_images),srcs_render::readability::CSS).into_bytes();
    all_images.extend(gallery(text.all_images, &browser_gallery));
    all_images.extend_from_slice(b"</body></html>");
    sink.bytes("Browser/images.html", &all_images)?;
    let dictroot = root.join("StarDict");
    fs::create_dir_all(&dictroot)?;
    let mut payload_limits = limits.clone();
    payload_limits.output_bytes = limits.output_bytes.saturating_sub(sink.used);
    let mut writer = PayloadWriter::new(&dictroot, &payload_limits, bits)?;
    let mut articles = Vec::new();
    let mut items = Vec::new();
    let mut aliases = Vec::new();
    for entry in &book.entries {
        let rendered = plan.definition(&book, entry, style)?;
        let payload = writer.append(&rendered.bytes)?;
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
            items.push(CatalogItem {
                id: id as u64,
                word: book.orths[id].value.clone(),
                payload,
            });
        }
        let first = *entry
            .orths
            .first()
            .ok_or_else(|| Error::Incomplete("definition has no headword".into()))?;
        aliases.push(CatalogAlias {
            word: srcs_render::entry_route(&namespace, entry.id),
            target_id: first as u64,
        });
    }
    for form in &book.forms {
        aliases.push(CatalogAlias {
            word: form.value.clone(),
            target_id: form.orth_id as u64,
        });
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
            aliases.push(CatalogAlias { word, target_id });
        }
    }

    for (file, &id) in &plan.page_ids {
        let page = &book.pages[file];
        let rendered = plan.chapter(&book, page, style)?;
        let payload = writer.append(&rendered.bytes)?;
        articles.push(article_audit(
            "chapter",
            id,
            file,
            page.body,
            &book.files[file],
            &rendered,
            payload,
        )?);
        let word = chapter_word(id, &page.title, labels);
        if source_words.contains(word.as_str()) {
            return Err(Error::Incomplete(
                "chapter key collides with source word".into(),
            ));
        }
        let target_id = (book.orths.len() + id) as u64;
        items.push(CatalogItem {
            id: target_id,
            word,
            payload,
        });
        aliases.push(CatalogAlias {
            word: srcs_render::page_route(&namespace, id),
            target_id,
        });
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
        let payload = writer.append(&rendered_gallery)?;
        items.push(CatalogItem {
            id: (book.orths.len() + book.pages.len() + i) as u64,
            word: title.into(),
            payload,
        });
    }
    let dictbytes = writer.finish()?;
    sink.account_dictionary(dictbytes)?;
    let catalog = stardict_io::write_catalog(
        &dictroot,
        &book.package.title,
        &items,
        &aliases,
        dictbytes,
        bits,
        limits,
    )?;
    for name in ["dictionary.idx", "dictionary.syn", "dictionary.ifo"] {
        sink.account_dictionary(dictroot.join(name).metadata()?.len())?;
    }
    // Separately implemented reader checks the files just written, including all
    // duplicate headwords, alias ordinals and exact shared physical ranges.
    let disk = stardict_io::open(&dictroot, limits)?;
    if disk.entries != catalog.index || disk.synonyms != catalog.synonyms {
        return Err(Error::Verify("catalog differs after disk readback".into()));
    }
    let mut dictfile = File::open(&disk.dictionary_path)?;
    for article in &articles {
        let at = stardict_io::IndexEntry {
            word: String::new(),
            offset: article.payload.offset,
            size: article.payload.size,
        };
        let html = stardict_io::read_payload(&mut dictfile, &at, limits.entry_bytes)?;
        if sha256(html.as_bytes()) != article.payload_sha256 {
            return Err(Error::Verify("article payload changed during write".into()));
        }
    }
    // Every generated internal route must resolve to exactly its intended payload.
    for entry in &book.entries {
        let targets = disk.lookup(&srcs_render::entry_route(&namespace, entry.id));
        if targets.len() != 1
            || disk.entries[targets[0]].offset != articles[entry.id].payload.offset
        {
            return Err(Error::Verify("internal definition route readback".into()));
        }
    }
    for (file, &id) in &plan.page_ids {
        let targets = disk.lookup(&srcs_render::page_route(&namespace, id));
        let expected = articles
            .get(book.entries.len() + id)
            .ok_or_else(|| Error::Verify("chapter audit ordinal".into()))?;
        if targets.len() != 1
            || disk.entries[targets[0]].offset != expected.payload.offset
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
        let hits = disk.lookup(&key);
        if hits.len() != 1 || disk.entries[hits[0]].offset != articles[expected].payload.offset {
            return Err(Error::Verify("reader exact fragment route readback".into()));
        }
    }
    sink.json("Audit/articles.json", &articles)?;
    sink.json("Audit/catalog-items.json", &items)?;
    sink.json("Audit/catalog-aliases.json", &aliases)?;
    let report=SourceReport{
        schema:2,backend:"srcs-rust".into(),implemented_content_checks_passed:true,rendering_status:"unverified_reader_dependent".into(),layout_profile:plan.layout_profile.clone(),
        source_sha256:namespace.clone(),source_archive_sha256:book.archive_sha256.clone(),source_headwords:book.orths.len(),source_aliases:book.forms.len(),
        definitions:book.entries.len(),chapters:book.pages.len(),supplement_entries:book.pages.len()+2,
        output_entries:catalog.index.len(),output_synonyms:catalog.synonyms.len(),source_body_bytes:book.source_body_bytes(),covered_source_body_bytes:book.source_body_bytes(),
        rawml_bytes:rawml.len(),source_files:book.files.len(),source_images:source_gallery.len(),compiled_images:resources.len(),internal_links:plan.links.len(),
        external_links:book.pages.values().map(|p|p.links.iter().filter(|l|l.target.is_none()).count()).sum(),
        source_image_references:book.pages.values().map(|p|p.images.len()).sum(),classified_pdb_records:cross.record_roles.len(),skipped_entries:0,offset_bits:bits,
        verification_scope:vec!["SRCS/compiled headword and inflection multisets with ownership and multiplicity".into(),
            "Every definition's whitespace-normalized visible text agrees with compiled MOBI".into(),
            "Every source chapter body is rendered by byte-preserving edits; all ZIP files retained exactly".into(),
            "All raster images decoded and retained; source link targets resolved".into(),
            "Actual StarDict index, synonyms and shared payload ranges independently read back".into(),
            "Complete bundle regenerated from original MOBI before atomic publication".into(),
            "Display/layout equivalence and reading-system behavior require reader acceptance".into()],
    };
    sink.json("report.json", &report)?;
    sink.bytes("README.txt",b"mobi2star native Rust source bundle\n\nStarDict/: import the whole directory including res/.\nBrowser/index.html: offline viewer (JavaScript runs only in your browser).\nSource/: byte-exact original publisher-source files.\nAudit/: original MOBI, embedded ZIP, decompressed compiled text and provenance.\n\nContent checks are scoped in report.json. Reader rendering remains unverified.\nVerify with: mobi2star verify BUNDLE --source ORIGINAL.mobi --json\n")?;
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
        schema: 2,
        backend: "srcs-rust".into(),
        tool: "mobi2star".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        source_sha256: namespace,
        offset_bits: bits,
        labels,
        files,
    };
    let encoded = serde_json::to_vec_pretty(&manifest)?;
    if total
        .checked_add(encoded.len() as u64 + 1)
        .is_none_or(|n| n > limits.output_bytes)
    {
        return Err(Error::Limit("aggregate bundle with manifest".into()));
    }
    write_json(root, "manifest.json", &manifest)?;
    sync_tree(root)?;
    Ok(report)
}
fn sync_tree(root: &Path) -> Result<()> {
    for item in fs::read_dir(root)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            sync_tree(&item.path())?;
        }
    }
    sync_directory(root)
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
    let manifest_bytes = read_bounded(&checked_member(root, "manifest.json")?, 16 * 1024 * 1024)?;
    let manifest: SourceManifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema != 2
        || manifest.backend != "srcs-rust"
        || manifest.tool != "mobi2star"
        || !matches!(manifest.offset_bits, 32 | 64)
    {
        return Err(Error::Verify("source manifest profile".into()));
    }
    if manifest.version != env!("CARGO_PKG_VERSION") {
        return Err(Error::Unsupported(
            "deterministic verification requires the producing mobi2star version".into(),
        ));
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
