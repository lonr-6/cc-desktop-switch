# Pre-Rebuild Workflow Audit

Date: 2026-05-08

## Verdict

The Rust mainline worktree is ready for a new main-controller window to start implementation.

No blocker was found in the workflow, project plan, or documented product decisions. The first implementation task must still be the Rust/Tauri + Leptos skeleton and pure Rust UI spike, because the current worktree intentionally contains documentation and repo-local workflow rules only.

## Scope Checked

- `AGENTS.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/README.md`
- `project-docs/runbooks/rust-mainline-workflow.md`
- `project-docs/handoff/2026-05-08-rust-mainline-rebuild-task-card.md`
- `project-docs/decisions/*`
- `project-docs/bugs/2026-05-08-known-issues-root-cause-register.md`
- `docs/architecture/rust-mainline-architecture.md`
- `docs/product/ui-ux-rust-mainline.md`
- `docs/product/rust-ui-spike-exit-criteria.md`
- `docs/testing/python-rust-parity-matrix.md`
- `docs/testing/eval-harness.md`
- `docs/security/diagnostics-redaction-threat-model.md`
- `.agents/skills/*`

## Subagent Review Summary

### Workflow / Plan Reviewer

Result: can start.

Findings:

- No blocker.
- Rust mainline, Python stable-line separation, `Default` no fallback, macOS x64 hard gate, and historical-document isolation are all documented.
- Follow-up items were P2 only: move plan status out of `Drafting`, tighten “show all provider models” wording, create `xtask` early, and avoid using release checklist deferral language to skip accepted stable-line behavior.

### Main-Prompt / Requirements Reviewer

Result: can start.

Findings:

- No planning gap blocking implementation.
- Do not start with feature migration. First build the Rust/Tauri + Leptos skeleton and pure Rust UI spike.
- Current worktree has no `Cargo.toml`, `src-tauri`, or `Trunk.toml`; this is expected before skeleton work.
- Key decisions are documented: Rust/Tauri future mainline, pure Rust UI, local gateway path, `Default` form-only behavior, macOS x64 RC hard gate, stable-line PR separation, oh-my-codex workflow lessons, skills, and eval harness.

## Tightened Decisions After Audit

- `PLANS.md` status moved from `Drafting` to `Ready for skeleton implementation`.
- Ordinary UI must delete “show all provider models”. A hidden advanced/debug capability can exist only if it cannot affect Claude Desktop model menu behavior.
- `v1.1.0-rc1` Local RC also requires Windows x64, macOS arm64, and macOS x64 buildability.
- Release checklist deferral language cannot be used to skip accepted Python stable-line behavior.

## Remaining Risks

- Pure Rust UI selection still needs an actual spike against Tauri command calls, packaging, bundle size, and developer experience.
- macOS x64 cannot be considered done until real build and install testing passes.
- macOS `configLibrary` / 1M behavior still needs real Claude Desktop `1.6259.1+` validation.
- OpenCode Go / new-api relay compatibility needs a test key or a redacted diagnostics package.
- Windows update-installer launch behavior needs a reproducible Windows 11 update path.

## Next Step

Open a new main-controller window, use `project-docs/handoff/2026-05-08-new-window-main-controller-prompt.md`, and start with the skeleton implementation phase.
