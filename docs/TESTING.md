# Testing

## Running the suite

```sh
./tools/qa.sh
```

The script formats the workspace and runs the whole workspace test suite, then Clippy, then a release build of the CLI. To run a single crate's tests:

```sh
cargo test -p srcs-render --locked
```

CI (`.github/workflows/ci.yml`) runs the same script on Ubuntu and macOS.

## What the tests cover

All fixtures are original and synthetic. The repository contains no third-party dictionary content.

- **Unit tests** in each crate cover:
  - container parsing, PalmDOC/HUFF decompression and index/tag parsing;
  - shared-reference cycles, ORDT decoding and inflection rules;
  - ZIP and path safety, XML and CSS limits;
  - StarDict encoding and readback;
  - the Collins readability profile ([READABILITY.md](READABILITY.md)).
- **Compiled-backend end-to-end tests** (`crates/mobi2star/tests/end_to_end.rs`) convert the MOBI fixtures in `tests/fixtures/`. There is one fixture per case: uncompressed, PalmDOC, HUFF, and old-style inflections. Each converted bundle is verified, then looked up for homographs, aliases and non-ASCII headwords. The same file also tests:
  - byte tampering, a wrong original and symlinks inside a bundle, all of which must fail verification;
  - that an existing output directory is left untouched and a failed conversion rolls back;
  - that encrypted input and broken resource references are hard errors;
  - 64-bit indexes;
  - truncated and mutated inputs, which must fail without panicking.
- **SRCS end-to-end tests** (`crates/mobi2star/tests/srcs_end_to_end.rs`) build MOBI files with an embedded source ZIP inside the test itself. The source ZIP holds XHTML, OPF, CSS and a PNG. These tests cover:
  - conversion and auto backend selection;
  - rollback when the source and compiled text disagree;
  - mismatched inflections and missing anchors;
  - rewritten payloads whose manifest hashes were recomputed to match;
  - extra files in a bundle and a wrong original.
- **Golden fixtures** in `tests/fixtures/readability/` pin the readability profile's byte-level output.

## What the tests do not cover

A passing suite shows that the implemented invariants hold on synthetic input. It does not certify any real dictionary.

Before relying on a converted dictionary:

1. Run `mobi2star convert` and `mobi2star verify --source ORIGINAL` on it.
2. Open it in the reader you plan to use. Rendering, fonts, pagination and link handling depend on the reader, and the report always marks them `unverified_reader_dependent`.
