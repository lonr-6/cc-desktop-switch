# P83 Subagent Review Hardening Root Cause

Date: 2026-05-13

## Summary

Four review passes found that several newly wired Rust mainline features had command/UI coverage but still missed fail-closed or protocol-equivalence details:

- runtime update download trusted manifest-controlled asset names before verifying signed `latest.json` metadata;
- update install could be called with an arbitrary local path instead of only the verified update staging output;
- clearing Claude Desktop config could mutate local config before managed-policy evidence was handled;
- OpenAI-compatible gateway conversion did not preserve tool calls and tool results;
- UI exposed a few visible settings controls that were not connected to runtime behavior;
- changing the active Provider did not refresh a running gateway runtime;
- second-pass `xhigh` review found that update signature trust was not pinned, selected sidecars were not mandatory, upstream error/SSE previews could still leak raw upstream model names, unauthenticated gateway helpers were too easy to call outside tests, and some UI controls still only changed local state;
- third-pass preparation found that selected Provider state and active Provider state were still conflated in the UI, `proxy_port` was missing from the running gateway fingerprint, the update trust override was not test-only, and Add Provider static-check buttons could imply draft validation while calling the active-provider command;
- third-pass review found the release workflow could still generate a public key that did not match the runtime pin, `download_update` could delete an app-owned version staging directory before metadata signature verification, successful gateway responses could still echo raw upstream model text outside error paths, and Provider/UI status controls could still imply actions or green states not backed by backend state.
- follow-up review found Add Provider could still reuse the selected Provider ID and overwrite an existing Provider, Apply did not refresh health/proxy state after success, `check_update` still trusted unsigned metadata for preview, stale verified update paths could remain after no-update/error results, the SSE frame parser only recognized LF separators, the release workflow still needed a manual-only publish boundary with macOS x64 in the release build matrix, and `rc-readiness` could still pass by matching old macOS evidence.
- fourth-round review found Dashboard edit still missed the new edit-target signal, stale Desktop readiness could survive provider/mapping/settings changes, Apply overwrote unknown `_meta.json` metadata, macOS collectors did not emit the P83 phase marker required by the fresh-evidence gate, unmapped raw-looking route errors/logs could echo request model text, and route-aware upstream previews replaced raw models only after truncation.
- fifth-round review found Add Provider had a second active-provider path that did not invalidate Desktop readiness, model mapping error handling did not refresh backend truth after partial mutation failures, `rc-readiness` could match broad P83 substrings instead of literal `phase: P83`, release docs still used route-to-upstream wording, Apply dry-run returned the real config-scoped gateway key in the UI/Tauri payload, and unmapped `claude-*` requests containing raw-like names could still be stored as runtime log route labels.
- sixth-round review found Apply/clear failure paths could keep old readback-green readiness, model-mapping error refresh could switch selection away from the edited Provider, UI Provider export returned Provider API keys to the WebView, dry-run schema exposed an internal `plan` shape while the UI silently ignored `planError`, clear local config deleted CCDS config before `_meta.json` could fail, update staging replacement could delete old verified staging before a failed rename, and settings port refresh could persist a bad port while stopping the old gateway.
- seventh-round review found Add Provider one-click Apply did not save model mappings for the Provider being edited, the production Tauri command surface still exposed the fixture-only `apply_local_config` command, imported mapping validation was weaker than edit-time route validation, diagnostics missed dash-form `ccds-gw-*` gateway keys, backend settings accepted `proxy_port=0`, apply could leave a newly written config entry if shared `_meta.json` failed afterward, and the publish job needed an explicit `rc-readiness` preflight before asset upload.
- eighth-round review found P83 macOS evidence could be forged by re-collecting old artifacts with a new `phase: P83`, macOS real Desktop evidence accepted only one passing architecture, UI treated `set_active_provider Ok(false)` as success, stopped Proxy state kept a green outer status dot, the shared settings refresh path could still stop the old gateway on a port-change race, legacy `scripts/New-Release.ps1` could generate unpinned metadata, and the Public Latest runbook checklist was weaker than the release workflow.
- ninth-round frontend review found the independent Add Provider model-mapping load/save controls still fell back from `editing_provider_id` to `selected_provider_id`, so a new Provider draft could accidentally overwrite the selected/active Provider's mappings.

## Root Cause

These were boundary defects rather than missing screens. The P80-P82 slices focused on restoring visible parity quickly, so some paths were wired at the command surface first and hardened afterward:

