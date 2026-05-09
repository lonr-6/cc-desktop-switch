# Task Card: Rust Mainline Rebuild

Date: 2026-05-08

## Goal

在新 worktree 中重构 CC Desktop Switch 的未来主线：

- Rust/Tauri 作为正式主线。
- UI 改为纯 Rust UI。
- 保留现有功能。
- 根治已知 issue 和历史问题。
- 旧布局保持，但状态、错误、诊断更清楚。

## Scope

In scope:

- Tauri v2 desktop shell
- Leptos/Trunk Rust UI
- Config storage and migration
- Provider CRUD, presets, model mapping, import/export
- Local gateway
- Claude Desktop Windows/macOS write/readback/health
- Diagnostics package
- Update flow
- Rust `xtask` release pipeline
- Windows and macOS packaging

Out of scope for first RC:

- Linux GUI
- Cloud account sync
- Paid support workflow
- Full pure native GUI outside Tauri
- New marketing-style landing UI

## Architecture Work Packages

| Package | Owner | Deliverable | Done when |
| --- | --- | --- | --- |
| P0 Governance | Main | `AGENTS.md`, `project-docs/*`, architecture docs | `PLANS.md`, skills, parity matrix, eval harness exist |
| P1 Skeleton | Main/Worker | Tauri + Leptos minimal app opens | Rust UI spike criteria pass or alternative stack decision exists |
| P2 ModelCatalog | Worker | model route tests, 1M/Max capability tests | No raw model names, no Default menu, no Default fallback, unmapped route 400 |
| P3 ConfigStore | Worker | compatible config load/save/import/export | Old config migrates with backup and schema tests |
| P4 DesktopApplyFlow | Worker | planner + health + platform writer interfaces | Failed step cannot show applied; readback mismatch is visible |
| P5 Gateway | Worker | Anthropic/OpenAI adapters, SSE, auth, logs | Non-JSON upstream, unmapped route, capability errors covered |
| P6 Diagnostics | Worker | redacted diagnostics package and issue fingerprints | Redaction tests prove no secrets leak |
| P7 Update | Worker | download/verify/installer launcher with logs | Download/hash/installer launch states are distinct |
| P8 UI | Worker | old layout rebuilt in Rust UI | Main flows fit old layout and pass keyboard/basic visual smoke |
| P9 Release | Worker | Windows/macOS assets, latest.json, signatures | Release gate blocks missing platform assets |
| P10 Review | Reviewers | correctness/security/release/UI review | Review report lists findings or residual gaps |

## Required Root Fixes

1. Unified local gateway path.
2. Claude-safe model routes only.
3. `Default` is only a form/config convenience and never a runtime fallback.
4. No raw provider model names in `inferenceModels`.
5. Unmapped routes return 400.
6. macOS `configLibrary` auto-create and verify.
7. Windows update installer launcher does not exit before installer is confirmed.
8. Full redacted diagnostics.

## Acceptance Criteria

- Existing `~/.cc-desktop-switch/config.json` can be read.
- Provider CRUD, presets, model mapping, backup, import/export work.
- `Apply to Claude Desktop` never reports success unless readback matches expected state.
- Claude Desktop model menu shows only expected safe routes.
- Gateway logs route mapping clearly.
- Diagnostics package contains no secrets.
- Windows x64, macOS arm64, and macOS x64 release assets can be produced before `v1.1.0-rc1` and Latest.
- Manual Windows and macOS smoke tests pass before public release.

## Verification Plan

Minimum automated checks after implementation:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
trunk build --release
cargo tauri build
```

Platform checks:

- Windows registry read/write/readback.
- Windows installer old directory detection.
- Windows update installer launch.
- macOS plist/root JSON/configLibrary write/readback.
- macOS fresh configLibrary creation.
- macOS 1M supports1m readback.

## Notes

- This task card is planning/governance only; implementation starts after the skeleton phase is explicitly opened.
- Any worker result that changes long-term rules must update `project-docs/decisions/` or `project-docs/bugs/`.
