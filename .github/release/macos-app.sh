#!/usr/bin/env bash
# Builds RbxNative.app around a release binary and signs it (macos-sign.sh).
# Used by package.sh for releases and by the test-build workflow, so both
# ship the same bundle; macos-dmg.sh then wraps it.
#
# Usage: macos-app.sh <binary> <version> <path/RbxNative.app>
set -euo pipefail

bin="$1"
version="$2"
app="$3"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/RbxNative"
# Made by make-icns.sh from the Flatpak's app SVG.
cp .github/release/RbxNative.icns "$app/Contents/Resources/RbxNative.icns"
chmod 755 "$app/Contents/MacOS/RbxNative"
sed "s/@VERSION@/$version/g" .github/release/Info.plist > "$app/Contents/Info.plist"
# The disk image holds only the app, so its credits and licence notices
# travel inside it. Run from the repository root with
# THIRD-PARTY-LICENSES.html already made (cargo about, as release.yml does).
cp LICENSE "$app/Contents/Resources/LICENSE.txt"
cp THIRD-PARTY-LICENSES.html .github/release/THIRD-PARTY-EMBEDDED.txt "$app/Contents/Resources/"
.github/release/notices.sh "$app/Contents/Resources"
# RbxNative's own certificate (ad-hoc without the secrets): the same
# identity every release keeps Keychain and Accessibility grants.
.github/release/macos-sign.sh "$app"
