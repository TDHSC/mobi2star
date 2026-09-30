# The browser page

<https://tdhsc.github.io/mobi2star/> converts a MOBI dictionary inside the visitor's browser. It runs the same Rust converter as the CLI, compiled to WebAssembly, and offers the result as a zip holding one StarDict folder. It can then preview the converted dictionary as the chosen reader shows it. The dropped file is never uploaded.

## What it produces

The page runs the CLI's dictionary-only profile, `convert --profile stardict`:
- The zip holds one folder, named from the dictionary title, containing the StarDict files, `dictionary.css` and `res/`. These files are byte-identical to `StarDict/` in a full CLI bundle made with the same options. CI checks this for every reader.
- The visitor chooses the reader and the language of added labels. The backend is detected (`auto`) and the offset width is 32.
- It runs the checks that happen during conversion: for publisher-source books, the source/compiled cross-check, the independent StarDict readback and every internal route; for compiled books, the readback and a replay of every entry. The readback reads the finished zip, reopened from its own bytes.
- It does not write the audit trees, the offline viewer or a manifest, and it does not regenerate the bundle to compare. Use the CLI when you want a bundle that `verify` can check later.

## How it is built

| Part | Role |
|---|---|
| `mobi2star::convert_dictionary_zip` | The pipeline: parse, write the dictionary through a `ZipTree`, read it back and check it. Uses no filesystem and no clock. |
| `crates/mobi2star-web` | The page's converter: its choices, `Limits::browser()`, and a summary. Plain Rust that builds and tests natively. |
| `crates/mobi2star-web/src/bindings.rs` | wasm-bindgen exports, compiled only for `wasm32`: `Source`, `convert`, `choices`, `version` and `lastPanic`. |
| `web/worker.js` | Runs one conversion. It streams the file into WebAssembly memory chunk by chunk (`Source`), so no second whole copy exists. The page starts a worker per conversion and terminates it afterwards, which frees its memory and implements Cancel. |
| `web/app.js`, `web/i18n.js`, `web/style.css`, `web/index.html` | The page: file choice, options, progress, errors and the result. No framework and no npm. All text comes from `i18n.js`, in English and Chinese. |

The preview adds the parts in [The preview](#the-preview).

`tools/build-web.sh` builds the WebAssembly with the `web` profile (release plus fat LTO), runs `wasm-bindgen --target web`, and assembles `_site/`:
- `index.html` sits at the root;
- everything else goes under `v/<version>/`, so a cached page never loads scripts and WebAssembly from two different releases.

## The preview

After a conversion, **Preview** opens the converted zip and shows it as the reader chosen in the options shows it. Search a word or take a random one, step through the results, and follow links the way that reader follows them; Back returns to the previous view. For `universal`, a switch picks among the five readers below. Each preview says how close it is.

| Option | Reader shown | Drawn by | What it follows | How close |
|---|---|---|---|---|
| `koreader` | KOReader's dictionary popup | MuPDF 1.27.0 (KOReader pins 1.27.2), on a canvas | KOReader's document: its MuPDF CSS fixes, its dictionary CSS and `dictionary.css` in `<head>`, the entry in `<body>`, the `(query : …)` line, and its `<br>` rewrite. Its lookups, and its links: `bword://X` looks up X literally, `#anchor` included, and does nothing when the anchor is in the entry shown. | the same engine and fonts: see below |
| `goldendict` | GoldenDict-ng's article view | the browser | GoldenDict-ng's article markup, linked `res/` stylesheets passed through a port of its `isolateCSS`, its accent- and case-folding lookup (up to 10 results), and its links: the word is looked up, then the view scrolls to the anchor. | same rules; your browser lays it out, where GoldenDict-ng uses Chromium |
| `goldendict-mobile` | GoldenDict Mobile | the browser | only what user reports establish: the entry with its inline `<style>`. Links are not followed. | reported |
| `readest` | Readest's dictionary popup | the browser | Readest's card and lookup (one entry), Tailwind's preflight in a lower cascade layer so the entry's `<style>` wins, no `res/`, so images do not appear. Readest follows no dictionary links. | same rules |
| `kobo` | a Kobo dictionary made with PyGlossary | the browser | exactly what PyGlossary writes: its prefix groups, synonym blocks, `[Image: …]` placeholders and stripped resources. How a Kobo draws it is not known, so links are not followed. | PyGlossary's output; device engine unknown |

What the preview does not show: the reader's own window around the entry (title bars, buttons, other dictionaries' results), fonts and settings a user changes on the device, and anything that depends on the device's screen beyond the presets.

### How it works

