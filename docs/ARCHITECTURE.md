# Architecture

## Dependency direction

```text
mobi2star (CLI, dispatch, transactions, bundle verification)
├── mobi-reader ──────────────────────────┐
├── srcs-reader → mobi-reader             │
│              → html-preserve           │
├── srcs-render → srcs-reader             ├── lexicon-core
│              → html-preserve           │
├── html-preserve                        │
└── stardict-io ──────────────────────────┘
```

Every lower layer is a reusable library. The CLI owns argument parsing and publication; source readers produce facts, renderers produce bytes, and the StarDict library owns file encoding. All application crates forbid unsafe Rust in their own source. External dependencies retain their separate safety and license boundaries.

## Shared MOBI foundation

`Container` wraps a checked `PalmDatabase` and `Header`. Both input adapters reuse `decode_text`, INDX parsing, byte limits, encoding checks and inflection rules. The container exposes bounded records, SRCS detection and raster resources. `Index::definition_spans` resolves physical spans and tag-22 shared references with iterative three-colour traversal; cycles, mixed reference/physical claims, missing lengths and invalid bounds are errors. ORDT decoding supports one-byte and two-byte codes and validates UTF-16 surrogate sequences.

The compiled adapter retains the alpha.1 `Document` model and its source-byte union coverage. The SRCS adapter adds a publisher-source model without forcing XML/package metadata into that older model.

## Publisher-source facts

`SourceArchive` preflights ZIP directory bounds and count before decompression, enforces the expanded-byte budget, checks CRC by exhausting each member, and rejects unsafe paths, ambiguous filename encodings, case/normalization collisions and special files. Each original file is retained exactly.

`SourceParser` combines quick-xml structural events with the shared raw tokenizer's byte spans. It records definition and orthography ownership, explicit inflections, page bodies, ancestor tags, anchors and resource references. No DOM serialization changes the source body. OPF metadata defines reading order; HTML pages outside the spine are appended to the retained chapter set.

`crosscheck` independently reads MOBI compiled facts and compares headword multisets, shared-definition groups, explicit form ownership/multiplicity and forward/reverse rule edges. Definition text is compared after entity decoding and whitespace normalization. Ancillary compiled indexes and every PDB record receive an audit role. Role classification retains known container metadata as metadata; it does not claim to reproduce every piece of Kindle platform behavior.

## Rendering

`srcs-render::Plan` maps actual source targets to source-hash-scoped routes and produces sorted, nonoverlapping edits. Each rendered article consists of a recorded wrapper, a byte-preserving edited source fragment and closing wrappers. Full chapters provide body coverage. Source resource attributes, source spellings and source archives remain available for replay and forensic comparison.

CSS scoping accepts a deliberately bounded grammar: simple selectors, declarations and nested media blocks. Complex selectors and resource/executable constructs trigger an error so their semantics can be added explicitly. Body/ancestor wrappers and CSS can behave differently across reading engines; the report always preserves the separate rendering acceptance status.

The offline browser generator emits local HTML/CSS/JavaScript and lookup records. It starts no browser or server, performs no requests, and does not invoke a helper process. Original image bytes are used directly. Raster decoding validates supported images; animation and reader-specific presentation remain reading-system concerns.

## Shared StarDict writer

`PayloadWriter` streams HTML into `.dict`. `CatalogItem` binds a stable source ID and word to a `Payload` byte range; `CatalogAlias` targets that ID. `write_catalog` resolves sorted ordinals once and emits `.idx`, `.syn` and `.ifo`. Exact shared ranges are allowed; partial overlaps, gaps, trailing unindexed bytes, invalid keys and excessive sizes are errors.

The `compiled` writer is now a thin adapter to these same primitives. The standalone StarDict reader parses the emitted files separately and checks ordering, counts, ordinals and physical coverage.

## Stylesheet delivery

Readers load a dictionary stylesheet in different ways (see [READERS.md](READERS.md)). One concept lives in each layer:

- `lexicon_core::TargetReader` names the reader, and `style_delivery()` maps it to a `StyleDelivery { link, inline }`. `STYLESHEET_FILE` and `LINK_TAG` are the shared names.
- Each renderer produces one dictionary-wide stylesheet and the per-payload references:
  - `srcs_render::Plan` groups pages by their ordered stylesheet list. Each `StyleSet` is scoped under its own wrapper class, so every set can share one file without changing any page's cascade. `Plan::stylesheet()` joins the sets; `Plan::style_prefix()` writes a payload's link and/or inline copy.
  - `html_preserve::stylesheet()` joins the source `<style>` bodies. `render_fragment()` adds the link, and copies the `<style>` elements only for inline delivery. Style bodies that cannot be joined safely are errors: unbalanced CSS, print-only media, or elements split by an entry boundary.
