# Changelog

<p align="center">
  <a href="../README.md">English</a> |
  <a href="../README.zh-CN.md">简体中文</a> |
  <a href="../README.ja.md">日本語</a> |
  <a href="CHANGELOG.md">Changelog</a>
</p>

## v1.0.24

Release notes: [docs/release-notes-v1.0.24.md](release-notes-v1.0.24.md)

- Added SOCKS upstream proxy dependency support for packaged Windows and macOS builds.
- Added a Claude Code helper health warning for Claude Desktop `Host Claude Code binary not available` errors.
- Added an explicit Xiaomi MiMo 1M context option for mapped `mimo-v2.5-pro` routes.
- Improved the macOS menu bar icon and Dock hiding behavior.
- Clarified local gateway background-running and Connectors/Skills boundaries.

## v1.0.23

Release notes: [docs/release-notes-v1.0.23.md](release-notes-v1.0.23.md)

- Fixed Windows in-app update installs where the package downloaded and the app exited but the installer did not appear.
- Added update helper logging under `%TEMP%\CC-Desktop-Switch\updates\update-helper.log`.
- Reduced startup blank-screen time by bundling Bootstrap, Bootstrap Icons, and font dependencies locally.
- Hid the macOS Dock icon and added a native menu bar status item for show/quit actions.
- Kept local update-test manifests out of the release asset set.

## v1.0.22

Release notes: [docs/release-notes-v1.0.22.md](release-notes-v1.0.22.md)

- Added `coworkEgressAllowedHosts` to Claude Desktop policy writes so Cowork/WebFetch can access external hosts after applying the local gateway config.
- Accepted Claude's dated Haiku route `claude-haiku-4-5-20251001` as an internal alias for the mapped Haiku slot.
- Preserved macOS `supports1m` metadata in JSON and `configLibrary` writes so DeepSeek 1M health checks can pass after re-applying.
- Avoided injecting DeepSeek Max request options into non-official DeepSeek-like relays such as OpenCode/one-api/new-api endpoints.
- Kept #7, #18, and #19 as support/diagnostics follow-ups; no issue replies or GitHub release are sent without maintainer approval.

## v1.0.21

Release notes: [docs/release-notes-v1.0.21.md](release-notes-v1.0.21.md)

- Fixed macOS fresh-environment writes by creating a `configLibrary` entry when none exists.
- Added Intel macOS x64 release assets alongside Windows x64 and macOS arm64.
- Added safe custom Claude route mappings that expose only `claude-*` model names to Claude Desktop.
- Fixed Windows in-app update installs so the downloaded installer opens visibly.
- Kept #7, #9, and #10 open for follow-up; this patch release does not claim those issues are fixed.

## v1.0.20

Release notes: [docs/release-notes-v1.0.20.md](release-notes-v1.0.20.md)

- Corrected quick-start and usage docs so third-party providers are described as using the local gateway by default.
- Added a Copilot FAQ: Copilot subscriptions are not directly supported; user-provided compatible endpoints are at the user's own risk.
- Added structured GitHub issue forms for bug reports, provider requests, and questions.
- Protected local Admin APIs with a runtime admin token while keeping `/api/ready` public and `/api/app/activate` compatible.
- Added redacted diagnostics summary/export/check endpoints using `ccds.diagnostics.v1`.
- Improved OpenAI/new-api relay diagnostics for non-JSON upstream responses, including streaming error events.
- Reworked the release workflow so Windows and macOS assets are staged first and `latest.json` is generated only after both required platforms exist.
- Added issue reply drafts for #3, #4, and #7 without claiming that #3 DeepSeek 1M behavior is fixed.

## v1.0.19

Release notes: [docs/release-notes-v1.0.19.md](release-notes-v1.0.19.md)

- Claude Desktop model menu now shows only explicitly mapped Claude-safe routes.
- `Default` is kept as an internal fallback and is no longer written as a Claude Desktop menu item.
- Unmapped Claude routes now return a clear 400 error instead of silently falling back.
- Health checks detect stale v1.0.18 routes and raw upstream model names.
- Windows startup is single-instance: launching the shortcut again brings the existing window forward.
- Windows and macOS arm64 release assets are available from GitHub Releases.

## v1.0.18

Release notes: [docs/release-notes-v1.0.18.md](release-notes-v1.0.18.md)

- Switched Claude Desktop configuration to the local CC Desktop Switch gateway by default.
- Added Claude-safe model routes for newer Claude Desktop versions that reject raw upstream model names.
- Kept real provider model IDs inside local gateway mapping.

## Earlier Releases

Older release notes are available under `docs/release-notes-v*.md`.
