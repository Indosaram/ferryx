# Ferryx Pages Review Remediation Plan

## Scope

Resolve every independently reproduced blocker in the documentation/Pages review while preserving the existing public design.

## Ordered work

1. Capture RED evidence for invalid root README commands, stale GitHub namespace links, missing LICENSE target, evidence command typo, and omitted `ui/**` Pages trigger.
2. Replace stale GitHub repository/release links across `site/astro.config.mjs` and public site components.
3. Correct README commands, remove the unsupported license link/claim, correct delivery evidence, and add the `ui/**` workflow trigger.
4. Prove the corrections in a clean archive: install both workspaces, run README validation commands, build the Pages site, inspect built HTML links, and run workflow YAML/whitespace checks.
5. Commit and push only the remediation files; deploy the published revision and verify live HTTP/HTML, repository metadata, and current workflow success.
6. Write `docs/reviews/FERRYX_DOCUMENTATION_REMEDIATION_2026-08-23.md` with RED/GREEN evidence and final reviewer verdict.

## Guardrails

- No styling or component-layout change.
- No new license terms: remove the unsupported README license link instead of inventing a legal document.
- Do not touch concurrent changes outside the remediation file list.
- Every temporary archive must be removed by the verification command that creates it.
