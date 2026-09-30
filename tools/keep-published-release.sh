#!/bin/sh
# Adds the currently published release's v/<version>/ files to _site/, so a
# page opened before this deployment can still start its worker and load
# its WebAssembly. Only that one release is kept; older ones are dropped.
# Best effort: if anything is missing or unreachable, nothing is kept.
#
#   tools/keep-published-release.sh SITE_URL
set -eu
[ $# -eq 1 ] || { printf 'usage: %s SITE_URL\n' "$0" >&2; exit 2; }
cd "$(dirname "$0")/.."
site=${1%/}
fetch() { curl -fsSL --retry 3 "$@"; }
skip() { printf '%s; nothing kept.\n' "$1"; exit 0; }

page=$(fetch "$site/index.html") || skip "No page is published at $site"
version=$(printf '%s\n' "$page" | sed -n 's|.*src="v/\([^"/]*\)/app\.js".*|\1|p' | head -n 1)
case $version in
  '') skip "The published page names no version" ;;
  *[!0-9A-Za-z.+-]*) printf 'Unexpected published version %s\n' "$version" >&2; exit 1 ;;
esac
[ ! -e "_site/v/$version" ] || skip "_site/ already has version $version"
files=$(fetch "$site/v/$version/files.txt") || skip "Version $version lists no files"

kept=$(mktemp -d)
trap 'rm -rf "$kept"' EXIT
for file in $files; do
  case $file in
    .* | *[!0-9A-Za-z._-]*) printf 'Unexpected file name %s\n' "$file" >&2; exit 1 ;;
  esac
  fetch -o "$kept/$file" "$site/v/$version/$file" || skip "Could not fetch $file of $version"
done
printf '%s\n' "$files" > "$kept/files.txt"
mkdir -p _site/v
cp -R "$kept" "_site/v/$version"
printf 'Kept the published files of %s.\n' "$version"
