# P46 RC Readiness Xtask Summary

Date: 2026-05-09

## Goal

Add a repeatable local audit command that maps the RC1 objective to concrete evidence and fails closed while required smoke evidence is missing.

## Changes

- Updated `xtask/src/main.rs`:
  - added `cargo xtask verify --stage rc-readiness`
  - prints a prompt-to-artifact checklist for RC1 readiness
  - checks local workspace structure, Rust UI source, absence of hand-written JS under `ui/src`, core `ModelCatalog`/Apply/release gate test markers, and Windows packaged app smoke evidence
  - searches handoff files for explicit pass evidence for Windows real Desktop smoke, macOS arm64/x64 workflow smoke, and macOS real Desktop smoke
  - returns non-zero until required real smoke evidence exists
- Updated docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 7 pass checks, 3 missing evidence checks, exit code 1 |
| `cargo test --workspace` | Passed; 110 passed, 1 ignored real Desktop smoke |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |

Missing evidence reported by `rc-readiness`:

- Windows real Claude Desktop local config smoke pass evidence
- macOS arm64 and macOS x64 build/smoke workflow evidence
- macOS real Claude Desktop local config smoke pass evidence

## Notes

This does not mark RC1 complete. The stage is designed to fail until the real platform smoke evidence is written into handoff documents with explicit `## Result` / `Pass` markers.

## Next Minimum Task

Get user confirmation for Windows managed-policy cleanup or use an unmanaged Windows profile, then rerun the opt-in real Desktop smoke and record pass evidence. In parallel, run the macOS arm64/x64 non-publishing smoke workflow and add its run evidence.
