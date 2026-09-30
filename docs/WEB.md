# The browser page

<https://tdhsc.github.io/mobi2star/> converts a MOBI dictionary inside the visitor's browser. It runs the same Rust converter as the CLI, compiled to WebAssembly, and offers the result as a zip holding one StarDict folder. The dropped file is never uploaded.

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

`tools/build-web.sh` builds the WebAssembly with the `web` profile (release plus fat LTO), runs `wasm-bindgen --target web`, and assembles `_site/`:
- `index.html` sits at the root;
- everything else goes under `v/<version>/`, so a cached page never loads scripts and WebAssembly from two different releases.

## Privacy model

- **The document.** `index.html` sets a Content Security Policy with `default-src 'none'` and `connect-src 'none'`. Scripts, styles and the worker may come only from the site itself, and the page itself can make no network requests.
- **The worker.** A CSP `<meta>` tag does not reach a worker, which takes its policy from HTTP headers that GitHub Pages cannot set. Inside the worker, "the file never leaves the device" rests on the code: `worker.js` fetches only its own `.wasm`, and the Rust side has no network access. Both are short enough to review.
- **Storage.** The page stores nothing about the file. `localStorage` holds only the chosen page language, and the page works without it.
- **Hosting.** As with any site, GitHub serves the page's files and sees those requests. The site has no analytics, cookies or third-party resources.

## Limits and measurements

`Limits::browser()` caps the input at 256 MiB and the output at 1.5 GiB, because WebAssembly memory is at most 4 GiB and holds the source, the parsed book and the archive at once. `Source` refuses a larger file, or one memory cannot hold, before reading any of it.

Collins COBUILD Advanced Learner's Dictionary is a 31 MB MOBI with 34,755 payloads. Measured on an Apple Silicon Mac:

| Where | Time | Memory | Output |
|---|---|---|---|
| Native `convert_dictionary_zip` | 5.1 s | 570 MB peak RSS | 25.4 MB zip |
| WebAssembly in Node 26 | 6.3 s | 388 MiB WebAssembly memory | same bytes |
| The page in a Chromium-based browser | a few seconds, not timed | not measured | same bytes (SHA-256) |

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

`tools/web-smoke.mjs` needs only Node. It loads `_site/` the way the worker does, then:
- converts the fixtures for every reader;
- unzips each result and requires it to equal the CLI's `--profile stardict` output, file by file;
- checks progress, error codes, and that both languages cover every reader and label choice.

```sh
cargo build -p mobi2star --release --locked
node tools/web-smoke.mjs target/release/mobi2star
node tools/web-smoke.mjs target/release/mobi2star --measure BOOK.mobi   # also time BOOK
```

`.github/workflows/web.yml` runs Clippy for `wasm32`, the build and this comparison. CI calls it on every push, and the release workflow calls it before deploying.

## Deployment

`.github/workflows/pages.yml` runs when a `v*` tag is pushed. It checks that the tag matches the crate version, builds the site, runs the smoke test, and deploys `_site/` to GitHub Pages. See [RELEASING.md](RELEASING.md).
