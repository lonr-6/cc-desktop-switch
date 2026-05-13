# P79 Doc Cleanup Summary

## Scope

This cleanup condensed temporary, untracked review artifacts before deleting them.

## Condensed Findings

- `project-docs/external-reviews/20260512-2203-rust-mainline-workflow-review` was a prepared external AI review packet.
- The synthesis files did not contain accepted implementation findings:
  - `03-synthesis/comparison-matrix.md` only had an empty table.
  - `03-synthesis/accepted-rejected.md` only had a title.
- `04-validation.md` recorded that the upload packet risk scan found no actual GitHub tokens, cloud keys, bearer tokens, or `sk-*` secrets; reported matches were documentation examples, run IDs, timestamps, or security rule text.
- Upload was still pending explicit user approval; no evidence in the folder is needed for current implementation work.
- `project-docs/external-reviews/20260512-2247-rust-mainline-workflow-review` was another generated external AI upload packet. It only recorded generated packet files and upload manifest metadata; no synthesis or accepted implementation findings were present.
- `project-docs/external-reviews/20260512-2315-rust-mainline-workflow-review` was regenerated later and did contain an external AI group review synthesis. The useful findings were condensed here before deletion:
  - Keep the current Rust/Tauri + Leptos + local-gateway architecture and the repo workflow (`AGENTS.md`, `PLANS.md`, `status.md`, handoff/bugs/runbooks, skills, eval harness).
  - Do not tag, publish, update `latest.json`, or mark RC complete before remaining P0 gates and human smoke are closed.
  - P78/P80-era P0 order was update runtime, clear Claude Desktop config, restart Claude Desktop, then Windows manual upgrade smoke on the newest package. P81 has since completed update runtime, leaving clear/restart Desktop next.
  - Refresh macOS arm64/x64 workflow and real Desktop smoke after final P0 changes, then keep real human Mac testing as a final gate.
  - Refresh `project-docs/runbooks/final-human-rc-smoke.md`; the review found stale UI checks and stale macOS run IDs.
  - Add a release workflow guard before publishing work; the review found `.github/workflows/release.yml` can tag/push and mark a release latest through `workflow_dispatch`, which must stay blocked by explicit approval.
  - Keep release fail-closed for macOS x64; missing x64 release assets should remain a hard blocker.
  - Continue redaction coverage for gateway auth fields and request logs (`gateway_api_key`, `x-api-key`, `Authorization`).
- `gpt-initial-snapshot.md` was a temporary browser accessibility snapshot and did not contain durable project facts.

## Deleted Temporary Artifacts

- `project-docs/external-reviews/20260512-2203-rust-mainline-workflow-review`
- `project-docs/external-reviews/20260512-2247-rust-mainline-workflow-review`
- `project-docs/external-reviews/20260512-2315-rust-mainline-workflow-review`
- `gpt-initial-snapshot.md`

## Preserved Docs

P74-P78 bug and handoff docs were kept because they still carry root-cause evidence and are referenced by `project-docs/status.md`.
