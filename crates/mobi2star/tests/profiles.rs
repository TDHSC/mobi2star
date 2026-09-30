//! The dictionary-only profile writes exactly the full bundle's dictionary,
//! for both backends and every reader, and says what it did not check. The
//! browser's zip archive holds exactly the same files.
mod common;
use common::{fixture, tree_files, two_set_fixture, PAGE};
use lexicon_core::{Error, LabelLanguage, Limits, TargetReader};
use mobi2star::{Backend, ConversionReport, OutputOptions, Stage};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::Path,
};

const UNCOMPRESSED: &[u8] = include_bytes!("../../../tests/fixtures/uncompressed.mobi");
const HUFF: &[u8] = include_bytes!("../../../tests/fixtures/huff.mobi");

/// The compiled full bundle keeps its dictionary at the root, beside these.
fn is_compiled_extra(path: &str) -> bool {
    path.starts_with("archive/")
        || [
            "edits.json",
            "entries.jsonl",
            "manifest.json",
            "report.json",
            "resources.json",
        ]
        .contains(&path)
}

/// The dictionary files of a full bundle.
fn full_dictionary(bundle: &Path, report: &ConversionReport) -> BTreeMap<String, Vec<u8>> {
    match report {
        ConversionReport::Source(_) => tree_files(&bundle.join("StarDict")),
        ConversionReport::Compiled(_) => tree_files(bundle)
            .into_iter()
            .filter(|(path, _)| !is_compiled_extra(path))
            .collect(),
    }
}

/// Every file in a zip archive, all of which must be under `folder/`.
fn unzip(zip: &[u8], folder: &str) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let files: BTreeMap<String, Vec<u8>> = (0..archive.len())
        .map(|i| {
            let mut file = archive.by_index(i).unwrap();
            let name = file.name().strip_prefix(&format!("{folder}/")).unwrap();
            let name = name.to_owned();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect();
    assert_eq!(files.len(), archive.len());
    files
}

/// Progress starts with parsing, counts every payload up to the total and
/// ends with the readback.
fn check_progress(stages: &[Stage]) {
    assert_eq!(stages.first(), Some(&Stage::Parsing));
    assert_eq!(stages.last(), Some(&Stage::Checking));
    assert!(stages.contains(&Stage::Writing));
    let counts: Vec<(usize, usize)> = stages
        .iter()
        .filter_map(|stage| match *stage {
            Stage::Rendering { done, total } => Some((done, total)),
            _ => None,
        })
        .collect();
    assert!(counts.windows(2).all(|w| w[0].0 < w[1].0));
    let &(done, total) = counts.last().expect("rendering progress");
    assert_eq!(done, total);
}

/// The dictionary-only report equals the full one, less the claims about
/// what only the full bundle has.
fn check_report(full: ConversionReport, dictionary: &ConversionReport) {
    match (full, dictionary) {
        (ConversionReport::Source(mut full), ConversionReport::Source(dictionary)) => {
            let scope = dictionary.verification_scope.join("\n");
            assert!(!scope.contains("regenerated") && !scope.contains("ZIP files"));
            assert_eq!(full.verification_scope.len(), scope.lines().count() + 1);
            full.verification_scope = dictionary.verification_scope.clone();
            assert_eq!(&full, dictionary);
        }
        (ConversionReport::Compiled(mut full), ConversionReport::Compiled(dictionary)) => {
            assert!(!dictionary.notes.join("\n").contains("manifest"));
            full.notes.pop();
            assert_eq!(&full, dictionary);
        }
        _ => panic!("the profiles chose different backends"),
    }
}

#[test]
fn stardict_profile_writes_the_full_bundles_dictionary() {
    let limits = Limits::default();
    let sources = [
        ("srcs", fixture(PAGE)),
        ("srcs-two-sets", two_set_fixture()),
        ("compiled", UNCOMPRESSED.to_vec()),
        ("compiled-huff", HUFF.to_vec()),
    ];
    for (name, source) in sources {
        for (i, &reader) in TargetReader::ALL.iter().enumerate() {
            let dir = tempfile::tempdir().unwrap();
            let input = dir.path().join("input.mobi");
            fs::write(&input, &source).unwrap();
            let options = OutputOptions {
                offset_bits: if i == 0 { 64 } else { 32 },
                labels: if i % 2 == 0 {
                    LabelLanguage::En
                } else {
                    LabelLanguage::Zh
                },
                reader,
            };
            let (full, full_report) = mobi2star::convert_with_backend(
                &input,
                &dir.path().join("full"),
                &limits,
                options,
                Backend::Auto,
            )
            .unwrap();
            let mut stages = Vec::new();
            let (bundle, report) = mobi2star::convert_dictionary(
                &input,
                &dir.path().join("dictionary"),
                &limits,
                options,
                Backend::Auto,
                &mut |stage| stages.push(stage),
            )
            .unwrap();
            let context = format!("{name} {reader:?}");
            let mut zip_stages = Vec::new();
            let archive = mobi2star::convert_dictionary_zip(
                source.clone(),
                &Limits::browser(),
                options,
                Backend::Auto,
                &mut |stage| zip_stages.push(stage),
            )
            .unwrap();
            assert_eq!(
                unzip(&archive.zip, &archive.folder),
                tree_files(&bundle.join("StarDict")),
                "{context}"
            );
            assert_eq!(
                serde_json::to_value(&archive.report).unwrap(),
                serde_json::to_value(&report).unwrap(),
                "{context}"
            );
            assert_eq!(zip_stages, stages, "{context}");
            let published: Vec<String> = fs::read_dir(&bundle)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            assert_eq!(published, ["StarDict", "report.json"], "{context}");
            assert_eq!(
                tree_files(&bundle.join("StarDict")),
                full_dictionary(&full, &full_report),
                "{context}"
            );
            let saved: serde_json::Value =
                serde_json::from_slice(&fs::read(bundle.join("report.json")).unwrap()).unwrap();
            assert_eq!(saved, serde_json::to_value(&report).unwrap(), "{context}");
            check_report(full_report, &report);
            check_progress(&stages);
        }
    }
}

#[test]
fn dictionary_only_output_cannot_be_verified() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.mobi");
    fs::write(&input, fixture(PAGE)).unwrap();
    let limits = Limits::default();
    let (bundle, _) = mobi2star::convert_dictionary(
        &input,
        &dir.path().join("out"),
        &limits,
        OutputOptions::default(),
        Backend::Auto,
        &mut |_| {},
    )
    .unwrap();
    for result in [
        mobi2star::verify_bundle(&bundle, None, &limits).map(|_| ()),
        mobi2star::verify(&bundle, None, &limits).map(|_| ()),
        mobi2star::verify_source(&bundle, None, &limits).map(|_| ()),
    ] {
        match result {
            Err(Error::Unsupported(message)) => assert!(message.contains("--profile stardict")),
            other => panic!("expected the dictionary-only error, got {other:?}"),
        }
    }
}

