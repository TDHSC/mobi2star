# What is checked

mobi2star refuses to publish output it has not checked. This page describes the checks a full CLI bundle goes through; the browser page runs the subset described in [WEB.md](WEB.md#what-it-produces).

## Content

The `srcs` backend works only from facts stated in the file. It reads every headword, definition block, explicit inflection, page, anchor and resource in the source XHTML, then cross-checks them against the compiled MOBI index, its shared-definition references and its inflection rules:

- Headwords and inflections are compared as multisets, including ownership and repeat counts.
- The visible text of each definition block is compared with the compiled text after whitespace normalization.
- Content is converted by copying original UTF-8 byte ranges plus the attribute rewrites that are needed. Every edit is recorded with its range and reason, so the original can be reconstructed.

## Output

After writing, a separate StarDict reader checks index ordering, alias targets, shared ranges, full payload coverage and readback. The bundle is then regenerated from the archived MOBI and compared by file set, size and SHA-256.

Regeneration reuses the same parser and renderer, so it cannot catch a bug that affects both runs identically. The cross-format text comparison, byte-exact archives, synthetic tests and reader acceptance cover that gap from different angles.

## The report

The report keeps content checks separate from rendering:

```json
{
  "backend": "srcs-rust",
  "implemented_content_checks_passed": true,
  "rendering_status": "unverified_reader_dependent",
  "skipped_entries": 0
}
```

How a dictionary looks in a particular reader is not something mobi2star can check; [READERS.md](READERS.md) records what is known about each reader and how confident that is.

See [ARCHITECTURE.md](ARCHITECTURE.md#publication-and-verification) for how publication and verification are built, and [TESTING.md](TESTING.md) for the test suite.
