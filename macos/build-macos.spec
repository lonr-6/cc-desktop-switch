# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for macOS app bundle builds."""

import os
from pathlib import Path

from PyInstaller.utils.hooks import collect_data_files, collect_submodules, copy_metadata

ROOT = Path(SPECPATH).parent
FRONTEND = ROOT / "frontend"
MACOS_DIR = ROOT / "macos"
APP_VERSION = os.environ.get("CCDS_VERSION", "1.0.9")
APP_NAME = "CC Desktop Switch"
EXECUTABLE_NAME = "CC-Desktop-Switch"
BUNDLE_IDENTIFIER = os.environ.get("CCDS_MACOS_BUNDLE_ID", "io.github.lonr6.ccdesktopswitch")
CODESIGN_IDENTITY = os.environ.get("MACOS_CODESIGN_IDENTITY") or None
ENTITLEMENTS_FILE = MACOS_DIR / "entitlements.plist"
ICON_FILE = MACOS_DIR / "assets" / "app-icon.icns"
ICON_FALLBACK = FRONTEND / "assets" / "app-icon.png"
ICON = str(ICON_FILE if ICON_FILE.exists() else ICON_FALLBACK)


def safe_collect_data_files(package):
    try:
        return collect_data_files(package)
    except Exception:
        return []


def safe_collect_submodules(package):
    try:
        return collect_submodules(package)
    except Exception:
        return []


def safe_copy_metadata(package):
    try:
        return copy_metadata(package)
    except Exception:
        return []


WEBVIEW_HIDDENIMPORTS = safe_collect_submodules("webview")
WEBVIEW_DATAS = safe_collect_data_files("webview") + safe_copy_metadata("pywebview")
PYSTRAY_HIDDENIMPORTS = safe_collect_submodules("pystray")
PYSTRAY_DATAS = safe_copy_metadata("pystray") + safe_copy_metadata("Pillow")

block_cipher = None

a = Analysis(
    [str(ROOT / "main.py")],
    pathex=[str(ROOT)],
    binaries=[],
    datas=[
        (str(FRONTEND), "frontend"),
        (str(ROOT / "LICENSE.txt"), "."),
    ] + WEBVIEW_DATAS + PYSTRAY_DATAS,
    hiddenimports=[
        "backend",
        "backend.main",
        "backend.config",
        "backend.registry",
        "backend.proxy",
        "backend.update",
        "backend.i18n",
    ] + WEBVIEW_HIDDENIMPORTS + PYSTRAY_HIDDENIMPORTS,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "tkinter",
        "matplotlib",
        "numpy",
        "pandas",
        "scipy",
        "setuptools",
        "pip",
        "cryptography",
        "zmq",
        "notebook",
        "IPython",
        "PyQt5",
        "PySide2",
        "PySide6",
    ],
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name=EXECUTABLE_NAME,
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=CODESIGN_IDENTITY,
    entitlements_file=str(ENTITLEMENTS_FILE) if ENTITLEMENTS_FILE.exists() else None,
    icon=ICON,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name=EXECUTABLE_NAME,
)

app = BUNDLE(
    coll,
    name=f"{APP_NAME}.app",
    icon=ICON,
    bundle_identifier=BUNDLE_IDENTIFIER,
    version=APP_VERSION,
    info_plist={
        "CFBundleName": APP_NAME,
        "CFBundleDisplayName": APP_NAME,
        "CFBundleShortVersionString": APP_VERSION,
        "CFBundleVersion": APP_VERSION,
        "CFBundleGetInfoString": f"{APP_NAME} {APP_VERSION}",
        "CFBundlePackageType": "APPL",
        "LSMinimumSystemVersion": "11.0",
        "NSHighResolutionCapable": True,
    },
)
