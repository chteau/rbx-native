#!/usr/bin/env bash
# Packs one release build: rbxstudio-<version>-<target>.zip on Windows,
# .tar.gz elsewhere, one top folder holding the program (an ad-hoc signed
# RbxNative.app on macOS), the README and the license notices. The updater
# looks for exactly these names (`updater/release.rs`).
#
# Usage: package.sh <version> <target> <dir with the built binary> <out dir>
# Run from the repository root, with THIRD-PARTY-LICENSES.html already there.
set -euo pipefail

version="$1"
target="$2"
bin="$3"
mkdir -p "$4"
out="$(cd "$4" && pwd)"

name="rbxstudio-$version-$target"
work="$(mktemp -d)"
stage="$work/$name"
mkdir -p "$stage"

cp LICENSE "$stage/LICENSE.txt"
cp THIRD-PARTY-LICENSES.html .github/release/THIRD-PARTY-EMBEDDED.txt "$stage/"
{
  cat .github/release/README.txt
  echo
  echo "RbxNative v$version, $target"
} > "$stage/README.txt"

case "$target" in
  *windows*)
    cp "$bin/rbxstudio.exe" "$stage/"
    (cd "$work" && 7z a -tzip -bso0 "$out/$name.zip" "$name")
    ;;
  *apple-darwin)
    app="$stage/RbxNative.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$bin/rbxstudio" "$app/Contents/MacOS/rbxstudio"
    # Made by make-icns.sh from the Flatpak's app SVG.
    cp .github/release/RbxNative.icns "$app/Contents/Resources/RbxNative.icns"
    chmod 755 "$app/Contents/MacOS/rbxstudio"
    sed "s/@VERSION@/$version/g" .github/release/Info.plist > "$app/Contents/Info.plist"
    # No paid certificate: an ad-hoc signature seals the bundle so Apple
    # Silicon runs it; Gatekeeper still asks on first open.
    codesign --force --deep --sign - "$app"
    # Without this, macOS tar adds ._* AppleDouble files that break the seal.
    COPYFILE_DISABLE=1 tar -C "$work" -czf "$out/$name.tar.gz" "$name"
    ;;
  *)
    cp "$bin/rbxstudio" "$stage/"
    chmod 755 "$stage/rbxstudio"
    tar -C "$work" -czf "$out/$name.tar.gz" "$name"
    ;;
esac

rm -rf "$work"
ls -l "$out"
