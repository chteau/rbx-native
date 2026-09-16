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
run() {
  if command -v ionice >/dev/null 2>&1; then
    ionice -c3 nice -n 10 "$@"
  else
    nice -n 10 "$@"
  fi
}
# The build's own status decides, and the filtering happens afterwards over its
# log. Piping it straight into `grep` cannot: `grep` matches nothing in a clean
# build and exits 1, and the `|| true` needed to survive that reports the status
# of `true` for the whole pipeline, `pipefail` and all. A failed build would then
# fall through to whichever binary the last successful one left in target/, and
# those stale numbers would be printed and written to JSON under the *current*
# commit. bench.ps1 checks $LASTEXITCODE for the same reason.
LOG="$OUT/build.log"
if ! run cargo build --release --example bench -p rbx_viewer >"$LOG" 2>&1; then
  cat "$LOG" >&2
  echo "error: the benchmark did not build; target/ may still hold an older binary" >&2
  exit 1
fi
grep -E '^(error|warning)' "$LOG" || true
# Built and run at normal priority on purpose: `nice`ing the measurement itself
# would time a process the scheduler is deprioritising, not the code.
exec ./target/release/examples/bench \
  --fixture assets/tests/TestPlace.rbxl \
  --fixture "$FIX/marked.rbxl" \
  --json "$OUT/bench.json" \
  "$@"
