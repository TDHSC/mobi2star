# Security and failure model

mobi2star is alpha software and has not received a security audit.

## Local-data boundary

The converter and verifier read local files, write private staging/verification directories and publish a new output child. Application Rust source invokes no helper process and includes no network client. It preserves the source MOBI inside the bundle; a complete bundle therefore contains the licensed input. Treat it as private data. The repository contains only project code and original synthetic fixtures.

The optional offline viewer contains normal browser JavaScript; application conversion and validation remain native Rust. Source XML scripts, event handlers, remote stylesheets, unknown media adapters and unsafe URI schemes are rejected in the SRCS path. Source files are also retained unchanged in `Source/`, for inspection as original input.

## Browser page

The page at <https://tdhsc.github.io/mobi2star/> runs the converter as WebAssembly in a web worker ([WEB.md](WEB.md)):
- The file is read in the visitor's browser and never uploaded. The page's Content Security Policy forbids the page itself any network request (`connect-src 'none'`). It cannot cover the worker, because GitHub Pages sends no CSP header; there the guarantee rests on the code, which fetches only its own `.wasm`.
- The site has no analytics, cookies or third-party resources. `localStorage` holds only the chosen page language.
- The same input budgets apply, with `Limits::browser()` caps. A hostile file can at worst stop the worker, which the page reports; each conversion runs in a fresh worker that is terminated afterwards.
- The preview shows untrusted entry HTML. Web-engine readers get it in a frame with scripts, forms and popups sandboxed off and its own policy (`default-src 'none'`, inline styles and `data:` images only), after it is parsed inertly and stripped of elements that load or navigate. Link clicks are cancelled and handed to the preview's rules as strings. KOReader's pages are laid out by MuPDF in the preview worker, on a document in memory with no file or network access. A hostile test entry is described in [WEB.md](WEB.md#security).
- The page bundles third-party code the repository does not contain: MuPDF's WebAssembly build and KOReader's fonts. `tools/web-vendor.lock` pins each by SHA-256, and the build refuses any other bytes.
- The page is built from the tagged source by the release workflow and served over HTTPS. The workflow compares its output with the CLI and deploys only after the release is published. The previous release's files, which the deployment keeps for pages already open, are fetched from the live site; they were built the same way from their own tag.

## Hostile-input controls

Input, text, ZIP expansion, entry sizes, entry/alias counts and output totals have explicit budgets. ZIP directory count is checked before allocation, and classic ZIP structures, central-directory ranges, CRC, file type and normalized/case-colliding paths are checked. Paths are bounded to 64 components and 4,096 bytes. UTF-8 filename identity must be unambiguous. Source references are resolved using URL/package rules independently of the host OS.

MOBI records, numeric bounds, index/tag shapes, compression outputs, reference cycles, UTF encodings and missing resources are checked. Source XML depth and event counts are bounded, internal DTD subsets and unsupported active content are errors, and CSS uses a restricted adapter. Image decode dimensions and allocation have configured limits; original image bytes are retained.

Unknown query/media semantics, encryption, KF8/hybrid renditions, LIGT and unsupported ZIP/XML/CSS profiles require dedicated adapters. Failure rolls back the reserved output tree. No option skips a failed entry.

## Integrity and trust

SHA-256 manifests describe exact file membership, size and bytes. `verify --source ORIGINAL` adds independent original-file identity. Regeneration checks whether the artifacts can be reproduced from the archived source with the producing version; supplying the original is important when source identity matters.

The implementation does not provide a malicious same-user concurrent-filesystem attacker boundary. File races, filesystem failures, dependency flaws and common-mode parser defects remain possible. Directory sync/atomic publication improve crash behavior; they do not establish universal crash-consistency guarantees across every filesystem.

Application source uses safe Rust; dependency internals and operating-system interfaces have their own safety boundaries. Dependencies are pinned by the committed `Cargo.lock`, which QA and CI use with `--locked`; review lockfile changes like code changes. The workspace uses resolver 3, so dependency updates prefer versions that support the declared minimum Rust version. No dependency security certification is implied by the direct version pins.

## Resource expectations

Verification creates a second full output tree under the system temporary directory. Its storage budget is additional to the existing/staged bundle. The memory settings bound individual data domains and decoders, rather than total process resident memory. Very large or malformed dictionaries should be tested in an appropriately constrained local environment.

Reader rendering, CSS inheritance, animation, font selection and reader-specific hyperlink handling are separately accepted. Content checks only attest to the implemented invariants described by the report.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's "Report a vulnerability" button on the repository's Security tab rather than in a public issue. Include the mobi2star version, the command you ran and, if possible, a minimal synthetic input that reproduces the problem. Do not attach copyrighted dictionary files.
