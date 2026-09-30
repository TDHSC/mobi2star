//! Writing one StarDict dictionary through a `Tree`, shared by both
//! backends, and checking what was actually written.
//!
//! Writing and checking are separate steps: a zip archive can only be read
//! back once it is finished, while a directory can be read back at once.
use crate::tree::Tree;
use lexicon_core::{checked_member, read_bounded, sha256, Error, Limits, Result};
use stardict_io::{
    CatalogAlias, CatalogItem, EncodedCatalog, ParsedDictionary, Payload, PayloadWriter,
};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

/// Makes the tree's open stream usable as the payload writer's sink while
/// the builder holds the only borrow of the tree.
struct TreeStream<'t, T: Tree>(&'t mut T);
impl<T: Tree> TreeStream<'_, T> {
    fn open(&mut self) -> io::Result<&mut dyn Write> {
        self.0.stream_writer().map_err(io::Error::other)
    }
}
impl<T: Tree> Write for TreeStream<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.open()?.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.open()?.flush()
    }
}

/// Builds `{dir}dictionary.dict/.idx/.syn/.ifo` in a tree. Stylesheet and
/// resource files must be written before `start`, because the tree has one
/// stream open for the payloads until `finish`.
pub(crate) struct DictionaryBuilder<'t, T: Tree> {
    dir: String,
    payloads: PayloadWriter<TreeStream<'t, T>>,
    digests: Vec<(Payload, String)>,
    items: Vec<CatalogItem>,
    aliases: Vec<CatalogAlias>,
}

/// What a finished builder wrote, kept for checking it and for audits.
pub(crate) struct WrittenDictionary {
    pub dir: String,
    pub encoded: EncodedCatalog,
    pub digests: Vec<(Payload, String)>,
    pub items: Vec<CatalogItem>,
    pub aliases: Vec<CatalogAlias>,
}

impl<'t, T: Tree> DictionaryBuilder<'t, T> {
    /// Opens `{dir}dictionary.dict`. Payloads may use whatever output budget
    /// the tree has left.
    pub fn start(tree: &'t mut T, dir: &str, limits: &Limits, bits: u8) -> Result<Self> {
        let mut payload_limits = limits.clone();
        payload_limits.output_bytes = limits.output_bytes.saturating_sub(tree.used());
        tree.stream(&format!("{dir}dictionary.dict"))?;
        Ok(Self {
            dir: dir.to_owned(),
            payloads: PayloadWriter::with_writer(TreeStream(tree), &payload_limits, bits)?,
            digests: Vec::new(),
            items: Vec::new(),
            aliases: Vec::new(),
        })
    }
    /// Appends one HTML payload and records its digest for the readback.
    pub fn append(&mut self, html: &[u8]) -> Result<Payload> {
        let payload = self.payloads.append(html)?;
        self.digests.push((payload, sha256(html)));
        Ok(payload)
    }
    pub fn item(&mut self, id: u64, word: String, payload: Payload) {
        self.items.push(CatalogItem { id, word, payload });
    }
    pub fn alias(&mut self, word: String, target_id: u64) {
        self.aliases.push(CatalogAlias { word, target_id });
    }
    /// Closes the payload stream and writes the index files.
    pub fn finish(self, title: &str, bits: u8, limits: &Limits) -> Result<WrittenDictionary> {
        let (TreeStream(tree), _) = self.payloads.into_inner()?;
        let dictionary_bytes = tree.end_stream()?;
        let encoded = stardict_io::encode_catalog(
            title,
            &self.items,
            &self.aliases,
            dictionary_bytes,
            bits,
            limits,
        )?;
        let dir = &self.dir;
        tree.put(&format!("{dir}dictionary.idx"), &encoded.idx)?;
        tree.put(&format!("{dir}dictionary.syn"), &encoded.syn)?;
        tree.put(&format!("{dir}dictionary.ifo"), &encoded.ifo)?;
        Ok(WrittenDictionary {
            dir: self.dir,
            encoded,
            digests: self.digests,
            items: self.items,
            aliases: self.aliases,
        })
    }
}

/// Reads finished output back for checking.
pub(crate) trait ReadBack {
    fn read(&mut self, path: &str, limit: usize) -> Result<Vec<u8>>;
    /// A reader over `path` and its length.
    fn open(&mut self, path: &str) -> Result<(Box<dyn Read + '_>, u64)>;
}

/// Output files in a directory.
pub(crate) struct DirReadBack<'a>(pub &'a Path);
impl ReadBack for DirReadBack<'_> {
    fn read(&mut self, path: &str, limit: usize) -> Result<Vec<u8>> {
        read_bounded(&checked_member(self.0, path)?, limit)
    }
    fn open(&mut self, path: &str) -> Result<(Box<dyn Read + '_>, u64)> {
        let file = File::open(checked_member(self.0, path)?)?;
        let length = file.metadata()?.len();
        Ok((Box::new(io::BufReader::new(file)), length))
    }
}

/// Reads the dictionary back from `output` and checks it independently of
/// the writer: the parsed index and synonyms must equal the catalog, and
/// every payload must hash to what was appended.
pub(crate) fn check_written(
    output: &mut impl ReadBack,
    written: &WrittenDictionary,
    limits: &Limits,
) -> Result<ParsedDictionary> {
    let dir = &written.dir;
    let ifo = output.read(&format!("{dir}dictionary.ifo"), 65536)?;
    let idx = output.read(&format!("{dir}dictionary.idx"), limits.input_bytes)?;
    let syn = output.read(&format!("{dir}dictionary.syn"), limits.input_bytes)?;
    let (dict, length) = output.open(&format!("{dir}dictionary.dict"))?;
    let parsed = stardict_io::parse(&ifo, &idx, &syn, length, limits)?;
    let catalog = &written.encoded.catalog;
    if parsed.entries != catalog.index || parsed.synonyms != catalog.synonyms {
        return Err(Error::Verify("catalog differs after readback".into()));
    }
    stardict_io::check_payloads(dict, &written.digests)?;
    Ok(parsed)
}
