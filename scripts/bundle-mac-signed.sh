#!/bin/bash
# Builds html2wp.app and the DMG signed with Developer ID and notarized by Apple,
# then checks them the way Gatekeeper will on a customer's Mac.
#
#   TAURI_SIGNING_PRIVATE_KEY=... scripts/bundle-mac-signed.sh
#
# Extra arguments go to `tauri build` (for a test build without updater
# artifacts: --config '{"bundle":{"createUpdaterArtifacts":false}}').
# Tauri signs the app with the hardened runtime, notarizes and staples it; this
# script then signs, notarizes and staples the DMG itself.
set -euo pipefail
cd "$(dirname "$0")/.."

# "Developer ID Application: Filip Dvoran (DGS99GNC2W)", by fingerprint: the
# keychain holds two certificates with that name and codesign refuses an
# ambiguous name.
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-482C7C78327EF389721A7E6CE762CE49117E7402}"
# App Store Connect team key "notarize" (Developer role).
export APPLE_API_KEY="${APPLE_API_KEY:-AQ6KC67HPV}"
export APPLE_API_ISSUER="${APPLE_API_ISSUER:-69a6de87-a8f7-47e3-e053-5b8c7c11a4d1}"
export APPLE_API_KEY_PATH="${APPLE_API_KEY_PATH:-$HOME/.private_keys/AuthKey_$APPLE_API_KEY.p8}"
[ -f "$APPLE_API_KEY_PATH" ] || { echo "Missing notarization key $APPLE_API_KEY_PATH" >&2; exit 1; }

VERSION=$(node -p "require('./package.json').version")
BASE=src-tauri/target/aarch64-apple-darwin/release/bundle
APP="$BASE/macos/html2wp.app"
DMG="$BASE/dmg/html2wp_${VERSION}_aarch64.dmg"

npm run bundle:mac -- "$@"

codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
  --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple "$DMG"

codesign --verify --deep --strict --verbose=2 "$APP"
codesign -dvv "$APP" 2>&1 | grep -E '^(Authority=Developer ID Application|TeamIdentifier|Timestamp|flags)'
xcrun stapler validate "$APP"
xcrun stapler validate "$DMG"
spctl --assess --type execute --verbose=2 "$APP"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
echo "Signed and notarized: $DMG"
