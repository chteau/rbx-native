#!/usr/bin/env bash
# Publishes one or more images to a secret GitHub gist and prints ready-to-paste
# Markdown links (each file's raw-content URL) for a PR description or review
# comment. There's no scriptable equivalent of the web UI's drag-and-drop image
# upload otherwise, and this project's PR template requires a screenshot for
# anything with a visible effect.
#
# `gh gist create` itself only accepts text files (its underlying API takes
# gist content as a JSON string) — it refuses anything it sniffs as binary.
# A gist is a real git repository underneath, though, so the actual trick is:
# create a placeholder gist to get one, clone it, `git add`/push the image(s)
# as ordinary binary blobs (which plain git handles natively), then read back
# each file's raw_url.
#
# "Secret" (gh gist create's own default, kept here) means unlisted, not
# private: anyone who gets the link can view it. Fine for an ordinary viewport
# screenshot; don't publish anything sensitive this way.
#
# Needs only `gh` and `git` (no separate `jq` — `gh api --jq` has its own
# built in). See publish-screenshot.ps1 for the native Windows equivalent.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: $(basename "$0") <image> [image...]" >&2
  echo "  publishes each image via a secret gist, prints a Markdown image link" >&2
  echo "  (that file's raw-content URL) for each one" >&2
  exit 2
fi

for image in "$@"; do
  if [ ! -f "$image" ]; then
    echo "not a file: $image" >&2
    exit 1
  fi
done

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
placeholder="$work_dir/README.md"
echo "rbx-native PR screenshot(s), published $stamp — see the other file(s) in this gist." \
  >"$placeholder"

gist_url=$(gh gist create --desc "rbx-native PR screenshot(s) — $stamp" "$placeholder")
gist_id=${gist_url##*/}

clone_dir="$work_dir/clone"
gh gist clone "$gist_id" "$clone_dir" >/dev/null

for image in "$@"; do
  cp "$image" "$clone_dir/$(basename "$image")"
done

git -C "$clone_dir" add -A
git -C "$clone_dir" commit -q -m "Add screenshot(s)"
git -C "$clone_dir" push -q origin HEAD

echo "Gist: $gist_url"
echo
for image in "$@"; do
  name=$(basename "$image")
  # `gh api --jq` takes exactly one filter argument, so the filename is
  # interpolated straight into it rather than passed as a jq --arg (gh
  # doesn't forward extra jq flags) — fine for the plain filenames this
  # script deals with, which never contain a `"` or `\`.
  raw_url=$(gh api "gists/$gist_id" --jq ".files[\"$name\"].raw_url")
  echo "![${name}](${raw_url})"
done
