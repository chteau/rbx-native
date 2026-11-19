#!/usr/bin/env bash
# The #announcements post for a minor or major release: a Release ping, what's
# new (announce/<tag>/post.md, or the changelog headlines), the screenshots in
# announce/<tag>/, and a link to the release's #releases post.
#
#   announce.sh release.json <link to the #releases post>      # posts to $WEBHOOK
#   announce.sh --dry-run release.json <link>                  # prints it instead
#
# release.json is `gh release view --json name,tagName,url,body`. Needs $ROLE.
set -euo pipefail

dry=false
if [[ ${1:-} == --dry-run ]]; then dry=true; shift; fi
release=$1 link=$2
here=$(cd "$(dirname "$0")" && pwd)
tag=$(jq -r .tagName "$release")
dir="$here/announce/$tag"

if [[ -f $dir/post.md ]]; then
  text=$(<"$dir/post.md")
else
  # "- **Headline.** The rest" -> "- Headline."
  text=$(jq -r '.body // ""' "$release" | sed -n 's/^[-*] \*\*\([^*]*\)\*\*.*/- \1/p' | head -n 8)
  [[ -n $text ]] && text=$'What\'s new:\n'"$text"
fi

title=$(jq -r '.name // .tagName' "$release")
content="<@&$ROLE>"$'\n'"## $title is out"$'\n\n'"$text"
footer=$'\n\n'"Full release notes and downloads: $link"$'\n'"Or update from inside RbxNative."
# Discord's limit is 2000 characters; cut the text, never the link.
max=$(( 2000 - ${#footer} ))
if (( ${#content} > max )); then content="${content:0:max-1}…"; fi
content+=$footer

shopt -s nullglob nocaseglob
images=()
for f in "$dir"/*.{png,jpg,jpeg,gif,webp}; do images+=("$f"); done
shopt -u nocaseglob
mapfile -t images < <(printf '%s\n' "${images[@]}" | sort | head -n 10)
[[ ${#images[@]} -eq 1 && -z ${images[0]} ]] && images=()

payload=$(jq -n --arg content "$content" --arg role "$ROLE" '{
  username: "RbxNative",
  content: $content,
  allowed_mentions: {roles: [$role]},
  attachments: [range($ARGS.positional | length) as $i | {id: $i, filename: $ARGS.positional[$i]}]
}' --args "${images[@]##*/}")

if $dry; then
  echo "$payload" | jq -r .content
  (( ${#images[@]} )) && printf 'image: %s\n' "${images[@]##*/}"
  exit 0
fi

form=(--form-string "payload_json=$payload")
for i in "${!images[@]}"; do form+=(-F "files[$i]=@${images[$i]}"); done
curl -fsS "${form[@]}" "$WEBHOOK?wait=true" -o /dev/null
