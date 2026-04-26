# macOS Packaging

This directory owns the macOS packaging layer for CC Desktop Switch.

The shared application code stays in the repository root:

- `main.py`
- `backend/`
- `frontend/`

Do not copy shared runtime code into this directory. macOS-specific files here should be limited to packaging, signing, notarization, DMG creation, and release notes that only apply to macOS.

## Local Build

Run from the repository root on macOS:

```bash
./macos/build-macos.sh
```

The script installs Python dependencies, creates an `.icns` icon from `frontend/assets/app-icon.png`, builds the macOS app, creates a macOS installer package, and creates a drag-and-drop DMG.

All macOS build outputs are written to:

```text
dist/mac/
```

`<arch>` is detected from `uname -m` and is usually `arm64` or `x64`.

The output directory contains:

- `CC Desktop Switch.app`
- `CC-Desktop-Switch-v<version>-macOS-<arch>.pkg`
- `CC-Desktop-Switch-v<version>-macOS-<arch>.dmg`

The DMG keeps the standard drag-and-drop layout: the `.app` bundle plus an Applications shortcut. If a user copies that app onto an existing app at the same Applications path, Finder handles the replacement prompt. The PKG installer installs to `/Applications/CC Desktop Switch.app` and removes the previous app at that official install location before writing the new one.

The script sets `PYINSTALLER_CONFIG_DIR` to `.tmp/pyinstaller/` by default so PyInstaller cache writes stay inside the repository during local or sandboxed builds.

The macOS icon preparation step removes the baked checkerboard background from the shared PNG before creating the `.icns` file.

## Optional Signing

For local unsigned testing, no environment variables are required.

For Developer ID signing, set:

```bash
export MACOS_CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
./macos/build-macos.sh
```

The script uses `macos/entitlements.plist` for hardened runtime signing.

For installer package signing, set:

```bash
export MACOS_INSTALLER_SIGN_IDENTITY="Developer ID Installer: Your Name (TEAMID)"
./macos/build-macos.sh
```

## Optional Notarization

Create a notarytool keychain profile outside the repository, then set:

```bash
export MACOS_CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export MACOS_NOTARY_KEYCHAIN_PROFILE="cc-desktop-switch-notary"
./macos/build-macos.sh
```

The script submits the DMG with `xcrun notarytool`, waits for the result, and staples the returned ticket when notarization succeeds.

## Manual Smoke Test

After building on macOS:

```bash
open "dist/mac/CC Desktop Switch.app"
```

Then verify:

- The desktop window opens.
- `http://127.0.0.1:18081/api/status` responds.
- Applying Claude Desktop configuration writes the macOS plist through `defaults`.
- The proxy starts on port `18080`.

## Notes

- PyInstaller is not a cross-compiler. Build macOS assets on macOS.
- Prefer the onedir `.app` bundle for macOS distribution. Onefile app bundles add startup overhead and are a poor fit for signed distribution.
- Public distribution should use Developer ID signing and Apple notarization.
- The first macOS package intentionally disables the `pystray` tray icon. Its AppKit backend calls `NSApplication.run`, which must stay on the main thread while pywebview owns the macOS UI loop.
