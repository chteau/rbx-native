#!/usr/bin/env bash
# Packs one release build. Every target gets RbxNative-<version>-<target>,
# a .zip on Windows and a .tar.gz elsewhere, with one top folder holding the
# program (RbxNative.exe, a signed RbxNative.app, or rbxnative), the README
# and the license notices: that's what the updater installs from
# (`updater/release.rs`). On top of it:
#
# - Windows: RbxNative-<version>-windows-x64-setup.exe, a per-user Inno
#   Setup installer (packaging/windows/RbxNative.iss).
# - macOS: RbxNative-<version>-macos-arm64.dmg, the same app beside an
#   Applications link, signed like the app.
# - Linux: RbxNative-<version>-x86_64.AppImage (appimage.sh), and
#   rbxstudio-<version>-<target>.tar.gz, the program under its 0.1.0 name,
#   because that's the only package 0.1.0's updater can find. The editor
#   renames itself once it runs from it (`updater/compat.rs`).
#
# Usage: package.sh <version> <target> <dir with the built binary> <out dir>
# Run from the repository root, with THIRD-PARTY-LICENSES.html already there.
set -euo pipefail

version="$1"
target="$2"
bin="$3"
mkdir -p "$4"
out="$(cd "$4" && pwd)"

name="RbxNative-$version-$target"
work="$(mktemp -d)"
stage="$work/$name"
mkdir -p "$stage"

# The README and license notices every package carries, into folder $1.
docs() {
  cp LICENSE "$1/LICENSE.txt"
  cp THIRD-PARTY-LICENSES.html .github/release/THIRD-PARTY-EMBEDDED.txt "$1/"
  {
    cat .github/release/README.txt
    echo
    echo "RbxNative v$version, $target"
  } > "$1/README.txt"
}
docs "$stage"

flatpak_id=io.github.chteau.rbx_native.Editor

case "$target" in
  *windows*)
    cp "$bin/rbxstudio.exe" "$stage/RbxNative.exe"
    # Play's network simulator loads these from beside the editor.
    .github/release/windivert.sh "$stage"
    (cd "$work" && 7z a -tzip -bso0 "$out/$name.zip" "$name")

    iscc="$(command -v iscc || true)"
    [ -n "$iscc" ] || iscc="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
    if [ ! -x "$iscc" ]; then
      choco install innosetup -y --no-progress >/dev/null
    fi
    # Git Bash would rewrite the /D switches into paths.
    MSYS_NO_PATHCONV=1 "$iscc" /Q "/DVersion=$version" \
      "/DSource=$(cygpath -w "$stage")" "/DOutput=$(cygpath -w "$out")" \
      "/DRepo=$(cygpath -w "$PWD")" "$(cygpath -w packaging/windows/RbxNative.iss)"
    ;;
  *apple-darwin)
    app="$stage/RbxNative.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$bin/rbxstudio" "$app/Contents/MacOS/RbxNative"
    # Made by make-icns.sh from the Flatpak's app SVG.
    cp .github/release/RbxNative.icns "$app/Contents/Resources/RbxNative.icns"
    chmod 755 "$app/Contents/MacOS/RbxNative"
    sed "s/@VERSION@/$version/g" .github/release/Info.plist > "$app/Contents/Info.plist"
    # RbxNative's own certificate (ad-hoc without the secrets): the same
    # identity every release keeps Keychain and Accessibility grants.
    .github/release/macos-sign.sh "$app"
    # Without this, macOS tar adds ._* AppleDouble files that break the seal.
    COPYFILE_DISABLE=1 tar -C "$work" -czf "$out/$name.tar.gz" "$name"

    dmg_root="$work/dmg"
    mkdir "$dmg_root"
    ditto "$app" "$dmg_root/RbxNative.app"
    ln -s /Applications "$dmg_root/Applications"
    dmg="$out/RbxNative-$version-macos-arm64.dmg"
    # hdiutil on CI runners sometimes fails with "Resource busy"; a retry
    # gets through.
    for attempt in 1 2 3; do
      hdiutil create -volname "RbxNative" -srcfolder "$dmg_root" -fs HFS+ \
        -format UDZO -ov "$dmg" && break
      [ "$attempt" = 3 ] && exit 1
      sleep 5
    done
    .github/release/macos-sign.sh "$dmg"
    ;;
  *)
    cp "$bin/rbxstudio" "$stage/rbxnative"
    chmod 755 "$stage/rbxnative"
    # What the install script puts in the desktop's menus.
    install -Dm644 "packaging/flatpak/$flatpak_id.svg" "$stage/share/icons/hicolor/scalable/apps/$flatpak_id.svg"
    install -Dm644 "packaging/flatpak/$flatpak_id.mime.xml" "$stage/share/mime/packages/$flatpak_id.xml"
    mkdir -p "$stage/share/applications"
    sed -e 's/^Name=.*/Name=RbxNative/' -e 's/^Exec=.*/Exec=rbxnative %f/' \
      "packaging/flatpak/$flatpak_id.desktop" > "$stage/share/applications/$flatpak_id.desktop"
    tar -C "$work" -czf "$out/$name.tar.gz" "$name"

    legacy="rbxstudio-$version-$target"
    mkdir "$work/$legacy"
    docs "$work/$legacy"
    cp "$stage/rbxnative" "$work/$legacy/rbxstudio"
    tar -C "$work" -czf "$out/$legacy.tar.gz" "$legacy"

    .github/release/appimage.sh "$version" "$stage" "$out/RbxNative-$version-x86_64.AppImage"
    ;;
esac

rm -rf "$work"
ls -l "$out"
