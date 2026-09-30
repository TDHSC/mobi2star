//! Characterization test: every file of a converted bundle, pinned by digest.
//! It proves that refactors of the output layer leave the bytes unchanged.
//! When output changes on purpose, update the table; a failure prints the
//! new values ready to paste.
mod common;
use common::{bundle_digest, fixture, two_set_fixture, PAGE};
use lexicon_core::{LabelLanguage, Limits, TargetReader};
use mobi2star::{Backend, OutputOptions};
use std::fs;

const UNCOMPRESSED: &[u8] = include_bytes!("../../../tests/fixtures/uncompressed.mobi");
const PALMDOC: &[u8] = include_bytes!("../../../tests/fixtures/palmdoc.mobi");
const HUFF: &[u8] = include_bytes!("../../../tests/fixtures/huff.mobi");
const OLD_INFLECTIONS: &[u8] = include_bytes!("../../../tests/fixtures/old-inflections.mobi");

fn digest(source: &[u8], options: OutputOptions) -> String {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.mobi");
    fs::write(&input, source).unwrap();
    let (bundle, _) = mobi2star::convert_with_backend(
        &input,
        &dir.path().join("out"),
        &Limits::default(),
        options,
        Backend::Auto,
    )
    .unwrap();
    bundle_digest(&bundle)
}

const EXPECTED: &[(&str, &str)] = &[
    (
        "compiled/uncompressed",
        "c2499739374960dab9ad2a514a15f0bb4a990f6e826eeb4b92171ffd3d0fe1f7",
    ),
    (
        "compiled/palmdoc",
        "9cc174d276bb9742273242a7db55c870d0495a531f4ed4cc5ee279d5ec86ced8",
    ),
    (
        "compiled/huff",
        "44e88796e7dbee5dd326bda1be466210bd6302c5c71853b7082105572c1e8ad9",
    ),
    (
        "compiled/old-inflections",
        "0e768457e1bdacb2a24a51c14dfb86c91d67cf3019e7c153472eeaa58983dda9",
    ),
    (
        "compiled/huff/readest",
        "0619b63aedef837700afa1acbf357dda470838bb67fefcc51edb7acd925cbf03",
    ),
    (
        "compiled/huff/universal-zh-64",
        "f387acbbf21e5d8f2179acc02c9b2496e6fe96239be22a29974782d648717b30",
    ),
    (
        "srcs/default",
        "78830d570a172a20f5868883a1b5bbc5a69d7050e54f455eaf1511f00f67e7c6",
    ),
    (
        "srcs/readest",
        "3fd7da9fefa04cef9553dad747879a84ba6385636bda6d279e7f2e0f9aba1619",
    ),
    (
        "srcs/universal-zh-64",
        "601a238cc4cd9478929a5c030c87945f066a31ba1a7b7e65803d48e09a3ac785",
    ),
    (
        "srcs/two-sets",
        "26e42f21af40f15d7535c9383225dc17ec7fa1bacd7e724e9bd6c58418d87b45",
    ),
];

#[test]
fn full_bundles_are_byte_for_byte_unchanged() {
    let default = OutputOptions::default();
    let readest = OutputOptions {
        reader: TargetReader::Readest,
        ..default
    };
    let everything = OutputOptions {
        offset_bits: 64,
        labels: LabelLanguage::Zh,
        reader: TargetReader::Universal,
    };
    let srcs = fixture(PAGE);
    let two_sets = two_set_fixture();
    let cases: Vec<(&str, &[u8], OutputOptions)> = vec![
        ("compiled/uncompressed", UNCOMPRESSED, default),
        ("compiled/palmdoc", PALMDOC, default),
        ("compiled/huff", HUFF, default),
        ("compiled/old-inflections", OLD_INFLECTIONS, default),
        ("compiled/huff/readest", HUFF, readest),
        ("compiled/huff/universal-zh-64", HUFF, everything),
        ("srcs/default", &srcs, default),
        ("srcs/readest", &srcs, readest),
        ("srcs/universal-zh-64", &srcs, everything),
        ("srcs/two-sets", &two_sets, default),
    ];
    let actual: Vec<(&str, String)> = cases
        .into_iter()
        .map(|(name, source, options)| (name, digest(source, options)))
        .collect();
    let table: String = actual
        .iter()
        .map(|(name, digest)| format!("    (\"{name}\", \"{digest}\"),\n"))
        .collect();
    let expected: Vec<(&str, String)> = EXPECTED.iter().map(|(n, d)| (*n, d.to_string())).collect();
    assert_eq!(
        actual, expected,
        "bundle bytes changed; new table:\n{table}"
    );
}
