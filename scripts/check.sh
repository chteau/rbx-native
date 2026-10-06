#!/usr/bin/env bash
# Full quality gate: formatting, lints (warnings fatal), tests. Heavy work runs
# at idle IO priority so a wgpu rebuild never freezes the desktop. `ionice` is
# Linux-only (util-linux); macOS (and anywhere else lacking it) falls back to
# `nice` alone rather than failing outright — see scripts/check.ps1 for the
# native Windows equivalent, which has no direct analogue of either.
set -euo pipefail
cd "$(dirname "$0")/.."
run() {
  if command -v ionice >/dev/null 2>&1; then
    ionice -c3 nice -n 10 "$@"
  else
    nice -n 10 "$@"
  fi
}
run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets -- -D warnings

# A fixed path here would race: two of this script running concurrently (one
# per agent, on separate branches, is routine in this project — see
# agents/AGENTS.md's "Orchestration and subagents") would each overwrite the
# other's log between the `tee` and the final `grep`, corrupting the printed
# total for whichever one loses the race. `mktemp` gives each invocation its
# own file; `trap` cleans it up on any exit, not just the successful path.
log="$(mktemp /tmp/rbx-native-test.XXXXXX.log)"
trap 'rm -f "$log"' EXIT
run cargo test --workspace "$@" 2>&1 | tee "$log" | grep -aE '^(test result|running|error|warning: unused)'
echo "TOTAL PASSED: $(grep -aoE '^test result: ok\. [0-9]+ passed' "$log" | awk '{s+=$4} END {print s}')"
