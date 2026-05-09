# P54 macOS Platform Workflow Static Gate Summary

Date: 2026-05-09

## Goal

Strengthen `cargo xtask verify --stage rc-readiness` so the macOS platform smoke workflow is checked for triggerability, architecture verification, build gates, bundle smoke, and retained evidence artifacts before anyone relies on it for `v1.1.0-rc1`.

## External Source Check

GitHub-hosted runner documentation was checked on 2026-05-09:

- `macos-14` is listed as an arm64 macOS runner.
- `macos-15-intel` is listed as an Intel macOS runner.
- Source: <https://docs.github.com/actions/reference/runners/github-hosted-runners>

This supports the current workflow labels, but does not replace a real workflow run.

## Changes

- Updated `xtask/src/main.rs`:
  - added a macOS workflow static check for `workflow_dispatch`
  - requires `expected_uname: arm64` and `expected_uname: x86_64`
  - requires `uname -m`
  - requires `cargo fmt --all -- --check`
  - requires `cargo test --workspace`
  - requires `cargo clippy --workspace --all-targets -- -D warnings`
  - requires `trunk build --release`
  - requires `cargo tauri build`
  - requires `plutil -lint`, `hdiutil verify`, `pkgbuild --install-location`, and `pkgutil --expand`
  - requires `rust-mainline-macos-${{ matrix.arch }}` and `retention-days: 7`
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 11 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The macOS workflow path is now statically better guarded. The RC goal is still not complete because the workflow has not actually run on GitHub Actions and real Desktop smoke evidence is still missing.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Run the non-publishing macOS workflow and record both matrix outputs in a handoff:

```text
.github/workflows/rust-mainline-platform-smoke.yml
```
