#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-1.0.9}"
APP_PATH="${2:-dist/mac/CC Desktop Switch.app}"
OUTPUT_PKG="${3:-dist/mac/CC-Desktop-Switch-v${VERSION}-macOS.pkg}"

if [[ ! -d "$APP_PATH" ]]; then
  echo "App bundle not found: $APP_PATH" >&2
  exit 1
fi

if ! command -v pkgbuild >/dev/null 2>&1; then
  echo "pkgbuild is required to create a macOS installer package." >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_ROOT="$ROOT/.tmp/pkg-root"
SCRIPTS_DIR="$ROOT/macos/pkg-scripts"
APP_NAME="$(basename "$APP_PATH")"

rm -rf "$PKG_ROOT"
mkdir -p "$PKG_ROOT/Applications"
mkdir -p "$(dirname "$OUTPUT_PKG")"

ditto "$APP_PATH" "$PKG_ROOT/Applications/$APP_NAME"

rm -f "$OUTPUT_PKG"
pkgbuild \
  --root "$PKG_ROOT" \
  --install-location "/" \
  --identifier "io.github.lonr6.ccdesktopswitch" \
  --version "$VERSION" \
  --scripts "$SCRIPTS_DIR" \
  "$OUTPUT_PKG"
