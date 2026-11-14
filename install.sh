#!/bin/sh
# RbxNative installer for Linux (x86_64) and macOS (Apple Silicon).
#
#   curl -fsSL https://rbxnative.chteau.bzh/install.sh | sh
#   curl -fsSL https://rbxnative.chteau.bzh/install.sh | sh -s -- --version 0.2.0
#   curl -fsSL https://rbxnative.chteau.bzh/install.sh | sh -s -- --uninstall
#
# Installs for the current user only, no sudo:
#   Linux: ~/.local/share/rbxnative, ~/.local/bin/rbxnative, plus a menu
#          entry, icon and the .rbxl/.rbxlx file types.
#   macOS: ~/Applications/RbxNative.app
# The download is checked against the release's SHA256SUMS, and that file
# against its minisign signature when `minisign` is installed. RbxNative
# keeps itself up to date from there. --uninstall removes only what this
# script added: settings, places, API keys and the keyring stay.
set -eu

REPO="chteau/rbx-native"
# The release signing key, the same one the editor's updater trusts
# (crates/rbx_studio/src/updater/verify.rs).
PUBLIC_KEY="RWTH57MZqQ9zSpsJv14IQQqWjzpg9EdLyKUQ610z7gC7zShLMJb5kuJ+"
ID="io.github.chteau.rbx_native.Editor"

say() { printf '%s\n' "$*"; }
fail() { printf 'rbxnative install: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
Usage: install.sh [--version X.Y.Z] [--uninstall]

  --version X.Y.Z  install that release instead of the latest one
  --uninstall      remove what this script installed (keeps your settings)
USAGE
}

version=""
action=install
while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a version, like 0.2.0"; version="${2#v}"; shift 2 ;;
    --version=*) version="${1#--version=}"; version="${version#v}"; shift ;;
    --uninstall) action=uninstall; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; fail "unknown option: $1" ;;
  esac
done

[ -n "${HOME:-}" ] || fail "HOME is not set"
[ "$(id -u)" != 0 ] || say "Note: running as root installs for root only. Run it as yourself, no sudo needed."

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) fail "this script is for Linux and macOS. On Windows run: irm https://rbxnative.chteau.bzh/install.ps1 | iex" ;;
esac

data="${XDG_DATA_HOME:-$HOME/.local/share}"
linux_dir="$data/rbxnative"
linux_bin="$HOME/.local/bin"
mac_app="$HOME/Applications/RbxNative.app"

refresh_desktop() {
  if command -v update-mime-database >/dev/null 2>&1; then update-mime-database "$data/mime" >/dev/null 2>&1 || true; fi
  if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$data/applications" >/dev/null 2>&1 || true; fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t "$data/icons/hicolor" >/dev/null 2>&1 || true; fi
}

uninstall() {
  if [ "$os" = macos ]; then
    if [ -d "$mac_app" ]; then rm -rf "$mac_app"; say "Removed $mac_app"; else say "RbxNative isn't in ~/Applications."; fi
    exit 0
  fi
  removed=0
  link="$linux_bin/rbxnative"
  if [ -L "$link" ]; then
    case "$(readlink "$link")" in "$linux_dir"/*) rm -f "$link"; say "Removed $link"; removed=1 ;; esac
  fi
  for file in "$data/applications/$ID.desktop" "$data/icons/hicolor/scalable/apps/$ID.svg" "$data/mime/packages/$ID.xml"; do
    if [ -e "$file" ]; then rm -f "$file"; say "Removed $file"; removed=1; fi
  done
  if [ -d "$linux_dir" ]; then rm -rf "$linux_dir"; say "Removed $linux_dir"; removed=1; fi
  if [ "$removed" = 1 ]; then refresh_desktop; say "RbxNative is uninstalled. Your settings and places are still there."; else say "RbxNative isn't installed here."; fi
  exit 0
}

[ "$action" = uninstall ] && uninstall

if [ "$os" = linux ]; then
  [ "$(uname -m)" = x86_64 ] || fail "RbxNative builds for x86_64 Linux only (this is $(uname -m))."
  target=x86_64-unknown-linux-gnu
else
  [ "$(uname -m)" = arm64 ] || fail "RbxNative builds for Apple Silicon Macs only (this is $(uname -m))."
  target=aarch64-apple-darwin
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  fail "needs curl or wget"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "needs sha256sum or shasum"
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/rbxnative-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM

# RBX_UPDATE_API points at a local mock, as it does for the updater.
api="${RBX_UPDATE_API:-https://api.github.com}/repos/$REPO/releases"
if [ -n "$version" ]; then api="$api/tags/v$version"; else api="$api/latest"; fi
fetch "$api" "$tmp/release.json" || fail "couldn't read the release from GitHub ($api)"
tag="$(sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' "$tmp/release.json" | head -n 1)"
[ -n "$tag" ] || fail "GitHub answered without a release tag"
version="${tag#v}"

# The download URL GitHub lists for asset $1, empty when there's none.
asset_url() {
  tr ',' '\n' < "$tmp/release.json" | sed -n "s|.*\"browser_download_url\": *\"\([^\"]*/$1\)\".*|\1|p" | head -n 1
}
has_asset() { [ -n "$(asset_url "$1")" ]; }
asset="RbxNative-$version-$target.tar.gz"
exe=rbxnative
[ "$os" = macos ] && exe=RbxNative
if ! has_asset "$asset"; then
  # 0.1.0 was Linux only, under the old name.
  asset="rbxstudio-$version-$target.tar.gz"
  exe=rbxstudio
  has_asset "$asset" || fail "release $tag has nothing for $target"
