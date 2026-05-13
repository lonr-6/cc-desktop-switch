# P83 Subagent Review Hardening Summary

Date: 2026-05-13

## Scope

P83 addresses the first frontend/backend/joint/API subagent review findings before the next human smoke package. The focus is root-cause hardening, not adding new parity features.

After the first through eighth `xhigh` re-review loops, the same four review lanes found additional trust-boundary, state-refresh, gateway normalization, release workflow, evidence-freshness, interface-contract, and UI truth-state issues. Those findings are treated as part of P83 and fixed before the next `xhigh` review loop.

## Implemented

- Runtime update download validates signed metadata before trusting asset selection.
- Runtime update validation pins the release public-key fingerprint and requires latest/selected-asset sha/signature sidecars.
- Runtime update trust override is test-only; production builds always use the pinned public-key fingerprint.
- Runtime install is limited to app-owned update staging output and revalidates the bundle before launch.
- Manifest-controlled file names are plain-file-name only.
- Clear Claude Desktop config blocks before mutation when managed config evidence exists.
- OpenAI-compatible gateway conversion now covers tool definitions, tool choice, tool result messages, response tool calls, and streaming tool-call deltas.
- Gateway local/upstream errors use an Anthropic-style error envelope and route-aware previews so raw upstream model names do not leak to Claude Desktop.
- Anthropic SSE normalization parses complete frames and replaces upstream model fields with the safe route even when frames are split across chunks.
- Anthropic SSE normalization also replaces raw upstream model substrings inside JSON string values, such as upstream error messages.
- Unauthenticated gateway constructors are private/test-only; production runtime uses authenticated constructors.
- Authenticated gateway constructors fail fast if called with an empty key instead of silently producing an unauthenticated router.
- Active Provider, Provider data, mappings, settings, import, delete, and `proxy_port` changes refresh a running gateway runtime.
- UI removes or wires visible no-op settings controls, makes Provider enable call the real command, splits selected Provider from backend active Provider, labels local gateway auth as required, makes restart messaging generic, disables installer launch until a verified download exists, clears stale verified update paths on URL/check/download failures, separates import preview from import, and makes Add Provider static checks explicit about targeting the current active Provider.
- Release workflow manifest generation now requires existing signing keys from GitHub secrets and checks the public key SHA256 against the runtime-pinned fingerprint before publishing assets can proceed.
- Runtime update download now uses a unique temporary staging directory and replaces `updates/<version>/` only after signed metadata and selected installer bundle verification pass.
- Successful Anthropic/OpenAI gateway responses, OpenAI stream content, and OpenAI tool-call delta ids/names/arguments now recursively replace raw upstream model text with the safe route.
- Provider management no longer exposes selected-row toolbar mutations without an explicit visible selection; row actions are command-backed. Desktop and Proxy status colors now follow backend readiness/runtime state instead of defaulting to green.
- Add Provider now clears the edit target instead of reusing the selected Provider ID, so creating a new Provider cannot overwrite the current/selected Provider.
- Apply refreshes health and proxy status after success, proxy port parsing is strict, and stale verified update paths are cleared on URL/check/download/no-update failures.
- `check_update` validates signed `latest.json` metadata before reporting available updates.
- SSE frame parsing accepts both LF and CRLF frame boundaries.
- The release workflow is manual-only, builds Rust/Tauri Windows plus macOS arm64/x64 artifacts, assembles signed staging assets from existing signing secrets, and only publishes Latest when `confirm_publish=PUBLISH_LATEST` and the `release-publish` environment gate allow it.
- The publish job runs `cargo xtask verify --stage rc-readiness` before downloading/uploading signed assets, so Latest cannot be published unless P83 macOS workflow and real Desktop evidence are already recorded.
- `rc-readiness` now requires P83-specific macOS arm64/x64 workflow evidence and P83-specific macOS real Desktop evidence, so old P75 evidence cannot mark the current code ready.
- Dashboard Provider edit now sets the edit target, not only selected row state.
- Provider/settings/model/import/delete mutations clear old Desktop readiness so old readback green cannot survive a config change.
- If a backend mutation writes config but gateway refresh fails, the UI pulls the latest backend config/settings state before reporting the error.
- Desktop apply writes `_meta.json` by preserving unknown metadata and only replacing active config id fields.
- macOS evidence collectors emit `phase: P83`, matching the fresh-evidence readiness gate without manual hand-editing.
- Unmapped raw-looking model requests return 400 without echoing the raw model in the response or runtime log route field.
- Upstream error previews replace raw upstream model text before truncation.
- Add Provider inner active-provider actions invalidate stale Desktop readiness the same way as the main Provider list.
- Model mapping save failures refresh backend mappings before showing the error, so a partial backend save plus gateway refresh failure cannot leave stale draft mappings in the UI.
- Apply dry-run uses a fixed placeholder gateway key in its plan payload and does not pass the real config-scoped gateway key through the UI/Tauri response.
- `rc-readiness` now matches literal `phase: P83` evidence, not a broad `P83` substring.
- Add Provider one-click Apply now saves model mappings for the current edit target before setting that Provider active and applying to Claude Desktop.
- The production Tauri command surface no longer exposes the fixture-only `apply_local_config` command.
- Imported Provider model mappings now reuse the same Desktop route validation policy as edits, including duplicate route, raw route, and `claude-default` rejection.
- Diagnostics redaction now covers dash-form gateway keys such as `ccds-gw-*`.
- Backend settings validation rejects `proxy_port=0` instead of accepting an unusable listener port.
- Desktop apply attempts to restore the previous local config if writing shared `_meta.json` fails after writing the config entry.
- P83 macOS evidence collectors now bind artifact evidence to the expected/current commit and write `expected_commit`, `commit_arm64`, and `commit_x64` into handoff summaries.
- The macOS real Desktop evidence collector now requires both arm64 and x86_64 pass evidence/logs instead of accepting the first passing artifact.
- `rc-readiness` now requires current-commit P83 macOS workflow and real Desktop evidence markers.
- `scripts/New-Release.ps1` is disabled in the Rust mainline worktree so humans cannot use the old auto-key release path by mistake.
- UI `set_active_provider Ok(false)` is treated as failure with backend refresh, and stopped proxy status no longer keeps a green outer dot.
- Settings port changes now start the new gateway without stopping the old listener first, stop the old listener after success, and leave the old listener/settings intact on failure.
- Independent model-mapping load/save controls now require an existing editing Provider and no longer fall back to the selected Provider while creating a new Provider draft.

