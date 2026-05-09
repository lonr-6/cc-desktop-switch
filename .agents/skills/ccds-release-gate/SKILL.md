---
name: ccds-release-gate
description: Use before publishing or packaging CC Desktop Switch Rust mainline releases, RC builds, update metadata, installer assets, signatures, or latest.json. Checks release completeness and blocks unsafe Latest releases.
---

# CC Desktop Switch Release Gate

Use this for packaging, update, signing, and release verification.

## Required Checks

1. Read `docs/testing/release-and-regression-gates.md`.
2. Confirm the release tier:
   - local RC
   - draft release
   - public latest
3. Verify required platform assets for that tier.
4. Verify `latest.json`, sha256 files, signatures, and public key.
5. Confirm Windows installer behavior and macOS assets according to the current decision docs.
6. Confirm release notes do not claim unsupported platforms or unverified signing state.

## Blockers

Block public Latest if:

- any required platform asset is missing
- `latest.json` references a missing asset
- signatures or hashes are generated before final asset staging
- installer path migration is unverified
- current docs contain conflicting release instructions

## Evidence

Report commands run, files checked, and exact missing assets. Do not claim a build or upload succeeded unless it was actually executed.