#[test]
fn a_failed_dictionary_conversion_publishes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.mobi");
    fs::write(&input, HUFF).unwrap();
    let output = dir.path().join("out");
    let result = mobi2star::convert_dictionary(
        &input,
        &output,
        &Limits::default(),
        OutputOptions::default(),
        Backend::Srcs,
        &mut |_| {},
    );
    assert!(matches!(result, Err(Error::Unsupported(_))));
    assert!(!output.exists());
}

#[test]
fn zip_archives_are_deterministic_and_named_from_the_title() {
    for source in [fixture(PAGE), two_set_fixture(), UNCOMPRESSED.to_vec()] {
        let build = || {
            mobi2star::convert_dictionary_zip(
                source.clone(),
                &Limits::browser(),
                OutputOptions::default(),
                Backend::Auto,
                &mut |_| {},
            )
            .unwrap()
        };
        let (first, second) = (build(), build());
        assert_eq!(first.zip, second.zip);
        assert_eq!(first.folder, second.folder);
    }
    let archive = mobi2star::convert_dictionary_zip(
        fixture(PAGE),
        &Limits::browser(),
        OutputOptions::default(),
        Backend::Auto,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(archive.folder, "Original Rust SRCS fixture");
}

#[test]
fn zip_input_is_bounded() {
    let limits = Limits {
        input_bytes: HUFF.len() - 1,
        ..Limits::browser()
    };
    let result = mobi2star::convert_dictionary_zip(
        HUFF.to_vec(),
        &limits,
        OutputOptions::default(),
        Backend::Auto,
        &mut |_| {},
    );
    assert!(matches!(result, Err(Error::Limit(_))));
}
