# Stage Summary: P7/P8 Post-Review Hardening

Date: 2026-05-08

## Stage

Post-review hardening after read-only architecture and requirements-equivalence reviews.

## Review Inputs

- Architecture review found that Desktop planner fallback to the DeepSeek fixture and `unwrap_or_default()` could hide invalid mappings.
- Architecture review found readback comparison was too loose for mode/auth/key/header/extra routes.
- Requirements review found parity gaps in Provider CRUD/import/export, gateway upstream adapter, diagnostics export, platform writers, app shell, and release gates.

## Completed

- Changed `build_desktop_plan()` to return `Result<DesktopPlan, RouteError>`.
- Removed Desktop planner fallback to `ModelCatalog::for_provider()` fixtures on production paths.
- Added failure for active providers with no Desktop-visible explicit mappings.
- Kept raw/invalid route errors from `ModelCatalog` instead of swallowing them.
- Tightened Desktop readback health:
  - mode must match `local_gateway`;
  - auth scheme must match;
  - gateway key presence must match;
  - gateway headers must match;
  - actual route set cannot omit expected routes or keep extra stale routes;
  - `supports1m` and `supportsMax` must match.
- Preserved existing model mappings when provider metadata/API key is updated.
- Passed configured `proxyPort` into readiness, dry-run, and Desktop plan base URL.
- Added Axum gateway router/server skeleton for `/v1/models` and `/v1/messages`.
- Kept public `/v1/messages` skeleton from echoing upstream model names before upstream forwarding is implemented.
- Added diagnostics redaction core for keys, gateway keys, Authorization, Cookie, secret headers, URL userinfo, query tokens, and upstream body previews.
- Added Windows release GUI subsystem to avoid black terminal in release builds.
- Added `cargo xtask verify --stage diagnostics` and `cargo xtask verify --all`.

## Changed Files

- `Cargo.lock`
- `src-tauri/Cargo.toml`
- `src-tauri/src/config.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/desktop.rs`
- `src-tauri/src/diagnostics.rs`
- `src-tauri/src/gateway.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/state.rs`
- `xtask/src/main.rs`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-08-p7-post-review-hardening-summary.md`

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed: 31 tests.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo xtask verify --stage config` passed.
- `cargo xtask verify --stage desktop` passed.
- `cargo xtask verify --stage gateway` passed.
- `cargo xtask verify --stage diagnostics` passed.
- `trunk build --release` passed from `ui/`.
- `cargo tauri build` passed on Windows x64 and produced MSI/NSIS bundles.
- `cargo xtask verify --all` passed and reran the full local gate.

## Remaining Blockers

- No real upstream adapter yet: Anthropic passthrough, OpenAI Chat conversion, SSE conversion, and upstream redacted diagnostics are still deferred.
- Gateway server is not yet managed by Tauri app lifecycle.
- Windows registry writer and macOS configLibrary writer are not implemented.
- Provider edit/delete/reorder/import/export parity is not implemented.
- Diagnostics export package and GitHub Issue flow are not implemented.
- Tray and single-instance behavior are not implemented.
- macOS arm64/x64 build and smoke gates have not run.

## Next Step

Implement the upstream adapter track: start with Anthropic passthrough and OpenAI Chat conversion fixtures, then attach redacted invalid-upstream-response diagnostics before enabling a real apply flow.
