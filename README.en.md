# CC Desktop Switch

<p align="center">
  <a href="README.md">简体中文</a> |
  <a href="README.en.md">English</a> |
  <a href="docs/CHANGELOG.md">Changelog</a>
</p>

<p align="center">
  <a href="https://github.com/Cmochance/cc-desktop-switch/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/Cmochance/cc-desktop-switch?style=social"></a>
  <a href="LICENSE.txt"><img alt="License" src="https://img.shields.io/github/license/Cmochance/cc-desktop-switch"></a>
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-1.80%2B-orange?logo=rust"></a>
  <a href="https://v2.tauri.app/"><img alt="Tauri" src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri"></a>
  <a href="https://github.com/Cmochance/cc-desktop-switch/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/Cmochance/cc-desktop-switch/total?label=downloads"></a>
</p>

CC Desktop Switch is a lightweight desktop config + forwarding tool for the **official Claude Desktop client and the OpenAI Codex CLI**. It centralises third-party provider management (DeepSeek, Kimi, Zhipu GLM, Alibaba Cloud Bailian, Xiaomi MiMo, Gemini, Grok, etc.) and runs a local gateway that translates client requests into each upstream's protocol before forwarding to the provider you pick.

- **Claude Desktop track**: manage Anthropic-compatible providers and write the Claude Desktop third-party inference config to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or the registry (Windows). Claude Desktop talks to the local gateway at `127.0.0.1`.
- **Codex CLI track**: a local gateway translates Codex CLI's Responses API requests (HTTP streaming / non-streaming + `/responses` fallback) into Chat Completions / Gemini Native / Anthropic Messages / Grok Web / other upstream formats. Codex CLI talks to this tool at `127.0.0.1:18080`.

Unlike `farion1231/cc-switch` and similar CLI tools, this project is desktop-first: provider management, model mapping, forwarding port, logs panel, and a tray-resident background process. Closing the window minimises the app to the system tray; right-click the tray icon and choose "Exit" to fully quit.

