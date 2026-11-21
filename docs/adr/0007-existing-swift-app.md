# ADR-0007 — The existing Swift macOS app

- **Status:** accepted
- **Date:** 2026-11-21
- **Related:** AGENTS.md §1, §12

## Context

The original macOS app ("Search") is the behavioural reference. Its Swift source
is not in this checkout. The rewrite is a separate Rust workspace.

## Decision

- Keep the Swift app as the **reference implementation and data-format source**.
  Do not delete it, rewrite it file-by-file, or transliterate it into gpui.
- Build the Rust app as a new product. Match behaviour where FEATURES.md says so;
  record deviations in TODO.md and the spec.
- The Swift app may continue to ship while the Rust app matures. Do not force a
  migration before the Rust app passes its daily-driver gate.
- Treat old data files as hostile input when importing them (P1-12).

## Consequences

- Two codebases exist during the transition. That is deliberate.
- A feature is not "done" because the Swift app has it; it is done when the Rust
  implementation passes its acceptance criteria.
- The old app is not a dependency of the new build.

## Verification

- No Rust crate imports Swift or Objective-C app code.
- The import path reads old files without writing to them (P1-12).
