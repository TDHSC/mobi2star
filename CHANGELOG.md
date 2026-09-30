# Changelog

## Unreleased

- The compiled manifest schema is now 4, a number neither format has used. 0.4.0-alpha.1 used 2, which 0.3 read as an SRCS manifest.
- The compiled backend now scopes its stylesheet under a per-book wrapper class, as the SRCS backend already did. `body`/`html` rules style the entry, not the whole KOReader popup, and inline copies no longer leak into other dictionaries. Scoping is shared (`html_preserve::css`), with an open grammar for compiled books that accepts any selector and applies CSS's own error recovery.
- Fixed: text left over at the end of one compiled `<style>` body could join the first selector of the next body in `dictionary.css`.
- Fixed: a compiled `<style>` with an `id`, `class`, `lang`, `dir` or `nonce`, or an empty `type` or `media`, stopped the conversion. Those attributes don't change what the CSS does; `title`, `disabled` and non-screen media still do and remain errors.

## 0.4.0-alpha.1 — Reader-targeted stylesheets

- Alpha: reader behavior was established from source code and a MuPDF render comparison, not on devices; docs/READERS.md gives the confidence for each reader. Bundles from 0.3 must be verified with 0.3 or reconverted.
- New `convert --reader koreader|goldendict|goldendict-mobile|readest|kobo|universal`, default `koreader`. It decides how entries reference the stylesheet: a hidden link to `res/dictionary.css`, an inline `<style>` copy, or both. The default drops the inline copies, which KOReader never applied: the Collins COBUILD `.dict` shrinks from 305 MB to 146 MB, and KOReader renders it identically. See docs/READERS.md.
- `dictionary.css` is now written for every book and both backends. Books without the Collins profile, and all compiled-backend books, used to show no publisher styles in KOReader.
- SRCS pages that link different stylesheets are scoped per stylesheet set, so one shared stylesheet file keeps each page's cascade.
- The compiled backend rejects `<style>` elements that cannot join a shared stylesheet: unbalanced CSS, media other than all/screen, other attributes, or elements split by an entry boundary.
- Bundle manifests record the reader (and, for the compiled backend, the offset width); schemas are compiled 2 and SRCS 3. Verification of a bundle from another mobi2star version reports which version produced it.
- Library: conversion functions take `OutputOptions { offset_bits, labels, reader }`.

## 0.3.0-alpha.2 — Collins readability profile fix

- Fixed: the Collins COBUILD readability profile never applied to the retail Kindle edition, whose OPF title is "COBUILD Advanced Learner's Dictionary" without "Collins". Those conversions fell back to the source-scoped layout.

## 0.3.0-alpha.1 — English labels and prebuilt binaries

- Alpha: the automated tests use synthetic fixtures only. Run `mobi2star verify` on each converted dictionary and check it in your reader before relying on it.
- Prebuilt binaries for macOS (Apple Silicon, Intel) and Linux (x86_64, ARM64; statically linked) are attached to each GitHub release, with SHA-256 checksums and build-provenance attestations.
- Generated labels are now English by default: lookup keys for chapters, uncovered text and image galleries, and the offline viewer interface. `convert --labels zh` restores the previous Chinese labels. The language is recorded in `manifest.json`, and verification regenerates with it.
- Fixed: SRCS pages that start with a UTF-8 byte-order mark failed to parse, because every tag span was shifted by three bytes.
- Library: `convert`, `convert_source`, `convert_with_backend` and `mobi_reader::read` now take a `LabelLanguage`, and bundle manifests have a required `labels` field.
- Dependencies are pinned by the committed `Cargo.lock`, and CI also tests the minimum supported Rust version, 1.85.

## 0.2.2-alpha.1 — Collins readability profile v2

- Headword-level part-of-speech labels now join the headword line. Numbered senses and grammar groups keep their own labels.
- Example (`□`) and run-on (`●`) markers are wrapped in hidden spans that keep the original bytes.
- StarDict entries, full chapters and the browser viewer now share one renderer and stylesheet.
- Added regression tests and updated the golden fixtures.

## 0.2.1-alpha.1 — Collins readability profile

- The Collins COBUILD layout profile is selected conservatively from book metadata and stylesheet signatures.
- Headers, senses, examples and run-on entries are wrapped at source-span boundaries, with audit counts for each.
- Grammar prefixes, strike-through examples, nested records, links, original text and assets are preserved.
- Redundant source breaks become inert spans that keep their attributes.
- The shared stylesheet wraps long words for narrow views and large fonts.
- The StarDict CSS file now shares the `.ifo` base name, and `route#anchor` links get exact lookup aliases. Readback checks both.

## 0.2.0-alpha.1 — SRCS backend

- New `srcs` backend for the publisher source embedded in a MOBI (ZIP/XHTML/OPF). It adds:
  - source/compiled cross-checks;
  - source-preserving rendering and a static offline viewer;
  - independent StarDict readback and deterministic regeneration;
  - typed backend dispatch.
- The MOBI container and decompression code is now a reusable interface.
- Added ORDT labels and iterative tag-22 shared-definition resolution.
- Both backends now use one streaming StarDict writer. Its reader accepts exact shared ranges and rejects partial overlaps.
- Conversion, fixture generation and QA now run entirely in Rust.

## 0.1.0-alpha.1

- Initial `compiled` backend: a five-crate converter for ordinary MOBI dictionaries.
