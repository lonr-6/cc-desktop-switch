---
name: ccds-review-pass
description: Use for CC Desktop Switch Rust mainline review passes before merge, RC build, or release. Focuses on correctness, compatibility, security, testing, diagnostics redaction, and release safety.
---

# CC Desktop Switch Review Pass

Use this for structured review.

## Review Lenses

Always check:

- correctness
- maintainability
- testing
- Python-to-Rust feature parity

Add when relevant:

- security and redaction
- API and config compatibility
- Windows/macOS platform behavior
- release/update safety
- UI/UX regression

## Evidence Rules

- Cite file paths and line numbers when possible.
- State whether each finding is blocker or follow-up.
- Distinguish verified facts from inference.
- Mention any tests not run.

## Output

Findings first, ordered by severity. If no blockers are found, still list residual verification gaps.
