# Testing

## Running the suite

```sh
./tools/qa.sh
```

The script checks formatting (run `cargo fmt --all` to fix it) and runs the whole workspace test suite, then Clippy, then a release build of the CLI. To run a single crate's tests:

```sh
cargo test -p srcs-render --locked
```

CI (`.github/workflows/ci.yml`) runs the same script on Ubuntu and macOS, runs the test suite on the minimum supported Rust version (1.85, the `rust-version` in `Cargo.toml`), and builds and tests the browser page through `web.yml` (see [WEB.md](WEB.md#building-and-testing-locally) to run that locally).

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
- **Reader matrix** (both end-to-end files): every `--reader` value is converted and checked for the link and inline references it should carry, the exact stylesheet files, a passing `verify`, and an unchanged offline viewer and render plan. Editing the recorded reader, rewriting a stylesheet, or verifying a bundle from another version must fail.
- **Decode goldens** (`crates/mobi2star/tests/decode_golden.rs`) check that each MOBI fixture decodes exactly to its `tests/fixtures/*.expected.json`: compression type, decompressed text, every entry's byte range and aliases, the image hash and internal link targets.
- **SRCS end-to-end tests** (`crates/mobi2star/tests/srcs_end_to_end.rs`) build MOBI files with an embedded source ZIP inside the test itself. The source ZIP holds XHTML, OPF, CSS and a PNG. These tests cover:
  - conversion and auto backend selection;
  - rollback when the source and compiled text disagree;
  - mismatched inflections and missing anchors;
  - rewritten payloads whose manifest hashes were recomputed to match;
  - extra files in a bundle and a wrong original.
- **Golden fixtures** in `tests/fixtures/readability/` pin the readability profile's byte-level output.
- **Bundle digests** (`crates/mobi2star/tests/bundle_digests.rs`) pin a digest of every file in full bundles for the MOBI fixtures and the synthetic SRCS books, over several readers, label languages and offset widths. Refactors of the output layer must leave them unchanged; a failure prints the new values.
- **Profiles** (`crates/mobi2star/tests/profiles.rs`): for both backends, every reader, both label languages and both offset widths:
  - `--profile stardict` must write exactly the full bundle's dictionary files;
  - `convert_dictionary_zip` must unzip to those same files, with the same report and progress;
  - archives must be byte-for-byte deterministic;
  - dictionary-only output must be refused by `verify`.
- **Browser build** (`tools/web-smoke.mjs`, the `web` CI job; see [WEB.md](WEB.md)):
  - loads the built WebAssembly in Node;
  - requires every archive to equal the CLI's `--profile stardict` output for the committed fixtures, including `tests/fixtures/srcs.mobi`, which a Rust test keeps equal to the generated SRCS book;
  - checks that the page's English and Chinese text cover every choice.

## What the tests do not cover

A passing suite shows that the implemented invariants hold on synthetic input. It does not certify any real dictionary.

Before relying on a converted dictionary:

1. Run `mobi2star convert` and `mobi2star verify --source ORIGINAL` on it.
2. Open it in the reader you plan to use. Rendering, fonts, pagination and link handling depend on the reader, and the report always marks them `unverified_reader_dependent`.
