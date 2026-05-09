# Decision: oh-my-codex Benchmark Lessons

Date: 2026-05-08

## Source

Reference repository: https://github.com/Yeachan-Heo/oh-my-codex

Snapshot facts checked on 2026-05-08:

- Repository has about 27.9k stars and 2.2k forks.
- It is a Codex CLI workflow layer, not a desktop product.
- It is actively optimized for macOS/Linux Codex CLI; native Windows and Codex App are explicitly not its default path.
- It uses skills, prompts, `.omx/` state, doctor checks, real smoke execution, release/readiness reports, parity matrices, and mission/evaluator bundles.

## What To Absorb

| Pattern | Adopt for CC Desktop Switch | Local form |
|---|---|---|
| Doctor plus real smoke | Yes | `Check configuration` should run both static checks and a real minimal provider/gateway request |
| False-green warning | Yes | A green config check must not mean Desktop/gateway/provider all work unless readback and request smoke passed |
| Workflow state directory | Adapt | Use app data diagnostics/runtime state, not `.omx/`; keep logs/exportable state structured |
| Skill catalog manifest | Adapt | Add a small manifest for repo-local skills if skill count grows |
| Skill categories/status | Yes | Keep active/deprecated/internal status for project skills and evals |
| Parity matrix | Already adopted | Keep `docs/testing/python-rust-parity-matrix.md` as a hard planning input |
| Release readiness reports | Yes | Add stage/release summaries before RC and public Latest |
| Mission/evaluator bundles | Adapt | Use local issue-fingerprint eval fixtures, not a generic research loop |
| Plugin packaging | Later | Useful if CCDS eventually ships reusable Codex project helpers |
| Multilingual README table | Already compatible | Keep English default with Chinese/Japanese entry points |

## What Not To Copy

| Pattern | Reason |
|---|---|
| tmux/team runtime as default | CCDS is a desktop app for ordinary users, not a Codex CLI orchestration layer |
| Big global `AGENTS.md` template | Our repo should keep `AGENTS.md` short and push detail into project docs |
| Native Codex hook management | Not part of CCDS product and could interfere with user Codex setups |
| Heavy autonomous multi-agent loops | Too much for this product; use explicit subagents only when user asks |
| macOS/Linux-first assumptions | CCDS must keep Windows first-class |

## Product Implications

1. Rename the internal diagnostic concept to `Doctor` only if it helps the UI. User-facing Chinese can remain `检查配置` and `报告问题`.
2. `检查配置` must distinguish:
   - config shape valid
   - provider auth works
   - model list works
   - gateway starts
   - Claude Desktop write/readback works
   - a minimal model request works
3. Diagnostics should expose a clear false-green state:
   - `static_check_passed`
   - `readback_passed`
   - `provider_smoke_passed`
   - `gateway_smoke_passed`
4. Release prep should generate a local readiness summary before packaging a test build.

## New Follow-Ups

- Add eval case for `diagnostics.false_green_readiness`.
- Add eval case for `provider.real_smoke_failed`.
- Consider `project-docs/templates/release-readiness.md` before first Rust RC.
- Consider `project-docs/templates/skill-catalog.md` if repo-local skills grow beyond the current four.
