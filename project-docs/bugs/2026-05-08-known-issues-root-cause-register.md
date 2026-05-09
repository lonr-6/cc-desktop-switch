# Known Issues and Root-Cause Register

Date: 2026-05-08

This register turns historical bugs and GitHub issues into Rust mainline requirements.

## GitHub Issues

| Issue | Symptom | Root Cause Hypothesis | Rust Mainline Fix | Required Test |
| --- | --- | --- | --- | --- |
| #3 | DeepSeek 1M/Haiku behavior confusing | Capability and mapping rules not clearly surfaced | `ModelCatalog` owns 1M capability and UI shows exact route capability | DeepSeek preset fixture with Sonnet/Opus/Haiku capabilities |
| #4 | User asks whether GitHub Copilot subscription can be provider | Copilot subscription is not an OpenAI-compatible endpoint | Keep unsupported; document self-provided compatible endpoint only | Docs/FAQ check |
| #5/#6 | Claude Desktop rejects raw model names like `deepseek-v4-pro` | Desktop validates custom model IDs | Claude-safe route aliases only; no raw model names | Route generation rejects raw Desktop IDs |
| #7 | 中转返回非 JSON，用户不知道是否支持 | Base URL/API format mismatch, upstream HTML/text, auth page, incompatible relay | `GatewayAdapter` returns structured `invalid_upstream_response` with redacted preview | Mock HTML/text response for non-stream and stream |
| #9 | OpenCode Go DeepSeek says model does not support Max | Provider capability not known before applying request options | `ProviderCapability` gates Max/thinking before request; UI disables unsupported options | OpenAI Chat provider with Max unsupported response |
| #10 | macOS DeepSeek 1M applied but still prompts not written | `configLibrary` read/write/readback incomplete, supports1m mismatch | `MacConfigLibraryStore.ensure_active_entry()` and readback health | macOS temp configLibrary + real macOS smoke |
| #12 | Windows update downloads then app closes but installer does not appear | Installer launcher not observable; app may exit before child is detached/started | `UpdateOrchestrator` verifies installer launch and logs failure | Windows update from previous version to RC |
| #13 PR | macOS fresh configLibrary missing | Existing code returns success without writing active entry | Absorb as Rust store abstraction, not patch | Unit test creates `_meta.json` and entry |
| #14 PR | Intel macOS x64 missing from release assets | Release pipeline only covers arm64 macOS | Require `macos-x64` assets before latest.json | Release manifest test with missing x64 fails |

## Historical Bugs

| Code | Symptom | New Rule |
| --- | --- | --- |
| `desktop.black_console` | Windows app opens a black terminal | Release build must use Windows GUI subsystem |
| `desktop.apply_false_success` | Apply fails but UI says applied | Apply result success depends on readback health |
| `desktop.registry_acl` | Windows registry ACL blocks HKCU write | Permission-only failures use UAC + HKEY_USERS current SID fallback |
| `desktop.managed_policy_blocks_local_config_smoke` | Existing `HKCU\SOFTWARE\Policies\Claude` policy blocks local configLibrary smoke before write | Treat as real blocker, keep Apply blocked, and add explicit migration/cleanup UX before claiming Windows real smoke |
| `desktop.raw_model_names_detected` | Claude Desktop rejects raw third-party model names | Desktop sees only Claude-safe route IDs |
| `gateway.unmapped_model_route` | Unconfigured route falls back to Default | Return 400 with actionable message |
| `provider.copilot_subscription_not_endpoint` | Copilot subscription account treated as provider API | Do not support account scraping; only user-provided compatible endpoints |
| `desktop.one_million_not_written` | 1M route missing supports1m | Health compares expected route supports1m to actual config |
| `update.latest_json_404` | latest.json points to missing asset | Release gate verifies fixed and latest URLs before publish |
| `installer.old_dir_not_detected` | Installer defaults to new directory | NSIS directory detection must happen before directory picker |

## Required Issue Fingerprints

The Rust diagnostics module must emit stable codes:

- `desktop.config_library_missing`
- `desktop.config_readback_mismatch`
- `desktop.managed_config_detected`
- `desktop.ccds_managed_policy_detected`
- `desktop.raw_model_names_detected`
- `desktop.stale_base_url`
- `desktop.one_million_not_written`
- `gateway.invalid_upstream_response`
- `gateway.invalid_stream_content_type`
- `gateway.unmapped_model_route`
- `provider.max_not_supported`
- `provider.copilot_subscription_not_endpoint`
- `provider.api_format_mismatch`
- `update.installer_launch_failed`
- `release.missing_platform_assets`

## Fix Recording Rule

When one of these issues is fixed:

1. Add or update an automated regression test.
2. Add exact verification command output to the relevant handoff.
3. Update this register if the root cause or issue fingerprint changes.
4. Update `project-docs/status.md` only with the current truth, not long logs.
