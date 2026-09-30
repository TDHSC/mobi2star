#!/bin/sh
# Native Rust quality gate; all fixtures and tests run in Rust.
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'Install the Rust toolchain, then rerun ./tools/qa.sh.' >&2; exit 1; }
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
# The converter stays MIT: nothing it builds may pull in the AGPL preview crate.
if out=$(cargo tree -p mobi2star -e normal --locked -i reader-view 2>&1); then
  printf '%s\n%s\n' 'mobi2star must not depend on reader-view (AGPL-3.0):' "$out" >&2
  exit 1
fi
case $out in
  *"did not match any packages"*) ;;
  *) printf '%s\n' "$out" >&2; exit 1 ;;
esac
cargo build --release -p mobi2star --locked
printf '%s\n' 'Native quality gate passed. Reader-specific rendering remains an acceptance task.'
