# The Discord #releases post for one release, from
# `gh release view --json name,tagName,url,body,publishedAt,isPrerelease,assets`.
# discord-release.yml runs it; run it on a saved release.json to preview.
#
# Discord's embed limits: description 4096, a field's value 1024, the whole
# embed 6000 characters. The changelog gets whatever the rest leaves and is
# cut at a line, with a link to the full notes.
#
# Arguments: --arg role <role id> --arg repo <https://github.com/owner/repo>
#            --arg flathub <Flathub page, or "" while it isn't there>

def link($label; $asset): if $asset then "[\($label)](\($asset.url))" else empty end;

# A list item line.
def item: test("^([-*] |[0-9]+\\. )");

# The notes as blocks: a heading, a list item or a paragraph, with its
# wrapped lines joined (Discord keeps every line break the file has).
def blocks:
  reduce split("\n")[] as $line ([];
    if ($line | test("^\\s*$")) then . + [""]
    elif ($line | test("^#") or item) or length == 0 or .[-1] == "" then . + [$line]
    else .[-1] += " " + ($line | sub("^\\s+"; ""))
    end)
  | map(select(. != ""));

# Back to text: list items one per line, a blank line around anything else.
def render:
  reduce .[] as $b ({text: "", item: false};
    {text: (if .text == "" then $b elif .item and ($b | item) then .text + "\n" + $b else .text + "\n\n" + $b end),
     item: ($b | item)})
  | .text;

# As many whole blocks of $text as fit in $max with $more after them.
def fit($text; $max; $more):
  ($text | blocks) as $all
  | ($all | render) as $whole
  | if ($whole | length) <= $max then $whole
    else
      (reduce $all[] as $b ({kept: [], done: false};
        if .done then .
        elif ((.kept + [$b]) | render | length) + ($more | length) > $max then .done = true
        else .kept += [$b]
        end) | .kept) as $kept
      # A heading with nothing under it goes too.
      | ($kept | until(length == 0 or (last | startswith("#") | not); .[:-1])) as $kept
      | (if ($kept | length) == 0 then $whole[0:($max - ($more | length))] else ($kept | render) end) + $more
    end;

def asset($pattern): [.assets[]? | select(.name | test($pattern))] | first;

(.body // "" | gsub("\r"; "") | sub("^\\s+"; "") | sub("\\s+$"; "")) as $body
| asset("^RbxNative-.*-windows-x64-setup\\.exe$") as $setup
| asset("^RbxNative-.*-x86_64-pc-windows-msvc\\.zip$") as $winzip
| (asset("^RbxNative-.*-macos-arm64\\.dmg$") // null) as $dmg
| (if $dmg then null else asset("^RbxNative-.*-aarch64-apple-darwin\\.tar\\.gz$") end) as $mactar
| asset("^RbxNative-.*-x86_64\\.AppImage$") as $appimage
| (asset("^RbxNative-.*-x86_64-unknown-linux-gnu\\.tar\\.gz$")
   // asset("^rbxstudio-.*-x86_64-unknown-linux-gnu\\.tar\\.gz$")) as $linuxtar
| ([
    ([link("Installer (.exe)"; $setup), link("Portable .zip"; $winzip)] | select(length > 0)
      | "**Windows** (x64): " + join(" · ")),
    ([link(".dmg"; $dmg), link(".tar.gz"; $mactar)] | select(length > 0)
      | "**macOS** (Apple Silicon): " + join(" · ")),
    ([link("AppImage"; $appimage), link(".tar.gz"; $linuxtar),
      (if $flathub != "" then "[Flathub](\($flathub))" else empty end)] | select(length > 0)
      | "**Linux** (x86_64): " + join(" · "))
  ] | join("\n")) as $downloads
| ([
    (if $downloads != "" then {name: "Download", value: $downloads} else empty end),
    {name: "Install from a terminal",
     value: "macOS and Linux:\n```sh\ncurl -fsSL https://rbxnative.chteau.bzh/install.sh | sh\n```\nWindows (PowerShell):\n```powershell\nirm https://rbxnative.chteau.bzh/install.ps1 | iex\n```"}
  ]) as $fields
| ((.name // .tagName) + (if .isPrerelease then " (pre-release)" else "" end)) as $title
| (($title | length) + (.tagName | length)
   + ([$fields[] | (.name | length) + (.value | length)] | add)) as $rest
| ([4096, 6000 - $rest - 40] | min) as $room
| (if $body == "" then "New release." else fit($body; $room; "\n\n[Full changelog](\(.url))") end) as $notes
| {
    username: "RbxNative",
    content: (if .isPrerelease then null else "<@&\($role)>" end),
    allowed_mentions: {roles: (if .isPrerelease then [] else [$role] end)},
    embeds: [{
      color: 7110619,
      title: $title,
      url: .url,
      description: $notes,
      fields: $fields,
      timestamp: .publishedAt,
      footer: {text: .tagName}
    }],
    components: [{type: 1, components: [
      {type: 2, style: 5, label: "Release notes", emoji: {name: "📦"}, url: .url},
      {type: 2, style: 5, label: "Changelog", emoji: {name: "📝"}, url: "\($repo)/blob/dev/CHANGELOG.md"},
      {type: 2, style: 5, label: "Install guide", emoji: {name: "💻"}, url: "https://rbxnative.chteau.bzh/docs/install.html"},
      {type: 2, style: 5, label: "Docs", emoji: {name: "📖"}, url: "https://rbxnative.chteau.bzh/docs/"}
    ]}]
  }
