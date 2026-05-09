---
name: ccds-rust-mainline-task
description: Use for CC Desktop Switch Rust/Tauri mainline implementation or refactor tasks in this repository, especially work touching model_catalog, desktop policy, local gateway, diagnostics, update, installer, or Rust/WASM UI. Do not use for Python stable-line hotfixes.
---

# CC Desktop Switch Rust Mainline Task

Follow this workflow for Rust/Tauri mainline implementation work.

## Start

1. Read `AGENTS.md`.
2. Read `project-docs/status.md`.
3. If the task is T2/T3, read or update `PLANS.md`.
4. Read only the module docs relevant to the task:
   - architecture: `docs/architecture/rust-mainline-architecture.md`
   - UI/UX: `docs/product/ui-ux-rust-mainline.md`
   - testing: `docs/testing/release-and-regression-gates.md`
   - parity: `docs/testing/python-rust-parity-matrix.md`

## Implementation Rules

- Keep the local gateway as the only normal user path.
- Keep Claude Desktop-visible model names as Claude-safe route aliases.
- Do not expose raw provider model names to Claude Desktop.
- Do not let `Default` enter the Claude Desktop model menu.
- Do not show success unless the full apply flow passes readback verification.
- Keep modules low-coupled; do not let UI code own provider, desktop, or gateway rules.

## Done When

- The task's plan or task card acceptance criteria are met.
- Relevant unit tests pass.
- Relevant eval cases in `docs/testing/eval-harness.md` are covered or explicitly deferred.
- Any new bug learning is added to `project-docs/bugs/`.
- `project-docs/status.md` is updated if current truth changed.