fi

download() {
  url="$(asset_url "$1")"
  [ -n "$url" ] || fail "release $tag has no $1"
  fetch "$url" "$tmp/$1" || fail "download failed: $url"
}
say "Downloading RbxNative $version ($asset)"
download "$asset"
download SHA256SUMS

if command -v minisign >/dev/null 2>&1; then
  download SHA256SUMS.minisig
  minisign -Vq -P "$PUBLIC_KEY" -m "$tmp/SHA256SUMS" -x "$tmp/SHA256SUMS.minisig" \
    || fail "SHA256SUMS is not signed by the RbxNative release key. Nothing was installed."
  say "Signature OK (minisign)"
else
  say "minisign isn't installed, so only the checksum is checked."
fi
expected="$(awk -v name="$asset" '{ file = $2; sub(/^\*/, "", file) } file == name { print $1; exit }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS has no line for $asset"
[ "$(sha256 "$tmp/$asset")" = "$expected" ] || fail "$asset doesn't match its checksum. Nothing was installed."
say "Checksum OK"

mkdir "$tmp/unpacked"
tar -xzf "$tmp/$asset" -C "$tmp/unpacked"
package="$(find "$tmp/unpacked" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[ -n "$package" ] || fail "the package is empty"

# Swaps folder $2 in for $1, keeping the old one until the new one is there.
replace_dir() {
  rm -rf "$1.new" "$1.old"
  mv "$2" "$1.new"
  if [ -e "$1" ]; then mv "$1" "$1.old"; fi
  mv "$1.new" "$1"
  rm -rf "$1.old"
}

if [ "$os" = macos ]; then
  [ -d "$package/RbxNative.app" ] || fail "the package has no RbxNative.app"
  mkdir -p "$HOME/Applications"
  replace_dir "$mac_app" "$package/RbxNative.app"
  # Downloaded by curl, so normally not quarantined, but a copy fetched
  # some other way would be, and Gatekeeper would stop the first launch.
  xattr -dr com.apple.quarantine "$mac_app" 2>/dev/null || true
  say ""
  say "RbxNative $version is in ~/Applications. Open it from Launchpad or Spotlight,"
  say "or run: open \"$mac_app\""
  exit 0
fi

mkdir -p "$data" "$linux_bin"
replace_dir "$linux_dir" "$package"
chmod 755 "$linux_dir/$exe"
ln -sfn "$linux_dir/$exe" "$linux_bin/rbxnative"

if [ -d "$linux_dir/share" ]; then
  mkdir -p "$data/applications" "$data/icons/hicolor/scalable/apps" "$data/mime/packages"
  cp "$linux_dir/share/icons/hicolor/scalable/apps/$ID.svg" "$data/icons/hicolor/scalable/apps/$ID.svg"
  cp "$linux_dir/share/mime/packages/$ID.xml" "$data/mime/packages/$ID.xml"
  # A desktop entry's Exec needs the full path, quoted.
  sed "s|^Exec=.*|Exec=\"$linux_dir/$exe\" %f|" "$linux_dir/share/applications/$ID.desktop" > "$data/applications/$ID.desktop"
  refresh_desktop
fi

say ""
if [ -d "$linux_dir/share" ]; then
  say "RbxNative $version is installed. It's in your app menu, or run: rbxnative"
else
  say "RbxNative $version is installed. Run: rbxnative"
fi
case ":${PATH:-}:" in
  *":$linux_bin:"*) ;;
  *) say "$linux_bin isn't in your PATH. Add it, or run $linux_bin/rbxnative" ;;
esac
