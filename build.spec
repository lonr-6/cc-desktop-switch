# -*- mode: python ; coding: utf-8 -*-
"""
CC Desktop Switch - PyInstaller 构建配置

使用方法：
    pyinstaller build.spec                    # 文件夹模式（启动快）
    set CCDS_ONEFILE=1 && pyinstaller build.spec  # 单文件 exe（便携）
    set CCDS_CONSOLE=1 && pyinstaller build.spec  # 调试时显示控制台

输出：
    dist/CC-Desktop-Switch/        ← 文件夹模式
    dist/CC-Desktop-Switch.exe     ← 单文件模式（加 --onefile）
"""

import os
from pathlib import Path
from PyInstaller.utils.hooks import collect_data_files, collect_submodules, copy_metadata

ROOT = Path(SPECPATH)
FRONTEND = ROOT / "frontend"
ONEFILE = os.environ.get("CCDS_ONEFILE") == "1"
CONSOLE = os.environ.get("CCDS_CONSOLE") == "1"
ICON_FILE = FRONTEND / "assets" / "app-icon.ico"
ICON = str(ICON_FILE) if ICON_FILE.exists() else None

WEBVIEW_HIDDENIMPORTS = collect_submodules("webview")
WEBVIEW_DATAS = collect_data_files("webview") + copy_metadata("pywebview")

block_cipher = None

a = Analysis(
    ["main.py"],
    pathex=[str(ROOT)],
    binaries=[],
    datas=[
        (str(FRONTEND), "frontend"),
        (str(ROOT / "LICENSE.txt"), "."),
    ] + WEBVIEW_DATAS,
    hiddenimports=[
        "backend", "backend.main", "backend.config",
        "backend.registry", "backend.proxy", "backend.update", "backend.i18n",
    ] + WEBVIEW_HIDDENIMPORTS,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "tkinter", "matplotlib", "numpy", "pandas", "PIL",
        "scipy", "setuptools", "pip",
        "cryptography", "zmq", "notebook", "IPython",
        "PyQt5", "PySide2", "PySide6",
    ],
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

if ONEFILE:
    exe = EXE(
        pyz,
        a.scripts,
        a.binaries,
        a.zipfiles,
        a.datas,
        [],
        name="CC-Desktop-Switch",
        debug=False,
        bootloader_ignore_signals=False,
        strip=False,
        upx=True,
        upx_exclude=[],
        runtime_tmpdir=None,
        console=CONSOLE,
        disable_windowed_traceback=False,
        argv_emulation=False,
        target_arch=None,
        codesign_identity=None,
        entitlements_file=None,
        icon=ICON,
    )
else:
    exe = EXE(
        pyz,
        a.scripts,
        [],
        exclude_binaries=True,
        name="CC-Desktop-Switch",
        debug=False,
        bootloader_ignore_signals=False,
        strip=False,
        upx=True,
        upx_exclude=[],
        runtime_tmpdir=None,
        console=CONSOLE,
        disable_windowed_traceback=False,
        argv_emulation=False,
        target_arch=None,
        codesign_identity=None,
        entitlements_file=None,
        icon=ICON,
    )
    COLLECT(
        exe,
        a.binaries,
        a.zipfiles,
        a.datas,
        strip=False,
        upx=True,
        upx_exclude=[],
        name="CC-Desktop-Switch",
    )
