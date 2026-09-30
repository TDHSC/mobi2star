<p align="center">
  <img src="web/icon.svg" width="96" height="96" alt="">
</p>

<h1 align="center">mobi2star</h1>

<p align="center">
  <b>Convert MOBI dictionaries to StarDict for KOReader and other e-readers, in your browser or on the command line.</b>
</p>

<p align="center">
  <a href="https://github.com/TDHSC/mobi2star/actions/workflows/ci.yml"><img src="https://github.com/TDHSC/mobi2star/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/TDHSC/mobi2star/releases"><img src="https://img.shields.io/github/v/release/TDHSC/mobi2star?include_prereleases" alt="Latest release"></a>
</p>

<p align="center">
  <a href="https://tdhsc.github.io/mobi2star/"><b>Open the web app</b></a> ·
  <a href="#install">Download the CLI</a> ·
  <a href="#put-it-on-your-e-reader">Put it on your e-reader</a> ·
  <a href="docs/READERS.md">Supported readers</a>
</p>

Every headword, homograph, inflection, link and image comes through. Anything mobi2star cannot convert stops the conversion with an error instead of silently disappearing. The web app runs entirely in your browser tab, so your dictionary is never uploaded.

**Alpha:** output layout, report fields and library APIs may still change. Not supported yet: DRM-protected and KF8/hybrid files ([all limits](#what-it-cannot-convert-yet)).

## Quick start

**In your browser, nothing to install:** open **<https://tdhsc.github.io/mobi2star/>**, choose a `.mobi` dictionary and your reader, look a few words up in **Preview**, then download the zip. The page runs the same Rust converter as the CLI, compiled to WebAssembly, and produces the same StarDict files. It accepts files up to 256 MiB; phones and tablets may run out of memory on large dictionaries. [How it works](docs/WEB.md)

**On the command line:** [download the binary](#install) for your platform, then:

```sh
mobi2star convert dictionary.mobi --output ./converted --profile stardict
# the dictionary to copy is ./converted/bundle/StarDict/
```

## Put it on your e-reader

Copy the dictionary folder into KOReader's dictionary folder: from the web app, the folder inside the zip; from the CLI, `StarDict/`, renamed to anything you like. Keep `dictionary.css` and `res/` inside it.

| Device | KOReader's dictionary folder |
|---|---|
| Kindle | `koreader/data/dict/` |
| Kobo | `.adds/koreader/data/dict/` (`.adds` is a hidden folder) |
| PocketBook | `applications/koreader/data/dict/` |
| Android | `/sdcard/koreader/data/dict/` |
| Linux | `~/.config/koreader/data/dict/` |
| macOS | `~/Library/Application Support/koreader/data/dict/` |

Then restart KOReader and long-press a word in a book. The dictionary is listed under **Dictionary settings → Manage dictionaries**. The folders come from [KOReader's wiki](https://github.com/koreader/koreader/wiki/Dictionary-support), which also covers other devices.

For another reader, pick it when converting ([Choosing a reader](#choosing-a-reader)) and copy the folder to wherever that reader keeps StarDict dictionaries.

## Why mobi2star

The usual dictionary converters, PyGlossary and penelope, can write MOBI but cannot read it. mobi2star goes the other way and keeps what matters in a dictionary:

- **Nothing silently dropped.** Homographs keep separate entries, inflected forms find their headword, and entries that share a definition still share it.
- **Links and pictures work.** Cross-references open the right entry, including KOReader's handling of `#anchor` links, and images come along.
- **See it before you copy it.** **Preview** in the web app shows the dictionary as your reader would. For KOReader it is drawn by MuPDF, the engine KOReader uses, with KOReader's fonts.
- **Made for your reader.** Readers load a dictionary's stylesheet in different ways, and the output follows the one you pick.
- **Private.** The web app never uploads your file. The CLI is one binary with no network access and no helper processes.
- **Checked, not assumed.** An independent StarDict reader re-reads everything written, and the CLI regenerates the whole bundle and compares it file by file. [What is checked](docs/VERIFICATION.md)
- **Readable Collins COBUILD.** A built-in layout profile makes Collins COBUILD dictionaries readable on narrow screens. [Details](docs/READABILITY.md)

## Choosing a reader

The web app asks for your reader; the CLI takes `--reader`, and the default is `koreader`.

| Reader | `--reader` | How it was established |
|---|---|---|
| KOReader | `koreader` | rendered with its engine, and read in its source |
| GoldenDict, GoldenDict-ng (desktop) | `goldendict` | read in its source |
| GoldenDict Mobile (Android) | `goldendict-mobile` | user reports |
| Readest | `readest` | read in its source |
| Kobo, through PyGlossary or penelope | `kobo` | read in their source |
| Several readers from one folder, or one not listed (such as Boox) | `universal` | combines the cases above |

Nothing has been tested on a physical device yet, so reports are welcome. `goldendict-mobile`, `readest` and `kobo` give every entry its own copy of the stylesheet, which makes the dictionary roughly twice as large. [READERS.md](docs/READERS.md) has the evidence for each reader.

## Install

The web app needs no installation. For the CLI, every [release](https://github.com/TDHSC/mobi2star/releases) has an archive for each supported platform:

| Platform | Archive suffix |
|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` |
| macOS, Intel | `x86_64-apple-darwin` |
| Linux, x86_64 (static, any distribution) | `x86_64-unknown-linux-musl` |
| Linux, ARM64 (static, any distribution) | `aarch64-unknown-linux-musl` |

Download the archive for your platform, extract it, and put `mobi2star` on your `PATH`. The macOS binaries are not signed or notarized, so macOS blocks them the first time. After extracting, clear the quarantine flag:

```sh
xattr -d com.apple.quarantine mobi2star
```

<details>
<summary>Verify the download</summary>

Each release includes a `SHA256SUMS` file and a build-provenance attestation for every archive:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify mobi2star-*.tar.gz --repo TDHSC/mobi2star
```

</details>

Each release also has `mobi2star-vX.Y.Z-web.tar.gz`: that release's browser page, the `index.html` and `v/X.Y.Z/` files published at <https://tdhsc.github.io/mobi2star/>, which any static host can serve. Pages keeps only the two newest releases; the archive keeps every version.

To build from source you need Rust 1.85 or later. From a checkout:

```sh
cargo install --path crates/mobi2star --locked
```

## Command-line usage

```sh
mobi2star inspect dictionary.mobi                          # which backend would be used
mobi2star convert dictionary.mobi --output ./converted     # convert, check, publish ./converted/bundle
mobi2star verify ./converted/bundle --source dictionary.mobi
mobi2star lookup ./converted/bundle run                    # every entry for "run"
```

By default `convert` writes a full bundle: the dictionary, an offline browser viewer, a byte-exact copy of the source, an audit trail and a SHA-256 manifest, so `verify` can re-check it later. `--profile stardict` writes only the dictionary, as the web app does. Use the CLI rather than the web app for the full bundle, for `verify`, or for files over the web app's 256 MiB limit. Every command accepts `--json`.

[docs/CLI.md](docs/CLI.md) has every option and the output layout.

## What it cannot convert yet

These fail with an error rather than producing an incomplete dictionary:

- encrypted (DRM) input
- KF8/hybrid files
- embedded fonts, audio or video
- LIGT records
- unknown content records
- with the publisher-source (`srcs`) backend, anything other than a single classic ZIP with a single OPF, UTF-8 compiled text and the supported XHTML/CSS subset; ZIP64 and multi-volume archives, for example

mobi2star does not remove DRM. See [ARCHITECTURE.md](docs/ARCHITECTURE.md) and [SECURITY.md](docs/SECURITY.md).

## Documentation

- [CLI.md](docs/CLI.md): every command and option, and the output layout
- [READERS.md](docs/READERS.md): how each reader loads a dictionary, and how sure we are
- [WEB.md](docs/WEB.md): how the web app and its preview work, and what they check
- [VERIFICATION.md](docs/VERIFICATION.md): what is checked before output is published
- [READABILITY.md](docs/READABILITY.md): the Collins COBUILD layout profile
- [ARCHITECTURE.md](docs/ARCHITECTURE.md), [TESTING.md](docs/TESTING.md), [SECURITY.md](docs/SECURITY.md) and [SOURCES.md](docs/SOURCES.md): the internals

Bug reports and patches are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Dictionary content and trademarks

This repository contains only project code and original synthetic test fixtures. It does not include or distribute any dictionary content. Convert only dictionaries you are entitled to use. A full bundle contains a copy of its input file, so treat it as private and do not redistribute it.

Collins COBUILD is a trademark of HarperCollins Publishers. This project is not affiliated with or endorsed by HarperCollins, Amazon or the StarDict and KOReader projects.

## License

The CLI and its libraries are [MIT](LICENSE). `crates/reader-view`, which ports reader behaviour from GoldenDict-ng, KOReader and Readest for the browser preview, is AGPL-3.0-or-later ([its license](crates/reader-view/LICENSE)); the converter never depends on it. The published browser page includes it and bundles MuPDF, so the page is AGPL-3.0-or-later. Dependencies and bundled fonts keep their own licenses; see [docs/WEB.md](docs/WEB.md#licensing).
