# Decision: Stable Line And Rust Mainline Branching

Date: 2026-05-08

## Decision

Community PRs and urgent fixes for the current Python stable product are handled on the Python stable line first.

The Rust mainline remains a separate rewrite worktree and does not directly merge stable-line implementation patches unless they are intentionally ported.

## Branch Responsibilities

| Line | Purpose | Rule |
|---|---|---|
| Python stable line | Current user-facing production/stable app | Merge and validate community PRs first when they fix current users |
| Rust mainline | Future Rust/Tauri architecture | Absorb final behavior, tests, and lessons after stable-line validation |
| Old Tauri branch | Historical reference only | Do not continue patch stacking |

## Why

- Current users still depend on the Python stable line.
- Rust mainline should stay clean and root-cause oriented, not become a patch pile.
- Stable-line PRs can prove real user behavior before Rust ports the contract.

## Porting Rule

When a stable-line PR is accepted:

1. Record the user-visible behavior in `docs/testing/python-rust-parity-matrix.md`.
2. Add or update an issue fingerprint/eval if the PR fixes a repeatable bug.
3. Port the behavior into Rust through the appropriate module boundary.
4. Do not copy Python implementation shape when Rust architecture has a cleaner module boundary.
