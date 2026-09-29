# Changelog

## Unreleased

- Generated labels are now English by default: lookup keys for chapters, uncovered text and image galleries, and the offline viewer interface. `convert --labels zh` restores the previous Chinese labels. The language is recorded in `manifest.json`, and verification regenerates with it.

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
