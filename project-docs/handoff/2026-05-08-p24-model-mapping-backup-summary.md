# P24 Model Mapping / Config Backup Summary

## Scope

- Added Provider model mapping read/update commands across config, state, Tauri command bridge, and Leptos UI.
- Added config backup list/read commands and UI controls.
- Kept `Default` as a config convenience only; it is forced out of runtime route resolution and Desktop model menus.
- Did not add release publishing, updater metadata, preset marketplace, or full conflict merge UX.

## Implemented

- `AppConfig::update_provider_model_mappings` normalizes mapping drafts and rejects unsafe route IDs.
- Non-`Default` route IDs must be `claude-*`; raw upstream route names and duplicate visible route IDs are rejected.
- Empty or missing non-`Default` upstream model is rejected; an all-`Default` mapping set is rejected.
- `ConfigProvider::model_mapping_summaries` exposes editable mapping summaries for UI without changing Desktop-visible model generation rules.
- Config backups are listed from the config-adjacent `backups` directory and read by file name only.
- Backup readback is redacted before it crosses the Tauri command boundary.
- Leptos UI now has model mapping load/save controls and config backup list/redacted-read controls.

## Verification

- `cargo fmt --all`
- `cargo xtask verify --stage model-mapping`
- `cargo xtask verify --stage config-backup`
- `trunk build --release` in `ui`
- `cargo xtask verify --all` passed after rerun: 85 workspace tests, clippy, UI release build, and Windows x64 Tauri build.
- Playwright via `trunk serve --address 127.0.0.1 --port 1421 --open false`: desktop 1440x1000 and mobile 390x900 had no horizontal overflow, model mapping controls rendered, backup controls rendered, and no console errors. The only warning was Chromium's Trunk/SRI preload warning.

## Current Limits

- Model mapping UI is JSON-backed for the spike; richer per-slot form controls remain pending.
- Config backup readback is shown in the common result box; file picker save/restore is not implemented.
- No real Claude Desktop write smoke was run in this step.
- No macOS arm64/x64 build or smoke was run in this step.
- One full-gate run showed a transient gateway stream mock failure, but `cargo xtask verify --stage gateway` and the subsequent `cargo xtask verify --all` passed. Treat this as a watch item if it recurs.

## Next Minimum Task

1. Add Provider import conflict merge UI and preset/template import.
2. Add Tauri file picker for diagnostics/export destinations.
3. Start real Windows Claude Desktop local config smoke, then macOS arm64/x64 gates.
