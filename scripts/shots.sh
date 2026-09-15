#!/usr/bin/env bash
# Renders the reference fixtures from fixed cameras into $1 (default: ./shots)
# so two builds can be compared side by side. Fixtures live outside the repo in
# $RBX_FIXTURES (default: ../rbx-native-fixtures/places).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-shots}"; mkdir -p "$OUT"
FIX="${RBX_FIXTURES:-../rbx-native-fixtures/places}"
# `ionice` is Linux-only; fall back to plain `nice` rather than failing
# outright anywhere else (macOS, in particular) — see check.sh's own copy of
# this same fallback, and shots.ps1 for the native Windows equivalent.
if command -v ionice >/dev/null 2>&1; then
  ionice -c3 nice -n 10 cargo build --release -p rbx_viewer 2>&1 | grep -E '^(error|warning)' || true
else
  nice -n 10 cargo build --release -p rbx_viewer 2>&1 | grep -E '^(error|warning)' || true
fi
V=target/release/rbxview
shot() { local name=$1; shift; echo "-- $name"; $V "$@" --screenshot "$OUT/$name.png" --size 960x540 || echo "FAILED: $name"; }
shot testplace_spawn   assets/tests/TestPlace.rbxl --eye 14,8,14 --look-at 0,1,0
shot testplace_wide    assets/tests/TestPlace.rbxl --eye 60,30,60 --look-at 0,0,0
shot testrust_overview "$FIX/testrust_17675488706.rbxl"
shot marked_overview   "$FIX/marked.rbxl"
shot marked_ground     "$FIX/marked.rbxl" --eye 40,12,40 --look-at 0,4,0
# Same camera as the user's Studio capture of this place (CFrame position and
# orientation converted to eye/look-at), so the two can be compared directly.
shot demo_global       "$FIX/DEMO_LIGHTING_MATERIALS.rbxl" --eye -121.423,54.07,-126.14 --look-at -64.1,7.5,-58.7
shot demo_night        "$FIX/DEMO_LIGHTING_MATERIALS.rbxl" --eye -121.423,54.07,-126.14 --look-at -64.1,7.5,-58.7 --clock-time 0