- `stardict_io::stylesheet_files()` owns the file layout: `dictionary.css` next to the `.ifo` always, and `res/dictionary.css` when payloads link to it.

The reader is chosen at render time, so `Audit/render-plan.json`, `edits.json` and the offline viewer do not depend on it.

## Publication and verification

`Transaction` exclusively creates a new owner-only output directory. Source conversion writes into its private staging child, reads the actual dictionary back, then runs `verify_source`. That verifier checks exact file membership and hashes, binds the supplied original when present, parses the actual StarDict files, rebuilds the full deterministic bundle from the archived source into a temporary directory, and compares the two inventories. A successful check allows publication as `OUTPUT/bundle`.

Rebuild verification is intentionally version-specific and reuses the producer's parser/renderer. It detects altered or missing artifacts even when their checksums have been rewritten, while common-mode implementation bugs remain possible. This is why the suite also contains source/compiled comparisons and synthetic adversarial fixtures, and why the report keeps a distinct rendering status.

`mobi2star::OutputOptions` carries every choice that changes bundle bytes: offset width, label language and target reader. Both manifests record these choices, and verification replays or regenerates with them. `manifest::check_header` reads the version before strict parsing, so a bundle from another mobi2star version gets a clear error. Verification dispatches on the manifest's `backend`.

Text that mobi2star generates itself comes from `lexicon_core::LabelLanguage`. That covers lookup keys for uncovered compiled text, chapters and image galleries, plus the offline viewer UI. Both manifests record the language, and verification regenerates with the recorded value. Changing the recorded language therefore fails verification instead of silently producing different keys. Source text is never translated.

The output budget is enforced while emitting source/audit/browser data and while appending dictionary payloads, with aggregate checks before publication. Peak storage includes the staged bundle plus its verification rebuild; memory limits are per declared domain rather than a hard process RSS ceiling.

## Library entry points

```rust
use std::path::Path;
use lexicon_core::Limits;
use mobi2star::{Backend, ConversionReport, OutputOptions};

fn convert_book(input: &Path, output: &Path) -> lexicon_core::Result<()> {
    let (bundle, report) = mobi2star::convert_with_backend(
        input, output, &Limits::default(), OutputOptions::default(), Backend::Auto,
    )?;
    match report {
        ConversionReport::Source(report) => {
            println!("{}: {} publisher-source definitions", bundle.display(), report.definitions);
        }
        ConversionReport::Compiled(report) => {
            println!("{}: {} compiled headwords", bundle.display(), report.source_headwords);
        }
    }
    Ok(())
}
```

`OutputOptions` groups every choice that changes bundle bytes (offset width, label language, target reader); conversions take it instead of separate parameters, and manifests record its values for verification. `convert` / `verify` remain the original typed compiled-adapter APIs. `convert_source` / `verify_source` expose the new typed source report. `convert_with_backend` / `verify_bundle` provide typed unified dispatch.


## Readability adapter

`readability::applies` checks publisher metadata plus stylesheet signatures. `readability::edits` returns source-byte edits and typed counts; it shares the existing tokenizer, checked spans, replay engine and transaction/verification pipeline. The profile contains a single CSS asset embedded with `include_str!`. It forms the plan's only style set, so the bundle writer writes identical bytes as `dictionary.css`. Unknown books follow the original source-scoping route.

Layout wrappers operate on sibling ranges, with nested entries and tables as barriers. A source `br` at an existing structural break is represented by an inert span retaining its attributes; source archive and edit provenance reconstruct the original. Labels, punctuation, IPA, examples and lookup ownership remain source facts. The profile uses native HTML block tags to retain paragraph structure when an engine ignores styles inside article bodies.

Exact fragment route aliases supplement base route aliases. The StarDict layer keeps ownership and physical payload sharing intact and independently looks up each full URI suffix after writing. KOReader can resolve those suffixes through sdcv; UI anchor scrolling is a separate reader concern.

Three original input/expected golden fixture pairs under `tests/fixtures/readability` pin the byte-level output for split headwords, menus/nested records and usage boxes.