| Part | Role |
|---|---|
| `crates/reader-view` | Per reader, the document it builds, its lookup and its link rules, ported from each reader's source (each file names the upstream file and commit). Pure Rust with native tests. AGPL-3.0-or-later, see [Licensing](#licensing). |
| `crates/mobi2star-web/src/preview.rs` | A `Session` over one converted zip: `info`, `search`, `follow`, `suggest` and a headword for Random, answering in JSON. The `Preview` class in `bindings.rs` wraps it. |
| `web/preview-worker.js` | A second worker, started when Preview is opened and terminated when the conversion it belongs to is cleared, on a new file or before a new conversion. It streams the zip in through `Source.forArchive` and answers the panel's requests. For KOReader it also runs MuPDF. |
| `web/preview.js` | The panel. Web-engine readers are drawn in a frame (`web/frame.html`), KOReader's pages on a canvas. |
| `web/koreader-page.js` | The MuPDF glue: fonts, layout, and one page drawn at a time with its links and text. The worker and `tools/web-smoke.mjs` share it. |

### KOReader

The worker lays out KOReader's document with MuPDF's WebAssembly build in the text box of KOReader's dictionary popup, computed from KOReader's own sizes (`Geometry::for_screen`) for a preset screen and the dictionary font size (KOReader's `dict_font_size`: 8 to 32, 20 by default). Each box-sized page is drawn in grayscale and shown at half its device pixels, with a button over each link, labelled with the link's text, and the page's text for screen readers.

Fonts follow KOReader's MuPDF (koreader-base `thirdparty/mupdf/external_fonts.patch`), which is built without MuPDF's own fonts:
- the family `Noto Sans`, which KOReader's dictionary CSS sets, is koreader-fonts' Noto Sans; other families get the URW fonts that MuPDF also builds in;
- a character the page's font lacks falls back to Noto Sans CJK SC, whatever its script, then to FreeSerif.

MuPDF's font hook answers one font per script and cannot chain, so a script that Noto Sans CJK SC has no characters for (Arabic, Hebrew, Thai, …) goes straight to FreeSerif, where KOReader ends up. After laying a page out, the preview also looks for characters drawn as boxes that FreeSerif has, such as Collins's `▸` before phrasal verbs, sets exactly those in FreeSerif, and lays the page out again. One difference remains: a common character, such as a symbol, inside a run of such a script may come out in FreeSerif where KOReader would find it in the CJK font first.

Noto Sans is fetched with MuPDF. Noto Sans CJK SC (16 MB) and FreeSerif (1.8 MB) are fetched the first time a page needs them. The worker fetches them synchronously, in the middle of the layout: MuPDF remembers a fallback it was refused for as long as it runs, so laying out again after an asynchronous fetch would not pick them up.

To check the engine, 300 sampled Collins entries were laid out both by the preview (mupdf.js 1.27.0) and by PyMuPDF 1.27.2 given the same fonts through the same fallback rule. Every entry had the same page count, and all 5,438 lines had the same text and the same boxes to 0.02 px.

Known differences from the device:
- after a followed link, KOReader's `(query : …)` line shows the link's internal key, as KOReader does with these dictionaries;
- KOReader's popup around the text box is not drawn.

### Security

Entries are untrusted HTML. The preview keeps them from running script, loading anything or navigating:
- **Web-engine readers.** Each document is parsed by the frame's own `DOMParser`, which runs nothing. The page removes `meta`, `base`, `link`, `script`, `iframe`, `frame`, `frameset`, `object`, `embed` and every `ping`, then imports the rest into the frame. The frame is `sandbox="allow-same-origin"`, with no scripts, forms or popups, and `web/frame.html` has its own policy: `default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'`. Stylesheets and images from `res/` arrive inlined as text and `data:` URIs, so nothing else may load. `allow-same-origin` lets the page fill the frame and catch its clicks; with scripts off, it gives the entry nothing. Capture-phase listeners on the frame cancel every click on a link (HTML and SVG `<a>`, image-map `<area>`, MathML `href`) and hand its target to the worker as a string.
- **KOReader.** MuPDF runs in the worker on a document held in memory, with no file system and no network, so an entry's references to other files resolve to nothing. Its link targets are strings handed to the same rules.
- **The page.** Its own policy only gains `frame-src 'self'`.

A hand-made dictionary whose entry tries a meta refresh, `<base>`, external stylesheets, `@import`, `@font-face`, CSS `url()`, scripts, frames, `object`, `embed`, images, `srcset`, SVG and video sources, a form, a `javascript:` link, `ping`, an image-map `<area>` and an SVG `xlink:href` link was previewed in all five readers in a Chromium-based browser, clicking every link and the form's button. The local server saw no request for any of them, the page's history did not change and the frame never navigated.

## Licensing

The converter, the CLI and the libraries they use are MIT. `crates/reader-view` is AGPL-3.0-or-later: it ports behaviour from GoldenDict-ng (GPL-3.0), KOReader (AGPL-3.0) and Readest (AGPL-3.0). `tools/qa.sh` fails if the converter ever depends on it.

The page links the preview into its WebAssembly and bundles MuPDF, which is AGPL-3.0, so the page as published is AGPL-3.0-or-later. Its footer links to this repository at the release's tag and to MuPDF's source at the commit of the bundled build.

`tools/web-vendor.lock` pins every file the page bundles but this repository does not contain, by URL and SHA-256: MuPDF's npm build and KOReader's fonts from koreader-fonts. `build-web.sh` downloads each into `target/web-vendor/`, refuses one whose hash differs, and copies its license next to it: Noto's OFL, and FreeSerif's GPL with the font exception.

## Privacy model

- **The document.** `index.html` sets a Content Security Policy with `default-src 'none'` and `connect-src 'none'`. Scripts, styles and the worker may come only from the site itself, and the page itself can make no network requests.
- **The workers.** A CSP `<meta>` tag does not reach a worker, which takes its policy from HTTP headers that GitHub Pages cannot set. Inside the workers, "the file never leaves the device" rests on the code: `worker.js` fetches only its own `.wasm`, `preview-worker.js` only that, MuPDF and the fonts, all from the site, and the Rust side has no network access. They are short enough to review.
- **Storage.** The page stores nothing about the file. `localStorage` holds only the chosen page language, and the page works without it.
- **Hosting.** As with any site, GitHub serves the page's files and sees those requests. The site has no analytics, cookies or third-party resources.

## Limits and measurements

`Limits::browser()` caps the input at 256 MiB and the output at 1.5 GiB, because WebAssembly memory is at most 4 GiB and holds the source, the parsed book and the archive at once. `Source` refuses a larger file, or one memory cannot hold, before reading any of it. The preview holds a converted zip and its unpacked files together while it opens them, so it refuses a dictionary whose zip and unpacked files exceed the same 1.5 GiB, before inflating anything.

Collins COBUILD Advanced Learner's Dictionary is a 31 MB MOBI with 34,755 payloads. Measured on an Apple Silicon Mac:

| Where | Time | Memory | Output |
|---|---|---|---|
| Native `convert_dictionary_zip` | 5.1 s | 570 MB peak RSS | 25.4 MB zip |
| WebAssembly in Node 26 | 6.3 s | 388 MiB WebAssembly memory | same bytes |
| The page in a Chromium-based browser | a few seconds, not timed | not measured | same bytes (SHA-256) |

The preview of the same dictionary, measured in Node 26 on the same Mac:
- Opening its 24 MiB zip takes 0.3 s and holds 190 MiB of WebAssembly memory, mostly the 146 MB of definitions; MuPDF and its fonts add their own.
- A lookup takes under 2 ms, apart from the first in GoldenDict-ng (0.1 s) and Readest (0.06 s), which build their lookup indexes.
- A KOReader page lays out and draws in 2 ms (14 ms at most over 200 entries).
- Over the network, the first KOReader page loads MuPDF (10 MB) and Noto Sans (1.4 MB); the fallback fonts follow only when a page needs them.

Phones and tablets are best effort. A large dictionary can exceed a mobile browser's memory, and the page then reports that the converter stopped, most likely from running out of memory. Out of memory aborts without a panic message, so the page can't be more specific.

## Building and testing locally

You need the `wasm32-unknown-unknown` target and the `wasm-bindgen-cli` version that `Cargo.lock` pins:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --locked --version "$(tools/build-web.sh --bindgen-version)"
./tools/build-web.sh
python3 -m http.server --directory _site 8000
```

Then open <http://localhost:8000/>.

The first build downloads the files in `tools/web-vendor.lock` (about 27 MB) and checks them with `openssl`; later builds reuse `target/web-vendor/`.

`tools/web-smoke.mjs` needs only Node. It loads `_site/` the way the workers do, then:
- converts the fixtures for every reader;
- unzips each result and requires it to equal the CLI's `--profile stardict` output, file by file;
- checks progress, error codes, and that both languages cover every reader, label, previewed reader, fidelity note, link outcome and screen;
- opens a converted zip in the preview, looks words up and follows links;
- draws a KOReader page with the bundled MuPDF and fonts, and checks its size, text, query line, fonts, links, a followed link, and a character only FreeSerif has.

```sh
cargo build -p mobi2star --release --locked
node tools/web-smoke.mjs target/release/mobi2star
node tools/web-smoke.mjs target/release/mobi2star --measure BOOK.mobi   # also time BOOK
```

`tools/web-page-test.mjs` checks what needs a browser: it serves `_site/`, drives headless Chrome (found on the PATH, or set `CHROME`) through the page and the preview, and checks the panel's behaviour and the frame's link handling ([TESTING.md](TESTING.md)).

```sh
node tools/web-page-test.mjs
```

`.github/workflows/web.yml` runs Clippy for `wasm32`, the build, this comparison and the browser test, caching the downloaded files by the lock file's hash. CI calls it on every push, and the release workflow calls it before deploying.

## Deployment

The release workflow deploys the page, only after the GitHub release itself is published, so a failed release publishes no page. See [RELEASING.md](RELEASING.md).

Each deployment replaces the whole site, so it also carries the previous release's `v/<version>/` files, listed in that release's `files.txt`. A page opened before the update, or served from the browser cache, can still start its worker. Only one earlier release is kept. If loading fails anyway, the page suggests reloading.
