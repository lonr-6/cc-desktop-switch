# Windows Real Desktop Smoke Evidence Summary

Date: 2026-05-09

## Result

Pass

fingerprint: desktop.real_windows_local_config_smoke
test_name: windows_real_desktop_local_config_smoke
command: cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture
evidence: D:\ccds-build\cc-desktop-switch-rust-mainline\target\real-desktop-smoke\windows-real-desktop-smoke-evidence.md
log: D:\ccds-build\cc-desktop-switch-rust-mainline\target\real-desktop-smoke\windows-real-desktop-smoke-20260509-153050.log
configLibrary: C:\Users\15618\AppData\Local\Claude-3p\configLibrary

## Verified Gates

- Wrapper evidence includes ## Result / Pass.
- Wrapper evidence was produced in mode: run.
- Wrapper evidence records exit_code: 0.
- Cargo test log includes windows_real_desktop_local_config_smoke.
- Cargo test log includes test result: ok.
- Rust smoke test covers backup, readback, loopback gateway, and restored Desktop config.

## Readiness Markers

- windows_real_desktop_local_config_smoke
- loopback gateway
- restored

## Notes

This file records completed Windows real Claude Desktop local config smoke evidence only. Preflight evidence is rejected by this collector.
