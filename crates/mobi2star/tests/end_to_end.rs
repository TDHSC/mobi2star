//! Synthetic-only acceptance tests. A passing suite is not a real-dictionary certification.
use lexicon_core::{Error, Limits};
use std::{fs, path::Path};

const PLAIN: &[u8] = include_bytes!("../../../tests/fixtures/uncompressed.mobi");
const PALM: &[u8] = include_bytes!("../../../tests/fixtures/palmdoc.mobi");
const HUFF: &[u8] = include_bytes!("../../../tests/fixtures/huff.mobi");
const OLD: &[u8] = include_bytes!("../../../tests/fixtures/old-inflections.mobi");
fn input(root: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let path = root.join("input.mobi");
    fs::write(&path, bytes).unwrap();
    path
}
#[test]
fn native_round_trip_all_compressions_and_inflection_formats() {
    for (bytes, alias) in [(PLAIN, "runs"), (PALM, "runs"), (HUFF, "runs"), (OLD, "cities")] {
        let dir = tempfile::tempdir().unwrap();
        let source = input(dir.path(), bytes);
        let (bundle, report) = mobi2star::convert(&source, &dir.path().join("output"), &Limits::default(), 32).unwrap();
        assert_eq!(report.source_headwords, 3);
        assert_eq!(report.source_aliases, 1);
        assert_eq!(report.supplement_entries, 2);
        assert_eq!(report.output_entries, 5);
        assert_eq!(report.output_synonyms, 6);
        assert_eq!(report.resolved_internal_links, 2);
        assert_eq!(report.resolved_resource_references, 1);
        assert_eq!(report.skipped_entries, 0);
        assert!(report.rendering_status.starts_with("unverified"));
        assert_eq!(report, mobi2star::verify(&bundle, Some(&source), &Limits::default()).unwrap());
        let parsed = stardict_io::open(&bundle, &Limits::default()).unwrap();
        assert_eq!(parsed.lookup(alias).len(), 1);
        assert_eq!(parsed.lookup("café").len(), 1);
        assert_eq!(fs::read(bundle.join("archive/source.mobi")).unwrap(), bytes);
    }
}
#[test]
fn homographs_are_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let (bundle, _) = mobi2star::convert(&source, &dir.path().join("out"), &Limits::default(), 32).unwrap();
    let parsed = stardict_io::open(&bundle, &Limits::default()).unwrap();
    assert_eq!(parsed.lookup("run").len(), 2);
    assert!(parsed.lookup("cafe").is_empty()); // no normalization or speculative aliases
    let mut file = fs::File::open(parsed.dictionary_path.clone()).unwrap();
    let payloads = parsed.lookup("run").into_iter().map(|n| stardict_io::read_payload(&mut file, &parsed.entries[n], 1024 * 1024).unwrap()).collect::<Vec<_>>();
    assert_ne!(payloads[0], payloads[1]);
    assert!(payloads.iter().any(|p| p.contains("第一义项")));
    assert!(payloads.iter().any(|p| p.contains("separate homograph")));
}
#[test]
fn byte_tampering_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let (bundle, _) = mobi2star::convert(&source, &dir.path().join("out"), &Limits::default(), 32).unwrap();
    let path = bundle.join("dictionary.dict");
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(mobi2star::verify(&bundle, None, &Limits::default()).is_err());
}
#[test]
fn existing_output_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("sentinel"), b"keep me").unwrap();
    assert!(mobi2star::convert(&source, &out, &Limits::default(), 32).is_err());
    assert_eq!(fs::read(out.join("sentinel")).unwrap(), b"keep me");
}
#[test]
fn output_failure_rolls_back_only_its_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let out = dir.path().join("out");
    let limits = Limits { output_bytes: 1, ..Limits::default() };
    assert!(mobi2star::convert(&source, &out, &limits, 32).is_err());
    assert!(!out.exists());
    assert_eq!(fs::read(source).unwrap(), PLAIN);
}
#[test]
fn encryption_is_a_hard_error_not_empty_output() {
    let mut bytes = PLAIN.to_vec();
    let offset = u32::from_be_bytes(bytes[78..82].try_into().unwrap()) as usize;
    bytes[offset + 12..offset + 14].copy_from_slice(&1u16.to_be_bytes());
    assert!(matches!(mobi_reader::read(bytes, &Limits::default()), Err(Error::Unsupported(_))));
}
#[test]
fn broken_resource_reference_is_a_hard_error() {
    let mut doc = mobi_reader::read(PLAIN.to_vec(), &Limits::default()).unwrap();
    doc.resources.clear();
    assert!(matches!(html_preserve::build(&doc, &Limits::default()), Err(Error::Incomplete(_))));
}
#[test]
fn wrong_original_source_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let (bundle, _) = mobi2star::convert(&source, &dir.path().join("out"), &Limits::default(), 32).unwrap();
    fs::write(&source, PALM).unwrap();
    assert!(mobi2star::verify(&bundle, Some(&source), &Limits::default()).is_err());
}
#[test]
fn explicit_64_bit_index_can_be_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let (bundle, _) = mobi2star::convert(&source, &dir.path().join("out"), &Limits::default(), 64).unwrap();
    assert_eq!(stardict_io::open(&bundle, &Limits::default()).unwrap().offset_bits, 64);
}
#[cfg(unix)]
#[test]
fn bundle_symlink_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let source = input(dir.path(), PLAIN);
    let (bundle, _) = mobi2star::convert(&source, &dir.path().join("out"), &Limits::default(), 32).unwrap();
    let path = bundle.join("res/mobi-000001.png");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&source, &path).unwrap();
    assert!(mobi2star::verify(&bundle, None, &Limits::default()).is_err());
}
#[test]
fn every_truncated_header_fails_without_panicking() {
    for n in 0..400 {
        assert!(mobi_reader::read(PLAIN[..n].to_vec(), &Limits::default()).is_err());
    }
}
#[test]
fn deterministic_mutation_smoke_test_does_not_panic() {
    let limits = Limits { input_bytes: 1 << 20, text_bytes: 1 << 20, entries: 4096, aliases: 4096, operations: 100000, ..Limits::default() };
    for n in 0..128usize {
        let mut bytes = PLAIN.to_vec();
        let pos = (n * 7919 + 17) % bytes.len();
        bytes[pos] ^= (n as u8).wrapping_add(1);
        let _ = mobi_reader::read(bytes, &limits);
    }
}
