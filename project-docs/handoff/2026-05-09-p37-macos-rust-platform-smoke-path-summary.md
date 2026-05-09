# P37 macOS Rust Platform Smoke Path Summary

Date: 2026-05-09

## Goal

Establish a reproducible macOS arm64 and macOS x64 Rust/Tauri build and bundle-smoke path without publishing a release.

## Changes

- Added `.github/workflows/rust-mainline-platform-smoke.yml`.
- Added `project-docs/runbooks/macos-rust-mainline-smoke.md`.
- Updated `.github/workflows/release.yml` so release manifest generation requires `macos-x64` in addition to `windows-x64` and `macos-arm64`.
- Updated `docs/testing/eval-harness.md`, `PLANS.md`, and `project-docs/status.md`.

## Runner Choice

Source checked 2026-05-09:

- GitHub-hosted runners reference: <https://docs.github.com/actions/reference/runners/github-hosted-runners>
- Tauri macOS app bundle reference: <https://v2.tauri.app/distribute/macos-application-bundle/>

Workflow matrix:

| Platform | Runner | Expected `uname -m` |
|---|---|---|
| macOS arm64 | `macos-14` | `arm64` |
| macOS x64 | `macos-15-intel` | `x86_64` |

## Workflow Coverage

The workflow runs:

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Then it validates:

- runner architecture
- `.app` bundle exists
- `Info.plist` parses with `plutil`
- app bundle has an executable under `Contents/MacOS`
- DMG exists and passes `hdiutil verify`
- PKG can be created from the app bundle with `pkgbuild`
- PKG expands with `pkgutil --expand`

Artifacts are uploaded as workflow artifacts only. The workflow does not create or publish a GitHub Release.

## Local Verification

| Command | Result |
|---|---|
| Python YAML parse for `.github/workflows/rust-mainline-platform-smoke.yml` and `.github/workflows/release.yml` | Passed |
| `rg -n -e macos-x64 -e macos-15-intel -e rust-mainline-platform-smoke -e RequiredPlatforms .github project-docs docs` | Confirmed workflow, runbook, release hard gate, and docs references |
| `cargo xtask verify --stage release` | Passed; release gate still rejects missing macOS x64 assets |

## Remaining Gap

This stage creates the path; it does not claim macOS smoke has run. The workflow still needs to be executed on GitHub Actions or equivalent macOS arm64/x64 runners, and real Claude Desktop local config smoke still needs manual evidence on both architectures.
