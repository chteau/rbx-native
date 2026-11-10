#!/usr/bin/env bash
# Puts WinDivert's x64 runtime next to the Windows build: WinDivert.dll,
# WinDivert64.sys (both loaded at runtime by Play's network simulator,
# `rbx_play::netsim::windivert`) and its license as WinDivert-LICENSE.txt.
# WinDivert is LGPLv3 (or GPLv2); the files are the upstream release's,
# unmodified, and the hash pins exactly that release.
#
# Usage: windivert.sh <dir to put the files in>
set -euo pipefail

version=2.2.2
sha256=63cb41763bb4b20f600b6de04e991a9c2be73279e317d4d82f237b150c5f3f15
dest="$1"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/windivert.zip" \
  "https://github.com/basil00/WinDivert/releases/download/v$version/WinDivert-$version-A.zip"
echo "$sha256  $work/windivert.zip" | sha256sum -c -
unzip -q "$work/windivert.zip" -d "$work"
src="$work/WinDivert-$version-A"
mkdir -p "$dest"
cp "$src/x64/WinDivert.dll" "$src/x64/WinDivert64.sys" "$dest/"
cp "$src/LICENSE" "$dest/WinDivert-LICENSE.txt"
