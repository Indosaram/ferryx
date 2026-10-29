# Evidence artifacts

Raw QA evidence (logs, screenshots, JSON captures) used to live under `docs/evidence/`.
That tree is no longer part of the repository. Links and paths of the form
`docs/evidence/...` or `evidence/...` in older reports under `docs/` point to artifacts
that now live outside the repository. They remain reachable in git history before the
removal commit (`git show <commit>:docs/evidence/<path>`).

The same applies to these internal evidence trees, which were removed from the tree for
the same reason (host-specific paths and logs): `docs/releases/local-release-hardening-evidence/`,
`docs/releases/local-pipeline-audit-2026-09-08/`, `docs/rendering-review-2026-09-06/`,
`docs/session-continuation-20260908/` and `docs/session-restore/p0-evidence/`.

Test fixtures that tests consume live next to their tests (for example
`src-tauri/tests/fixtures/paired-daemon/`).

QA scripts write new evidence to the untracked `.qa-evidence/` directory at the repository
root (ignored by `.gitignore`). Set `FERRYX_QA_EVIDENCE_DIR` to use another location.
