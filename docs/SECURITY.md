# Security and failure model

mobi2star is alpha software and has not received a security audit.

## Local-data boundary

The converter and verifier read local files, write private staging/verification directories and publish a new output child. Application Rust source invokes no helper process and includes no network client. It preserves the source MOBI inside the bundle; a complete bundle therefore contains the licensed input. Treat it as private data. The repository contains only project code and original synthetic fixtures.

The optional offline viewer contains normal browser JavaScript; application conversion and validation remain native Rust. Source XML scripts, event handlers, remote stylesheets, unknown media adapters and unsafe URI schemes are rejected in the SRCS path. Source files are also retained unchanged in `Source/`, for inspection as original input.

## Hostile-input controls

Input, text, ZIP expansion, entry sizes, entry/alias counts and output totals have explicit budgets. ZIP directory count is checked before allocation, and classic ZIP structures, central-directory ranges, CRC, file type and normalized/case-colliding paths are checked. Paths are bounded to 64 components and 4,096 bytes. UTF-8 filename identity must be unambiguous. Source references are resolved using URL/package rules independently of the host OS.

MOBI records, numeric bounds, index/tag shapes, compression outputs, reference cycles, UTF encodings and missing resources are checked. Source XML depth and event counts are bounded, internal DTD subsets and unsupported active content are errors, and CSS uses a restricted adapter. Image decode dimensions and allocation have configured limits; original image bytes are retained.

Unknown query/media semantics, encryption, KF8/hybrid renditions, LIGT and unsupported ZIP/XML/CSS profiles require dedicated adapters. Failure rolls back the reserved output tree. No option skips a failed entry.

## Integrity and trust

SHA-256 manifests describe exact file membership, size and bytes. `verify --source ORIGINAL` adds independent original-file identity. Regeneration checks whether the artifacts can be reproduced from the archived source with the producing version; supplying the original is important when source identity matters.

The implementation does not provide a malicious same-user concurrent-filesystem attacker boundary. File races, filesystem failures, dependency flaws and common-mode parser defects remain possible. Directory sync/atomic publication improve crash behavior; they do not establish universal crash-consistency guarantees across every filesystem.

Application source uses safe Rust; dependency internals and operating-system interfaces have their own safety boundaries. First-time dependency resolution still needs a real `Cargo.lock`. Preserve and review that lock and the dependency tree before treating a build as reproducible. No dependency security certification is implied by the direct version pins.

## Resource expectations

Verification creates a second full output tree under the system temporary directory. Its storage budget is additional to the existing/staged bundle. The memory settings bound individual data domains and decoders, rather than total process resident memory. Very large or malformed dictionaries should be tested in an appropriately constrained local environment.

Reader rendering, CSS inheritance, animation, font selection and reader-specific hyperlink handling are separately accepted. Content checks only attest to the implemented invariants described by the report.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's "Report a vulnerability" button on the repository's Security tab rather than in a public issue. Include the mobi2star version, the command you ran and, if possible, a minimal synthetic input that reproduces the problem. Do not attach copyrighted dictionary files.
