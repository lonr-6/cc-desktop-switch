# P51 macOS Real Smoke Evidence Wrapper Summary

Date: 2026-05-09

## Goal

Add a safe macOS wrapper for real Claude Desktop local config smoke evidence capture. The wrapper must default to read-only preflight and require an explicit opt-in before any real Desktop write test can run.

## Changes

- Added `scripts/macos/run-real-desktop-smoke.sh`:
  - `--mode preflight` is the default and is read-only
  - off macOS, preflight writes `UnsupportedPlatform` evidence instead of attempting a Desktop smoke
  - preflight records platform, arch, and `~/Library/Application Support/Claude-3p/configLibrary` existence
  - evidence is written to `target/real-desktop-smoke/macos-real-desktop-smoke-evidence.md`
  - `--mode run` refuses to execute unless `--allow-real-desktop-write` is passed
  - run mode sets `CCDS_ALLOW_REAL_DESKTOP_WRITE=1` only for the ignored Rust test process
- Updated `xtask/src/main.rs`:
  - `cargo xtask verify --stage rc-readiness` now statically checks the macOS wrapper exists and requires explicit write opt-in
- Updated docs:
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `C:\Program Files\Git\bin\bash.exe scripts/macos/run-real-desktop-smoke.sh --mode preflight` | Passed on Windows Git Bash by writing `UnsupportedPlatform` evidence |
| `C:\Program Files\Git\bin\bash.exe scripts/macos/run-real-desktop-smoke.sh --mode run` | Expected failure; refused without `--allow-real-desktop-write` |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; new macOS wrapper static check passes, real evidence checks still report 3 missing |
| `git diff --check` | Passed; only existing CRLF warnings for tracked baseline files |

Preflight output on Windows Git Bash:

```text
result=UnsupportedPlatform
platform=MINGW64_NT-10.0-26200
configLibraryExists=False
```

## Result

Partial.

The macOS evidence wrapper is ready, but macOS real Claude Desktop local config smoke still has not passed. `Preflight` and `UnsupportedPlatform` must not be treated as pass evidence.

## Remaining Gaps

- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.
- Windows real Claude Desktop local config smoke is still blocked by managed policy.

## Next Minimum Task

On unmanaged macOS arm64 and x64 profiles, run:

```bash
scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write
```

Then archive pass evidence in a handoff with `## Result` / `Pass`, `macOS real Claude Desktop local config smoke`, `configLibrary`, and `safe route`.
