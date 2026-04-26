#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PYTHON_BIN="${PYTHON_BIN:-python3}"
VERSION="${CCDS_VERSION:-1.0.9}"
ARCH="$(uname -m)"
if [[ "$ARCH" == "x86_64" ]]; then
  RELEASE_ARCH="x64"
else
  RELEASE_ARCH="$ARCH"
fi

MAC_DIST="$ROOT/dist/mac"
APP_PATH="$MAC_DIST/CC Desktop Switch.app"
COLLECT_PATH="$MAC_DIST/CC-Desktop-Switch"
PKG_PATH="$MAC_DIST/CC-Desktop-Switch-v${VERSION}-macOS-${RELEASE_ARCH}.pkg"
DMG_PATH="$MAC_DIST/CC-Desktop-Switch-v${VERSION}-macOS-${RELEASE_ARCH}.dmg"
ICNS="$ROOT/macos/assets/app-icon.icns"
PNG="$ROOT/frontend/assets/app-icon.png"
PREPARED_PNG="$ROOT/macos/assets/app-icon-prepared.png"
export PYINSTALLER_CONFIG_DIR="${PYINSTALLER_CONFIG_DIR:-$ROOT/.tmp/pyinstaller}"

ensure_icon() {
  mkdir -p "$ROOT/macos/assets"
  if [[ -f "$ICNS" ]]; then
    rm -f "$ICNS"
  fi
  if [[ ! -f "$PNG" ]]; then
    echo "Missing source icon: $PNG" >&2
    exit 1
  fi
  "$PYTHON_BIN" "$ROOT/macos/prepare-icon.py" "$PNG" "$PREPARED_PNG" "$ICNS"
}

echo "Installing Python dependencies..."
"$PYTHON_BIN" -m pip install --upgrade pip
"$PYTHON_BIN" -m pip install -r requirements.txt

echo "Preparing macOS icon..."
ensure_icon

echo "Building macOS app bundle..."
mkdir -p "$PYINSTALLER_CONFIG_DIR"
rm -rf "$MAC_DIST"
mkdir -p "$MAC_DIST"
CCDS_VERSION="$VERSION" "$PYTHON_BIN" -m PyInstaller --noconfirm --clean --distpath "$MAC_DIST" "$ROOT/macos/build-macos.spec"

if [[ ! -d "$APP_PATH" ]]; then
  echo "App bundle not found: $APP_PATH" >&2
  exit 1
fi
rm -rf "$COLLECT_PATH"

if [[ -n "${MACOS_CODESIGN_IDENTITY:-}" ]]; then
  echo "Signing app bundle..."
  codesign --force --deep --options runtime --timestamp \
    --entitlements "$ROOT/macos/entitlements.plist" \
    --sign "$MACOS_CODESIGN_IDENTITY" \
    "$APP_PATH"
  codesign --verify --deep --strict --verbose=2 "$APP_PATH"
else
  echo "Skipping Developer ID signing because MACOS_CODESIGN_IDENTITY is not set."
fi

if [[ "${MACOS_SKIP_DMG:-0}" == "1" ]]; then
  echo "Skipping PKG and DMG creation because MACOS_SKIP_DMG=1."
  exit 0
fi

echo "Creating installer package..."
"$ROOT/macos/make-pkg.sh" "$VERSION" "$APP_PATH" "$PKG_PATH"

if [[ -n "${MACOS_INSTALLER_SIGN_IDENTITY:-}" ]]; then
  echo "Signing installer package..."
  SIGNED_PKG="${PKG_PATH%.pkg}-signed.pkg"
  productsign --sign "$MACOS_INSTALLER_SIGN_IDENTITY" "$PKG_PATH" "$SIGNED_PKG"
  mv "$SIGNED_PKG" "$PKG_PATH"
fi

echo "Creating DMG..."
"$ROOT/macos/make-dmg.sh" "$VERSION" "$APP_PATH" "$DMG_PATH"

if [[ -n "${MACOS_CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --timestamp --sign "$MACOS_CODESIGN_IDENTITY" "$DMG_PATH"
fi

if [[ -n "${MACOS_NOTARY_KEYCHAIN_PROFILE:-}" ]]; then
  echo "Submitting DMG for notarization..."
  xcrun notarytool submit "$DMG_PATH" \
    --keychain-profile "$MACOS_NOTARY_KEYCHAIN_PROFILE" \
    --wait
  xcrun stapler staple "$DMG_PATH"
fi

echo "Built: $APP_PATH"
echo "Installer: $PKG_PATH"
echo "Packaged: $DMG_PATH"