- release verification already had strict bundle validation, but runtime update download did not reuse enough of that validation before trusting asset metadata;
- the install command boundary accepted a path-like argument without tying it back to the app-owned update staging directory;
- Desktop clear reused local writer behavior without making managed config a pre-mutation blocker;
- gateway OpenAI conversion covered text and streaming semantics before tool-use parity;
- UI parity restored layout and actions, but left a few legacy settings affordances visible before backend support existed;
- Provider state changes and gateway lifecycle were implemented in separate modules, so active-provider changes did not automatically refresh the in-memory runtime.
- The runtime update verifier accepted the manifest public key as data but did not anchor it to a pinned application trust fingerprint.
- Gateway redaction handled broad secrets but did not route-normalize every upstream preview path or parse Anthropic SSE frames before replacement.
- UI state reused one signal for both selected row/edit target and backend active Provider, so visual default labels could drift from the actual runtime active provider.
- Gateway runtime fingerprint omitted `proxy_port`, so a settings save could call refresh but still reuse the old listener.
- The release manifest script allowed auto-creating local signing keys in the same path used by the workflow, while runtime update verification had moved to a pinned public-key fingerprint.
- Runtime update download reused the final version staging directory before signed metadata was validated, so a bad manifest could delete old app-owned staging output before being rejected.
- Gateway response normalization focused first on top-level `model`, error previews, and SSE model fields, but not every successful response string value such as ids, content, or tool-call arguments.
- Provider management still had toolbar actions driven by an implicit selected row even after active/default state moved to backend truth, and Desktop/Proxy status styling still had optimistic green defaults.
- The UI did not model edit target as a separate concept from selected row, so "Add Provider" could inherit an existing row identity.
- Update preview and readiness checks were too optimistic: they validated the downloadable installer path later in the flow, but not signed metadata during `check_update`, and they kept stale verified paths after failed/no-update checks.
- Release automation mixed staging and publishing concerns in one workflow shape, so a tag or workflow mistake could still publish Latest before human approval.
- RC readiness matching was evidence-shaped but not phase-shaped; it could prove that old P75 artifacts passed while failing to prove that the current P83 artifacts passed.
- The UI edit-target fix was first applied to the Provider list path but not every duplicate edit entry point on the Dashboard.
- Desktop readiness was cached as a status snapshot but did not have a consistent invalidation rule for every command that changes Provider, model mapping, settings, import, or delete state.
- The local config writer treated `_meta.json` as a CCDS-owned file during Apply, while clear already treated it as shared metadata.
- Gateway unmapped-route diagnostics reused request model text in user-facing error and runtime log paths, which is useful for debugging but unsafe when the request itself may contain a raw upstream model name.
- Upstream preview truncation happened before route replacement, so a raw model split at the preview boundary could leave a raw-looking prefix.
- Some UI actions had duplicate entry points, so the first stale-readiness fix covered the main Provider list but missed the Add Provider side panel.
- Model mapping mutations can persist config before a later runtime refresh fails, so the UI must reload backend truth on error instead of keeping optimistic draft state.
- Dry-run reused the full Desktop plan shape, including the config-scoped gateway API key, even though dry-run only needs non-secret topology evidence.
- Readiness handoff matching looked for the phase value too loosely, so unrelated prose could satisfy a phase check without a collector-generated marker.
- UI export reused the full import/export package for display convenience, but that package is intentionally secret-bearing for file backup and should not cross into the WebView preview path.
- Gateway refresh and update staging replacement stopped or deleted the old working state before proving the new state could take over.
- Clear Desktop config handled local config and `_meta.json` as separate mutations without ordering them to keep the old state intact on early failure.
- Add Provider reused multiple command paths that were not all brought under the same "save mappings -> set active -> apply" sequence.
- Some fixture affordances and import paths survived beyond their original test boundary, so they bypassed the stricter production probe and route-validation rules.
- Redaction and settings validation had format/range coverage for common cases but missed a generated gateway-key shape and an OS-invalid port value.
- Release publish safety was split between workflow inputs and release environment gates, but the publish job still needed to prove the current repository evidence gates before upload.
- Fresh-evidence gating used phase-shaped text but not commit-shaped evidence, so a stale workflow artifact could be restamped into a current-looking handoff.
- The macOS real smoke workflow ran per matrix architecture, but the collector collapsed evidence to the first pass instead of preserving the matrix requirement.
- UI command handling assumed `Ok(false)` meant an idempotent success, but the backend uses `false` for provider-not-found.
- Gateway restart logic reused a shared stop-on-error helper for settings port changes even though a port change can keep the old listener alive until the new one is ready.
- A legacy release script remained executable after the guarded release workflow became the only acceptable mainline publish path.
- Model mapping controls used the same target resolver for edit and create flows, even though a new draft does not yet have a Provider ID.

## Fix Strategy

