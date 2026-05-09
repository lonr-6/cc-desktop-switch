# P25 Provider Preset Import Summary

## Scope

- Added built-in Provider presets for DeepSeek and Kimi.
- Added Tauri commands and state methods for preset list, preview, import, and replace.
- Added Leptos UI controls for loading presets, previewing conflicts, importing with an API key, and replacing an existing preset provider.
- Did not add an external preset marketplace or remote template fetch.

## Implemented

- Presets create normal persisted `ConfigProvider` entries with explicit `claude-*` routes.
- `Default` mappings in presets use `route_id = None` and remain non-runtime.
- Preset import reuses the existing import dry-run conflict model.
- Replace import preserves the saved API key when the preset API key field is blank.
- Provider summaries still expose only `has_api_key`, never the actual API key.

## Verification

- `cargo fmt --all`
- `cargo xtask verify --stage provider-preset`
- `trunk build --release` in `ui`
- `cargo xtask verify --all` passed: 87 workspace tests, clippy, UI release build, and Windows x64 Tauri build.
- Playwright via `trunk serve --address 127.0.0.1 --port 1421 --open false`: desktop 1440x1000 and mobile 390x900 rendered preset/mapping/backup controls, had no horizontal overflow, and had no console errors. The only warning was Chromium's Trunk/SRI preload warning.

## Current Limits

- Preset coverage is intentionally small: DeepSeek and Kimi only.
- Conflict details are now available through preset preview, but there is no checkbox-level merge UI yet.
- No real Provider API key smoke was run.

## Next Minimum Task

1. Add richer import conflict merge controls.
2. Add Tauri file picker for diagnostics/export destinations.
3. Start real Windows Claude Desktop local config smoke.
