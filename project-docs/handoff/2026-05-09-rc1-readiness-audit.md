# v1.1.0-rc1 Readiness Audit

Date: 2026-05-09

## Objective

Move the Rust/Tauri mainline to a verifiable `v1.1.0-rc1` candidate state.

## Prompt-to-Artifact Checklist

| Requirement | Evidence | Status |
|---|---|---|
| Work only in `D:\ccds-build\cc-desktop-switch-rust-mainline` | All changed/added paths are under this worktree | Pass |
| Do not modify `D:\cc desktop swtich` | Not touched in current work | Pass |
| Do not modify `D:\ccds-build\cc-desktop-switch-v1.0.18` | Not touched in current work | Pass |
| Do not publish or upload GitHub Release | No `gh release`, push, or release upload command was run | Pass |
| Rust/Tauri is future mainline | Cargo workspace with `src-tauri`, `ui`, `xtask`; Tauri build passes locally | Pass |
| Pure Rust UI, Leptos + Trunk, no hand-written JS business logic | UI is Rust/WASM under `ui`; build command is `trunk build --release` | Pass for current UI surface |
| Local gateway is ordinary user path | Gateway lifecycle, Apply flow, Desktop writer, and UI all route through local gateway | Pass |
| Claude Desktop sees only `claude-*` safe routes | `ModelCatalog`, Desktop writer/readback, Provider import/template tests reject raw routes | Pass in tests |
| `Default` is config convenience only | Model mapping, import, template, gateway route tests keep `Default` non-runtime and non-Desktop-visible | Pass in tests |
| Unmapped route returns 400, no fallback | `cargo xtask verify --stage gateway` coverage in eval harness | Pass in tests |
| Apply failure cannot show applied | Apply result `success=true` only after write/readback pass; missing provider/port/managed policy fail before write | Pass in tests |
| Windows real Claude Desktop local config smoke with backup/readback/gateway/restore | P62 elevated cleanup removed old policy after backup; `scripts/windows/run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite` passed; collector handoff records backup/readback/loopback gateway/restore evidence | Pass |
| Windows real Desktop smoke evidence capture | `scripts/windows/run-real-desktop-smoke.ps1` default preflight is read-only and run mode requires `-AllowRealDesktopWrite`; P62 run evidence records `mode: run`, `exit_code: 0`, and cargo log path | Pass |
| Windows real Desktop smoke wrapper readiness static gate | `cargo xtask verify --stage rc-readiness` checks Windows wrapper fingerprint, test name, `-AllowRealDesktopWrite`, `CCDS_ALLOW_REAL_DESKTOP_WRITE`, and `Readiness Markers` | Pass |
| Windows real Desktop smoke evidence collector | `scripts/windows/Collect-RealDesktopSmokeEvidence.ps1` validates `mode: run`, `exit_code: 0`, `## Result` / `Pass`, cargo `test result: ok`, and readiness markers before writing handoff evidence | Pass |
| macOS arm64/x64 build and smoke path | Non-publishing GitHub Actions workflow and runbook exist for `macos-14` arm64 and `macos-15-intel` x64 | Path ready, not run |
| macOS platform smoke evidence artifact | Workflow writes `platform-smoke-evidence.md` into each non-release artifact; runbook explains how to combine both matrix outputs into handoff evidence | Path ready, not run |
| macOS platform workflow static quality gate | `cargo xtask verify --stage rc-readiness` checks `workflow_dispatch`, arm64/x64 expected uname values, `uname -m`, Rust/UI/Tauri gates, Info.plist/DMG/PKG smoke, artifact name, and retention | Pass |
| macOS platform evidence collector | `scripts/macos/Collect-PlatformSmokeEvidence.ps1` validates downloaded arm64/x64 evidence files and writes a combined handoff with `## Result` / `Pass`, `platform.macos_arm64_x64_smoke_path`, `macos-14`, and `macos-15-intel`; fixture self-test passed | Pass |
| macOS real Claude Desktop local config smoke with backup/readback/gateway/restore | Ignored opt-in test and runbook command exist; Windows guard run skips as macOS-only | Harness ready, not run on macOS |
| macOS real Desktop smoke evidence capture | `scripts/macos/run-real-desktop-smoke.sh` default preflight is read-only, returns `UnsupportedPlatform` off macOS, and run mode requires `--allow-real-desktop-write` | Path ready, not run on macOS |
| macOS real Desktop smoke evidence collector | `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` validates `platform: Darwin`, `mode: run`, `exit_code: 0`, `## Result` / `Pass`, cargo `test result: ok`, and readiness markers before writing handoff evidence | Pass |
| macOS x64 is hard gate | Release gate test rejects missing `macos-x64`; legacy release workflow and `New-ReleaseManifest.ps1` defaults now require `macos-x64`; offline manifest smoke verified missing x64 fails | Pass for gate, smoke not run |
| Provider preset marketplace/template import minimum boundary | Built-in presets and secretless `ccds.providerTemplate` import exist; route/Default/duplicate safety covered | Pass for template boundary; remote marketplace not implemented |
| Release metadata / sha256 / sig / public key / platform asset completeness gate | `cargo xtask verify --stage release` passes; PowerShell manifest fixture smoke generated `latest.json`, `.sha256`, `.sig`, public key, and per-asset signatures for Windows x64, macOS arm64, and macOS x64; P39 adds staging-directory validation for `latest.json` references to actual asset files; P44 verifies `latest.json.sha256` and per-asset `.sha256` contents against actual bytes; P45 verifies `latest.json.sig` and per-asset `.sig` against the public key and actual bytes | Pass in Rust gate and PowerShell fixture smoke |
| UI/UX closer to old layout while pure Rust | Leptos UI includes provider CRUD/import/template, gateway/apply, diagnostics, model mapping, backups, issue actions | Partial; richer keyboard/startup/visual polish still open |
| `cargo fmt --all -- --check` | Covered by latest `cargo xtask verify --all` | Pass |
| `cargo test --workspace` | P49 local run: 110 passed, 2 ignored real Desktop smoke tests | Pass, with ignored real-smoke blocker |
| `cargo clippy --workspace --all-targets -- -D warnings` | Covered by latest `cargo xtask verify --all` | Pass |
| `trunk build --release` | Covered by latest `cargo xtask verify --all` | Pass |
| `cargo tauri build` current platform | P49 `cargo xtask verify --all` produced Windows MSI and NSIS bundles | Pass |
| Windows packaged app smoke | P29/P30 records show packaged app single-instance and tray close-to-hide smoke passed | Pass |
| Windows Claude Desktop local config smoke | P62 collector handoff: `project-docs/handoff/2026-05-09-windows-real-desktop-smoke-evidence-summary.md` | Pass |
| macOS arm64 and macOS x64 build/smoke records | P37 workflow/runbook exists but has not been executed | Missing evidence |
| Docs and handoff updated | P33-P57 handoffs, status, PLANS, eval harness updated | Pass |
| Repeatable readiness audit command | `cargo xtask verify --stage rc-readiness` exists and fails closed while required smoke evidence is missing | Pass for audit tool; current result incomplete |

