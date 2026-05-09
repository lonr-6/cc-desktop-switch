# Runbook: macOS Rust Mainline Build and Smoke

Date: 2026-05-09

## Purpose

`v1.1.0-rc1` requires both macOS arm64 and macOS x64 build/smoke evidence. This runbook is for the Rust/Tauri mainline, not the old Python packaging path.

## Runner Labels

Use explicit GitHub-hosted runner labels, not `macos-latest`, so the architecture cannot silently drift.

Source checked 2026-05-09 and rechecked during P54:

- GitHub-hosted runners reference: <https://docs.github.com/actions/reference/runners/github-hosted-runners>
- Tauri macOS app bundle reference: <https://v2.tauri.app/distribute/macos-application-bundle/>

Required CI matrix:

| Platform | Runner | Expected `uname -m` |
|---|---|---|
| macOS arm64 | `macos-14` | `arm64` |
| macOS x64 | `macos-15-intel` | `x86_64` |

`cargo xtask verify --stage rc-readiness` now statically checks that the workflow keeps these explicit runner labels, expected `uname -m` values, Rust/UI/Tauri gates, bundle smoke, and artifact retention. This is only a path-quality gate; it does not replace a real GitHub Actions run.

## Workflow

Non-publishing workflow:

```text
.github/workflows/rust-mainline-platform-smoke.yml
```

It runs:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd ui && trunk build --release
cargo tauri build
```

Then it checks:

- runner architecture matches the expected architecture
- `CC Desktop Switch.app` exists
- `Contents/Info.plist` passes `plutil -lint`
- at least one executable exists under `Contents/MacOS`
- a DMG exists and passes `hdiutil verify`
- a PKG is created with `pkgbuild --install-location /Applications --component ...`
- the PKG expands with `pkgutil --expand`

Artifacts are uploaded only as workflow artifacts:

```text
rust-mainline-macos-arm64
rust-mainline-macos-x64
```

Each artifact also includes:

```text
platform-smoke-evidence.md
```

After both matrix jobs pass, download both evidence files and write a combined handoff under `project-docs/handoff/` that includes:

- `## Result`
- `Pass`
- `platform.macos_arm64_x64_smoke_path`
- `macos-14`
- `macos-15-intel`
- the workflow run URL
- the arm64 and x64 artifact names

Preferred local collector after downloading artifacts:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-PlatformSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
```

The collector recursively finds both `platform-smoke-evidence.md` files, verifies:

- `## Result` / `Pass`
- `platform.macos_arm64_x64_smoke_path`
- `macos-14` with `actual_uname: arm64`
- `macos-15-intel` with `actual_uname: x86_64`
- Rust, UI, Tauri, DMG, and PKG smoke markers

Then it writes a combined handoff with `## Result` / `Pass`, `platform.macos_arm64_x64_smoke_path`, `macos-14`, and `macos-15-intel`, so `cargo xtask verify --stage rc-readiness` can recognize the platform evidence.

The workflow does not publish a GitHub Release.

## Automated Real Config Smoke in Workflow

The same non-publishing workflow also runs the macOS real Claude Desktop local config smoke inside each macOS matrix job. It sets `HOME` to a temporary runner directory before running:

```bash
scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write
```

This means the smoke uses the real macOS local configLibrary path shape under a disposable home directory:

```text
$RUNNER_TEMP/ccds-real-smoke-home/Library/Application Support/Claude-3p/configLibrary
```

The workflow copies the generated evidence and cargo test log into each uploaded artifact under:

```text
real-desktop-smoke/
```

After downloading workflow artifacts, validate the real Desktop smoke evidence with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
```

This automated workflow smoke does not replace final human GUI testing on a real Mac. It is the RC automation gate for write/readback/restore and safe-route behavior.

## Local macOS Commands

On an arm64 or Intel Mac:

```bash
rustup component add rustfmt clippy
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cargo install tauri-cli --version "^2" --locked
cargo xtask verify --all
```

For local bundle smoke:

```bash
app_path="$(find target/release/bundle -type d -name 'CC Desktop Switch.app' -print -quit)"
plutil -lint "$app_path/Contents/Info.plist"
find "$app_path/Contents/MacOS" -type f -perm -111
dmg_path="$(find target/release/bundle -type f -name '*.dmg' -print -quit)"
hdiutil verify "$dmg_path"
pkgbuild --install-location /Applications --component "$app_path" "dist/platform-smoke/local/CC-Desktop-Switch-local.pkg"
pkgutil --expand "dist/platform-smoke/local/CC-Desktop-Switch-local.pkg" "$TMPDIR/ccds-pkg-expanded"
```

## Real Claude Desktop Local Config Smoke

Run this only on an unmanaged macOS user profile where it is acceptable to temporarily write:

```text
~/Library/Application Support/Claude-3p/configLibrary/_meta.json
~/Library/Application Support/Claude-3p/configLibrary/cc-desktop-switch-local-gateway.json
```

The test backs up those files first, applies the local gateway config, reads it back, runs the gateway smoke, stops the gateway, then restores the original files.

Default read-only preflight wrapper:

```bash
scripts/macos/run-real-desktop-smoke.sh --mode preflight
```

The wrapper writes evidence to:

```text
target/real-desktop-smoke/macos-real-desktop-smoke-evidence.md
```

Do not treat `Preflight` or `UnsupportedPlatform` evidence as pass evidence.

Preferred real smoke wrapper:

```bash
scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write
```

Direct test command:

```bash
export CCDS_ALLOW_REAL_DESKTOP_WRITE=1
cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture
unset CCDS_ALLOW_REAL_DESKTOP_WRITE
```

Pass criteria:

- test output ends with `test result: ok`
- Desktop readback uses the loopback gateway URL
- `inferenceModels` contain only `claude-*` safe routes
- `Default` is absent
- no raw upstream model route such as `deepseek-v4-pro` is written as a Desktop route name
- original `_meta.json` and gateway config files are restored after the test

After the wrapper reports `result=Pass`, validate the evidence and cargo test log before writing a handoff:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```

The collector rejects preflight evidence, `UnsupportedPlatform` evidence, failed evidence, missing logs, non-Darwin evidence, and logs without `test result: ok`. Its generated handoff contains the `macOS real Claude Desktop local config smoke`, `configLibrary`, and `safe route` markers used by `cargo xtask verify --stage rc-readiness`.

## Manual App Smoke

The CI workflow validates the bundle without relying on an interactive GUI session. Before RC, still run manual app smoke on both architectures:

1. Launch `CC Desktop Switch.app`.
2. Confirm the window opens.
3. Save a Provider with a non-secret test key or fixture.
4. Start the local gateway and run gateway smoke.
5. Apply to Claude Desktop local configLibrary on an unmanaged profile.
6. Confirm `inferenceModels` contains only `claude-*` safe routes.
7. Confirm `Default` is absent.
8. Confirm hiding/restoring the app keeps the gateway available.

## Release Gate

The legacy `.github/workflows/release.yml` now passes `macos-x64` to `New-ReleaseManifest.ps1` as a required platform. If someone tries to publish without macOS x64 assets, manifest generation must fail.