## Verification Run

- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo test --workspace` passed in the latest full gate: 153 passed, 2 ignored real Desktop smoke tests.
- `trunk build --release` passed from `ui/`. Running the same command at repository root is not the current UI build command and fails because Trunk cannot find the UI root package there.
- `cargo tauri build` passed and produced Windows MSI/NSIS bundles under `target/release/bundle/`.
- `cargo xtask verify --stage rc-readiness` intentionally failed with 2 missing P83-specific macOS evidence requirements: fresh macOS arm64/x64 workflow evidence and fresh macOS real Desktop local config smoke evidence.
- `cargo test -p cc-desktop-switch --lib settings_port_change_refreshes_running_gateway_runtime -- --nocapture` passed.
- `cargo test -p cc-desktop-switch --lib anthropic_sse_normalizer_handles_split_frames_and_model_whitespace -- --nocapture` passed.
- `cargo xtask verify --all` passed after the latest review fixes: 153 passed, 2 ignored real Desktop smoke tests; Windows MSI/NSIS bundles were rebuilt at `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi` and `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`.
- `scripts/macos/Collect-PlatformSmokeEvidence.ps1` and `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` were syntax-checked against existing downloaded artifacts with output under `target/p83-collector-check/`; this only verifies collector output shape and is not counted as fresh P83 handoff evidence.
- The earlier seventh-round local gate passed on Windows x64: `cargo fmt --all -- --check`, `trunk build --release` from `ui/`, `cargo test --workspace` (153 passed, 2 ignored real Desktop smoke tests), `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo xtask verify --all`.
- Focused checks after the interface/backend fixes also passed: `cargo test -p cc-desktop-switch --lib provider_import_rejects_duplicate_ids_and_raw_route_ids -- --nocapture`, `cargo test -p cc-desktop-switch --lib redacts_keys_headers_cookies_and_url_tokens -- --nocapture`, `cargo test -p cc-desktop-switch --lib update_settings_rejects_zero_proxy_port -- --nocapture`, and an `rg` guard confirming `apply_local_config` is no longer exposed from current command code.
- `cargo xtask verify --stage rc-readiness` intentionally failed with 2 missing requirements, both literal `phase: P83` macOS evidence gates.
- `git diff --check` reported no whitespace errors; it only printed normal Windows line-ending warnings for modified files.
- Sixth-round review fixes added fail-closed Apply/clear readiness invalidation, model-mapping error refresh that preserves the edited Provider, redacted UI Provider export, a dry-run DTO without the internal `plan`, fail-closed clear ordering for invalid `_meta.json`, rollback-preserving update staging replacement, and settings port rollback that keeps the old gateway alive on occupied new ports.
- Eighth-round review fixes were verified with current-commit collector fixtures: platform/real collectors pass with matching commit, platform collector rejects a bad commit, and real Desktop collector rejects single-architecture evidence.
- `cargo test -p cc-desktop-switch --lib settings_port_change_refreshes_running_gateway_runtime -- --nocapture`, `settings_port_change_to_occupied_port_keeps_existing_gateway_and_settings`, and `update_settings_rejects_zero_proxy_port` passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/New-Release.ps1` was verified to fail closed.
- A fresh post-eighth-round local gate passed on Windows x64: `cargo fmt --all -- --check`, `trunk build --release` from `ui/`, `cargo test --workspace` (153 passed, 2 ignored real Desktop smoke tests), `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo xtask verify --all`.
- Latest `cargo xtask verify --stage rc-readiness` intentionally fails with 2 missing current-commit P83 macOS evidence requirements; the missing output includes `expected_commit`, `commit_arm64`, and `commit_x64` needles.
- Ninth-round frontend mapping fix passed `cargo fmt --all -- --check`, `trunk build --release`, `cargo test --workspace` (153 passed, 2 ignored real Desktop smoke tests), `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo xtask verify --all`; Windows MSI/NSIS bundles were rebuilt after the fix.
- Tenth-round focused frontend re-review reported no blocker/high/medium/low for the mapping target issue. Earlier ninth-round backend, joint, and API/interface reviews were also clean. Remaining release hold is only the documented external P83 macOS workflow/real Desktop evidence and final human smoke.

## Review Loop

After the latest local gate, run four read-only `xhigh` subagent reviews again:

- frontend review: `ui/src/app.rs`, `ui/src/commands.rs`, `ui/styles.css`;
- backend review: update, release gate, desktop writer, commands, state;
- joint review: UI/backend/docs and smoke readiness;
- interface/API review: gateway, gateway adapter, command schema.

If any review reports a real blocker, high, or medium issue, fix it before creating final human smoke packages.

## Known Non-Goals

- No release publishing.
- No GitHub Release upload or `latest.json` update.
- No Python stable-line worktree changes.
- No macOS real-machine claim without actual workflow or human evidence.