## Current Non-Negotiable Gaps

1. macOS arm64/x64 build and smoke have not run.
   - P37 created the workflow path.
   - P48 added per-architecture `platform-smoke-evidence.md` artifacts.
   - Next: run `.github/workflows/rust-mainline-platform-smoke.yml`, download both evidence files, and save a combined handoff.

2. Real macOS Claude Desktop local config smoke has a harness but has not run on macOS.
   - P47 added `macos_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores`.
   - Need both architectures before RC.

3. Remote marketplace is not implemented.
   - Minimum template boundary is done.
   - Signed remote source validation remains follow-up unless scoped into RC.

## Current Verdict

Not ready to mark `v1.1.0-rc1` complete.

The Rust mainline is locally buildable and has strong fixture coverage, but required real Windows and macOS platform smoke evidence is still missing. P46 added `cargo xtask verify --stage rc-readiness`; P47 added the macOS real Desktop smoke harness; P48 added macOS platform smoke evidence artifacts. The latest readiness audit still must remain incomplete until real smoke evidence is recorded.

P49 refreshed the current-platform full gate on Windows x64. This improves current local confidence but does not satisfy the real Windows/macOS evidence requirements.

P50 added a Windows real Desktop smoke evidence wrapper. Its `Preflight` output is useful blocker evidence, but it is not pass evidence.

