# Local Eval Harness

This project uses lightweight local evals instead of a separate eval platform at first.

## Purpose

Turn repeated community issues into deterministic checks. Evals are not just tests; each eval links a user symptom, an issue fingerprint, expected behavior, and evidence.

## Future Command Shape

The Rust workspace should eventually expose:

```powershell
cargo xtask verify --stage model-catalog
cargo xtask verify --stage apply-flow
cargo xtask verify --stage app-shell
cargo xtask verify --stage desktop
cargo xtask verify --stage desktop-config
cargo xtask verify --stage desktop-writer
cargo xtask verify --stage gateway
cargo xtask verify --stage gateway-lifecycle
cargo xtask verify --stage diagnostics
cargo xtask verify --stage file-picker
cargo xtask verify --stage provider-parity
cargo xtask verify --stage provider-import
cargo xtask verify --stage model-mapping
cargo xtask verify --stage provider-preset
cargo xtask verify --stage config-backup
cargo xtask verify --stage release
cargo xtask verify --stage rc-readiness
cargo xtask verify --all
scripts/windows/Collect-RealDesktopSmokeEvidence.ps1
scripts/macos/Collect-PlatformSmokeEvidence.ps1
scripts/macos/Collect-RealDesktopSmokeEvidence.ps1
```

Current `xtask` implements focused stages for `model-catalog`, `apply-flow`, `app-shell`, `config`, `config-backup`, `provider-service`, `provider-parity`, `provider-import`, `provider-preset`, `model-mapping`, `desktop`, `desktop-config`, `desktop-writer`, `gateway`, `gateway-lifecycle`, `diagnostics`, `file-picker`, `release`, `rc-readiness`, and `verify --all`.

## Eval Cases

