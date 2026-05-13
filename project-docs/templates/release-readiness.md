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
| Rust UI build | `Push-Location ui; trunk build --release; Pop-Location` |  |
| Tauri build | `cargo tauri build` |  |
| Release manifest | `cargo xtask verify --stage release` |  |
| RC readiness | `cargo xtask verify --stage rc-readiness` |  |
| Release signing key guard | `scripts/New-ReleaseManifest.ps1 -RequireExistingKey -ExpectedPublicKeySha256 <pinned-sha256>` or release workflow equivalent |  |
| Release publish guard | Manual workflow dispatch, `confirm_publish=PUBLISH_LATEST`, and `release-publish` environment only when publishing Latest |  |

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
| `latest.json.sha256` |  |  |  |
| `latest.json.sig` |  |  |  |
| Release public key matches runtime pin |  |  |  |

## Manual Smoke

| Platform | Scenario | Result |
|---|---|---|
| Windows | Install over stable version |  |
| Windows | Apply provider and verify registry readback |  |
| Windows | Update launches installer |  |
| macOS arm64 | Fresh configLibrary write/readback |  |
| macOS x64 | Install and launch |  |
| macOS x64 | Apply provider and verify Desktop policy |  |
| macOS arm64/x64 | Fresh current-phase non-publishing workflow evidence collected |  |

## Readiness Verdict

- Ready / Not ready:
- Blockers:
- Residual risk:
- User confirmation:
