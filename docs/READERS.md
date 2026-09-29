# Readers and `--reader`

StarDict readers disagree about how a dictionary's stylesheet reaches its HTML entries. `convert --reader` picks the output that suits the reader you use. Dictionary content is the same for every choice; only the stylesheet references in each entry, and whether `res/dictionary.css` exists, change.

## What each choice writes

`dictionary.css` next to `dictionary.ifo` is written in every case.

| `--reader` | Per-entry reference | `res/dictionary.css` | Collins COBUILD `.dict` |
|---|---|---|---|
| `koreader` (default), `goldendict` | `<link rel="stylesheet" href="dictionary.css" style="display:none"/>` | yes | 146 MB |
| `goldendict-mobile`, `readest`, `kobo` | a full `<style>` copy | no | 305 MB |
| `universal` | the link, then a full `<style>` copy | yes | 308 MB |

Pick `universal` when one dictionary folder serves several readers, or when your reader isn't listed.

## Evidence per reader

Confidence levels:
- **tested**: we rendered it with the reader's own engine;
- **source**: read in the reader's source code;
- **reported**: from user reports;
- **unknown**: not established.

Nothing below has been tested on a physical device.

| Reader | Loads `dictionary.css` next to the `.ifo` | Loads a linked `res/` stylesheet | Applies inline `<style>` | Use | Confidence |
|---|---|---|---|---|---|
| KOReader | yes | no | **no** | `koreader` | tested + source |
| GoldenDict-ng, GoldenDict (desktop) | no | yes, scoped to the dictionary | yes, but page-wide | `goldendict` | source |
| SilverDict | no | yes, scoped | yes, page-wide | `goldendict` | source |
| GoldenDict Mobile (Android) | no | no | yes | `goldendict-mobile` | reported |
| Readest | no | no | yes, app-wide | `readest` | source |
| QDict, OSS-Dict (Android) | no | no | yes | `readest` | source |
| PyGlossary / penelope → Kobo | no | no (resources dropped) | yes, the only path on Kobo | `kobo` | source |
| Dictionary Universal (iOS) | yes, per its docs | unknown | unknown | `universal` | reported |
| Onyx Boox built-in dictionary | unknown | unknown | unknown | `universal` | unknown |
| StarDict 3, ColorDict (text mode), sdcv | no | no | shown as text | `koreader` | source / reported |

### KOReader

KOReader reads `<ifo path without .ifo>.css` and places it in the `<head>` of the HTML it renders. The entry goes into `<body>` unchanged (`readerdictionary.lua`, `htmlboxwidget.lua`). KOReader bundles MuPDF 1.27.2, and none of its MuPDF patches touch CSS loading.

MuPDF behaves as follows (`source/html/html-parse.c`):
- `html_load_css` reads `<style>` and `<link>` only from direct children of `<head>`, so an inline `<style>` inside an entry has no effect;
- its default stylesheet hides `head`, `script` and `style`, but **not** `link`.

We confirmed this by rendering with MuPDF 1.27.2, wrapped exactly as KOReader does:
- a rule in `<head>` applies; the same rule in a `<style>` inside the body does not, with either the HTML or the XHTML parser;
- a Collins entry with only its inline copy renders identically to one with no CSS at all.

An unhidden `<link>` in the body creates an empty box. That box stops the first block's top margin from collapsing, which moved some Collins entries down by 0.3em. So the link carries `style="display:none"`.

With that attribute, the default output renders identically to the previous inline output: 317 KOReader-style renders of sampled Collins entries, 1,348 pages.

### GoldenDict-ng and GoldenDict

Neither loads a file named after the `.ifo`.

A `<link href>` inside an entry is rewritten to a resource of that dictionary, looked up in `res/` (or `res.zip`). A CSS resource is passed through `isolateCSS`, which scopes it under the dictionary's own id, so it cannot affect other dictionaries (goldendict-ng `src/dict/stardict.cc`, `dictionary.cc`).

Inline `<style>` is honored but applies to the whole article page, which holds every dictionary's results at once. The full-text search index also keeps the text inside `<style>`, so every entry gets indexed with CSS words.

### Readest, QDict, OSS-Dict, GoldenDict Mobile

These insert entry HTML into a single web view and load no stylesheet files for StarDict, so an inline `<style>` is their only styling path:
- **Readest:** `starDictProvider.ts` and `DictionaryResultsView.tsx`;
- **QDict:** `DictPlugs.c` and `QDictions.java`;
- **GoldenDict Mobile:** user reports say linked `res/` stylesheets fail there, for example goldendict issue #766.

### Converting to Kobo

PyGlossary's StarDict reader passes entry HTML through verbatim and ignores `<basename>.css`. Its Kobo writer drops resource files. penelope behaves the same way. Kobo dictionaries support a `<style>` per entry (dictutil docs), so inline copies are the only way styling reaches a Kobo dictionary built this way.

The link tag is self-closed because PyGlossary copies it verbatim into EPUB XHTML, where an unclosed `<link>` is ill-formed.

### StarDict 3, ColorDict, sdcv

- StarDict 3's HTML parser drops unknown tags but keeps their text, so an inline stylesheet shows up as visible text.
- ColorDict's text modes do the same (user report).
- sdcv prints raw HTML.

The default output leaves them only the one-line link.

### Boox and other e-ink devices

Onyx Boox devices open StarDict folders natively. Whether they apply inline `<style>`, `dictionary.css` or `res/` links has not been established. Use `universal`, or KOReader if it is installed.

Sources were read at their `master` or current release on 2026-09-29; installed versions may differ.
