# Release Readiness Template

## Release

- Version:
- Tier: local RC / draft release / public Latest
- Date:
- Branch/worktree:

## Required Decisions

| Decision | Status | Evidence |
|---|---|---|
| `Default` is not runtime fallback |  |  |
| macOS x64 included for `v1.1.0-rc1` |  |  |
| Stable-line PR behavior absorbed or intentionally deferred |  |  |

## Automated Gates

| Gate | Command | Result |
|---|---|---|
| Format | `cargo fmt --all -- --check` |  |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` |  |
| Rust tests | `cargo test --workspace` |  |
| Rust UI build | `trunk build --release` |  |
| Tauri build | `cargo tauri build` |  |
| Release manifest | `cargo xtask verify --stage release` |  |

## Asset Gate

| Asset | Present | Hash | Signature |
|---|---|---|---|
| Windows Setup |  |  |  |
| Windows Portable zip |  |  |  |
| Windows x64 exe |  |  |  |
| macOS arm64 pkg |  |  |  |
| macOS arm64 dmg |  |  |  |
| macOS x64 pkg |  |  |  |
| macOS x64 dmg |  |  |  |
| `latest.json` |  |  |  |

## Manual Smoke

| Platform | Scenario | Result |
|---|---|---|
| Windows | Install over stable version |  |
| Windows | Apply provider and verify registry readback |  |
| Windows | Update launches installer |  |
| macOS arm64 | Fresh configLibrary write/readback |  |
| macOS x64 | Install and launch |  |
| macOS x64 | Apply provider and verify Desktop policy |  |

## Readiness Verdict

- Ready / Not ready:
- Blockers:
- Residual risk:
- User confirmation:
