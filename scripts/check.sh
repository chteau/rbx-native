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
run cargo test --workspace "$@" 2>&1 | tee /tmp/rbx-native-test.log | grep -aE '^(test result|running|error|warning: unused)' 
echo "TOTAL PASSED: $(grep -aoE '^test result: ok\. [0-9]+ passed' /tmp/rbx-native-test.log | awk '{s+=$4} END {print s}')"
