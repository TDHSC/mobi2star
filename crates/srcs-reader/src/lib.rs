//! Publisher-source adapter. ZIP, XHTML, OPF, images and compiled-index checks.
//! All transformations remain in srcs-render; this crate produces source facts.
#![forbid(unsafe_code)]
pub mod archive;
pub mod crosscheck;
pub mod markup;
pub mod model;
pub mod package;
pub mod uri;
pub use archive::SourceArchive;
pub use model::*;
pub use markup::{entities, plain_text, SourceParser};
pub use crosscheck::{crosscheck, Crosscheck};
use lexicon_core::{Error, Limits, Result};
use std::io::Cursor;

pub fn validate_image(bytes: &[u8], cap: usize) -> Result<(u32, u32)> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(cap as u64);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| Error::Malformed(format!("image decode: {e}")))?;
    Ok((decoded.width(), decoded.height()))
}
impl SourceBook {
    pub fn parse(archive: SourceArchive, limits: &Limits) -> Result<Self> {
        let mut book = Self { files: archive.files, archive_sha256: archive.sha256,
            archive_record: archive.record, pages: Default::default(), entries: Vec::new(),
            orths: Vec::new(), forms: Vec::new(), package: Default::default() };
        let names: Vec<String> = book.files.keys().filter(|n| is_html(n)).cloned().collect();
        let mut budget = limits.operations;
        for name in names {
            // Only one page is temporarily copied; full-book reserialization is avoided.
            let raw = book.files[&name].clone();
            let page = SourceParser::parse(&mut book, &name, &raw, limits, &mut budget)?;
            book.pages.insert(name, page);
        }
        if book.entries.is_empty() || book.orths.is_empty() { return Err(Error::Incomplete("source contains no dictionary entries".into())); }
        book.package = package::parse(&book.files, &book.pages)?;
        book.validate_references()?;
        Ok(book)
    }
    fn validate_references(&self) -> Result<()> {
        for page in self.pages.values() {
            for link in &page.links {
                if let Some(target) = &link.target {
                    let dest = self.pages.get(&target.file).ok_or_else(|| Error::Unsupported(format!("local link requires a non-HTML adapter: {} → {}", page.file, target.file)))?;
                    if !target.anchor.is_empty() && !dest.ids.contains_key(&target.anchor) {
                        return Err(Error::Incomplete(format!("missing source anchor: {}#{}", target.file, target.anchor)));
                    }
                }
            }
            for image in &page.images {
                let target = image.target.as_ref().ok_or_else(|| Error::Unsupported("remote image requires offline packaging".into()))?;
                let data = self.files.get(&target.file).ok_or_else(|| Error::Incomplete(format!("missing image {}", target.file)))?;
                if !target.anchor.is_empty() || mobi_reader::container::image_type(data).is_none() {
                    return Err(Error::Unsupported(format!("source image format/reference {}", target.file)));
                }
            }
            for path in &page.stylesheets {
                if !self.files.contains_key(path) { return Err(Error::Incomplete(format!("missing stylesheet {path}"))); }
            }
        }
        Ok(())
    }
    pub fn source_body_bytes(&self) -> usize { self.pages.values().map(|p| p.body.len()).sum() }
    pub fn ordered_entries(&self) -> Result<Vec<usize>> {
        let mut files = self.package.spine.clone();
        for path in self.pages.keys() { if !files.contains(path) { files.push(path.clone()); } }
        let mut result = Vec::new();
        for file in files { result.extend(self.pages[&file].entries.iter().copied()); }
        if result.len() != self.entries.len() { return Err(Error::Incomplete("OPF dictionary entry traversal cardinality".into())); }
        Ok(result)
    }
}
pub fn is_html(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".xhtml", ".html", ".htm"].iter().any(|ext| lower.ends_with(ext))
}
