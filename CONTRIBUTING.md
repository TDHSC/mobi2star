# Contributing

## Reporting a problem

Open an issue with:

- the mobi2star version (`mobi2star --version`, or the version in the browser page's footer);
- the dictionary's title and where it came from;
- the output of `mobi2star inspect dictionary.mobi --json`, or the browser page's error message;
- the reader and device you use, if the problem is how the dictionary looks.

Do not attach dictionary files: they are usually copyrighted. A minimal synthetic input that reproduces the problem is the most useful thing you can send. Report suspected vulnerabilities privately, as [SECURITY.md](docs/SECURITY.md#reporting-a-vulnerability) describes.

## Building and testing

You need a Rust toolchain. The minimum supported version is 1.85; current stable is recommended.

```sh
./tools/qa.sh                                    # check formatting, test, lint, release build
cargo install --path crates/mobi2star --locked   # install the CLI from this checkout
```

[TESTING.md](docs/TESTING.md) describes the suite and what it does not cover, and [WEB.md](docs/WEB.md#building-and-testing-locally) how to build and test the browser page.

## Project layout

| Crate | Responsibility |
|---|---|
| `lexicon-core` | Byte ranges, entry model, budgets, errors, hashing and path validation |
| `mobi-reader` | PDB/MOBI container, PalmDOC/HUFF decompression, INDX/ORDT, shared references and inflection rules |
| `html-preserve` | Raw HTML byte positions and local edits for the `compiled` backend |
| `srcs-reader` | ZIP/XHTML/OPF reading, source fact model, image decoding and compiled-index cross-checks |
| `srcs-render` | Replayable byte edits, CSS scoping, the Collins readability profile, StarDict payloads and the offline viewer |
| `stardict-io` | StarDict encoding, parsing and payload checks shared by both backends, with thin disk wrappers |
| `mobi2star` | CLI, backend selection, output to a directory or a zip, staged publication, source binding and end-to-end verification |
| `mobi2star-web` | The browser page's converter and preview, and their WebAssembly bindings |
| `reader-view` | How readers display a dictionary, for the browser preview (AGPL-3.0-or-later) |

[ARCHITECTURE.md](docs/ARCHITECTURE.md) explains how they fit together, and [RELEASING.md](docs/RELEASING.md) how a release is made.