| Fingerprint | Input | Expected result | Stage |
|---|---|---|---|
| `desktop.raw_model_names_detected` | Desktop policy contains `deepseek-v4-pro` | Health reports invalid raw model names and asks user to reapply | desktop |
| `desktop.stale_base_url` | Desktop actual gateway URL differs from expected | Health reports expected/actual URL | desktop |
| `desktop.config_library_missing` | macOS plist exists but active configLibrary entry missing | Health reports missing active configLibrary | desktop |
| `desktop.local_config_readback` | Local configLibrary is written from a Desktop plan | Readback matches base URL, auth, headers, safe routes, and capabilities | desktop-writer |
| `desktop.managed_config_detected` | Managed policy/config evidence exists | Apply blocks before gateway start and Desktop write; CC Desktop Switch-created policy also surfaces `desktop.ccds_managed_policy_detected` | desktop-config/apply-flow |
| `desktop.real_windows_local_config_smoke` | Windows real Claude Desktop local configLibrary is probed under explicit opt-in | Existing files are backed up, local config is written, read back, loopback gateway `/v1/models` passes, and files are restored; if old CC Desktop Switch managed policy blocks the run, cleanup uses an export-first guarded script | desktop-config/manual |
| `desktop.real_macos_local_config_smoke` | macOS real Claude Desktop local configLibrary is probed under explicit opt-in | Existing `_meta.json` and gateway config files are backed up, local config is written, read back, loopback gateway `/v1/models` passes, safe routes are verified, `Default` is absent, and files are restored | desktop-config/manual |
| `apply.false_success_blocked` | Provider missing or gateway port conflict occurs before write | Apply result has `success=false`, records failed step, and does not write Desktop config | apply-flow |
| `app.single_instance` | User launches a second app instance | Existing main window is shown/focused and a second independent runtime is not kept | app-shell |
| `app.tray_close_behavior` | User closes the main window or uses tray actions | Close hides the main window, tray show restores/focuses it, and tray quit exits explicitly | app-shell |
| `app.tray_default_icon` | Packaged app creates the tray/taskbar shell | Tray uses stable id, tooltip, and the bundled app default icon instead of a blank transparent icon | app-shell/manual |
| `desktop.one_million_not_written` | Route supports 1M but Desktop policy lacks `supports1m` | Health asks user to reapply/restart | desktop |
| `gateway.invalid_upstream_response` | OpenAI-compatible upstream returns HTTP 200 HTML | Anthropic-style structured error with redacted preview | gateway |
| `gateway.openai_stream_semantic_conversion` | OpenAI Chat SSE emits `chat.completion.chunk` frames | Gateway returns Anthropic-style SSE events and safe route model | gateway |
| `gateway.unmapped_model_route` | Claude Desktop requests an unmapped route | HTTP 400, no fallback to `Default` | gateway |
| `gateway.port_in_use` | Configured local gateway port is already bound | Start command fails and health carries the gateway startup issue code | gateway-lifecycle |
| `provider.crud_reorder_roundtrip` | Provider is edited, reordered, deleted, and the active provider changes | Config roundtrip preserves provider IDs, hides secrets in summaries, enforces exact reorder sets, and restarts gateway when active provider config changes | provider-parity |
| `provider.auth_scheme_roundtrip` | Imported or migrated Provider uses `x-api-key` or no auth header | Config migration, UI summary/save, and gateway upstream headers preserve the selected auth scheme instead of silently falling back to Bearer | config/gateway |
| `provider.import_export_roundtrip` | Provider export package or legacy CC-Switch config is imported | Dry-run reports conflicts before write, `Default` stays non-runtime, raw route IDs are rejected, and apply writes only when allowed | provider-import |
| `provider.import_skip_existing_merge` | Import package contains both conflicting and new Providers | Default import blocks conflict, `skipExisting` imports only new Providers, and existing Provider secrets/settings are preserved | provider-import |
| `provider.template_import` | User pastes a secretless Provider template package | Import preview/apply accepts no API key, accepts `openai_chat` alias, enforces `claude-*` routes, keeps `Default` non-runtime, rejects duplicate template IDs, rejects secret-bearing fields, and rejects non-HTTP(S) `baseUrl` | provider-import |
| `provider.marketplace_import` | User imports a Provider marketplace manifest | Import accepts only HTTPS plain source URLs, verifies the embedded template package sha256, rejects hash mismatch, rejects query/fragment/userinfo source URLs, and still reuses template secret/safe-route/Default validation | provider-import |
| `provider.model_mapping_edit` | User edits explicit model mappings for a Provider | Route aliases are generated or accepted only when `claude-*`, duplicate aliases are rejected, and `Default` remains non-runtime | model-mapping |
| `provider.preset_import` | User imports a built-in Provider preset | Preset import uses explicit safe routes, blocks conflicts until replace, and does not leak API key in summaries | provider-preset |
| `config.backup_redacted_readback` | Existing config backups are listed and opened from UI | Only files under the config `backups` directory can be read and returned text is redacted | config-backup |
| `provider.max_not_supported` | User requests Max on provider without support | Clear capability error | gateway |
| `provider.real_smoke_failed` | Static provider config is valid but minimal upstream request fails | Check result says static passed but real smoke failed with redacted error | gateway |
| `diagnostics.false_green_readiness` | Doctor/static checks pass but readback or real request smoke fails | UI must not show fully ready; diagnostics names the failed layer | diagnostics |
| `ui.readiness_dashboard` | User runs Health from the Leptos dashboard | Dashboard status cards and readiness list update from the latest `health` snapshot, distinguishing static config, Desktop readback, provider smoke, and gateway smoke instead of showing a false-green state | ui/manual |
| `ui.legacy_layout_parity` | User opens the Rust UI after the P71 visual pass | Header, pill navigation, dashboard status cards, large action buttons, recent operations, and Provider add/preset two-column flow preserve the old CC Desktop Switch layout feel while staying pure Rust/Leptos | ui/manual |
| `ui.current_frontend_parity` | User opens the Rust UI after the P75 latest-frontend parity slice | Header icon tabs, provider switch cards, provider assets, Add Provider two-column flow, red compatibility box, row-style model mapping, Proxy, Settings, Guide, CC Switch-style empty/provider card density, scroll behavior, and command-backed Provider/Proxy/Settings actions follow the current desktop frontend baseline; mobile is not an acceptance target | ui/manual |
| `diagnostics.gateway_runtime_logs` | Gateway start/stop/error events occur | Diagnostics package includes recent redacted runtime logs with issue codes | diagnostics/gateway-lifecycle |
| `diagnostics.secret_leak` | Logs/config contain API key, gateway key, Authorization, URL token | Export redacts all secrets | diagnostics |
| `diagnostics.file_picker_save` | User chooses a save location for diagnostics or Provider export | Tauri dialog plugin is registered, command returns cancel vs saved path, and saved JSON stays redacted where applicable | file-picker |
| `update.latest_json_404` | latest metadata references missing asset or invalid JSON | Release verify fails with `release.latest_json_asset_missing` or `release.latest_json_invalid` | release |
| `release.sha256_mismatch` | `latest.json.sha256` or an asset `.sha256` does not match the actual file bytes | Release verify fails with `release.latest_json_sha256_mismatch` or `release.asset_sha256_mismatch`; invalid sidecar content fails with `release.*_sha256_invalid` | release |
| `release.signature_mismatch` | `latest.json.sig` or an asset `.sig` does not match the actual file bytes and release public key | Release verify fails with `release.latest_json_sig_mismatch` or `release.asset_sig_mismatch`; invalid public key, unsupported algorithm, or malformed signature fails before release | release |
| `release.macos_x64_missing` | macOS x64 pkg/dmg assets are absent | Release verify fails; macOS x64 cannot be deferred for `v1.1.0-rc1` | release |
| `rc1.readiness_incomplete` | RC1 evidence is audited before marking the goal complete | `cargo xtask verify --stage rc-readiness` prints prompt-to-artifact evidence and returns non-zero until Windows real Desktop smoke, macOS arm64/x64 workflow smoke, and macOS real Desktop smoke pass evidence exist; final pass handoffs must carry collector-style fingerprint/test/log/workflow markers, not only free-text summaries | rc-readiness |
| `release.manifest_macos_x64_required_by_default` | `New-ReleaseManifest.ps1` is run without overriding `RequiredPlatforms` | Missing macOS x64 pkg/dmg fails before `latest.json`; complete Windows/macOS arm64/macOS x64 fixture generates latest.json, sha256, sig, and public key | release/manual |
| `platform.macos_arm64_x64_smoke_path` | Rust/Tauri mainline platform workflow runs on explicit macOS arm64 and Intel runners | Runner arch is checked, Rust workspace/UI/Tauri build pass, app bundle/Info.plist/DMG/PKG bundle smoke passes, and artifacts are retained without publishing | platform/manual |
| `installer.old_dir_not_detected` | Old install location exists | Installer preselects old directory before directory page | release |

