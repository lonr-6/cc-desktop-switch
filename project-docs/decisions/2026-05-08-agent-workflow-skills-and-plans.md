# Decision: Codex Skills, PLANS.md, And Local Harness Workflow

Date: 2026-05-08

## Context

The Rust rewrite is long-running and cross-platform. It needs durable rules that survive context compaction, subagent work, and future sessions.

OpenAI Codex guidance recommends:

- reusable guidance in `AGENTS.md`
- planning before difficult tasks
- `PLANS.md` or execution-plan templates for long-running work
- skills for repeated workflows
- subagents for explicit parallel exploration or review
- evals/traces for repeatable quality checks

## Decision

Use the following split:

| Surface | Purpose | What belongs here |
|---|---|---|
| `AGENTS.md` | Short durable repo rules | current mainline, hard constraints, where to read next |
| `PLANS.md` | Active long-task control | T2/T3 plan index, execution gates, decisions |
| `.agents/skills/*` | Reusable Codex workflows | repeated implementation, issue triage, release gate, review pass |
| `project-docs/*` | Project memory | status, bugs, decisions, handoff, templates |
| `docs/testing/eval-harness.md` | Repeatable quality checks | issue fingerprints and expected behavior |

## Adopted Repo Skills

| Skill | Use |
|---|---|
| `ccds-rust-mainline-task` | Rust/Tauri implementation tasks |
| `ccds-issue-triage` | GitHub issues, screenshots, diagnostics, community reports |
| `ccds-release-gate` | RC/release/update/signature/latest checks |
| `ccds-review-pass` | merge/release review with parity/security/testing lenses |

## Not A Skill

Do not turn these into skills:

- product decisions
- one-time architecture records
- historical bug descriptions
- release notes
- broad “do everything” workflows

Those belong in normal docs. Skills stay focused and procedural.

## Model/Agent Fit

GPT-5.5 is appropriate as the main controller for this rewrite because it has strong reasoning, large context, and tool-use ability. That does not replace validation:

- use subagents for bounded read-heavy review
- use `xtask`/tests/evals as evidence
- use `PLANS.md` to prevent drift
- use stage summaries after large phases

## Resolved Product/Workflow Decisions

- macOS x64 is a hard gate for `v1.1.0-rc1`.
- `Default` is only a provider form/config convenience and is not any runtime fallback.
- Python stable-line PRs are merged/validated first on the stable line; Rust mainline stays separate and absorbs final behavior/tests.
