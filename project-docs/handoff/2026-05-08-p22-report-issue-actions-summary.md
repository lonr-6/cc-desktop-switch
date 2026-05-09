# P22 Report Issue Actions Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P22 established the first local "Report issue" action boundary.

Implemented:

- Redacted GitHub Issue draft generation with title, body, and prefilled URL.
- Copy diagnostics summary to the system clipboard.
- Save diagnostics package to the local config-adjacent `diagnostics/` directory.
- Open prefilled GitHub Issue URL using platform commands.
- Leptos Diagnostics buttons:
  - Summary
  - Copy
  - Package
  - Save
  - Issue draft
  - Open issue

Safety:

- No GitHub Release upload or publish path was added.
- Issue draft body and URL are generated from redacted diagnostics content.
- Diagnostics package save path is local and under the app config area, not a remote upload.

Not implemented:

- File picker for choosing a save location.
- Runtime gateway log capture.
- Attach-package automation for GitHub Issue.
- Provider real smoke diagnostics.

## Files Changed

- `src-tauri/src/diagnostics.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p22-report-issue-actions-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
cargo xtask verify --stage diagnostics
trunk build --release
cargo xtask verify --all
trunk serve --address 127.0.0.1 --port 1421 --open false
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 76 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Browser verification:

- Playwright desktop viewport: 1440x1000.
- Playwright mobile viewport: 390x900.
- Page rendered with no console errors after cache-buster refresh.
- Known warning: Chromium reports Trunk-generated preload SRI warning; this is not a runtime app error.

Known Tauri build warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Eval Coverage

Updated diagnostics coverage:

- `diagnostics.false_green_readiness`: summary and issue draft preserve false-green issue codes.
- `diagnostics.secret_leak`: diagnostics package save and issue draft URL/body do not leak API keys or gateway keys.

## Blockers

- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.
- Provider and gateway real smoke checks require user-supplied API keys or redacted diagnostics.
- Tauri file picker and gateway runtime log capture remain missing.

## Next Minimum Task

Implement real smoke boundaries:

1. Add provider static check and minimal upstream smoke command with redacted errors.
2. Add local gateway smoke command against `/v1/models` and mapped `/v1/messages` when possible.
3. Feed smoke outcomes into readiness and diagnostics package.
4. Keep failed smoke checks as explicit issue codes, not generic readiness success.
