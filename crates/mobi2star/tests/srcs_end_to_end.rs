//! Fixtures are produced entirely in Rust from this project's original synthetic
//! MOBI. No publisher content or external interpreter is needed by this suite.
mod common;
use common::{
    edit_manifest, fixture, rewrite_with_digest, two_set_fixture, write_source, BASE, PAGE,
};
use lexicon_core::{sha256, LabelLanguage, Limits, TargetReader, LINK_TAG};
use mobi2star::OutputOptions;
use mobi_reader::Container;
use std::{fs, path::Path};
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
    edit_manifest(&bundle, |manifest| {
        assert_eq!(manifest["labels"], "zh");
        manifest["labels"] = "en".into();
    });
    assert!(mobi2star::verify_source(&bundle, None, &limits).is_err());
}
/// Relative path and SHA-256 of every file under `root`, sorted.
fn tree_digest(root: &Path) -> Vec<(String, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push((name, sha256(&fs::read(&path).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}
#[test]
fn each_reader_gets_its_stylesheet_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), PAGE);
    let limits = Limits::default();
    let mut unaffected = None;
    for &reader in TargetReader::ALL {
        let delivery = reader.style_delivery();
        let options = OutputOptions {
            reader,
            ..Default::default()
        };
        let (bundle, report) = mobi2star::convert_source(
            &source,
            &dir.path().join(format!("{reader:?}")),
            &limits,
            options,
        )
        .unwrap();
        assert_eq!(
            (report.reader, report.layout_profile.as_str()),
            (reader, "source")
        );
        // Source-profile books get their scoped CSS as the KOReader companion file.
        let css = fs::read_to_string(bundle.join("StarDict/dictionary.css")).unwrap();
        assert!(css.starts_with(".m2s_") && css.contains("font-family:serif"));
        assert_eq!(
            fs::read_to_string(bundle.join("StarDict/res/dictionary.css")).ok(),
            delivery.link.then(|| css.clone()),
            "{reader:?}"
        );
        // References come first, link before inline copy, then the wrapper.
        let mut prefix = String::new();
        if delivery.link {
            prefix.push_str(LINK_TAG);
        }
        if delivery.inline {
            prefix.push_str(&format!("<style>{css}</style>"));
        }
        assert!(
            common::payload(&bundle.join("StarDict"), "run")
                .starts_with(&format!("{prefix}<div class=\"m2s_")),
            "{reader:?}"
        );
        // The reader changes StarDict payloads only, never the viewer or the plan.
        let mut digest = tree_digest(&bundle.join("Browser"));
        digest.extend(
            tree_digest(&bundle.join("Audit"))
                .into_iter()
                .filter(|(n, _)| n == "render-plan.json"),
        );
        assert_eq!(
            unaffected.get_or_insert_with(|| digest.clone()),
            &digest,
            "{reader:?}"
        );
        mobi2star::verify_source(&bundle, Some(&source), &limits).unwrap();
    }
    // Verification regenerates with the recorded reader.
    let bundle = dir.path().join("Universal/bundle");
    assert!(matches!(
        mobi2star::verify_bundle(&bundle, Some(&source), &limits),
        Ok(mobi2star::ConversionReport::Source(_))
    ));
    edit_manifest(&bundle, |manifest| {
        assert_eq!(manifest["reader"], "universal");
        manifest["reader"] = "koreader".into();
    });
    assert!(mobi2star::verify_source(&bundle, None, &limits).is_err());
}
#[test]
fn pages_with_different_stylesheets_keep_their_own_rules() {
    let mobi = two_set_fixture();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("two-sets.mobi");
    fs::write(&source, &mobi).unwrap();
    let scope = html_preserve::css::scope_class(&sha256(&mobi));
    let limits = Limits::default();
    for reader in [TargetReader::Koreader, TargetReader::Readest] {
        let options = OutputOptions {
            reader,
            ..Default::default()
        };
        let out = dir.path().join(format!("{reader:?}"));
        let (bundle, report) = mobi2star::convert_source(&source, &out, &limits, options).unwrap();
        assert_eq!(report.chapters, 2);
        // The companion file holds both sets, each under its own class.
        let css = fs::read_to_string(bundle.join("StarDict/dictionary.css")).unwrap();
        assert!(css.contains(&format!(".{scope} .definition{{")), "{css}");
        assert!(
            css.contains(&format!(".{scope}-s1 .note{{color:gray}}")),
            "{css}"
        );
        assert!(!css.contains(&format!(".{scope} .note")), "{css}");
        // Each payload sits in its own set's wrapper and inlines only its own set.
        let root = bundle.join("StarDict");
        let entry = common::payload(&root, "run");
        let chapter = common::payload(&root, "[Chapter 000002] Second chapter");
        assert!(entry.contains(&format!("<div class=\"{scope}\">")));
        assert!(chapter.contains(&format!("<div class=\"{scope}-s1\">")));
        assert!(!chapter.contains(&format!("<div class=\"{scope}\">")));
        if reader.style_delivery().inline {
            assert!(chapter.contains(".note{color:gray}") && !chapter.contains(".definition"));
            assert!(entry.contains(".definition") && !entry.contains(".note"));
        }
        mobi2star::verify_source(&bundle, Some(&source), &limits).unwrap();
    }
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
    let name = "StarDict/dictionary.dict";
    let mut bytes = fs::read(bundle.join(name)).unwrap();
    let at = bytes.windows(3).position(|w| w == b"run").unwrap();
    bytes[at] = b'f';
    rewrite_with_digest(&bundle, name, &bytes);
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
