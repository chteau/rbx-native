#!/usr/bin/env bash
# Local Run against live Roblox: the latest API dump against the one
# COVERAGE.md was generated from, what of the new or changed members Local
# Run lacks (by security level, measured by the coverage probe against the
# latest dump), and whether the linked Luau is behind live Roblox. Writes a
# markdown report (the watchdog issue's body) to the file given, or stdout.
#
#   scripts/local-run-watchdog.sh [report.md]
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-/dev/stdout}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# The same source as sync-api-dump.yml.
read -r studio upload < <(curl -fsS https://clientsettingscdn.roblox.com/v2/client-version/WindowsStudio64 |
  jq -r '"\(.version) \(.clientVersionUpload)"')
[[ $upload == version-* ]] || { echo "unexpected Studio version: $upload" >&2; exit 1; }
curl -fsS "https://setup.rbxcdn.com/${upload}-API-Dump.json" -o "$work/latest.json"

base=$(git log -1 --format=%H -- docs/research/local-run/COVERAGE.md)
git show "$base:assets/API-Dump.json" >"$work/base.json"

COVERAGE_DUMP="$work/latest.json" COVERAGE_MISSING="$work/missing.tsv" \
  cargo run -q -p rbx_sim --example coverage >"$work/totals.txt"

auth=()
[[ -n ${GITHUB_TOKEN:-} ]] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")
upstream=$(curl -fsS "${auth[@]}" https://api.github.com/repos/luau-lang/luau/releases/latest | jq -r .tag_name)
linked=0.$(sed -n 's/^version = ".*+luau\([0-9]*\)"/\1/p' vendor/luau0-src/Cargo.toml)
constant=$(sed -n 's/^pub const LUAU_VERSION: &str = "\(.*\)";/\1/p' crates/rbx_lua/src/luau.rs)

python3 scripts/local-run-watchdog.py "$work/base.json" "$work/latest.json" \
  "$work/missing.tsv" "$work/totals.txt" "$studio" "$upstream" "$linked" "$constant" >"$out"
