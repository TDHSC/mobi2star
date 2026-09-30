# Command-line reference

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

## `convert` options

- `--backend auto|srcs|compiled` selects the backend; the default is `auto`.
  - `srcs` reads the publisher source that KindleGen embeds in many MOBI files (the SRCS record: a ZIP of XHTML, OPF, CSS and images).
  - `compiled` reads the compiled MOBI text of ordinary dictionaries.
  - `auto` uses `srcs` whenever an embedded source is present.
- `--offset-bits 32|64` sets the StarDict offset width. The default of 32 is the most portable; use 64 only if your reader supports it.
- `--labels en|zh` sets the language of the text that mobi2star generates itself. The default is `en`.
  - This covers lookup keys for chapters, image galleries and text outside any headword (such as `[Chapter 000001] Preface` or `[Supplement 000001]`), plus the offline viewer's interface.
  - Dictionary content is never translated.
  - The choice is recorded in `manifest.json`, so `verify` needs no extra option.
- `--reader` sets the reader the dictionary is built for, which decides how entries reference the stylesheet. See [READERS.md](READERS.md).
- `--profile stardict` publishes only `StarDict/` and `report.json`, the files the browser page produces. They are byte-identical to the full bundle's `StarDict/` and pass the checks that run during conversion, but with no manifest `verify` cannot check them later. The default, `bundle`, writes the full bundle.

The output directory must not exist yet. `convert` creates it with owner-only permissions (`0700`) and publishes `OUTPUT/bundle` only after every check passes. If any check fails, the staging tree is removed and existing files are left untouched.

Verification rebuilds the full bundle in the system temporary directory (set `TMPDIR` to move it), so reserve free space for about twice the bundle size. The default bundle budget is 8 GiB.

## Output layout

The `srcs` backend produces:

```text
bundle/
├── StarDict/            # import this directory into your reader
│   ├── dictionary.ifo
│   ├── dictionary.idx
│   ├── dictionary.dict
│   ├── dictionary.syn
│   ├── dictionary.css   # loaded by KOReader
│   └── res/
│       ├── dictionary.css  # linked from entries (not written for inline-only readers)
│       ├── source/      # images from the publisher source
│       └── compiled/    # images from the compiled MOBI
├── Browser/             # static offline viewer
│   ├── index.html
│   ├── images.html
│   ├── lookup-data.js
│   ├── viewer.js
│   ├── viewer.css
│   ├── content/         # full chapters with original styles and images
│   └── compiled/
├── Source/              # byte-exact copy of every file in the embedded ZIP
├── Audit/               # original MOBI, raw text, parsed facts, render plan, cross-checks
├── manifest.json        # SHA-256 of every file
└── report.json
```

`Browser/index.html` can be opened directly in a web browser.

The `compiled` backend writes a flat bundle: the `dictionary.*` files (including `dictionary.css`) and `res/` at the bundle root, plus `archive/`, `manifest.json` and `report.json`. `lookup` and `verify` recognize both layouts. To import it, copy the `dictionary.*` files and `res/` into a folder of their own; the rest is the audit record. With `--profile stardict`, both backends write the dictionary to `bundle/StarDict/`.

A bundle contains a full copy of its input file (`Audit/original.mobi`), so treat it as private and do not redistribute it.
