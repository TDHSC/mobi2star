#!/bin/sh
# Builds the browser page into _site/: index.html at the root, and the page's
# scripts, styles and the WebAssembly converter under v/<version>/, so a
# cached page never mixes files of two releases.
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
# Vendored files (tools/web-vendor.lock): cached in target/web-vendor/,
# fetched when missing, and never used unless their SHA-256 matches.
vendor=target/web-vendor
mkdir -p "$vendor"
sha256() { openssl dgst -sha256 -r "$1" | cut -d' ' -f1; }
grep -v '^#' tools/web-vendor.lock | while read -r name sum license url; do
  [ -n "$name" ] || continue
  file="$vendor/$name"
  if [ ! -f "$file" ] || [ "$(sha256 "$file")" != "$sum" ]; then
    curl -fsSL --retry 3 -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  actual=$(sha256 "$file")
  if [ "$actual" != "$sum" ]; then
    printf '%s\n' "$name ($license): expected sha256 $sum, got $actual" >&2
    exit 1
  fi
done

# A cdylib only for this build, so native builds stay plain libraries.
cargo rustc -p mobi2star-web --profile web --target wasm32-unknown-unknown --locked --crate-type cdylib
rm -rf _site
assets="_site/v/$version"
wasm-bindgen --target web --no-typescript --out-dir "$assets" --out-name mobi2star \
  target/wasm32-unknown-unknown/web/mobi2star_web.wasm
cp web/app.js web/worker.js web/worker-common.js web/preview.js web/preview-worker.js \
  web/frame.html web/i18n.js web/style.css web/icon.svg "$assets/"
mupdf="$vendor/mupdf-1.27.0.tgz"
for name in mupdf.js mupdf-wasm.js mupdf-wasm.wasm; do
  tar -xzOf "$mupdf" "package/dist/$name" > "$assets/$name"
done
tar -xzOf "$mupdf" package/LICENSE > "$assets/mupdf-LICENSE.txt"
cp "$vendor"/NotoSans-*.ttf "$vendor/NotoSansCJKsc-Regular.otf" "$vendor/noto-LICENSE.txt" "$assets/"
# MuPDF's glue may load nothing but its own WebAssembly (and node:fs in Node).
if [ "$(grep -c '^import ' "$assets/mupdf.js")" != 1 ] \
  || ! grep -q '^import libmupdf_wasm from "\./mupdf-wasm\.js";$' "$assets/mupdf.js" \
  || [ "$(grep -oE 'import\("[^"]*"\)' "$assets/mupdf.js" | sort -u)" != 'import("node:fs")' ]; then
  printf '%s\n' 'mupdf.js imports something other than its own WebAssembly loader' >&2
  exit 1
fi
# The next deployment keeps these files (tools/keep-published-release.sh).
files=$(cd "$assets" && ls)
printf '%s\n' "$files" > "$assets/files.txt"
sed "s/__VERSION__/$version/g" web/index.html > _site/index.html
printf '%s\n' "Built _site/ for mobi2star $version."
