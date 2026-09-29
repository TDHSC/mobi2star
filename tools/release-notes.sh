#!/bin/sh
# Print the CHANGELOG.md notes for VERSION, without the heading.
# Fails if the section is missing or empty, so a release cannot ship without notes.
set -eu
[ $# -eq 1 ] || { printf 'usage: %s VERSION\n' "$0" >&2; exit 2; }
cd "$(dirname "$0")/.."
notes=$(awk -v version="$1" '
    /^## / { if (found) exit; if ($2 == version) { found = 1; next } }
    found { print }
' CHANGELOG.md | sed '/./,$!d')
[ -n "$(printf '%s' "$notes" | tr -d '[:space:]')" ] || { printf 'CHANGELOG.md has no notes for %s\n' "$1" >&2; exit 1; }
printf '%s\n' "$notes"
