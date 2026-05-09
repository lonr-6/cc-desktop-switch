# macOS Platform Smoke Evidence Summary

Date: 2026-05-09

## Result

Pass

fingerprint: platform.macos_arm64_x64_smoke_path
macos-14
macos-15-intel
workflow_run_arm64: https://github.com/lonr-6/cc-desktop-switch/actions/runs/25599626985
workflow_run_x64: https://github.com/lonr-6/cc-desktop-switch/actions/runs/25599626985
artifact_arm64: rust-mainline-macos-arm64
artifact_x64: rust-mainline-macos-x64

## arm64

- runner: macos-14
- expected_uname: arm64
- actual_uname: arm64
- version: 1.1.0-rc1
- commit: bc0b02492c1b3ad9e0ed87e478add4468391905d
- evidence: D:\ccds-build\cc-desktop-switch-rust-mainline\target\github-artifacts\macos-smoke-25599626985-v2\arm64\platform-smoke-evidence.md

## x64

- runner: macos-15-intel
- expected_uname: x86_64
- actual_uname: x86_64
- version: 1.1.0-rc1
- commit: bc0b02492c1b3ad9e0ed87e478add4468391905d
- evidence: D:\ccds-build\cc-desktop-switch-rust-mainline\target\github-artifacts\macos-smoke-25599626985-v2\x64\platform-smoke-evidence.md

## Verified Gates

- Both workflow artifacts include ## Result / Pass.
- Both workflow artifacts include platform.macos_arm64_x64_smoke_path.
- arm64 evidence uses macos-14 and actual_uname: arm64.
- x64 evidence uses macos-15-intel and actual_uname: x86_64.
- Both workflow artifacts include Rust, UI, Tauri, DMG, and PKG smoke markers.

## Notes

This file records downloaded workflow artifact evidence only. It does not publish a release and does not replace real macOS Claude Desktop local config smoke.
