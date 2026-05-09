# Rust Mainline Architecture

## Goal

Build a clean Rust/Tauri mainline for CC Desktop Switch while preserving the current user workflows.

The new architecture should be simple to debug:

- one module owns one responsibility;
- no duplicate model routing logic;
- no apply success without readback verification;
- no handwritten JS business layer;
- no release script sprawl.

## Stack

- Desktop shell: Tauri v2
- UI: Leptos + Trunk, Rust/WASM in Tauri WebView
- Runtime: Rust
- Local gateway: Rust HTTP server, preferably Axum/Hyper-based
- Config: JSON-compatible local config at `~/.cc-desktop-switch/config.json`
- Tooling: Rust `xtask`

## Module Map

```text
src-tauri/
  src/
    app_shell/        Tauri lifecycle, tray, single instance, windows
    commands/         thin Tauri command wrappers
    config/           config repository, migration, backup, import/export
    provider/         presets, checks, model discovery, API format detection
    model_catalog/    Claude-safe routes, upstream mappings, 1M/Max capability
    desktop/          planner, health, Windows/macOS writers
    gateway/          local gateway, adapters, auth, logs, SSE
    diagnostics/      redaction, diagnostics package, issue fingerprints
    update/           latest.json, download, verify, installer launcher
    release/          shared release metadata helpers if needed

ui/
  src/
    app.rs            Leptos app root
    pages/            dashboard, provider, diagnostics, settings
    components/       reusable controls
    store/            UI state
    commands/         typed Tauri command client
    i18n/             language strings

xtask/
  src/
    main.rs
    frontend.rs
    release.rs
    contracts.rs
```

## Core Data Flows

### One-Click Apply

```text
UI action
-> ProviderService.save()
-> ProviderService.set_active()
-> ModelCatalog.for_active_provider()
-> GatewayService.ensure_running()
-> DesktopPlanner.build_plan()
-> PlatformWriter.write(plan)
-> PlatformReader.read()
-> DesktopHealth.compare(expected, actual)
-> UI shows success or exact failure
```

### Gateway Request

```text
Claude Desktop
-> local gateway auth
-> ModelCatalog.resolve_route()
-> ProviderCapability.validate_request_options()
-> GatewayAdapter.forward()
-> ResponseNormalizer.normalize()
-> GatewayLog.record()
```

### Diagnostics

```text
Config snapshot
Desktop status
Desktop health
Gateway status/logs
Update logs
Recent failure
-> Redactor
-> diagnostics summary/package
```

## ModelCatalog Contract

`ModelCatalog` must answer:

- Which routes are visible to Claude Desktop?
- Which provider owns each route?
- Which upstream model does a route map to?
- Does this route support 1M?
- Does this route support Max/thinking?
- Should a request be rejected?

Desktop and gateway must not build model lists independently.

## Desktop Plan Contract

The platform-neutral plan must include:

- `base_url`
- `gateway_api_key`
- `auth_scheme`
- `gateway_headers`
- `inference_models`
- `expected_routes`
- `expected_capabilities`
- `mode = local_gateway`

Windows/macOS writers receive the plan and return write/readback results.

## Platform Rules

### Local Apply

- Ordinary user Apply writes Claude 3P local user configuration first, not MDM-managed policy.
- Windows local user target: `%LOCALAPPDATA%\Claude-3p\configLibrary\`.
- macOS local user target: `~/Library/Application Support/Claude-3p/configLibrary/`.
- `_meta.json` records the active local configuration, and each config is stored as `<id>.json`.
- Read back immediately after write and compare with `DesktopHealth`.
- Preserve unrelated Claude settings in the selected local config file.

### Managed Export

- Windows registry policy (`HKLM/HKCU\SOFTWARE\Policies\Claude`) and macOS managed preferences / `.mobileconfig` are managed or export paths, not the first ordinary user Apply path.
- When managed configuration is detected, the app must diagnose that local config is ignored instead of claiming success.
- Registry/mobileconfig export still needs fixture coverage before release.

## UI Boundary

UI code can:

- render pages;
- keep temporary form state;
- call typed commands;
- show command results.

UI code cannot:

- decide model routing;
- decide Desktop write target;
- guess Provider capability;
- parse secrets into diagnostics;
- claim success after partial failure.

## References

- Tauri + Leptos setup uses `trunk serve`, `trunk build`, `frontendDist`, and `withGlobalTauri`: https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/start/frontend/leptos.mdx
- Leptos uses Rust components, signals, and `mount_to_body`: https://github.com/leptos-rs/leptos
- Claude 3P configuration reference: https://claude.com/docs/cowork/3p/configuration
