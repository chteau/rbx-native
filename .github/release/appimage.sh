#!/usr/bin/env bash
# Builds RbxNative-<version>-x86_64.AppImage from a staged Linux package
# folder (package.sh's): the program, its desktop entry, icon and MIME
# types. It links the same system libraries as the tarball and bundles
# none. The updater replaces the AppImage file itself (`updater/install.rs`).
#
# appimagetool and the runtime it embeds are pinned by hash.
#
# Usage: appimage.sh <version> <staged package folder> <out .AppImage>
set -euo pipefail

version="$1"
stage="$2"
out="$3"

tool_version=1.9.1
tool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
runtime_version=20251108
runtime_sha256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
id=io.github.chteau.rbx_native.Editor

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/appimagetool" \
  "https://github.com/AppImage/appimagetool/releases/download/$tool_version/appimagetool-x86_64.AppImage"
curl -fsSL -o "$work/runtime" \
  "https://github.com/AppImage/type2-runtime/releases/download/$runtime_version/runtime-x86_64"
printf '%s  %s\n' "$tool_sha256" "$work/appimagetool" "$runtime_sha256" "$work/runtime" | sha256sum -c -
chmod +x "$work/appimagetool"

appdir="$work/RbxNative.AppDir"
mkdir -p "$appdir/usr/bin"
cp "$stage/rbxnative" "$appdir/usr/bin/rbxnative"
cp -r "$stage/share" "$appdir/usr/share"
cp "$stage"/LICENSE.txt "$stage"/THIRD-PARTY-* "$appdir/usr/share/"
ln -s usr/bin/rbxnative "$appdir/AppRun"
cp "$appdir/usr/share/applications/$id.desktop" "$appdir/$id.desktop"
cp "$appdir/usr/share/icons/hicolor/scalable/apps/$id.svg" "$appdir/$id.svg"
ln -s "$id.svg" "$appdir/.DirIcon"

# No FUSE on CI runners: the tool runs from its own extracted copy.
ARCH=x86_64 VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 \
  "$work/appimagetool" --no-appstream --runtime-file "$work/runtime" "$appdir" "$out"
chmod 755 "$out"
