#!/bin/sh
# Builds the browser page into _site/: the WebAssembly converter and its JS
# glue under v/<version>/, so a cached page never mixes files of two releases.
# --bindgen-version prints the wasm-bindgen-cli version it needs.
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
bindgen=$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)
if [ "${1:-}" = "--bindgen-version" ]; then
  printf '%s\n' "$bindgen"
  exit 0
fi
if [ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $bindgen" ]; then
  printf '%s\n' "Needs wasm-bindgen-cli $bindgen, matching Cargo.lock:" \
    "  cargo install wasm-bindgen-cli --version $bindgen --locked" >&2
  exit 1
fi
# A cdylib only for this build, so native builds stay plain libraries.
cargo rustc -p mobi2star-web --profile web --target wasm32-unknown-unknown --locked --crate-type cdylib
rm -rf _site
wasm-bindgen --target web --no-typescript --out-dir "_site/v/$version" --out-name mobi2star \
  target/wasm32-unknown-unknown/web/mobi2star_web.wasm
printf '%s\n' "Built _site/ for mobi2star $version."