- Verify `latest.json`, `latest.json.sha256`, `latest.json.sig`, and public key before reading installer asset metadata.
- Reject manifest-controlled filenames unless they are plain file names with no path separators, roots, drive prefixes, or traversal.
- Restrict `install_update` to the config-scoped `updates/<version>/` staging directory and re-run bundle validation before launching an installer.
- Make `clear_desktop_config` fail closed when `DesktopConfigProbe` reports managed evidence.
- Convert Anthropic `tools`, `tool_choice`, `tool_use`, and `tool_result` to OpenAI Chat equivalents, and convert OpenAI `tool_calls` responses and streaming deltas back to Anthropic-style `tool_use`.
- Use Anthropic-style gateway error envelopes for local gateway errors.
- Replace raw upstream model names with the safe route in route-aware upstream error previews and parsed Anthropic SSE data frames.
- Make unauthenticated gateway constructors private/test-only; production runtime must go through key-authenticated constructors.
- Remove or wire visible no-op UI controls; make Provider enable call the real command; disable installer launch until there is a verified download path and clear stale verified paths on URL/check/download failures.
- Refresh the running gateway after active Provider, Provider data, mappings, settings, import, or delete changes.
- Pin runtime update trust to the known release public-key fingerprint, with an explicit environment override only for tests/local fixtures.
- Keep the trust override behind `#[cfg(test)]`; production builds always use the pinned fingerprint.
- Split UI `selected_provider_id` from `active_provider_id`, and only render default/enable state from backend active state.
- Include `proxy_port` in gateway runtime fingerprint and cover it with a regression test.
- Make Add Provider static-check buttons explicitly check the active Provider until a draft-specific command exists.
- Require the release workflow to inject an existing release signing key from secrets and validate the public key SHA256 against the runtime pin before manifest/signature generation can proceed.
- Write runtime update downloads to a unique temporary staging directory and replace `updates/<version>/` only after metadata, public key, installer hash, and installer signature verification pass.
- Normalize raw upstream model strings recursively across successful Anthropic/OpenAI response payload values and OpenAI streaming text/tool-call deltas.
- Remove implicit selected-row Provider toolbar mutations and render Desktop/Proxy status colors from backend readiness/runtime state.
- Add a dedicated UI edit-target signal so Add Provider starts with `provider_id=None`; edit/save/apply use that edit target rather than the selected row.
- Refresh health and proxy status after Apply, validate proxy ports strictly, and clear verified update paths whenever URL/check/download/no-update state invalidates them.
- Verify signed `latest.json` metadata during `check_update`, not only during download/install.
- Parse SSE frames using both LF and CRLF separators.
- Make the release workflow `workflow_dispatch` only, build Rust/Tauri Windows plus macOS arm64/x64 artifacts, assemble signed staging assets, and require explicit `confirm_publish=PUBLISH_LATEST` plus `release-publish` environment for Latest publication.
- Require P83-specific macOS workflow and real Desktop handoff markers before `rc-readiness` can pass.
- Set the UI edit target in every edit entry point and invalidate Desktop readiness on every config-changing command.
- When a mutation command can save config before returning a gateway-refresh error, refresh UI provider/settings state from backend truth before displaying the error.
- Preserve unknown `_meta.json` fields and only replace active config id keys during Apply.
- Emit `phase: P83` from macOS evidence collectors by default.
- Use a generic unmapped-route message for gateway 400 responses and omit non-`claude-*` request models from runtime log route fields.
- Replace route-aware raw upstream model text before truncating error previews.
- Apply the same stale-readiness invalidation to Add Provider's inner active-provider path.
- Refresh backend mappings after model mapping mutation failures.
- Use a fixed placeholder gateway key in dry-run output instead of the real config-scoped gateway key.
- Require literal `phase: P83` evidence in `rc-readiness`.
- Return a redacted Provider export package to the UI; keep full Provider exports limited to `Save as` file output.
- Return a command-specific dry-run DTO without the internal secret-bearing `plan`, and surface `planError` explicitly.
- Invalidate readiness at the start of Apply/clear actions and only write a success health snapshot after successful Apply.
- Preserve the edited Provider selection when refreshing backend truth after model-mapping mutation failures.
- Rewrite clear local config ordering so `_meta.json` parse/write failures happen before deleting the CCDS config file.
- Replace update staging via a rollback directory so an old verified staging directory can be restored if the final rename fails.
- Preflight settings port changes and restore old settings if gateway refresh fails; when moving to a different port, keep the old listener running until the new listener is ready.
- Save edited Provider model mappings before Add Provider one-click Apply changes the active Provider or writes Claude Desktop config.
- Remove fixture-only `apply_local_config` from the production Tauri handler and command surface.
- Route imported mappings through the same `validate_desktop_route_id` policy used by model mapping edits.
- Extend diagnostics redaction to `ccds-gw-*` gateway key strings.
- Reject `proxy_port=0` in backend settings validation.
- Restore the previous local config entry when apply writes config successfully but shared `_meta.json` write fails.
- Run `cargo xtask verify --stage rc-readiness` inside the publish job before any signed assets are uploaded to Latest.
- Add `ExpectedCommit` checks to macOS evidence collectors, defaulting to current `git rev-parse HEAD`, and write top-level `expected_commit`, `commit_arm64`, and `commit_x64` fields into handoff summaries.
- Require macOS real Desktop pass evidence/logs for both `arch: arm64` and `arch: x86_64`.
- Make `rc-readiness` match current-commit P83 macOS evidence, not just phase text.
- Treat `set_active_provider Ok(false)` as a failed mutation and refresh backend Provider state before reporting the result.
- Use a neutral stopped proxy dot and disable non-active Provider speed-test buttons.
- For settings port changes, start the new listener before stopping the old one, stop the old listener after success, and keep old listener/settings on failure.
- Disable `scripts/New-Release.ps1` in this worktree and document the guarded release path in the Rust mainline workflow runbook.
- Make independent model-mapping load/save require `editing_provider_id`; new Provider drafts write mappings only through the save/apply path after a real Provider ID exists.

