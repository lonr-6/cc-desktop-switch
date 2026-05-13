# Release and Regression Gates

## Purpose

Rust mainline cannot produce `v1.1.0-rc1` or publish as Latest until automated and manual gates pass.

## Unit Test Areas

- `model_catalog`
  - only explicit routes exposed
  - no Default in menu
  - no raw provider model in Desktop config
  - unmapped route rejected
  - 1M capability attached to visible route
  - Max capability gated

- `desktop`
  - build expected plan
  - compare expected/actual base URL
  - detect raw model names
  - detect stale routes
  - detect missing supports1m

- `gateway`
  - `/v1/models` matches `ModelCatalog`
  - `/v1/messages` maps route to upstream model
  - OpenAI Chat conversion
  - Anthropic passthrough
  - SSE conversion
  - non-JSON upstream diagnostics
  - unmapped route 400

- `diagnostics`
  - redact API key
  - redact gateway key
  - redact Authorization
  - redact cookies
  - redact URL query token
  - redact custom secret headers
  - preserve issue fingerprints

- `update`
  - latest.json parse
  - platform pick
  - latest.json sha256/signature/public-key verification before asset trust
  - pinned release public-key fingerprint
  - temp staging before replacing `updates/<version>/`
  - selected asset sha256/signature verify
  - installer launcher success/failure logs

## Base Commands

Target commands after skeleton exists:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
Push-Location ui; trunk build --release; Pop-Location
cargo tauri build
```

Preferred aggregate command:

```powershell
cargo xtask verify --all
```

## Windows Manual Smoke

- Install over Python stable version.
- Old install directory detected before directory picker.
- Launch has no black terminal.
- Re-launch focuses existing instance.
- Close window keeps app in tray.
- Apply DeepSeek.
- Verify local Claude Desktop `configLibrary` readback; managed policy paths should block normal Apply instead of being the ordinary user path.
- Verify Claude Desktop menu shows only safe route names.
- Verify gateway logs show safe route/request metadata and do not expose raw upstream model names.
- Simulate registry write failure if possible.
- Check update download and installer launch.
- Uninstall and verify shortcuts/uninstall entry.

## macOS Manual Smoke

- Fresh Claude Desktop environment with no configLibrary.
- Apply DeepSeek 1M.
- Verify `_meta.json` and active configLibrary entry are created.
- Verify 1M route has `supports1m`.
- Verify Claude Desktop menu safe route names.
- Verify app hides/shows correctly.
- Verify PKG and DMG install.
- Verify macOS arm64 and x64 assets.

## Release Asset Gate

Required before `v1.1.0-rc1` and before Latest:

- Windows Setup
- Windows Portable zip
- Windows x64 exe
- macOS arm64 pkg
- macOS arm64 dmg
- macOS x64 pkg
- macOS x64 dmg
- `.sha256` for every asset
- `.sig` for every asset
- `latest.json`
- `latest.json.sha256`
- `latest.json.sig`
- public key

Missing required platform assets, including macOS x64, must fail the release manifest step. `latest.json` must also fail validation if it is invalid JSON or references an asset file that is absent from the staging directory. Directory validation must read `latest.json` and manifest-referenced asset bytes, compute sha256, and fail when `.sha256` sidecar content is invalid or does not match the actual file. Directory validation must also parse the `RSA-CSP-BLOB-SHA256` public key and verify `latest.json.sig` plus every manifest asset `.sig` against the actual file bytes.

The GitHub release workflow must not generate a new local release signing key. It must inject the existing release key pair from `CCDS_RELEASE_PRIVATE_KEY_BASE64` and `CCDS_RELEASE_PUBLIC_KEY_BASE64`, then pass `-RequireExistingKey` and `-ExpectedPublicKeySha256` to `scripts/New-ReleaseManifest.ps1`. Manifest generation must fail if the public key SHA256 does not match the runtime-pinned fingerprint.

The GitHub release workflow is manual-only. It must not publish on `v*` tag push. It may assemble signed staging assets from Windows x64, macOS arm64, and macOS x64 builds, but publishing Latest requires explicit `confirm_publish=PUBLISH_LATEST`, the `release-publish` environment gate, and a publish-job `cargo xtask verify --stage rc-readiness` pass before any signed asset upload. P83 macOS handoff evidence must be bound to the current commit through `expected_commit`, `commit_arm64`, and `commit_x64`; macOS real Desktop smoke must include both arm64 and x86_64 pass evidence/logs. A non-publishing workflow run is the expected path for platform smoke. The legacy `scripts/New-Release.ps1` entrypoint is disabled in the Rust mainline worktree and must not be used to assemble public update metadata.

Windows bundle note: MSI requires numeric-only app version metadata. Keep RC labels such as `v1.1.0-rc1` in release metadata, tags, filenames, or readiness reports, but use numeric Tauri bundle metadata such as `1.1.0` for Windows packaging.