P51 added a matching macOS real Desktop smoke evidence wrapper. Its `Preflight` or `UnsupportedPlatform` output is not pass evidence.

P52 aligned the Windows and macOS wrapper `Readiness Markers` with the `rc-readiness` handoff matching keywords. The audit still requires `## Result` / `Pass`, so preflight evidence remains non-pass evidence.

P53 added a matching static `rc-readiness` check for the Windows real Desktop smoke wrapper.

P54 added a stronger static `rc-readiness` check for the macOS platform workflow.

P55 added a downloaded-artifact evidence collector for macOS platform workflow outputs. The latest local audit is 12 pass / 3 missing and still exits non-zero until real Windows/macOS evidence is recorded.

P56 added a Windows real Desktop smoke evidence collector for wrapper pass evidence and cargo test logs. The latest local audit is 13 pass / 3 missing and still exits non-zero until real Windows/macOS evidence is recorded.

P57 added a macOS real Desktop smoke evidence collector for wrapper pass evidence and cargo test logs. The latest local audit is 14 pass / 3 missing and still exits non-zero until real Windows/macOS evidence is recorded.

P58 tightened final pass handoff matching. Windows real Desktop smoke now requires collector-style fingerprint, test name, evidence path, and log path; macOS platform smoke now requires arm64/x64 workflow run and artifact fields; macOS real Desktop smoke now requires fingerprint, test name, `platform: Darwin`, evidence path, and log path. The latest local audit remains 14 pass / 3 missing and still exits non-zero until real Windows/macOS evidence is recorded.

P60 added a decision card for the external evidence execution path. It is an authorization/control document, not pass evidence.

P61 attempted the authorized Windows managed-policy cleanup in the current profile. The script exported a `.reg` backup before mutation, but Windows denied deletion of `HKCU\SOFTWARE\Policies\Claude`; preflight still reports the managed policy and no local `configLibrary`. The latest `cargo xtask verify --stage rc-readiness` remains 14 pass / 3 missing, so no Windows pass handoff was generated.

P62 reran the same cleanup path through elevated PowerShell, exported `C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-elevated-20260509153014.reg`, removed the old policy, and passed Windows real Claude Desktop local config smoke. The collector generated `project-docs/handoff/2026-05-09-windows-real-desktop-smoke-evidence-summary.md`, and the latest `cargo xtask verify --stage rc-readiness` now matches Windows pass evidence and remains incomplete only for the two macOS evidence gates.

P63 checked GitHub Actions availability in read-only mode. The remote repository currently exposes only the `Release` workflow; `gh workflow view rust-mainline-platform-smoke.yml` returns 404 because the non-publishing macOS platform smoke workflow exists locally but not on the remote default branch. Therefore macOS platform evidence cannot be produced without explicit authorization to push/PR the workflow or another authorized macOS runner path.

P64 connected the macOS real Desktop local config smoke to the non-publishing platform workflow. Each macOS matrix job now runs the real smoke under a temporary runner `HOME`, uploads `macos-real-desktop-smoke-evidence.md` plus its cargo log in the workflow artifact, and `rc-readiness` statically checks that this step remains present. This is still path evidence only until the workflow runs on GitHub and collectors generate handoff summaries.