## Current Local Coverage

| Fingerprint | Current command | Status |
|---|---|---|
| `desktop.raw_model_names_detected` | `cargo xtask verify --stage model-catalog` | Covered by explicit raw route rejection test |
| `gateway.unmapped_model_route` | `cargo xtask verify --stage gateway` | Covered by `/v1/messages` mapping core returning 400 with no `Default` fallback |
| `provider.max_not_supported` | `cargo xtask verify --stage gateway` | Covered by gateway request option validation returning 400 |
| `desktop.stale_base_url` | `cargo xtask verify --stage desktop` | Covered by Desktop readback health comparison |
| `desktop.one_million_not_written` | `cargo xtask verify --stage desktop` | Covered by Desktop readback `supports1m` mismatch detection |
| `desktop.config_readback_mismatch` | `cargo xtask verify --stage desktop` | Covered by missing and extra route strict comparison |
| `desktop.mode_mismatch` | `cargo xtask verify --stage desktop` | Covered by local gateway mode mismatch detection |
| `desktop.local_config_readback` | `cargo xtask verify --stage desktop-writer` | Covers local configLibrary write/readback, safe-route model entries, capability preservation, unrelated setting preservation, and JSON-string compatibility |
| `desktop.managed_config_detected` | `cargo xtask verify --stage desktop-config` and `cargo xtask verify --stage apply-flow` | Covers managed config evidence detection, `desktop.ccds_managed_policy_detected` issue-code surfacing, and Apply blocking before gateway/write |
| `desktop.real_windows_local_config_smoke` | Preferred wrapper: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite`; direct test: `cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture` with `CCDS_ALLOW_REAL_DESKTOP_WRITE=1`; cleanup runbook: `project-docs/runbooks/windows-managed-policy-cleanup.md` | Covered by P62 real Windows run. The old `HKCU\SOFTWARE\Policies\Claude` blocker was first backed up, then removed via elevated PowerShell with a second backup at `C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-elevated-20260509153014.reg`; status reported `exists=False`. `scripts/windows/run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite` passed and wrote `target\real-desktop-smoke\windows-real-desktop-smoke-evidence.md`; `scripts/windows/Collect-RealDesktopSmokeEvidence.ps1` generated `project-docs/handoff/2026-05-09-windows-real-desktop-smoke-evidence-summary.md`. The wrapper records `mode: run`, `exit_code: 0`, and the cargo log includes `test result: ok`; collector handoff records backup/readback/loopback gateway/restore coverage. P62 also fixed the wrapper so cargo stderr progress lines are captured and judged by exit code rather than as `NativeCommandError` |
| `desktop.real_macos_local_config_smoke` | Preferred wrapper: `scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write`; direct test: `CCDS_ALLOW_REAL_DESKTOP_WRITE=1 cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture`; runbook: `project-docs/runbooks/macos-rust-mainline-smoke.md` | Harness exists and shares the same backup/write/readback/gateway-smoke/restore helper as the Windows real smoke. P51 added a default read-only wrapper that writes `target/real-desktop-smoke/macos-real-desktop-smoke-evidence.md`; P52 aligned the wrapper evidence markers with `rc-readiness`; P57 added `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` to reject `Preflight` / `UnsupportedPlatform` / fail evidence and generate handoff only from `platform: Darwin`, `mode: run`, `exit_code: 0`, `## Result` / `Pass`, and `test result: ok` logs. P64 made the wrapper write a relative log filename for downloaded artifact compatibility, verified the collector against a relative-log fixture under `target/macos-real-smoke-relative-fixture`, and added the real smoke run to the non-publishing macOS platform workflow. Real macOS execution evidence is still missing |
| `apply.false_success_blocked` | `cargo xtask verify --stage apply-flow` | Covers successful fixture apply and failure before write when provider is missing or gateway port is occupied |
| `app.single_instance` | `cargo xtask verify --stage app-shell`; latest Windows packaged app smoke | Covers compile-time registration of the official single-instance plugin and focus/show callback; P43 latest packaged smoke verified second launch exits and only one app process remains |
| `app.tray_close_behavior` | `cargo xtask verify --stage app-shell`; latest Windows packaged app smoke | Covers compile-time registration of Tauri tray icon support, close-request hide handler, tray show action, left-click restore, explicit quit action, and P74 static checks for stable tray id/tooltip/default app icon; P43 latest packaged smoke verified close hides the `CC Desktop Switch` main window while the process remains alive and third launch restores it |
| config migration | `cargo xtask verify --stage config` | Covers Python stable config load, backup-before-save, `providerId`, route identity preservation, and P74 Provider `authScheme` preservation |
| `provider.auth_scheme_roundtrip` | `cargo test --workspace` | P74 covers config migration/save summaries and gateway upstream header generation for Bearer, `x-api-key`, and no-auth Providers |
| provider mapping preservation | `cargo xtask verify --stage config` | Covers provider metadata update preserving existing model mappings |
| provider service | `cargo xtask verify --stage provider-service` | Covers provider save/list/set active persisted config path and summary secret redaction |
| `provider.crud_reorder_roundtrip` | `cargo xtask verify --stage provider-parity` | Covers provider edit/delete/reorder persisted roundtrip, exact-set reorder validation, active provider reassignment after delete, summary secret redaction, and gateway restart on changed active provider fingerprint |
| `provider.import_export_roundtrip` | `cargo xtask verify --stage provider-import` | Covers Provider export package roundtrip, CC-Switch legacy import, dry-run without config write, conflict blocking until replace is requested, duplicate provider ID rejection, and raw route ID rejection |
| `provider.import_skip_existing_merge` | `cargo xtask verify --stage provider-import` | Covers `skipExisting` merge mode importing non-conflicting Providers while preserving conflicting existing Providers and their API keys |
| `provider.template_import` | `cargo xtask verify --stage provider-import` | Covers secretless Provider template package import, `openai_chat` / `open_ai_chat` API format compatibility, safe route validation, `Default` suppression, duplicate template ID rejection, secret-bearing field rejection, and non-HTTP(S) `baseUrl` rejection |
| `provider.marketplace_import` | `cargo xtask verify --stage provider-import` | Covers offline `ccds.providerMarketplace` manifest import, HTTPS plain source URL enforcement, template package sha256 match, hash mismatch rejection, and nested template secret-field rejection |
| `provider.model_mapping_edit` | `cargo xtask verify --stage model-mapping` | Covers persisted mapping edit, generated safe route aliases, `Default` route suppression, raw route rejection, and duplicate route rejection |
| `provider.preset_import` | `cargo xtask verify --stage provider-preset` | Covers DeepSeek/Kimi preset listing, preset import preview/apply, conflict blocking until replace, safe route mappings, `Default` suppression, and summary secret redaction |
| `config.backup_redacted_readback` | `cargo xtask verify --stage config-backup` | Covers config backup listing, filename-only readback, directory traversal rejection, and state-level redaction before returning backup contents |
| provider smoke checks | `cargo xtask verify --stage provider-service` | Covers provider static smoke, local gateway `/v1/models` smoke, and provider real smoke stopping before network when API key is missing |
| gateway models | `cargo xtask verify --stage gateway` | Covers `/v1/models` response from Desktop-safe catalog with no raw upstream field |
| gateway router | `cargo xtask verify --stage gateway` | Covers Axum `/v1/models` and `/v1/messages` skeleton; valid messages return 501 without upstream model echo until real forwarding exists |
| gateway Anthropic passthrough | `cargo xtask verify --stage gateway` | Covers safe route replacement with upstream model in internal upstream request only |
| gateway OpenAI Chat conversion | `cargo xtask verify --stage gateway` | Covers system/messages/max_tokens/temperature/stream conversion and invalid message shape as `provider.api_format_mismatch` |
| `gateway.invalid_upstream_response` | `cargo xtask verify --stage gateway` | Covers non-JSON upstream preview redaction and Anthropic-style error envelope |
| `gateway.invalid_stream_content_type` | `cargo xtask verify --stage gateway` | Covers expected stream receiving non-event-stream response fingerprint |
| gateway non-stream forwarding | `cargo xtask verify --stage gateway` | Covers local mock upstream forwarding through `reqwest`, Anthropic safe-route response normalization, and OpenAI Chat response conversion |
| gateway SSE runtime forwarding | `cargo xtask verify --stage gateway` | Covers local mock upstream `text/event-stream` forwarding through Axum response body and safe-route model field normalization |
| `gateway.openai_stream_semantic_conversion` | `cargo xtask verify --stage gateway` | Covers OpenAI Chat SSE chunks converted into Anthropic-style message/content events |
| gateway lifecycle | `cargo xtask verify --stage gateway-lifecycle` | Covers active-provider requirement, start/stop/status, ephemeral port binding, and configured port conflict error code |
| `diagnostics.false_green_readiness` | `cargo xtask verify --stage diagnostics` | Covers readiness, copy-summary output, and GitHub Issue draft naming the false-green layers instead of reporting full readiness |
| `ui.readiness_dashboard` | `trunk build --release`; Playwright at `http://127.0.0.1:1421/?p41=1` | P41 browser smoke covered desktop and mobile rendering with no console errors, and the UI code binds readiness cards/list to the latest `health` snapshot without hand-written JS business logic |
| `ui.legacy_layout_parity` | `trunk build --release`; Playwright at `http://127.0.0.1:1421/?p71=rerender` | P71 browser smoke covered desktop `1366x900` and mobile `390x900`, including Dashboard and Provider. Screenshots are under `target/ui-smoke/p71/`. Console result: 0 errors, 1 existing Chromium/Trunk SRI warning |
| `ui.current_frontend_parity` | `trunk build --release`; Playwright at `http://127.0.0.1:1425/?p75=...` | P75 supersedes P74 for current local evidence. Browser smoke covers desktop Settings and Add Provider scroll with mocked Tauri bridge: Settings `scrollTop 0 -> 466`, Add Provider `scrollTop 0 -> 700`, console errors `0`. Current frontend markers verified in code: five icon tab shell, provider switch cards with provider assets, CC Switch-style empty state/card density, Add Provider two-column form, red third-party compatibility box, editable row-style model mapping, orange one-click apply button, Settings rows, Proxy status/log/metrics, Guide steps, plus command-backed settings save, backup, proxy log refresh/clear, provider URL copy, provider delete, and start gateway with saved port. Mobile is not an acceptance target for this desktop app |
| `diagnostics.gateway_runtime_logs` | `cargo xtask verify --stage diagnostics` and `cargo xtask verify --stage gateway-lifecycle` | Covers runtime log inclusion, redaction, and gateway no-provider/start/stop lifecycle log entries |
| `diagnostics.secret_leak` | `cargo xtask verify --stage diagnostics` | Covers diagnostics package export/save and redaction for API keys, gateway keys, Authorization, Cookie, secret headers, URL userinfo, query tokens, upstream body previews, redacted config JSON, and issue draft URL/body |
| `diagnostics.file_picker_save` | `cargo xtask verify --stage file-picker` | Covers compile-time integration of the Tauri dialog plugin and save-as commands for diagnostics package and Provider export |
| `update.latest_json_404` | `cargo xtask verify --stage release` | Covers `latest.json` presence/parse, invalid JSON rejection, staging-directory validation of `latest.json` referenced asset files, plus `latest.json.sha256`, `latest.json.sig`, public key, and per-asset hash/signature requirements |
| `release.sha256_mismatch` | `cargo xtask verify --stage release` | Covers actual sha256 calculation for `latest.json` and manifest assets, mismatch rejection, and invalid sidecar rejection |
| `release.signature_mismatch` | `cargo xtask verify --stage release` | Covers `RSA-CSP-BLOB-SHA256` public key parsing, signature algorithm validation, base64 signature parsing, RSA/SHA256 verification for `latest.json` and manifest assets, and a Windows-only compatibility fixture that signs with `scripts/New-ReleaseManifest.ps1` |
| `release.macos_x64_missing` | `cargo xtask verify --stage release` | Covers required Windows x64, macOS arm64, and macOS x64 release asset IDs, including hard failure for missing macOS x64 pkg/dmg in both memory-level input and directory-level fixture paths |
| `rc1.readiness_incomplete` | `cargo xtask verify --stage rc-readiness` | P75 local run passes against existing handoff evidence and still statically checks stable installer identity, NSIS hooks, and tray icon binding. Because P75 changes installer/UI/command artifacts after the previous macOS workflow, fresh non-publishing macOS workflow evidence is still required before RC packaging even if the static audit command passes |
| `release.manifest_macos_x64_required_by_default` | `scripts/New-ReleaseManifest.ps1` smoke under `target/release-manifest-smoke` | 2026-05-09 local smoke verified default manifest generation fails without macOS x64 assets and succeeds with Windows x64, macOS arm64, and macOS x64 fixture assets, producing `latest.json`, `.sha256`, `.sig`, and public key |
| `installer.old_dir_not_detected` | `cargo tauri build`; generated NSIS inspection; Windows manual upgrade smoke | P75 static/build evidence covers stable Tauri identifier `io.github.lonr6.ccdesktopswitch`, per-machine NSIS mode, old NSIS template/hooks, generated NSIS restore function, and MSI/GUID uninstall entry scan by `DisplayName`. Real old-directory selection still requires a Windows in-place installer run because it depends on the user's installed registry state |
| `platform.macos_arm64_x64_smoke_path` | `.github/workflows/rust-mainline-platform-smoke.yml`, `scripts/macos/Collect-PlatformSmokeEvidence.ps1`, and `project-docs/runbooks/macos-rust-mainline-smoke.md` | Workflow path targets `macos-14` arm64 and `macos-15-intel` x64. P75 follow-up run `25724455576` passed arm64/x64 Rust workspace gate, Leptos build, Tauri build, bundle/pkg smoke, macOS real Desktop local config smoke, and artifact upload; `project-docs/handoff/2026-05-12-p75-macos-platform-smoke-evidence-summary.md` records the downloaded arm64/x64 artifact evidence |
| local full gate | `cargo xtask verify --all` or focused equivalent commands | Runs fmt, workspace tests, clippy, UI release build, and Tauri build on the current platform. P75 focused local run passed `cargo fmt --all -- --check`, `trunk build --release`, `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` with 111 passed and 2 ignored real Desktop smoke tests, Playwright desktop scroll smoke, `cargo tauri build`, and `cargo xtask verify --stage rc-readiness` |

## Eval Report Format

Each eval run should record:

- command
- input fixture
- pass/fail
- relevant output excerpt
- linked issue or bug fingerprint
- date and platform
