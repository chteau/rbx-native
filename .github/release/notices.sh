#!/usr/bin/env bash
# Copies CREDITS.md and the licence texts that have to travel with the
# fonts, icons and code built into RbxNative (the `notice` lines of
# assets/credits.toml) into folder $1, the texts under $1/licenses.
# package.sh runs it for every package, macos-app.sh for the app bundle,
# and the Flatpak manifest for its licence folder.
#
# Usage: notices.sh <dir>   (from the repository root)
set -euo pipefail

dest="$1"
mkdir -p "$dest/licenses"
cp CREDITS.md "$dest/"
# Paths hold no spaces (the credits tests check), and BSD sed has no \t.
sed -n 's/^notice = "\(.*\) as \(.*\)"$/\1 \2/p' assets/credits.toml |
  while read -r file name; do
    cp "$file" "$dest/licenses/$name"
  done
