#!/usr/bin/env bash
# Puts the "Wisp Release Signing" certificate on a GitHub macOS build machine
# so codesign can use it. macOS remembers privacy permissions (such as Full
# Disk Access) by an app's signature, so signing every release with the same
# certificate is what keeps those permissions across updates.
#
# Expects WISP_SIGNING_CERTIFICATE (the .p12, base64) and WISP_SIGNING_PASSWORD.
# Exports APPLE_SIGNING_IDENTITY for the steps after it, which Tauri signs with.
set -euo pipefail

identity="Wisp Release Signing"
here="$(cd "$(dirname "$0")" && pwd)"
keychain="$RUNNER_TEMP/wisp-signing.keychain-db"
keychain_password="$(openssl rand -hex 16)"
p12="$RUNNER_TEMP/wisp-signing.p12"

printf '%s' "$WISP_SIGNING_CERTIFICATE" | base64 --decode > "$p12"
trap 'rm -f "$p12"' EXIT

security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$WISP_SIGNING_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" > /dev/null
# Searched alongside the machine's own keychains.
existing=$(security list-keychains -d user | tr -d '"')
# shellcheck disable=SC2086
security list-keychains -d user -s "$keychain" $existing

# codesign only uses a certificate macOS trusts for code signing; a self-signed
# one has to be trusted explicitly. The certificate itself is public.
sudo security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain "$here/wisp-release-signing.pem"

if ! security find-identity -v -p codesigning "$keychain" | grep -q "$identity"; then
  echo "::error::The signing certificate was imported but codesign cannot use it."
  security find-identity -p codesigning "$keychain"
  exit 1
fi
echo "APPLE_SIGNING_IDENTITY=$identity" >> "$GITHUB_ENV"
echo "Ready to sign as \"$identity\"."
