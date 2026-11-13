#!/usr/bin/env bash
# Signs a macOS binary or .app with RbxNative's own code-signing
# certificate (self-signed, stored as the MACOS_SIGN_P12 /
# MACOS_SIGN_PASSWORD repo secrets). Every build carrying the same identity
# is what lets Keychain's "Always Allow" and the Accessibility permission
# stick from one build to the next; an ad-hoc signature is a new identity
# each time. Gatekeeper still asks on first open (no Apple certificate).
#
# Without the secrets (a fork's CI) it falls back to an ad-hoc signature.
#
# Usage: macos-sign.sh <binary or .app>
set -euo pipefail

target="$1"
identifier="io.github.chteau.rbx-native.Editor"

if [ -z "${MACOS_SIGN_P12:-}" ] || [ -z "${MACOS_SIGN_PASSWORD:-}" ]; then
  echo "::warning::No MACOS_SIGN_P12 secret: ad-hoc signature, Keychain will ask again after every build."
  codesign --force --deep --sign - --identifier "$identifier" "$target"
  codesign -dv "$target" 2>&1 | grep -E "Identifier|Signature"
  exit 0
fi

work="$(mktemp -d)"
keychain="$work/rbxnative.keychain-db"
keychain_password="$(openssl rand -base64 24)"
trap 'security delete-keychain "$keychain" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

printf '%s' "$MACOS_SIGN_P12" | base64 --decode > "$work/sign.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/sign.p12" -k "$keychain" -P "$MACOS_SIGN_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

# Sign by the certificate's hash: a self-signed certificate isn't trusted,
# so it never shows up as a "valid" identity by name.
hash="$(security find-identity -p codesigning "$keychain" | awk '/RbxNative Code Signing/ { print $2; exit }')"
if [ -z "$hash" ]; then
  echo "::error::The signing certificate didn't import."
  security find-identity "$keychain"
  exit 1
fi
codesign --force --deep --sign "$hash" --keychain "$keychain" --identifier "$identifier" --timestamp=none "$target"
codesign -dvv "$target" 2>&1 | grep -E "Identifier|Authority|Signature"
