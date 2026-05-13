# Runbook: Rust Mainline Workflow

## Purpose

Use this runbook for all Rust mainline implementation, review, and release work.

## Start of Task

1. Read:
   - `AGENTS.md`
   - `project-docs/status.md`
   - `PLANS.md` for T2/T3 tasks
   - relevant `project-docs/bugs/*`
   - relevant `project-docs/decisions/*`
   - relevant repo-local skill under `.agents/skills/*`
2. Check worktree:
   ```powershell
   git status --short --branch
   ```
3. Decide task tier:
   - T0: docs or one small isolated change.
   - T1: one module or one user-visible flow.
   - T2: cross-module change, platform write, gateway, update, diagnostics.
   - T3: release, architecture migration, security-sensitive change.
4. For T1+ create or update a task-card in `project-docs/handoff/`.
5. For T2/T3 create or update a plan entry in `PLANS.md`.

## Skill Use

Use repo-local skills for repeated workflows:

- `ccds-rust-mainline-task`: implementation/refactor work.
- `ccds-issue-triage`: issue and diagnostics analysis.
- `ccds-release-gate`: RC/release/update checks.
- `ccds-review-pass`: structured review before merge/release.

Do not create a broad skill that tries to replace all project docs. Skills are procedural; docs remain the source of truth for decisions, bugs, and architecture.

## Implementation Loop

1. Write failing or targeted tests first when feasible.
2. Implement smallest module change.
3. Run the narrow test.
4. Update docs if the rule changed.
5. Run broader tests before handoff.

## Subagent Use

Use subagents only when the user explicitly allows or when the current task has clear parallel slices.

Good subagent tasks:

- Compare Python stable behavior with Rust behavior after stable-line PRs are merged or accepted.
- Review model routing logic.
- Review security/redaction.
- Review release manifest completeness.
- Inspect macOS or Windows platform-specific paths.

Bad subagent tasks:

- Duplicate the same file edits.
- Make broad rewrites without a bounded write set.
- Replace main control of architecture decisions.

## Required Review Matrix

For T2/T3 changes, request or perform reviews across:

- Correctness
- Security and secret handling
- Platform behavior
- Release/update compatibility
- UI/UX clarity
- Test coverage

## End of Task

Before final response:

1. Run relevant tests or state clearly why not.
2. Check:
   ```powershell
   git status --short
   git diff --stat
   ```
3. Update:
   - `project-docs/status.md` for current truth.
   - `project-docs/bugs/*` for bug conclusions.
   - `project-docs/decisions/*` for long-term rules.
   - `project-docs/handoff/*` for task handoff.
   - `docs/testing/eval-harness.md` if a new repeated issue fingerprint appears.

## Release Gate

Release tiers:

- Local RC: can be built for the user's manual testing and must be clearly labeled as local/unpublished.
- Draft release: can upload assets for validation but must not be marked Latest.
- Public Latest: requires full platform and update verification.

For `v1.1.0-rc1`, Local RC is also a hard platform gate: Windows x64, macOS arm64, and macOS x64 assets must all be buildable before it is presented as the first Rust mainline release candidate.

`absorbed or intentionally deferred` in release checklists means only “not applicable to this RC after explicit decision”. It must not be used to skip behavior that was already accepted or fixed on the Python stable line.

Do not publish Public Latest unless:

- Windows x64 assets exist.
- macOS arm64 assets exist.
- macOS x64 assets exist.
- `latest.json` includes all required platforms.
- `.sha256`, `.sig`, `latest.json.sig`, and public key are present.
- Manifest signing uses an existing release key and validates the public key SHA256 against the runtime-pinned fingerprint.
- Fresh P83 macOS arm64/x64 workflow evidence is recorded for the current commit.
- Fresh P83 macOS real Desktop smoke evidence is recorded for both arm64 and x64 on the current commit.
- `cargo xtask verify --stage rc-readiness` passes on the commit being published.
- Windows and macOS manual smoke tests passed.
- User explicitly confirms upload/release with `confirm_publish=PUBLISH_LATEST`.
- The GitHub Actions `release-publish` environment gate approves the publish job.
