#!/bin/sh
# Native Rust quality gate; all fixtures and tests run in Rust.
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'Install the Rust toolchain, then rerun ./tools/qa.sh.' >&2; exit 1; }
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked
cargo build --release -p mobi2star --locked
printf '%s\n' 'Native quality gate passed. Reader-specific rendering remains an acceptance task.'
