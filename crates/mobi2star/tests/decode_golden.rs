//! Golden decode results for the synthetic MOBI fixtures. Each expected file
//! records the decompressed text, every entry's byte range and aliases, the
//! image hash and the internal link targets. The files were produced with the
//! Chinese label set, so supplement keys are decoded with `LabelLanguage::Zh`.
use lexicon_core::{sha256, LabelLanguage, Limits};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    mode: String,
    source_sha256: String,
    rawml_sha256: String,
    rawml: String,
    source_headwords: usize,
    source_aliases: usize,
    rows: Vec<Row>,
    image_sha256: String,
    link_targets: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: u64,
    headword: String,
    start: usize,
    end: usize,
    html: String,
    aliases: Vec<String>,
}

/// (mode, fixture, expected decode, MOBI compression type)
const FIXTURES: [(&str, &[u8], &str, u16); 4] = [
    (
        "uncompressed",
        include_bytes!("../../../tests/fixtures/uncompressed.mobi"),
        include_str!("../../../tests/fixtures/uncompressed.expected.json"),
        1,
    ),
    (
        "palmdoc",
        include_bytes!("../../../tests/fixtures/palmdoc.mobi"),
        include_str!("../../../tests/fixtures/palmdoc.expected.json"),
        2,
    ),
    (
        "huff",
        include_bytes!("../../../tests/fixtures/huff.mobi"),
        include_str!("../../../tests/fixtures/huff.expected.json"),
        17480,
    ),
    (
        "old-inflections",
        include_bytes!("../../../tests/fixtures/old-inflections.mobi"),
        include_str!("../../../tests/fixtures/old-inflections.expected.json"),
        1,
    ),
];

#[test]
fn fixtures_decode_to_their_golden_records() {
    let limits = Limits::default();
    for (mode, bytes, expected, compression) in FIXTURES {
        let expected: Expected = serde_json::from_str(expected).unwrap();
        assert_eq!(expected.mode, mode);
        let header = mobi_reader::inspect(bytes).unwrap().header;
        assert_eq!(header.compression, compression, "{mode}");

        let doc = mobi_reader::read(bytes.to_vec(), &limits, LabelLanguage::Zh).unwrap();
        assert_eq!(sha256(bytes), expected.source_sha256, "{mode}");
        assert_eq!(doc.namespace, expected.source_sha256, "{mode}");
        assert_eq!(doc.rawml, expected.rawml.as_bytes(), "{mode}");
        assert_eq!(sha256(&doc.rawml), expected.rawml_sha256, "{mode}");
        assert_eq!(doc.source_headwords, expected.source_headwords, "{mode}");
        assert_eq!(doc.source_aliases, expected.source_aliases, "{mode}");

        assert_eq!(doc.entries.len(), expected.rows.len(), "{mode}");
        for (entry, row) in doc.entries.iter().zip(&expected.rows) {
            assert_eq!(entry.id, row.id, "{mode}");
            assert_eq!(entry.headword, row.headword, "{mode}");
            assert_eq!(
                (entry.span.start, entry.span.end),
                (row.start, row.end),
                "{mode} {}",
                row.headword
            );
            assert_eq!(
                entry.span.bytes(&doc.rawml).unwrap(),
                row.html.as_bytes(),
                "{mode} {}",
                row.headword
            );
            let aliases: Vec<&str> = entry.aliases.iter().map(|a| a.word.as_str()).collect();
            assert_eq!(aliases, row.aliases, "{mode} {}", row.headword);
        }

        assert_eq!(doc.resources.len(), 1, "{mode}");
        let image = doc.resources[0].source_span.bytes(&doc.source).unwrap();
        assert_eq!(sha256(image), expected.image_sha256, "{mode}");

        let plan = html_preserve::build(&doc, &limits).unwrap();
        let targets: Vec<usize> = plan.links.iter().map(|link| link.target_position).collect();
        assert_eq!(targets, expected.link_targets, "{mode}");
    }
}
