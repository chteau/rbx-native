#!/usr/bin/env bash
# Re-records the conformance goldens on the sandbox place, then prints which
# fields moved against the committed goldens: Roblox drifting (physics,
# signal order, timing) shows up here before a game notices.
#
# Needs what record_goldens does: a Roblox client it can launch (Sober on
# Linux), cloudflared, and an Open Cloud key that may publish the sandbox
# place (the keyring's, or RBX_API_KEY). Uploads go to the sandbox place only.
#
#   scripts/rerecord-goldens.sh [--dry-run] [--only A,B] [--base place.rbxl] [--passes N]
#
# --dry-run builds everything and resolves the universe but neither publishes
# nor joins; the drift summary then runs over the goldens as they are.
# SANDBOX_LOCK=<file> holds that flock while recording. CLOUDFLARED=<path>
# when cloudflared isn't on PATH.
set -euo pipefail
cd "$(dirname "$0")/.."

place=3086963177
dry=0 only="" base=bare passes=""
while (($#)); do
  case $1 in
    --dry-run) dry=1 ;;
    --only) only=$2; shift ;;
    --base) base=$2; shift ;;
    --passes) passes=$2; shift ;;
    *) echo "usage: $0 [--dry-run] [--only A,B] [--base place.rbxl] [--passes N]" >&2; exit 2 ;;
  esac
  shift
done

goldens=crates/rbx_sim/conformance/goldens
committed=$(mktemp -d)
trap 'rm -rf "$committed"' EXIT
git archive HEAD "$goldens" | tar -x -C "$committed"
committed="$committed/$goldens"

universe=$(curl -fsS "https://apis.roblox.com/universes/v1/places/$place/universe" | jq -r .universeId)
[[ $universe =~ ^[0-9]+$ ]] || { echo "no universe for place $place" >&2; exit 1; }
cloudflared=${CLOUDFLARED:-$(command -v cloudflared || true)}
[[ -n $cloudflared ]] || { echo "cloudflared not found: set CLOUDFLARED" >&2; exit 1; }

record=(cargo run -q -p rbx_sim --example record_goldens --
  "$universe" "$place" "$base" "$cloudflared")
[[ -n $only ]] && record+=(--only "$only")
[[ -n $passes ]] && record+=(--passes "$passes")
cargo build -q -p rbx_sim --example record_goldens --example conformance

if ((dry)); then
  echo "dry run: would record with: ${record[*]}"
else
  lock=()
  [[ -n ${SANDBOX_LOCK:-} ]] && lock=(flock "$SANDBOX_LOCK")
  "${lock[@]}" "${record[@]}"
fi

drift=(cargo run -q -p rbx_sim --example conformance -- --drift "$committed")
[[ -n $only ]] && drift+=(--only "$only")
"${drift[@]}"
git status --short -- "$goldens"
