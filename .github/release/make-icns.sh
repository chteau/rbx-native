#!/usr/bin/env bash
# Builds RbxNative.icns, the macOS bundle icon, from the app SVG with resvg
# (`cargo install resvg`): every size macOS asks for, PNG-encoded, with the
# margin Apple's icon grid leaves (824 of 1024) so it sits right in the Dock.
#
# Usage: make-icns.sh <app.svg> <out.icns>
#   .github/release/make-icns.sh packaging/flatpak/io.github.chteau.rbx_native.Editor.svg .github/release/RbxNative.icns
set -euo pipefail

svg="$1"
out="$2"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cp "$svg" "$work/app.svg"
cat > "$work/framed.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1024 1024">
  <image x="100" y="100" width="824" height="824" xlink:href="app.svg"/>
</svg>
SVG
for px in 16 32 64 128 256 512 1024; do
  resvg --width "$px" --height "$px" --resources-dir "$work" "$work/framed.svg" "$work/$px.png"
done

# icns: "icns", total length, then one (type, length, PNG) record per size.
python3 - "$work" "$out" <<'PY'
import struct, sys
work, out = sys.argv[1], sys.argv[2]
types = [("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128), ("ic08", 256),
         ("ic09", 512), ("ic10", 1024), ("ic11", 32), ("ic12", 64), ("ic13", 256),
         ("ic14", 512)]
body = b""
for kind, px in types:
    png = open(f"{work}/{px}.png", "rb").read()
    body += kind.encode() + struct.pack(">I", 8 + len(png)) + png
open(out, "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY
