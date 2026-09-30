# mobi2star

A native Rust converter from MOBI dictionaries to [StarDict](https://stardict-4.sourceforge.net/StarDictFileFormat), built for readers such as KOReader. It keeps every headword, homograph, inflection, link and image, and it treats unsupported content as an error rather than silently skipping it.

> **Status: alpha.** Output layout, report fields and library APIs may change between releases. Content checks are automated; how a dictionary looks in a particular reader still needs to be checked on that reader.

## Use it in your browser

Open **<https://tdhsc.github.io/mobi2star/>**, choose a MOBI dictionary and your reader, and download a zip that holds the StarDict folder. The conversion runs inside your browser tab, so the file is never uploaded.

- The page runs the same Rust converter as the CLI, compiled to WebAssembly, and produces the same StarDict files.
- **Preview** shows the result as your reader would before you copy it over: look words up and follow links. KOReader's preview is drawn by MuPDF, the engine KOReader uses, with KOReader's fonts.
- It accepts files up to 256 MiB. Phones and tablets may run out of memory on large dictionaries.
- It is republished with every release. See [docs/WEB.md](docs/WEB.md) for how it works and what it checks.

Use the command-line tool for the full audit bundle, later verification with `verify`, or larger limits.

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

Each release also has `mobi2star-vX.Y.Z-web.tar.gz`: that release's browser page, the `index.html` and `v/X.Y.Z/` files published at <https://tdhsc.github.io/mobi2star/>, which any static host can serve. Pages keeps only the two newest releases; the archive keeps every version.

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

You need a Rust toolchain. The minimum supported version is 1.85; current stable is recommended. From a checkout:

```sh
cargo install --path crates/mobi2star --locked
```

[CONTRIBUTING.md](CONTRIBUTING.md) covers building and testing.

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

[docs/CLI.md](docs/CLI.md) lists every `convert` option (backend, offset width, label language, reader and profile) and the output layout.

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

`convert` publishes `OUTPUT/bundle`. With the `srcs` backend, import `bundle/StarDict/`; [docs/CLI.md](docs/CLI.md#output-layout) describes both backends' layouts.

## What is checked

Publisher-source conversions are cross-checked against the compiled MOBI index. Every conversion is re-read by an independent StarDict reader, and a full bundle is regenerated and compared file by file before it is published. [docs/VERIFICATION.md](docs/VERIFICATION.md) describes each check and its limits.

## Limitations

The `srcs` backend currently accepts a single classic ZIP with a single OPF, UTF-8 compiled text and the supported XHTML/CSS subset. The following fail with an error until a dedicated adapter exists:

- encrypted (DRM) input
- KF8/hybrid files
- ZIP64 or multi-volume ZIP archives
- embedded fonts, audio or video
- LIGT records
- unknown content records

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/SECURITY.md](docs/SECURITY.md).

Further reading: [docs/CLI.md](docs/CLI.md), [docs/READERS.md](docs/READERS.md), [docs/WEB.md](docs/WEB.md), [docs/VERIFICATION.md](docs/VERIFICATION.md), [docs/TESTING.md](docs/TESTING.md) and [docs/SOURCES.md](docs/SOURCES.md). The crates are listed in [CONTRIBUTING.md](CONTRIBUTING.md#project-layout).

## Dictionary content and trademarks

This repository contains only project code and original synthetic test fixtures. It does not include or distribute any dictionary content. Convert only dictionaries you are entitled to use.

A bundle contains a full copy of its input file (`Audit/original.mobi`), so treat it as private and do not redistribute it.

Collins COBUILD is a trademark of HarperCollins Publishers. This project is not affiliated with or endorsed by HarperCollins, Amazon or the StarDict and KOReader projects.

## License

The CLI and its libraries are [MIT](LICENSE). `crates/reader-view`, which ports reader behaviour from GoldenDict-ng, KOReader and Readest for the browser preview, is AGPL-3.0-or-later ([its license](crates/reader-view/LICENSE)); the converter never depends on it. The published browser page includes it and bundles MuPDF, so the page is AGPL-3.0-or-later. Dependencies and bundled fonts keep their own licenses; see [docs/WEB.md](docs/WEB.md#licensing).
