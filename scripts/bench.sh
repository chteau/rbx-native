#!/usr/bin/env bash
# Times scene reload and rendering on this machine's real GPU and writes both a
# table and target/bench/bench.json. Every argument is forwarded to the harness
# (`--help` lists them); the fixture pair below is only the default.
#
# Deliberately NOT called from check.sh: this needs a GPU, takes minutes, and
# reports a number rather than a pass or a fail — none of which belongs in a
# gate. BENCHMARKS.md at the repo root holds the recorded baseline.
#
# Fixtures live outside the repo in $RBX_FIXTURES (default:
# ../rbx-native-fixtures/places), the same variable scripts/shots.sh reads; a
# missing one is reported as skipped rather than failing the run.
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="target/bench"
FIX="${RBX_FIXTURES:-../rbx-native-fixtures/places}"
mkdir -p "$OUT"
# `ionice` is Linux-only; fall back to plain `nice` rather than failing outright
# anywhere else (macOS, in particular) — see check.sh's own copy of this same
# fallback, and bench.ps1 for the native Windows equivalent.
if command -v ionice >/dev/null 2>&1; then
  ionice -c3 nice -n 10 cargo build --release --example bench -p rbx_viewer 2>&1 | grep -E '^(error|warning)' || true
else
  nice -n 10 cargo build --release --example bench -p rbx_viewer 2>&1 | grep -E '^(error|warning)' || true
fi
# Built and run at normal priority on purpose: `nice`ing the measurement itself
# would time a process the scheduler is deprioritising, not the code.
exec ./target/release/examples/bench \
  --fixture assets/tests/TestPlace.rbxl \
  --fixture "$FIX/marked.rbxl" \
  --json "$OUT/bench.json" \
  "$@"
