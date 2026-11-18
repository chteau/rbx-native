#!/usr/bin/env bash
# The disk image RbxNative is installed from: the app (macos-app.sh) beside
# an Applications link, signed like the app.
#
# Usage: macos-dmg.sh <path/RbxNative.app> <out.dmg>
set -euo pipefail

app="$1"
dmg="$2"

root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
ditto "$app" "$root/RbxNative.app"
ln -s /Applications "$root/Applications"
# hdiutil on CI runners sometimes fails with "Resource busy"; a retry
# gets through.
for attempt in 1 2 3; do
  hdiutil create -volname "RbxNative" -srcfolder "$root" -fs HFS+ \
    -format UDZO -ov "$dmg" && break
  [ "$attempt" = 3 ] && exit 1
  sleep 5
done
.github/release/macos-sign.sh "$dmg"
