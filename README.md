# mobi2star

A native Rust converter from MOBI dictionaries to [StarDict](https://stardict-4.sourceforge.net/StarDictFileFormat), built for readers such as KOReader. It keeps every headword, homograph, inflection, link and image, and it treats unsupported content as an error rather than silently skipping it.

> **Status: alpha.** Output layout, report fields and library APIs may change between releases. Content checks are automated; how a dictionary looks in a particular reader still needs to be checked on that reader.

## Features

- **Two backends.** `srcs` reads the publisher source that KindleGen embeds in many MOBI files (the SRCS record: a ZIP of XHTML, OPF, CSS and images). `compiled` reads the compiled MOBI text of ordinary dictionaries. The default, `auto`, uses `srcs` whenever an embedded source is present.
- **Nothing is dropped.** Homographs keep separate index records, entries that share one definition share one `.dict` range, and explicit inflections become `.syn` aliases.
- **Working links and images.** Internal links resolve to lookup routes, including exact `route#anchor` aliases for KOReader's link handling. Images are written to `res/`.
- **Offline browser viewer.** The `srcs` backend also writes a static HTML lookup page with the full chapters.
- **Verification built in.** After writing, an independent StarDict reader re-parses the output, the whole bundle is regenerated from the archived source and compared file by file, and a SHA-256 manifest covers every file.
- **Readability profile for Collins COBUILD.** A built-in layout adapter that makes Collins COBUILD dictionaries readable on narrow screens. See [docs/READABILITY.md](docs/READABILITY.md).
- **Local and bounded.** One binary with no helper processes and no network access. Input, decompression and output sizes have explicit budgets, and application code forbids `unsafe`.

## Install

### Prebuilt binaries

Every [release](https://github.com/TDHSC/mobi2star/releases) has an archive for each supported platform:

| Platform | Archive suffix |
|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, x86_64 (static, any distribution) | `x86_64-unknown-linux-musl` |
| Linux, ARM64 (static, any distribution) | `aarch64-unknown-linux-musl` |

Download the archive for your platform, extract it, and put `mobi2star` on your `PATH`.

Each release also includes a `SHA256SUMS` file and a build-provenance attestation for every archive. To check an archive:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify mobi2star-*.tar.gz --repo TDHSC/mobi2star
```

The macOS binaries are not signed or notarized, so macOS blocks them the first time. After extracting, clear the quarantine flag:

```sh
xattr -d com.apple.quarantine mobi2star
```

### From source

You need a Rust toolchain. The minimum supported version is 1.85; current stable is recommended.

```sh
./tools/qa.sh                                    # check formatting, test, lint, release build
cargo install --path crates/mobi2star --locked   # install the CLI
```

## Usage

```sh
mobi2star inspect dictionary.mobi
mobi2star convert dictionary.mobi --output ./converted
mobi2star verify ./converted/bundle --source dictionary.mobi
mobi2star lookup ./converted/bundle run
```

| Command | What it does |
|---|---|
| `inspect` | Reads the container header and reports which backend would be used. Passing `inspect` does not guarantee that conversion will succeed. |
| `convert` | Converts, audits, re-reads and verifies, then publishes `OUTPUT/bundle`. |
| `verify` | Re-checks an existing bundle. `--source` also requires the bundle to match that original file exactly. |
| `lookup` | Prints every exact-spelling match, including homographs and aliases. |

Every command accepts `--json` for machine-readable output (errors go to stderr). Size budgets can be raised with `--max-input-mib`, `--max-text-mib`, `--max-entry-mib` and `--max-output-mib`.

`convert` options:

- `--backend auto|srcs|compiled` selects the backend; the default is `auto`.
- `--offset-bits 32|64` sets the StarDict offset width. The default of 32 is the most portable; use 64 only if your reader supports it.
- `--labels en|zh` sets the language of the text that mobi2star generates itself. The default is `en`.
  - This covers lookup keys for chapters, image galleries and text outside any headword (such as `[Chapter 000001] Preface` or `[Supplement 000001]`), plus the offline viewer's interface.
  - Dictionary content is never translated.
  - The choice is recorded in `manifest.json`, so `verify` needs no extra option.
- `--reader` sets the reader the dictionary is built for, which decides how entries reference the stylesheet. See [Choosing a reader](#choosing-a-reader).

The output directory must not exist yet. `convert` creates it with owner-only permissions (`0700`) and publishes `OUTPUT/bundle` only after every check passes. If any check fails, the staging tree is removed and existing files are left untouched.

Verification rebuilds the full bundle in the system temporary directory (set `TMPDIR` to move it), so reserve free space for about twice the bundle size. The default bundle budget is 8 GiB.

### Choosing a reader

Readers load a dictionary's stylesheet in different ways, so `convert --reader` tailors the output. `dictionary.css` next to the `.ifo` is always written.

| `--reader` | Use it for | What each entry carries |
|---|---|---|
| `koreader` (default) | KOReader; also works for GoldenDict desktop | a hidden `<link>` to `res/dictionary.css` |
| `goldendict` | GoldenDict and GoldenDict-ng on desktop | same as `koreader` |
| `goldendict-mobile` | GoldenDict Mobile on Android | its own `<style>` copy |
| `readest` | Readest | its own `<style>` copy |
| `kobo` | converting to a Kobo dictionary with PyGlossary or penelope | its own `<style>` copy |
| `universal` | one folder shared by several readers, or a reader not listed here (e.g. Boox) | both |

Inline copies make the dictionary roughly twice as large; for Collins COBUILD the `.dict` is 146 MB with `koreader` and 305 MB with `readest`. [docs/READERS.md](docs/READERS.md) records how each reader was assessed and how confident that assessment is.

### Importing into a reader

Copy the whole `StarDict/` directory into your reader's dictionary folder, including `dictionary.css` and `res/`. `Browser/index.html` can be opened directly in a web browser.

## Output layout

The `srcs` backend produces:

```text
bundle/
├── StarDict/            # import this directory into your reader
│   ├── dictionary.ifo
│   ├── dictionary.idx
│   ├── dictionary.dict
│   ├── dictionary.syn
│   ├── dictionary.css   # loaded by KOReader
│   └── res/
│       ├── dictionary.css  # linked from entries (not written for inline-only readers)
│       ├── source/      # images from the publisher source
│       └── compiled/    # images from the compiled MOBI
├── Browser/             # static offline viewer
│   ├── index.html
│   ├── images.html
│   ├── lookup-data.js
│   ├── viewer.js
│   ├── viewer.css
│   ├── content/         # full chapters with original styles and images
│   └── compiled/
├── Source/              # byte-exact copy of every file in the embedded ZIP
├── Audit/               # original MOBI, raw text, parsed facts, render plan, cross-checks
├── manifest.json        # SHA-256 of every file
└── report.json
```

The `compiled` backend writes a flat bundle: the `dictionary.*` files (including `dictionary.css`) and `res/` at the bundle root, plus `archive/`, `manifest.json` and `report.json`. `lookup` and `verify` recognize both layouts.

## What is checked

The `srcs` backend works only from facts stated in the file. It reads every headword, definition block, explicit inflection, page, anchor and resource in the source XHTML, then cross-checks them against the compiled MOBI index, its shared-definition references and its inflection rules:

- Headwords and inflections are compared as multisets, including ownership and repeat counts.
- The visible text of each definition block is compared with the compiled text after whitespace normalization.
- Content is converted by copying original UTF-8 byte ranges plus the attribute rewrites that are needed. Every edit is recorded with its range and reason, so the original can be reconstructed.

After writing, a separate StarDict reader checks index ordering, alias targets, shared ranges, full payload coverage and readback. The bundle is then regenerated from the archived MOBI and compared by file set, size and SHA-256.

Regeneration reuses the same parser and renderer, so it cannot catch a bug that affects both runs identically. The cross-format text comparison, byte-exact archives, synthetic tests and reader acceptance cover that gap from different angles.

The report keeps content checks separate from rendering:

```json
{
  "backend": "srcs-rust",
  "implemented_content_checks_passed": true,
  "rendering_status": "unverified_reader_dependent",
  "skipped_entries": 0
}
```

## Limitations

The `srcs` backend currently accepts a single classic ZIP with a single OPF, UTF-8 compiled text and the supported XHTML/CSS subset. The following fail with an error until a dedicated adapter exists:

- encrypted (DRM) input
- KF8/hybrid files
- ZIP64 or multi-volume ZIP archives
- embedded fonts, audio or video
- LIGT records
- unknown content records

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/SECURITY.md](docs/SECURITY.md).

## Project layout

| Crate | Responsibility |
|---|---|
| `lexicon-core` | Byte ranges, entry model, budgets, errors, hashing and path validation |
| `mobi-reader` | PDB/MOBI container, PalmDOC/HUFF decompression, INDX/ORDT, shared references and inflection rules |
| `html-preserve` | Raw HTML byte positions and local edits for the `compiled` backend |
| `srcs-reader` | ZIP/XHTML/OPF reading, source fact model, image decoding and compiled-index cross-checks |
| `srcs-render` | Replayable byte edits, CSS scoping, the Collins readability profile, StarDict payloads and the offline viewer |
| `stardict-io` | Streaming StarDict writer shared by both backends, plus an independent reader |
| `mobi2star` | CLI, backend selection, staged publication, source binding and end-to-end verification |

Further reading: [docs/READERS.md](docs/READERS.md), [docs/TESTING.md](docs/TESTING.md) and [docs/SOURCES.md](docs/SOURCES.md).

## Dictionary content and trademarks

This repository contains only project code and original synthetic test fixtures. It does not include or distribute any dictionary content. Convert only dictionaries you are entitled to use.

A bundle contains a full copy of its input file (`Audit/original.mobi`), so treat it as private and do not redistribute it.

Collins COBUILD is a trademark of HarperCollins Publishers. This project is not affiliated with or endorsed by HarperCollins, Amazon or the StarDict and KOReader projects.

## License

[MIT](LICENSE). Dependencies keep their own licenses.
