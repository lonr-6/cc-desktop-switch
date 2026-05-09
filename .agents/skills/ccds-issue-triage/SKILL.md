---
name: ccds-issue-triage
description: Use when triaging CC Desktop Switch GitHub issues, user bug reports, screenshots, diagnostics packages, or community feedback. Maps symptoms to known issue fingerprints, root causes, tests, and follow-up tasks.
---

# CC Desktop Switch Issue Triage

Use this for issue analysis and bug intake.

## Inputs

- Issue URL or report text
- Screenshots or logs if provided
- CCDS version
- Claude Desktop version
- Platform
- Provider and API format

## Workflow

1. Check `project-docs/bugs/2026-05-08-known-issues-root-cause-register.md`.
2. Assign an issue fingerprint if possible.
3. Separate confirmed facts, likely inference, and missing evidence.
4. Check whether the issue is already covered by the Rust mainline plan.
5. Propose the smallest test or diagnostic needed to prove the root cause.
6. If the issue changes project direction, add or update a decision document.

## Output

Return:

- fingerprint
- impact
- likely module
- needed evidence
- implementation implication
- user-facing reply draft when requested

Never include API keys, gateway keys, Authorization headers, cookies, tokens, or real conversation content in the output.
