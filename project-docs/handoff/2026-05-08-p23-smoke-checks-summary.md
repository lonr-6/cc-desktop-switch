# P23 Smoke Checks Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P23 established the first smoke-check boundary.

Implemented:

- Provider static smoke:
  - checks active provider existence;
  - validates base URL shape;
  - checks API key presence.
- Local gateway smoke:
  - ensures gateway starts;
  - calls `/v1/models`;
  - requires non-empty Claude-safe route list.
- Provider real smoke command:
  - stops before network if provider is missing or API key is missing;
  - builds a minimal upstream message request through the existing adapter;
  - reports upstream failures through redacted issue codes.
- Leptos buttons:
  - Static smoke
  - Gateway smoke
  - Provider smoke

Not implemented:

- Real smoke against a user-supplied provider key in this work session.
- Persisted smoke history.
- Runtime gateway request logs inside diagnostics package.
- Automatic readiness refresh after smoke commands.

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
- `project-docs/handoff/2026-05-08-p23-smoke-checks-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
cargo xtask verify --stage diagnostics
cargo xtask verify --stage provider-service
trunk build --release
cargo xtask verify --all
trunk serve --address 127.0.0.1 --port 1421 --open false
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 79 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Browser verification:

- Playwright desktop viewport: 1440x1000.
- Playwright mobile viewport: 390x900.
- Page rendered with no console errors after cache-buster refresh.
- Known warning: Chromium reports Trunk-generated preload SRI warning.
- Trunk dev server occasionally served a stale build-error overlay before manual `trunk build`; cache-buster refresh then rendered cleanly.

Known Tauri build warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Eval Coverage

Added smoke coverage:

- Provider static smoke blocks missing provider/API key.
- Gateway smoke exercises local `/v1/models`.
- Provider real smoke stops before network when API key is missing.

## Blockers

- Real provider smoke still needs a user-supplied API key or redacted diagnostics package.
- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.
- Model mapping edit UI, Provider backup list, and richer import conflict UI remain missing.

## Next Minimum Task

Implement model mapping edit and Provider backup surfaces:

1. Expose model mapping summaries to UI without raw provider model names entering Desktop menu.
2. Add safe edit path for explicit route mappings.
3. Add backup list/read command for config backups.
4. Keep `Default` out of runtime fallback and Desktop menu.
