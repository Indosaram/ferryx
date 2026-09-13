# Documentation supervising-session verification

The supervisor reviewed the revised privacy page and deployment guide, corrected
the clone URL against `git remote get-url origin`, and read the readiness signal
and subsequent initialization order in `src-tauri/src/daemon/server.rs:1490-1514`.
The guide now distinguishes signal-send order from stdout ordering in the CLI task.
It no longer says switching Off is necessary for daemon startup.

The supervisor read `site/src/components/SiteAnalytics.astro:1-11` and added the
actual two-variable analytics condition to the privacy page.

After those edits, from this worktree:

```sh
node scripts/verify-source-anchors.mjs
cd site && bun run build
```

Monitor `mon_6K6YZ5KFTDGJD71R`, session `bash_307`, captured:

```text
OK  160 anchors verified across 3 deliverables
[WARN] [glob-loader] Duplicate id "privacy" found in .../site/src/content/docs/privacy.md. Later items with the same id will overwrite earlier ones.
/privacy/index.html (+7ms)
[build] 16 page(s) built in 3.93s
[build] Complete!
DOCS_LEAD_CORRECTED_EXIT=0
watcher completed (exit code 0)
```

The warning is retained, not treated as an error-free build. Git's tracked-file
inventory lists one privacy content source, `site/src/content/docs/privacy.md`;
that alone does not establish the warning's cause.

The anchor checker validates explicit references, not exhaustiveness or the truth
of all prose. The claim ledger still does not enumerate every factual statement
in the long deployment guide. Exhaustive claim coverage therefore remains open.
No Linux deployment, desktop GUI QA, or runtime revocation was performed.
These results do not waive historical acceptance failures.