## Regression Coverage

- `cargo test -p cc-desktop-switch --lib update -- --nocapture`
- `cargo test -p cc-desktop-switch --lib release_gate -- --nocapture`
- `cargo test -p cc-desktop-switch --lib gateway_adapter -- --nocapture`
- `cargo test -p cc-desktop-switch --lib gateway -- --nocapture`
- `cargo test -p cc-desktop-switch --lib state -- --nocapture`
- `Push-Location ui; trunk build --release; Pop-Location`
- `cargo test -p cc-desktop-switch --lib settings_port_change_refreshes_running_gateway_runtime -- --nocapture`
- `cargo test -p cc-desktop-switch --lib anthropic_sse_normalizer_handles_split_frames_and_model_whitespace -- --nocapture`
- `cargo xtask verify --all`
- `release_gate::tests::powershell_manifest_rejects_public_key_fingerprint_mismatch`
- `update::tests::download_update_keeps_existing_staging_when_metadata_verification_fails`
- `gateway_adapter::tests::forward_anthropic_passthrough_replaces_raw_model_in_success_payload_values`
- `gateway_adapter::tests::openai_chat_response_replaces_raw_model_in_success_payload_values`
- `gateway_adapter::tests::forward_openai_stream_replaces_raw_model_in_success_payload_values`
- `gateway_adapter::tests::sse_normalizer_accepts_crlf_frame_boundaries`
- `update::tests::check_update_rejects_unsigned_metadata_preview`
- `desktop_writer::tests::desktop_writer_preserves_unrelated_local_config_meta_values`
- `gateway::tests::router_messages_endpoint_does_not_echo_raw_unmapped_model`
- `gateway_adapter::tests::route_preview_replaces_raw_model_before_truncation`
- `cargo test -p cc-desktop-switch --lib dry_run_never_reports_applied_success -- --nocapture`
- `config::tests::provider_export_redacted_package_omits_api_keys_for_ui`
- `desktop_writer::tests::clear_local_config_library_does_not_delete_config_when_meta_parse_fails`
- `state::tests::settings_port_change_to_occupied_port_keeps_existing_gateway_and_settings`
- `config::tests::provider_import_rejects_duplicate_ids_and_raw_route_ids`
- `diagnostics::tests::redacts_keys_headers_cookies_and_url_tokens`
- `state::tests::update_settings_rejects_zero_proxy_port`
- `rg` guard confirmed `apply_local_config` is no longer present in current Tauri command code.
- Current-commit collector fixture checks: platform and real collectors pass with matching commit, platform collector rejects a bad commit, and real Desktop collector rejects single-architecture evidence.
- `scripts/New-Release.ps1` disabled guard exits non-zero.
- `state::tests::settings_port_change_refreshes_running_gateway_runtime` now also verifies the old port closes after a successful port change.
- Ninth-round frontend mapping fix: `cargo fmt --all -- --check`, `trunk build --release`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo xtask verify --all`.
- `scripts/macos/Collect-PlatformSmokeEvidence.ps1` and `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` collector shape check under `target/p83-collector-check/`
- `cargo xtask verify --stage rc-readiness` now reports the two missing macOS requirements using literal `phase: P83` needles.
- `git diff --check`
- Full workspace gate and xhigh review are tracked in the P83 handoff.

## Remaining Risk

Real installer launch, Windows in-place upgrade behavior, and macOS arm64/x64 smoke still require fresh package artifacts and human or workflow evidence. `rc-readiness` now intentionally fails until P83-specific macOS workflow and real Desktop evidence exist. P83 does not publish, tag, or upload release assets.