Current v2.x release (see [Changelog](docs/CHANGELOG.md) and [Releases](https://github.com/Cmochance/cc-desktop-switch/releases)).

## Project Identity and Heritage

This repository is `Cmochance/cc-desktop-switch` — forked from `lonr-6/cc-desktop-switch`. v2 onward carries two structural changes:

1. **Full rewrite from Python to Rust + Tauri 2**. The original Python stack (FastAPI + PyInstaller + plain JS) was replaced with a Rust workspace + Tauri 2 + axum + reqwest + tokio. The scaffolding was lifted from the sibling repo `codex-app-transfer` (Tauri shell + protocol-conversion crates + record-replay contract tests), so many internal Rust identifiers (`codex_app_transfer_*`), the config directory (`~/.codex-app-transfer/`), the bundle identifier (`store.alyse.codex-app-transfer`), and the gateway key prefix (`cas_`) still **literally contain `codex-app-transfer`**. These are legacy IDs — renaming them would break existing users' configs and the app-update chain, so this release keeps them as-is and a follow-up PR will migrate them.
2. **codex-app-transfer is integrated into cc-desktop-switch**. On top of the original Claude Desktop integration, v2 adds the Codex CLI gateway and protocol-conversion layer (`crates/codex_integration/` + `crates/proxy/` + `crates/adapters/`), so a single desktop app serves both Claude Desktop and Codex CLI clients.

In short: **product name = cc-desktop-switch**; **internal code IDs still spell codex-app-transfer**, kept as a historical marker of the scaffolding source. If the README, installer, or product UI says "Codex App Transfer", that's a residual string from early v2 — being collapsed to CC Desktop Switch starting 2026-05-19.

## Preview

| Dashboard | Providers |
|---|---|
| ![Board](docs/img/Board.png) | ![Providers](docs/img/Providers.png) |
| **Settings** | **Logs** |
| ![Settings](docs/img/Settings.png) | ![Logs](docs/img/Logs.png) |

### Codex CLI in action

With any provider enabled, Codex CLI's model picker shows `<provider> / <real-model>`-style real model names. Tool loops / `previous_response_id` history replay / thinking-mode reasoning_content injection are all handled transparently by the local proxy:

![Codex CLI real conversation](docs/img/codex-cli-real-chat.png)

## What it does

- Manage multiple providers; map OpenAI model names (`gpt-5.5` / `gpt-5.4` / `gpt-5.4-mini` / `gpt-5.3-codex` / `gpt-5.2`) to the provider's real model IDs
- Translate Codex CLI's Responses API streaming / non-streaming requests into upstream protocols: Chat Completions, Gemini Native (`:streamGenerateContent`), Gemini CLI OAuth (Cloud Code Assist), Anthropic Messages (`/v1/messages`), Grok Web (`/rest/app-chat/conversations/new`), Responses passthrough, etc.
- Multi-turn tool conversation context + `previous_response_id` history replay + autocompact expansion + thinking / reasoning_content injection — all aligned with the OpenAI Responses API protocol
- **Two-layer session history persistence**: L1 in-memory LRU + L2 sqlite with 30-day TTL (`~/.codex-app-transfer/sessions.db`), preserving history across `.app` restarts
- Codex CLI config guardrails: snapshots `~/.codex/{config.toml,auth.json}` before apply; restores via per-key smart merge on exit / next start
- Real-time logs panel auto-refreshing every 2s; unified `tracing::warn!(error_id, detail)` with stable tokens — operators can grep / aggregate
- Feedback dialog automatically attaches diagnostic material (environment info, sanitized config, recent error snapshot with full request / response) — fewer back-and-forth follow-ups
- Chinese / English UI; light / dark / green / orange / gray / white themes
- Cross-platform single-instance lock (double-click brings the existing window forward) + cross-process file lock prevents multi-instance config-write lost-updates
- Windows / macOS / Linux system tray

## Download

Latest: `https://github.com/Cmochance/cc-desktop-switch/releases/latest`

Recommended asset naming:

```text
CC-Desktop-Switch-v<version>-Windows-x64-Setup.exe       Windows NSIS installer (recommended)
CC-Desktop-Switch-v<version>-Windows-x64.msi             Windows MSI (enterprise MDM / GPO)
CC-Desktop-Switch-v<version>-macOS-arm64.dmg             macOS Apple Silicon
CC-Desktop-Switch-v<version>-macOS-x64.dmg               macOS Intel x64 (v2.1.0+, closes #61)
CC-Desktop-Switch-v<version>-Linux-x86_64.deb            Debian / Ubuntu
CC-Desktop-Switch-v<version>-Linux-x86_64.AppImage       Generic Linux x86_64; `chmod +x` and run
```

Each binary ships with `.sha256` and `.sig` (RSA-3072 PKCS#1 v1.5 + SHA-256); the release public key is published as an asset on each release — download it from [Releases](https://github.com/Cmochance/cc-desktop-switch/releases) to verify signatures.

> Historical Releases (v2.1.x and earlier) are still named `Codex-App-Transfer-v<version>-…`; see [Project Identity and Heritage](#project-identity-and-heritage) for the rename context.

Windows builds are not Authenticode-signed yet, so Windows may show an "unknown publisher" warning — use the `.sha256` / `.sig` to verify download integrity.

## Quick Start

1. Launch CC Desktop Switch; the desktop window opens
2. On the dashboard, click the top-right "+" → pick a preset or add a custom provider; fill in API Base URL, API Key, model mappings
3. On the "Forwarding" page, click "Start" — the local port `18080` begins listening
4. In Codex CLI's config (`~/.codex/config.toml`), point `base_url` to `http://127.0.0.1:18080` and set the API Key to the Gateway API Key shown in this tool
5. Reopen Codex CLI; the model picker auto-lists the current provider's model mappings
6. ⚠️ **Switch Codex CLI to Full access** (`/approvals` → "Full access"): third-party providers will get stuck on the approval prompt under Codex CLI's default `auto` approval mode. Full access lets tool calls through directly — this is a **practical prerequisite** for using third-party providers

If the desktop window can't open (rare — usually Tauri webview init failed / system webview missing), try restarting first; if it persists, re-download from [Releases](https://github.com/Cmochance/cc-desktop-switch/releases) and check `~/.codex-app-transfer/logs/proxy-*.log`, or open an [Issue](https://github.com/Cmochance/cc-desktop-switch/issues). v2 has no standalone HTTP admin UI (the admin panel runs in-process via Tauri's `cas://` scheme — **port 18081 is no longer listened on**).

## Provider compatibility matrix

| Provider | Multi-turn | autocompact | tool_call_repair | Notes |
|---|---|---|---|---|
| Kimi (Moonshot Platform / Kimi For Coding) | ✅ | ✅ | ✅ | Thinking 3-layer defense |
| DeepSeek V4 (incl. Max thinking) | ✅ | ✅ | ✅ | Vision input stripped to avoid 400 |
| Xiaomi MiMo (Token Plan / Pay for Token) | ✅ | ✅ | ✅ | Image-only requests get space text-part fallback |
| MiniMax M2.x / Text-01 | ✅ | ✅ | ✅ | `role=system` → user (v2.1.6 fix for 400) |
| Google AI Studio (`gemini_native`) | ✅ | ✅ | ✅ | Auto-selects Gemini 3 `/v1alpha` + Gemini 2.x `/v1beta` |
| Google Gemini CLI OAuth | ✅ | ✅ | ✅ | Browser login once; no API key needed |
| Anthropic Messages (custom Claude-compatible) | ✅ (PR #153) | ✅ (PR #153) | ✅ (PR #153) | `apiFormat=anthropic_messages`; Claude preset pending real validation |
| Grok Web (SuperGrok / X Premium+) | ✅ | ✅ | ✅ (v2.1.6 adds tool_calls flatten) | Experimental, TOS gray area, personal use only |
| Google Antigravity OAuth | ✅ | ✅ | ✅ | Backend ready, UI pending |
| Zhipu GLM / Alibaba Cloud Bailian | ⚠️ experimental | — | — | OpenAI Chat-compatible reverse proxy |
| Responses passthrough (custom) | — | — | — | Direct upstream connection, bypasses proxy (suitable for OpenAI official / native Responses reverse proxy) |

## Model mapping

Codex CLI prompts by OpenAI model names; third-party providers use real IDs like `deepseek-v4-pro` / `kimi-k2.6` / `glm-5.1` / `gemini-3-pro`.

This tool maps via `provider.models[slot]` (`gpt-5.5` → `deepseek-v4-pro` etc.); Codex CLI's model picker shows `<provider> / <real-model>` real names. Upstream `chatcmpl-...` response IDs are auto-rewritten to Codex CLI-validatable `resp_<base64>`, preserving deployment-affinity encoding so `previous_response_id` is consistent across turns.

## Development (v2 / Rust)

```bash
git clone https://github.com/Cmochance/cc-desktop-switch.git
cd cc-desktop-switch
cargo tauri dev          # launch desktop window with hot-reload
cargo test --workspace --lib   # run unit tests
make mac-app             # local macOS bundle to dist/mac/
```

Fixture reverse-diff (contract tests):

```bash
cargo run --bin xtask -- gen-fixtures
```

Bundling (refer to `.github/workflows/release.yml`):

```bash
cargo tauri build --bundles app,dmg          # macOS arm64
cargo tauri build --bundles nsis,msi         # Windows x64
cargo tauri build --bundles deb,appimage     # Linux x86_64
```

## Troubleshooting

### Codex CLI reports `404 Not Found url: http://127.0.0.1:18080/responses`

Old versions only exposed `/v1/responses`; Codex CLI 0.126+ falls back to `/responses` (without `/v1/`). This tool added the route alias — update to v1.0.1+.

### Codex CLI reports `stream disconnected before completion`

Usually means `response.id` / `response.model` weren't returned in the shape Codex CLI expects. This tool rewrites upstream `chatcmpl-...` to `resp_<base64>` while preserving the requested model name — confirm forwarding logs show `resp_...` instead of `chatcmpl-...`.

### Upstream 400: `thinking is enabled but reasoning_content is missing`

Kimi / DeepSeek with thinking enabled require historical assistant messages with `tool_call` to carry `reasoning_content`. v1.0.1+ auto-fills a default empty string and maps reasoning items from Responses input to the corresponding assistant message.

### Upstream 400: `'reasoning_effort' does not support 'xhigh'`

If Codex user config sets `model_reasoning_effort` to `xhigh` / `max`, this tool auto-degrades to `high`. `auto` / `none` etc. (which the Chat endpoint doesn't accept) are dropped.

### MiniMax 400: `invalid message role: system (2013)`

v2.1.5 and earlier did not convert `role=system` to `role=user`, causing MiniMax `/v1/chat/completions` to 400 the entire request. v2.1.6+ fixes this (closes #139): all `role=system` messages are converted to `role=user` with content prefixed by `[System]\n`.

### Port conflicts

v2 only listens on `18080` (forwarding) by default; the admin UI now uses Tauri in-process `cas://` and no longer occupies 18081. Use `netstat -ano | findstr :18080` to find usage, or change the port in Settings → Port and restart forwarding.

### Windows "unknown publisher" warning

The current Windows build is not Authenticode-signed. The Release page provides `.sha256` and `.sig` to verify the installer hasn't been tampered with.

### Where are the logs?

- App UI: real-time panel below the forwarding page, auto-refreshes every 2s
- Disk: `~/.codex-app-transfer/logs/proxy-YYYY-MM-DD.log` — click "View Logs" to open directly
- "Clear logs" moves the current log to `logs/backup/` with a timestamp suffix — never deletes outright

## Tech Stack

- **Backend / forwarding**: Rust 1.80+ · axum 0.8 · reqwest 0.12 (rustls-tls) · tokio
- **Protocol adapters**: `crates/adapters/` — Responses ↔ Chat / Gemini Native / Gemini CLI OAuth / Anthropic Messages / Grok Web (request body + streaming response state machine + reasoning_content + tool_calls)
- **Frontend**: HTML + CSS + vanilla JavaScript + Bootstrap 5.3.3 (localized, no CDN dependency)
- **Desktop shell**: Tauri 2 + tray-icon 0.23; the `cas://` URI scheme glues frontend/ and axum in-process, no TCP loopback
- **Storage**: `~/.codex-app-transfer/config.json` (config, compatible with v1.x), `~/.codex-app-transfer/sessions.db` (L2 sqlite session persistence), `~/.codex/{config.toml,auth.json}` (Codex CLI integration)
- **Packaging**: `cargo tauri build` single command produces dmg/AppImage/deb/exe/msi; `xtask release-bundle` finalizes sha256 + RSA-3072 sig + latest.json + draft GitHub release

## Disclaimer

This project focuses on **OpenAI Codex CLI** integration; it is **not** an official OpenAI / Anthropic / Google / xAI project and does not reuse their trademarks / logos / release identities.

Upstream API keys / OAuth tokens are stored locally in `~/.codex-app-transfer/` (Unix 0600 + atomic write); the forwarding service only listens on `127.0.0.1` and does not hijack the system proxy.

Some experimental providers (Grok Web / Gemini CLI OAuth / Antigravity OAuth) involve upstream TOS gray areas — Grok Web reverse-proxies grok.com's Web backend, Gemini CLI OAuth uses the undocumented internal endpoint `cloudcode-pa.googleapis.com/v1internal` — strictly limited to **personal use**, **must not** be deployed as a public service, **users assume the risk**.

## Acknowledgements

- [`farion1231/cc-switch`](https://github.com/farion1231/cc-switch) — provider switching paradigm inspiration
- [`lonr-6/cc-desktop-switch`](https://github.com/lonr-6/cc-desktop-switch) — v1.x desktop shell skeleton + README structure reference + **Claude Desktop config-write feature 1:1 port** (full `crates/claude_desktop/`, sourced from upstream `backend/config.py:18-185` Provider schema and BUILTIN_PRESETS, `backend/model_alias.py:1-225` MODEL_SLOTS and multi-provider routing, `backend/registry.py:13-122` DESKTOP_CONFIG / CCDS_MARKER / serialize_gateway_headers, etc.; macOS plist + Application Support config.json write has no upstream implementation and is designed by cas)
- [`BerriAI/litellm`](https://github.com/BerriAI/litellm) — bidirectional protocol translation patterns
- [`tauri-apps/tauri`](https://tauri.app/) — v2 + `cas://` architecture base
- [`Piebald-AI/claude-code-system-prompts`](https://github.com/Piebald-AI/claude-code-system-prompts) — autocompact prompt blueprint
- [`7as0nch/mimo2codex`](https://github.com/7as0nch/mimo2codex) — MiMo protocol reference
- [`router-for-me/CLIProxyAPI`](https://github.com/router-for-me/CLIProxyAPI) — Gemini OAuth wire-level reference
- [`chenyme/grok2api`](https://github.com/chenyme/grok2api) — Grok Web reverse-engineering reference + dynamic statsig algorithm + tool_calls flatten pattern

### Community contributors

Contributors who improved this project via PRs (in reverse-chronological order of first commit; full list at [Contributors](https://github.com/Cmochance/cc-desktop-switch/graphs/contributors)):

- [@lukegood](https://github.com/lukegood) — MiniMax M2.x compatibility ([#47](https://github.com/Cmochance/cc-desktop-switch/pull/47))
- [@cw881014](https://github.com/cw881014) — early protocol-layer fixes, 3 PRs ([#1](https://github.com/Cmochance/cc-desktop-switch/pull/1) / [#7](https://github.com/Cmochance/cc-desktop-switch/pull/7) / [#12](https://github.com/Cmochance/cc-desktop-switch/pull/12))

If you've submitted a PR and want to rename / add a link / remove yourself, open an issue.

## License

MIT License. Full text at [LICENSE.txt](LICENSE.txt).
