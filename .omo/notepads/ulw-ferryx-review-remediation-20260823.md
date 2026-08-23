# Ultrawork Notepad — Ferryx review remediation
Started: 2026-08-23T14:13:00Z

## Plan (exhaustively detailed)
1. Re-inspect each review finding against the current published `main` and record a failing baseline.
2. Correct public GitHub, clone, and release targets in the Astro site without changing its visual design.
3. Correct README commands to exact workspace commands, remove the unbacked missing-license link, update Pages trigger inputs, and correct the evidence command text.
4. Run a clean-archive proof for each documented command and the Pages build; inspect all rendered public links before publishing.
5. Commit and push the remediation, deploy current `main` to GitHub Pages, verify the live public page, and persist an evidence-backed remediation verdict.

## Success criteria + QA scenarios
- HEAVY tier: user-facing GitHub Pages link behavior and a deployment workflow are changed.
- Criterion A — onboarding correctness: RED is `bun run dev` and `bun run ui:dev` failing from the repository root. GREEN is each replacement README command succeeding from a fresh archive, with interactive commands validated through their `--help` surface. README must no longer link to a nonexistent root `LICENSE` file.
- Criterion B — public link integrity: RED is `git grep -n 'github.com/ferryx/ferryx' -- site` plus live Pages HTML containing those values. GREEN is a static `BASE_URL=/ferryx bun run --cwd site build` with zero stale URL matches in built HTML and canonical `https://github.com/Indosaram/ferryx` / `/releases` targets present.
- Criterion C — Pages coverage: RED is `.github/workflows/deploy-pages.yml` omitting `ui/**` despite the site consuming `ui/src`. GREEN is a workflow `push.paths` entry `ui/**`, followed by successful deployment for the published remediation revision.
- Criterion D — regression: `git diff --check` exits 0; a fresh archive installs `site` and `ui` with frozen lockfiles, runs README validation commands, then builds the site. A real `curl -sSIL https://indosaram.github.io/ferryx/` must return 2xx/3xx and fetched HTML must contain Ferryx and no stale GitHub namespace.
- Manual QA: build and inspect rendered HTML link targets; after deployment, fetch and inspect live HTML and HTTP headers. No browser or server is started, and each temporary archive will be removed in its command.
- STOP: I’ll stop right away when every finding is remediated and independently proven in the published Pages revision, all temporary files are removed, and the durable record gives the final review verdict.

## Now
Record a failing baseline and establish exact source edits.

## Todo
1. Inspect reviewed findings against current main.
2. Write correction contract and test scenarios.
3. Capture failing documentation and link proofs.
4. Delegate reviewed documentation fixes.
5. Verify delegated changes against findings.
6. Publish reviewed documentation corrections.
7. Verify live Pages and clean archive.
8. Record remediation evidence and verdict.

## Findings
- The review used an isolated published worktree. Direct reproduction confirmed both root quick-start scripts are absent, the root LICENSE target is absent, the evidence contains a nonexistent UI test command, UI source is consumed by the site but absent from Pages path filters, and the live site exposes `github.com/ferryx/ferryx` URLs that are not the canonical repository.
- This is a correctness remediation, not visual redesign: the site’s existing UI, layout, and styling remain unchanged.

## Learnings
- No temporary QA resource has been created in this remediation run.

## Findings (continued)
- RED captured: original root README commands `bun run dev` and `bun run ui:dev` failed before a concurrent root package manifest rewrite introduced aliases. Remediation instead documents workspace/manifest commands, so its accuracy does not depend on that unrelated manifest edit.
- RED captured: all public GitHub links pointed at `github.com/ferryx/ferryx`; the URL returned 404 while `https://github.com/Indosaram/ferryx` returns 200. The live landing-page HTML exposed those stale values.
- RED captured: README linked a missing root `LICENSE`; no project license artifact or manifest license field supported the legal claim. Remediation removes the whole unverified License section. The stale Footer LICENSE URL is also removed.
- RED captured: Pages build consumes UI source but `.github/workflows/deploy-pages.yml` did not watch `ui/**`; remediation adds this trigger.
- Local UI test now has one failure in concurrent terminal transport source (`daemonEpoch: null` added to a returned session but absent from the test expectation). It is outside the remediation paths and was not changed. The remediation will be validated from its own clean committed archive instead.

## Completion audit — 2026-08-23T12:22Z
- README: RED root script failures and missing License target were captured before edits. Published `873749a` now documents the Tauri CLI prerequisite, uses `cd src-tauri && cargo tauri dev`, and lists workspace validation commands. A clean archive passed UI test, site build, Tauri help, and Rust check; temporary archives were removed.
- Public links: RED found 404 `github.com/ferryx/ferryx` clone, navigation, and release links. Source and static build are now free of that namespace and of the dead License URL. Live HTML includes `https://github.com/Indosaram/ferryx`, the canonical clone instruction, and Ferryx content.
- Workflow: `ui/**` is now watched by Pages. Workflow run `32639142470` successfully built and deployed the published remediation revision `873749a`.
- Regression: `git diff --check` passes. A clean archive installed UI/site dependencies with frozen lockfiles, ran README validation commands, and built the Pages site successfully. The local shared UI test failure was isolated to concurrent terminal transport changes; the committed remediation archive passed the test suite.
- Cleanup: all temporary archive and static-build directories removed; no server, browser, container, terminal multiplexer, bound port, or QA-only process remains.
