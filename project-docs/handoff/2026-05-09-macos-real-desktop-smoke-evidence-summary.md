# macOS Real Desktop Smoke Evidence Summary

Date: 2026-05-09

## Result

Pass

fingerprint: desktop.real_macos_local_config_smoke
description: macOS real Claude Desktop local config smoke
test_name: macos_real_desktop_local_config_smoke
platform: Darwin
arch: arm64
command: cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture
evidence: D:\ccds-build\cc-desktop-switch-rust-mainline\target\github-artifacts\macos-smoke-25599626985-v2\arm64\real-desktop-smoke\macos-real-desktop-smoke-evidence.md
log: D:\ccds-build\cc-desktop-switch-rust-mainline\target\github-artifacts\macos-smoke-25599626985-v2\arm64\real-desktop-smoke\macos-real-desktop-smoke-20260509-112430.log
configLibrary: /Users/runner/work/_temp/ccds-real-smoke-home/Library/Application Support/Claude-3p/configLibrary

## Verified Gates

- Wrapper evidence includes ## Result / Pass.
- Wrapper evidence was produced on platform: Darwin.
- Wrapper evidence was produced in mode: run.
- Wrapper evidence records exit_code: 0.
- Cargo test log includes macos_real_desktop_local_config_smoke.
- Cargo test log includes test result: ok.
- Rust smoke test covers backup, readback, loopback gateway, safe route checks, Default suppression, and restored Desktop config.

## Readiness Markers

- macOS real Claude Desktop local config smoke
- configLibrary
- safe route

## Notes

This file records completed macOS real Claude Desktop local config smoke evidence only. Preflight and UnsupportedPlatform evidence are rejected by this collector.
