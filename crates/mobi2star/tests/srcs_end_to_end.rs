//! Fixtures are produced entirely in Rust from this project's original synthetic
//! MOBI. No publisher content or external interpreter is needed by this suite.
use lexicon_core::{sha256, LabelLanguage, Limits};
use mobi2star::OutputOptions;
use mobi_reader::{pdb::PalmDatabase, Container};
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, ZipWriter};
const BASE: &[u8] = include_bytes!("../../../tests/fixtures/uncompressed.mobi");
const PAGE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns:idx="idx"><head><title>Original synthetic fixture</title><link rel="stylesheet" href="style.css"/></head><body>
<p>前言：本词典用于测试。</p>
<idx:entry><idx:orth value="run"><idx:infl><idx:iform value="runs"/></idx:infl></idx:orth><div id="first" class="definition"><b>run</b><p>第一义项：中文与 café。</p><img src="image.png" alt="测试图片"/><a title="x &gt; y" href="#third">显示文字不同</a></div></idx:entry>
<idx:entry><idx:orth value="run"/><div id="second"><b>run</b><p>A separate homograph, never overwritten.</p></div></idx:entry>
<idx:entry><idx:orth value="café"/><div id="third"><b>café</b><p>重音不能被归一化丢失。<a href="#first">返回</a></p></div></idx:entry>
<p>附录：符号说明、版权测试文本。</p></body></html>"##;
const OPF: &str = r#"<?xml version="1.0"?><package xmlns:dc="dc"><metadata><dc:title>Original Rust SRCS fixture</dc:title></metadata><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="img" href="image.png" media-type="image/png"/></manifest><spine><itemref idref="a"/></spine></package>"#;
fn zip_source(page: &str, image: &[u8]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in [
        ("OEBPS/a.xhtml", page.as_bytes()),
        ("OEBPS/package.opf", OPF.as_bytes()),
        (
            "OEBPS/style.css",
            b"body{font-family:serif}.definition{font-weight:normal}".as_slice(),
        ),
        ("OEBPS/image.png", image),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn fixture(page: &str) -> Vec<u8> {
    let pdb = PalmDatabase::parse(BASE).unwrap();
    let mut records = pdb
        .records
        .iter()
        .map(|s| s.bytes(BASE).unwrap().to_vec())
        .collect::<Vec<_>>();
    let archive = zip_source(page, records.last().unwrap());
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
fn write_source(root: &Path, page: &str) -> std::path::PathBuf {
    let path = root.join("original.mobi");
    fs::write(&path, fixture(page)).unwrap();
    path
}
#[test]
fn rust_srcs_conversion_reopens_all_content() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let (bundle, report) = mobi2star::convert_source(
        &source,
        &dir.path().join("converted"),
        &limits,
        OutputOptions::default(),
    )
    .unwrap();
    assert_eq!(report.source_headwords, 3);
    assert_eq!(report.source_aliases, 1);
    assert_eq!(report.definitions, 3);
    assert_eq!(report.chapters, 1);
    assert_eq!(report.source_images, 1);
    assert_eq!(report.compiled_images, 1);
    assert_eq!(report.internal_links, 2);
    assert_eq!(report.source_body_bytes, report.covered_source_body_bytes);
    // 3 definition routes + 1 inflection + 1 chapter route + 2 exact route#anchor aliases (#third, #first).
    assert_eq!(report.output_entries, 6);
    assert_eq!(report.output_synonyms, 7);
    assert_eq!(
        report,
        mobi2star::verify_source(&bundle, Some(&source), &limits).unwrap()
    );
    let disk = stardict_io::open(&bundle.join("StarDict"), &limits).unwrap();
    assert_eq!(disk.lookup("run").len(), 2);
    assert_eq!(disk.lookup("runs").len(), 1);
    assert_eq!(disk.lookup("café").len(), 1);
    assert_eq!(
        fs::read(bundle.join("Source/OEBPS/a.xhtml")).unwrap(),
        PAGE.as_bytes()
    );
    assert!(
        fs::read_to_string(bundle.join("Browser/content/OEBPS/a.xhtml.html"))
            .unwrap()
            .contains("m2s-entry-0")
    );
}
#[test]
fn auto_dispatch_uses_srcs_and_lookup_accepts_bundle_root() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let (bundle, report) = mobi2star::convert_with_backend(
        &source,
        &dir.path().join("out"),
        &limits,
        OutputOptions {
            offset_bits: 64,
            ..Default::default()
        },
        mobi2star::Backend::Auto,
    )
    .unwrap();
    let report = serde_json::to_value(report).unwrap();
    assert_eq!(report["backend"], "srcs-rust");
    assert_eq!(report["offset_bits"], 64);
    let disk = stardict_io::open(&mobi2star::dictionary_root(&bundle), &limits).unwrap();
    assert_eq!(disk.offset_bits, 64);
}
#[test]
fn generated_labels_default_to_english_and_chinese_is_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let (bundle, _) = mobi2star::convert_source(
        &source,
        &dir.path().join("en"),
        &limits,
        OutputOptions::default(),
    )
    .unwrap();
    let disk = stardict_io::open(&bundle.join("StarDict"), &limits).unwrap();
    for key in [
        "[Chapter 000001] Original synthetic fixture",
        "[Source images]",
        "[Compiled MOBI images]",
    ] {
        assert_eq!(disk.lookup(key).len(), 1, "{key}");
    }
    let index = fs::read_to_string(bundle.join("Browser/index.html")).unwrap();
    assert!(
        index.contains("<html lang=\"en\">")
            && index.contains("data-found=\"Definitions found: {n}\"")
    );
    assert!(
        !index
            .chars()
            .any(|c| ('\u{3000}'..='\u{9fff}').contains(&c)),
        "English viewer has no generated CJK text"
    );
    assert!(fs::read_to_string(bundle.join("Browser/images.html"))
        .unwrap()
        .contains("<title>All source and compiled images</title>"));
    let (bundle, _) = mobi2star::convert_source(
        &source,
        &dir.path().join("zh"),
        &limits,
        OutputOptions {
            labels: LabelLanguage::Zh,
            ..Default::default()
        },
    )
    .unwrap();
    let disk = stardict_io::open(&bundle.join("StarDict"), &limits).unwrap();
    for key in [
        "〔原书章节 000001〕 Original synthetic fixture",
        "〔原始源文件图片〕",
        "〔MOBI 编译图片〕",
    ] {
        assert_eq!(disk.lookup(key).len(), 1, "{key}");
    }
    assert!(fs::read_to_string(bundle.join("Browser/index.html"))
        .unwrap()
        .contains("<html lang=\"zh-CN\">"));
    // Verification regenerates with the language recorded in the manifest.
    mobi2star::verify_source(&bundle, Some(&source), &limits).unwrap();
    let mpath = bundle.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&mpath).unwrap()).unwrap();
    assert_eq!(manifest["labels"], "zh");
    manifest["labels"] = "en".into();
    fs::write(mpath, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    assert!(mobi2star::verify_source(&bundle, None, &limits).is_err());
}
#[test]
fn source_compiled_mismatch_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), &PAGE.replace("第一义项", "changed definition"));
    let out = dir.path().join("out");
    assert!(
        mobi2star::convert_source(&source, &out, &Limits::default(), OutputOptions::default())
            .is_err()
    );
    assert!(!out.exists());
    assert!(source.exists());
}
#[test]
fn source_inflection_mismatch_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(
        dir.path(),
        &PAGE.replace("value=\"runs\"", "value=\"running\""),
    );
    assert!(mobi2star::convert_source(
        &source,
        &dir.path().join("out"),
        &Limits::default(),
        OutputOptions::default()
    )
    .is_err());
}
#[test]
fn missing_anchor_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(
        dir.path(),
        &PAGE.replace("href=\"#third\"", "href=\"#absent\""),
    );
    assert!(mobi2star::convert_source(
        &source,
        &dir.path().join("out"),
        &Limits::default(),
        OutputOptions::default()
    )
    .is_err());
}
#[test]
fn changed_payload_with_rehashed_manifest_fails_regeneration() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let (bundle, _) = mobi2star::convert_source(
        &source,
        &dir.path().join("out"),
        &limits,
        OutputOptions::default(),
    )
    .unwrap();
    let path = bundle.join("StarDict/dictionary.dict");
    let mut bytes = fs::read(&path).unwrap();
    let at = bytes.windows(3).position(|w| w == b"run").unwrap();
    bytes[at] = b'f';
    fs::write(path, &bytes).unwrap();
    let mpath = bundle.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&mpath).unwrap()).unwrap();
    for f in manifest["files"].as_array_mut().unwrap() {
        if f["path"] == "StarDict/dictionary.dict" {
            f["sha256"] = sha256(&bytes).into();
        }
    }
    fs::write(mpath, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    assert!(mobi2star::verify_source(&bundle, None, &limits).is_err());
}
#[test]
fn extra_file_and_wrong_original_fail() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let (bundle, _) = mobi2star::convert_source(
        &source,
        &dir.path().join("out"),
        &limits,
        OutputOptions::default(),
    )
    .unwrap();
    let wrong = dir.path().join("wrong.mobi");
    fs::write(&wrong, BASE).unwrap();
    assert!(mobi2star::verify_source(&bundle, Some(&wrong), &limits).is_err());
    fs::write(bundle.join("unexpected"), b"extra").unwrap();
    assert!(mobi2star::verify_source(&bundle, None, &limits).is_err());
}
#[test]
fn source_container_archive_roundtrip() {
    let source = fixture(PAGE);
    let mobi = Container::open(&source, &Limits::default()).unwrap();
    let (n, zip) = mobi.source_archive().unwrap().unwrap();
    let archive = srcs_reader::SourceArchive::read(zip, n, &Limits::default()).unwrap();
    assert_eq!(archive.files["OEBPS/a.xhtml"], PAGE.as_bytes());
}
