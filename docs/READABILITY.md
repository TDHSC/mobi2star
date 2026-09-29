# Collins COBUILD readability profile

The `srcs` backend normally keeps each book's own stylesheet, scoped so it cannot leak into the reader. Collins COBUILD dictionaries render poorly that way on narrow screens: headword lines break apart, and examples, senses and derived forms run together. The `collins-readable-v2` profile fixes that layout without changing any source text.

Code: `crates/srcs-render/src/readability.rs`. Stylesheet: `crates/srcs-render/src/readable.css`.

## Selection

Selection is deliberately conservative. The profile applies only when both of these hold:

- the book title contains `Collins COBUILD`;
- one of its stylesheets contains the publisher's `amzn-mobi` signature together with the `.hw`, `.hwtxt`, `.entry` and `span.ex` selectors.

Every other book keeps the source-scoped adapter. The chosen profile is recorded as `layout_profile` in `report.json`, which is `source` when the profile does not apply.

## Layout rules

Every change is a recorded byte-level edit to the source. Each edit either wraps a run of sibling elements or splits a text node. Original tags, attributes, characters and whitespace are all kept, so the source can be rebuilt by replaying the edit log.

- **Header line.** The headword, its pronunciation, and any part-of-speech label that belongs directly to the headword are grouped into `m2s-header`. That label is wrapped in `m2s-pos`.
- **Senses, examples, derived forms.** Numbered senses (`m2s-sense`), examples (`m2s-example`) and run-on derived forms (`m2s-runon`) become separate blocks.
  - A numbered sense keeps its own part-of-speech label.
  - A grammar pattern that comes before an example, such as `[ADJ n]`, stays inside that example.
  - Register labels such as `INFORMAL` are not treated as parts of speech.
- **Structural markers.** When `□` introduces an example or `●` introduces a derived form, the marker is wrapped in a hidden `m2s-example-marker` or `m2s-runon-marker` span. Entity-encoded forms (`&#x25A1;`, `&#9679;`) are handled the same way. The same characters are left visible when they appear inside example text, attributes, comments or inline references.
- **Barriers.** Tables, usage boxes, menus and nested entries are never merged with the content around them.
- **Redundant breaks.** A source `<br>` that sits on a structural boundary becomes an inert `m2s-source-break` span that keeps its attributes. Otherwise MuPDF-based readers render it as an extra blank line.
- **Kept as-is.** Strike-through examples (`span.st`, which mark incorrect usage in the original), images, chapter headings and appendices are unchanged.

The wrappers are plain HTML block elements, so paragraph structure survives in readers that ignore styles inside the dictionary entry.

Input that already contains the profile's wrappers is rejected instead of being wrapped a second time.

## Stylesheet

`Plan::build` applies the same edits and the same `readable.css` to three outputs: StarDict entries, full chapters and the offline browser viewer.

In the StarDict output the CSS appears in two places. It is embedded in each entry, and it is also written as `StarDict/dictionary.css`, because KOReader loads the CSS file that shares the `.ifo` base name.

The stylesheet is tuned for narrow screens and large fonts:

- relative font sizes and left alignment;
- indented examples;
- `overflow-wrap: break-word` so long unbroken runs still wrap;
- scalable images and compact table borders.

## Links

KOReader's `bword://` handler passes the entire rest of the URL to lookup, fragment included. To match that, the writer emits two lookup keys for every resolved internal link:

- the plain route;
- an exact `route#anchor` alias.

The independent StarDict reader checks every one of those keys after writing. Whether the reader then scrolls to the anchor inside the article is up to the reader, and has to be checked on the device.

## Tests

The unit tests in `readability.rs` cover:

- header grouping, including several sibling headwords and hyphenated POS spellings;
- which sense a POS label or grammar pattern belongs to;
- marker detection, including entity forms and markers quoted inside examples;
- comments, nested tables and redundant breaks;
- rejection of malformed or already-transformed input;
- the shared stylesheet.

Three golden input/expected pairs in `tests/fixtures/readability/` (`headword`, `menu` and `usage`) pin the byte-level output. Their markup follows the publisher's structure, but the text is original and synthetic.

Tests can only prove the byte-level output. The final layout depends on the reader's fonts, popup size and pagination, so check it on the target device.

## Reader behavior references

- KOReader dictionary support: same-name CSS, HTML mode and the MuPDF dictionary renderer. https://github.com/koreader/koreader/wiki/Dictionary-support
- KOReader `readerdictionary.lua`: how the same-name CSS is loaded, and the exact lookup key in `onHtmlDictionaryLinkTapped`. https://github.com/koreader/koreader/blob/master/frontend/apps/reader/modules/readerdictionary.lua
- KOReader `htmlboxwidget.lua`: CSS injection into the document head and MuPDF line breaking. https://github.com/koreader/koreader/blob/master/frontend/ui/widget/htmlboxwidget.lua

These describe upstream `master` as of 2026-09-29; installed KOReader versions may behave differently.
