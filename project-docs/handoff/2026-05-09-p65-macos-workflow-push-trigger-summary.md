# P65 macOS Workflow Push Trigger Summary

## Result

In progress.

## What Changed

- Added a non-publishing `push` trigger to `.github/workflows/rust-mainline-platform-smoke.yml`.
- The trigger is limited to `codex/**` branches and Rust mainline workflow/source paths.
- Added `push:` and `codex/**` to `cargo xtask verify --stage rc-readiness` static workflow checks.
- Updated `project-docs/status.md`, `PLANS.md`, and `docs/testing/eval-harness.md`.

## Why

- `gh workflow run rust-mainline-platform-smoke.yml --ref codex/rust-mainline-rewrite` returned 404 because the new workflow is not on the default branch yet.
- Draft PR #21 was created, but initial status checks were empty.
- A branch-limited `push` trigger lets the test branch run the macOS smoke workflow without triggering `Release`, without tagging, and without publishing update metadata.

## Verification

- `cargo fmt --all -- --check` passed before this change.
- `cargo xtask verify --stage rc-readiness` still failed closed with only the expected two macOS evidence gaps before this change.
- P65 still needs a fresh push, workflow run, artifact download, and collector handoff before it can become pass evidence.

## Next Step

- Commit and push P65.
- Watch `Rust Mainline Platform Smoke`.
- Download `rust-mainline-macos-arm64` and `rust-mainline-macos-x64` artifacts.
- Run both macOS evidence collectors.
