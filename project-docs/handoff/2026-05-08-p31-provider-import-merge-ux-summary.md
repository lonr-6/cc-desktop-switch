# P31 Provider Import Merge UX Summary

## Scope

P31 adds the first safe Provider import merge UX for mixed import packages.

The safety rule remains unchanged: conflict import does not overwrite existing Providers unless the user explicitly chooses replace. This phase adds a second safe action for mixed packages: import new Providers and skip conflicting existing Providers.

## Implemented

- Added `skipExisting` merge mode to Provider import preview/apply commands.
- Kept default import behavior strict:
  - conflicts block writes by default
  - `replaceExisting` is still required to overwrite conflicts
- Added skip-existing behavior:
  - conflicting Provider IDs are skipped
  - non-conflicting incoming Providers are imported
  - existing conflicting Provider config and API key remain unchanged
- Extended Provider import preview with:
  - `unresolvedConflictCount`
  - `skippedConflictCount`
  - `replacedConflictCount`
  - `skipExisting`
- Added Leptos UI actions:
  - `Preview new only`
  - `Import new only`
  - conflict summary box
- Added `provider.import_skip_existing_merge` to the local eval harness.

## Verification

- `cargo fmt --all`
- `cargo xtask verify --stage provider-import`
- `trunk build --release`
- Browser viewport smoke via `trunk serve --address 127.0.0.1 --port 1421 --open false`
- `cargo xtask verify --all`

Latest full gate result on Windows x64:

- `cargo fmt --all -- --check`: pass
- `cargo test --workspace`: pass, 92 tests
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `trunk build --release`: pass
- `cargo tauri build`: pass, produced local MSI and NSIS bundles

Browser smoke:

- Desktop viewport `1440x1000`: page loaded, console errors 0
- Mobile viewport `390x900`: page loaded, console errors 0
- Known browser noise: Chromium warns that Trunk preload `integrity` is ignored, and logs verbose password-field-not-in-form messages.

Known build warning:

- Cargo reports an output filename collision for `cc_desktop_switch.pdb` between the bin and lib targets. Build still passes. This was resolved later in P32 by renaming the internal lib crate.

## Limits

- Per-conflict individual selection is not implemented yet; P31 uses a conservative global `skipExisting` mode.
- External preset marketplace / template import remains deferred.

## Next

1. Continue to Windows packaged app manual smoke.
2. Establish macOS arm64/x64 app shell and bundle smoke before `v1.1.0-rc1`.
3. Add external preset marketplace / template import after platform smoke.
