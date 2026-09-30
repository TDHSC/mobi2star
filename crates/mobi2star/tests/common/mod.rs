//! Helpers shared by the integration tests. Each test binary compiles this
//! module separately and uses only some of it.
#![allow(dead_code)]
use lexicon_core::{sha256, Limits};
use mobi_reader::pdb::PalmDatabase;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, ZipWriter};

/// Reads a bundle's manifest.json, lets `edit` change it, and writes it
/// back. The manifest is not in its own digest list, so nothing is rehashed.
pub fn edit_manifest(bundle: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = bundle.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

/// Replaces a bundle file and updates its manifest digest, so only
/// source-derived checks can catch the change.
pub fn rewrite_with_digest(bundle: &Path, name: &str, bytes: &[u8]) {
    fs::write(bundle.join(name), bytes).unwrap();
    edit_manifest(bundle, |manifest| {
        for file in manifest["files"].as_array_mut().unwrap() {
            if file["path"] == name {
                file["bytes"] = bytes.len().into();
                file["sha256"] = sha256(bytes).into();
            }
        }
    });
}

/// Payload of the first index entry for `word` in the StarDict files at `root`.
pub fn payload(root: &Path, word: &str) -> String {
    let limits = Limits::default();
    let parsed = stardict_io::open(root, &limits).unwrap();
    let mut dict = fs::File::open(stardict_io::dictionary_file(root).unwrap()).unwrap();
    let entry = &parsed.entries[parsed.lookup(word)[0]];
    stardict_io::read_payload(&mut dict, entry, limits.entry_bytes).unwrap()
}

// Synthetic SRCS books: the compiled fixture plus an embedded publisher-source ZIP.
pub const BASE: &[u8] = include_bytes!("../../../../tests/fixtures/uncompressed.mobi");
pub const PAGE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns:idx="idx"><head><title>Original synthetic fixture</title><link rel="stylesheet" href="style.css"/></head><body>
<p>前言：本词典用于测试。</p>
<idx:entry><idx:orth value="run"><idx:infl><idx:iform value="runs"/></idx:infl></idx:orth><div id="first" class="definition"><b>run</b><p>第一义项：中文与 café。</p><img src="image.png" alt="测试图片"/><a title="x &gt; y" href="#third">显示文字不同</a></div></idx:entry>
<idx:entry><idx:orth value="run"/><div id="second"><b>run</b><p>A separate homograph, never overwritten.</p></div></idx:entry>
<idx:entry><idx:orth value="café"/><div id="third"><b>café</b><p>重音不能被归一化丢失。<a href="#first">返回</a></p></div></idx:entry>
<p>附录：符号说明、版权测试文本。</p></body></html>"##;
pub const OPF: &str = r#"<?xml version="1.0"?><package xmlns:dc="dc"><metadata><dc:title>Original Rust SRCS fixture</dc:title></metadata><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="img" href="image.png" media-type="image/png"/></manifest><spine><itemref idref="a"/></spine></package>"#;
pub fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
pub const STYLE: &[u8] = b"body{font-family:serif}.definition{font-weight:normal}";
pub fn zip_source(page: &str, image: &[u8]) -> Vec<u8> {
    zip_files(&[
        ("OEBPS/a.xhtml", page.as_bytes()),
        ("OEBPS/package.opf", OPF.as_bytes()),
        ("OEBPS/style.css", STYLE),
        ("OEBPS/image.png", image),
    ])
}
pub fn fixture(page: &str) -> Vec<u8> {
    fixture_from(|image| zip_source(page, image))
}
/// A MOBI built from the synthetic base with `archive(image)` as its SRCS
/// record; `image` is the base's image record.
pub fn fixture_from(archive: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let pdb = PalmDatabase::parse(BASE).unwrap();
    let mut records = pdb
        .records
        .iter()
        .map(|s| s.bytes(BASE).unwrap().to_vec())
        .collect::<Vec<_>>();
    let archive = archive(records.last().unwrap());
    let mut srcs = b"SRCS\0\0\0\x10\0\0\0\0\0\0\0\0".to_vec();
    srcs.extend(archive);
    records.push(srcs);
    let count = records.len();
    let mut out = BASE[..78].to_vec();
    out[76..78].copy_from_slice(&(count as u16).to_be_bytes());
    let mut offset = 78 + 8 * count + 2;
    for (i, r) in records.iter().enumerate() {
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(i as u32).to_be_bytes());
        offset += r.len();
    }
    out.extend_from_slice(&[0, 0]);
    for r in records {
        out.extend(r);
    }
    out
}
pub fn write_source(root: &Path, page: &str) -> std::path::PathBuf {
    let path = root.join("original.mobi");
    fs::write(&path, fixture(page)).unwrap();
    path
}
/// A two-page SRCS book whose second page links a different stylesheet, so
/// its pages form two style sets.
pub fn two_set_fixture() -> Vec<u8> {
    let second = r#"<html xmlns:idx="idx"><head><title>Second chapter</title><link rel="stylesheet" href="note.css"/></head><body><p class="note">A note page.</p></body></html>"#;
    let opf = r#"<?xml version="1.0"?><package xmlns:dc="dc"><metadata><dc:title>Original Rust SRCS fixture</dc:title></metadata><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="note" href="note.css" media-type="text/css"/><item id="img" href="image.png" media-type="image/png"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#;
    fixture_from(|image| {
        zip_files(&[
            ("OEBPS/a.xhtml", PAGE.as_bytes()),
            ("OEBPS/b.xhtml", second.as_bytes()),
            ("OEBPS/package.opf", opf.as_bytes()),
            ("OEBPS/style.css", STYLE),
            ("OEBPS/note.css", b".note{color:gray}"),
            ("OEBPS/image.png", image),
        ])
    })
}
/// Every file under `root`, by `/`-separated relative path.
pub fn tree_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
                continue;
            }
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(name, fs::read(&path).unwrap());
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}
/// SHA-256 over sorted "path<TAB>sha256" lines for every file under `root`.
/// The manifest's version field is blanked, so a version bump alone does not
/// change the digest.
pub fn bundle_digest(root: &Path) -> String {
    let lines: Vec<String> = tree_files(root)
        .into_iter()
        .map(|(name, mut bytes)| {
            if name == "manifest.json" {
                let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                manifest["version"] = "".into();
                bytes = serde_json::to_vec(&manifest).unwrap();
            }
            format!("{name}\t{}", sha256(&bytes))
        })
        .collect();
    sha256(lines.join("\n").as_bytes())
}
