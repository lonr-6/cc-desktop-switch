#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-1.0.9}"
SOURCE_PATH="${2:-dist/mac/CC Desktop Switch.app}"
OUTPUT_DMG="${3:-dist/mac/CC-Desktop-Switch-v${VERSION}-macOS.dmg}"

if [[ ! -e "$SOURCE_PATH" ]]; then
  echo "DMG source not found: $SOURCE_PATH" >&2
  exit 1
fi

if ! command -v hdiutil >/dev/null 2>&1; then
  echo "hdiutil is required to create a DMG." >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT_DMG")"
STAGING="$(mktemp -d)"
cleanup() {
  rm -rf "$STAGING"
}
trap cleanup EXIT

cp -R "$SOURCE_PATH" "$STAGING/"
if [[ "$SOURCE_PATH" == *.app ]]; then
  ln -s /Applications "$STAGING/Applications"
fi

rm -f "$OUTPUT_DMG"
hdiutil create \
  -volname "CC Desktop Switch ${VERSION}" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$OUTPUT_DMG"
