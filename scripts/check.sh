#!/usr/bin/env bash
# Full quality gate: formatting, lints (warnings fatal), tests. Heavy work runs
# at idle IO priority so a wgpu rebuild never freezes the desktop.
set -euo pipefail
cd "$(dirname "$0")/.."
run() { ionice -c3 nice -n 10 "$@"; }
run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo test --workspace "$@" 2>&1 | tee /tmp/rbx-native-test.log | grep -aE '^(test result|running|error|warning: unused)' 
echo "TOTAL PASSED: $(grep -aoE '^test result: ok\. [0-9]+ passed' /tmp/rbx-native-test.log | awk '{s+=$4} END {print s}')"
