#!/usr/bin/env bash
# Assemble Syncplay.app from a built binary and package it as a DMG.
#
# Usage: package.sh <binary> <version> <outdir> [arch]
# arch defaults to uname -m; CI passes it explicitly because the x86_64
# build is cross-compiled on an arm64 runner.
#
# With no Apple env vars set the app is ad-hoc signed (enough for local
# testing). Set these to get a notarized Developer ID build:
#   APPLE_CERTIFICATE           base64-encoded .p12 developer ID certificate
#   APPLE_CERTIFICATE_PASSWORD  password for the .p12
#   APPLE_API_KEY               base64-encoded .p8 App Store Connect key
#   APPLE_API_KEY_ID            App Store Connect key id
#   APPLE_API_ISSUER            App Store Connect issuer UUID
set -euo pipefail

BINARY="$1"
VERSION="$2"
OUTDIR="$3"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$OUTDIR/Syncplay.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/syncplay"
sed "s/@VERSION@/$VERSION/g" "$SCRIPT_DIR/Info.plist" >"$APP/Contents/Info.plist"

# The .icns is optional; a missing icon just falls back to the default.
if command -v rsvg-convert >/dev/null 2>&1; then
	ICONSET="$OUTDIR/icon.iconset"
	mkdir -p "$ICONSET"
	for size in 16 32 128 256 512; do
		rsvg-convert -w "$size" -h "$size" "$SCRIPT_DIR/../../icon.svg" -o "$ICONSET/icon_${size}x${size}.png"
		rsvg-convert -w $((size * 2)) -h $((size * 2)) "$SCRIPT_DIR/../../icon.svg" -o "$ICONSET/icon_${size}x${size}@2x.png"
	done
	iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
fi

IDENTITY=""
if [ -n "${APPLE_CERTIFICATE:-}" ]; then
	WORK="$(mktemp -d)"
	KEYCHAIN="$WORK/build.keychain-db"
	KEYCHAIN_PASSWORD="$(openssl rand -hex 16)"
	security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	security set-keychain-settings -lut 3600 "$KEYCHAIN"
	security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	echo "$APPLE_CERTIFICATE" | base64 --decode >"$WORK/cert.p12"
	security import "$WORK/cert.p12" -k "$KEYCHAIN" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
	security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
	# shellcheck disable=SC2046
	security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | sed -e 's/ *//' -e 's/"//g')
	IDENTITY="$(security find-identity -v -p codesigning "$KEYCHAIN" | grep -o '"[^"]*"' | head -1 | tr -d '"')"
	echo "Signing identity: $IDENTITY"
else
	IDENTITY="-"
fi

codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP/Contents/MacOS/syncplay"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

if [ "$IDENTITY" != "-" ] && [ -n "${APPLE_API_KEY:-}" ]; then
	echo "$APPLE_API_KEY" | base64 --decode >"$WORK/notary.p8"
	ditto -c -k --keepParent "$APP" "$WORK/Syncplay.zip"
	SUBMIT_LOG="$WORK/notary-submit.log"
	if ! xcrun notarytool submit "$WORK/Syncplay.zip" \
		--key "$WORK/notary.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" \
		--wait | tee "$SUBMIT_LOG"; then
		SUBMISSION_ID="$(grep -m1 'id:' "$SUBMIT_LOG" | awk '{print $2}')"
		[ -n "$SUBMISSION_ID" ] && xcrun notarytool log "$SUBMISSION_ID" \
			--key "$WORK/notary.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" || true
		exit 1
	fi
	xcrun stapler staple "$APP"
fi

ARCH="${4:-$(uname -m)}"
DMG="$OUTDIR/syncplay_${VERSION}_${ARCH}.dmg"
rm -f "$DMG"
hdiutil create -volname Syncplay -srcfolder "$APP" -ov -format UDZO "$DMG"
if [ "$IDENTITY" != "-" ]; then
	codesign --force --sign "$IDENTITY" "$DMG"
fi
echo "Created $DMG"
