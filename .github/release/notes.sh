#!/usr/bin/env bash
# Release notes for a tag, from CHANGELOG.md, on stdout.
#
# A `## v<version>` (or `## <version>`) section wins when there is one.
# Otherwise the notes are every dated `## YYYY-MM-DD` section newer than the
# previous release tag's commit date (all of them for the first release),
# each one kept under its date. Fails when that comes out empty.
#
# Usage: notes.sh <version> <tag> [changelog]
set -euo pipefail

version="$1"
tag="$2"
changelog="${3:-CHANGELOG.md}"

section() {
  awk -v want="$1" '
    /^## / { inside = ($2 == want || $2 == "v" want) ; next }
    inside { print }
  ' "$changelog"
}

empty() { ! grep -q '[^[:space:]]' <<<"$1"; }

notes="$(section "$version")"
if empty "$notes"; then
  since=""
  if previous="$(git describe --tags --abbrev=0 --match 'v*' "$tag^" 2>/dev/null)"; then
    since="$(git log -1 --format=%cs "$previous")"
  fi
  notes="$(awk -v since="$since" '
    /^## [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]/ {
      inside = ($2 > since)
      if (inside) print "### " $2
      next
    }
    /^## / { inside = 0; next }
    inside { print }
  ' "$changelog")"
fi

if empty "$notes"; then
  echo "notes.sh: CHANGELOG.md has nothing for $tag" >&2
  exit 1
fi

# GitHub refuses a release body over 125000 characters, and the first
# release's notes are the whole changelog. Keep the newest dated sections
# that fit and point at the file for the rest.
limit=120000
if [ "${#notes}" -gt "$limit" ]; then
  link="https://github.com/chteau/rbx-native/blob/$tag/CHANGELOG.md"
  notes="$(awk -v limit="$limit" '
    /^### / && kept + length(section) > limit { exit }
    /^### / { printf "%s", section; kept += length(section); section = "" }
    { section = section $0 "\n" }
    END { if (kept + length(section) <= limit) printf "%s", section }
  ' <<<"$notes")"
  notes="$notes"$'\n\n'"Older changes are in [CHANGELOG.md]($link)."
fi
printf '%s\n' "$notes"
