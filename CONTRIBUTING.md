# Contributing to CC Desktop Switch

Thank you for helping improve CC Desktop Switch. Contributions are welcome for provider compatibility, local gateway behavior, desktop integration, packaging, documentation, tests, and security hardening.

## Before You Start

- Search existing issues and pull requests to avoid duplicate work.
- Open an issue before large features, behavior changes, new providers, or architecture changes. Small, well-scoped bug fixes may go directly to a pull request.
- Keep each pull request focused on one problem. Separate unrelated refactors, dependency upgrades, and formatting changes.
- Report vulnerabilities through the private process in [SECURITY.md](SECURITY.md), not through a public issue or pull request.
- Never commit real API keys, gateway keys, cookies, authorization headers, private provider endpoints, user configuration, or unredacted logs.

## Development Setup

Requirements:

- Python 3.11 or newer;
- Node.js 24 for the same frontend syntax checks used in CI;
- NSIS when modifying `installer.nsi` or Windows installer behavior.

```bash
git clone https://github.com/lonr-6/cc-desktop-switch.git
cd cc-desktop-switch
python -m pip install -r requirements.txt
python main.py
```

To open the browser fallback during development:

```bash
python main.py --browser
```

Use a temporary configuration directory when testing credential or migration behavior so normal user settings are not modified:

```bash
CCDS_CONFIG_DIR=/tmp/ccds-dev python main.py --browser
```

On PowerShell:

```powershell
$env:CCDS_CONFIG_DIR = "$env:TEMP\ccds-dev"
python main.py --browser
```

## Required Verification

Run the checks that apply to your change before opening a pull request:

```bash
python -m compileall -q backend main.py tests
python -m unittest discover -s tests -v
node --check frontend/js/api.js
node --check frontend/js/app.js
node --check frontend/js/i18n.js
```

Installer changes should also pass:

```bash
mkdir -p dist/CC-Desktop-Switch
printf 'syntax-check' > dist/CC-Desktop-Switch/placeholder.txt
makensis -V2 installer.nsi
```

GitHub Actions repeats the Python and frontend checks on Ubuntu, Windows, and macOS and compiles the NSIS script on Linux.

## Change-Specific Expectations

### Provider presets and model mappings

- Link to authoritative provider documentation in the issue or pull request.
- State the API format, base URL, authentication scheme, model IDs, streaming behavior, and any nonstandard headers or request fields.
- Do not add shared, scraped, trial, or embedded credentials.
- Preserve explicit model routing; do not silently fall back to an unrelated upstream model.

### Configuration and credentials

- Keep existing configurations backward compatible or include a tested migration.
- Use atomic writes for files that contain credentials or active settings.
- Avoid logging secrets, full request headers, or complete configuration objects.
- Add regression tests for import, export, backup, permissions, and failure cleanup when relevant.

### Local gateway and networking

- Treat all custom URLs, redirects, headers, proxy settings, and upstream responses as untrusted input.
- Preserve local authentication boundaries for the admin API and model gateway.
- Use explicit timeouts and bounded error handling.
- Do not weaken TLS verification or forward credentials to a different host without a documented, reviewed reason.

### Claude Desktop and operating-system integration

- Limit registry, policy, plist, process, and filesystem changes to resources owned by CC Desktop Switch.
- Preserve unrelated administrator or organization-managed settings.
- Include rollback, backup, or cleanup behavior for destructive changes.
- Test platform-specific code on the affected operating system when possible.

### Installers, releases, and dependencies

- Keep release downloads pinned to trusted project-controlled locations.
- Preserve checksum/signature verification and fail closed on integrity mismatches.
- Explain why a new dependency is necessary and prefer maintained packages with a narrow permission surface.

## Tests

A bug fix should normally include a test that fails before the fix and passes after it. Tests must not require real provider credentials, modify the user's normal Claude Desktop configuration, depend on paid APIs, or make uncontrolled network requests.

Use temporary directories, mock upstream services, and sanitized fixtures. Cross-platform behavior should avoid assuming POSIX permission bits on Windows.

## AI-Assisted Contributions

AI-assisted contributions are welcome, but the human author remains responsible for every line submitted. Review generated changes, understand their security impact, run the relevant tests, and disclose substantial AI-generated implementation in the pull request description. Do not send repository secrets, user logs, private provider data, or third-party proprietary code to an external model.

## Pull Request Checklist

- [ ] The change solves a clearly described problem and is not a duplicate.
- [ ] The diff is focused and contains no unrelated generated or formatting churn.
- [ ] Security and backward-compatibility implications are explained.
- [ ] Tests cover the changed behavior and pass locally.
- [ ] Documentation or changelog entries are updated when users will notice the change.
- [ ] No credentials, private logs, build artifacts, or local configuration are committed.
- [ ] Provider-specific claims link to an authoritative source.

Maintainers may ask for a smaller scope, additional tests, or a design discussion before merging. This protects user credentials and desktop configuration while keeping reviews practical.
