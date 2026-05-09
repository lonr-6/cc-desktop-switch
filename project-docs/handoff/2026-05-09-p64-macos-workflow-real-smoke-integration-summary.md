# P64 macOS Workflow Real Smoke Integration Summary

Date: 2026-05-09

## Result

Path ready, not run remotely

## Scope

This phase connected macOS real Claude Desktop local config smoke to the non-publishing Rust mainline platform workflow. It did not push, trigger GitHub Actions, publish a release, upload a GitHub Release, create a tag, or modify `latest.json`.

## What Changed

- `.github/workflows/rust-mainline-platform-smoke.yml`
  - Adds a macOS real Desktop local config smoke step to each macOS matrix job.
  - Runs `scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write`.
  - Sets `HOME` to `$RUNNER_TEMP/ccds-real-smoke-home` so the smoke uses a disposable macOS local configLibrary path.
  - Copies `macos-real-desktop-smoke-evidence.md` and cargo test logs into each uploaded artifact under `real-desktop-smoke/`.
- `scripts/macos/run-real-desktop-smoke.sh`
  - Writes a relative `log:` field in evidence so downloaded artifacts can be validated on Windows.
- `xtask/src/main.rs`
  - Adds a static `rc-readiness` check requiring the workflow real-smoke step and retained evidence/log paths.
- Documentation updated:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; new macOS workflow real-smoke static check passed, two real macOS evidence handoffs still missing |
| `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` against `target/macos-real-smoke-relative-fixture` | Passed; relative log path resolves after artifact download shape |

## Why This Is Not Pass Evidence

No GitHub macOS runner has executed this workflow yet. No real `rust-mainline-macos-arm64` or `rust-mainline-macos-x64` artifact has been downloaded, and no macOS collector handoff has been generated.

## Next Minimum Task

Commit and push the test branch, then trigger only the non-publishing `Rust Mainline Platform Smoke` workflow. After it passes, download both macOS artifacts and run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-PlatformSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
cargo xtask verify --stage rc-readiness
```
