# Changelog

<p align="center">
  <a href="../README.md">English</a> |
  <a href="../README.zh-CN.md">简体中文</a> |
  <a href="../README.ja.md">日本語</a> |
  <a href="CHANGELOG.md">Changelog</a>
</p>

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
