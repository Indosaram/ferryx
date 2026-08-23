# Ferryx Documentation Delivery Review

Status: failed
Review revision: `1cd5bd33256535763fbca391a098f1766ff8dd9a`
Review baseline: `c9d757f^`
Isolated review worktree: `/tmp/ferryx-docs-review-1cd5bd3`

## Review contract

The review covers the published documentation-delivery series:

- Pages workflow repair in `.github/workflows/deploy-pages.yml`.
- Root `README.md` rewrite and correction of its executable validation command.
- Relocation of active product and branding specifications to `docs/reference/`.
- Deletion of 19 obsolete root plans, audits, checklists, and implementation receipts.
- Durable delivery evidence in `docs/reference/FERRYX_DOCUMENTATION_DELIVERY_EVIDENCE.md`.

The required result is a Ferryx-facing root README with verified commands, `README.md` as the sole root Markdown file, GitHub Pages associated with the repository and homepage at <https://indosaram.github.io/ferryx/>, a successful Pages deployment for the reviewed revision, a live HTTP 2xx/3xx response containing Ferryx, a clean whitespace check, and a clean archive-based Pages build after installing `site` and `ui` dependencies.

## Review lanes

1. Goal and constraint verification
2. Hands-on QA
3. Documentation/workflow quality
4. Security review
5. Historical and contextual review

## Overall verdict: FAILED

The review cannot approve the documentation delivery. The Pages deployment is healthy and the root cleanup is real, but user-facing onboarding and repository links are inaccurate. Every finding below was reproduced independently in the isolated review worktree at `1cd5bd3`.

## Blocking findings

### HIGH - README desktop quick start commands do not exist

`README.md:48` tells users to run `bun run dev`; `README.md:54` similarly lists `bun run ui:dev`. Neither script exists in the root `package.json`:

```text
$ bun run dev
error: Script not found "dev"

$ bun run ui:dev
error: Script not found "ui:dev"
```

Use the actual workspace command, `bun run --cwd ui dev`, and document the desktop command only from a verified package location.

### HIGH - Live Pages sends users to an unrelated GitHub namespace

The published landing page exposes `https://github.com/ferryx/ferryx`, which returns 404, even though the canonical repository is `https://github.com/Indosaram/ferryx`.

Affected sources:

- `site/astro.config.mjs:20`
- `site/src/components/CTA.tsx:29,35,41`
- `site/src/components/Hero.tsx:22,40`
- `site/src/components/Navbar.tsx:42`
- `site/src/components/Footer.tsx:19,28`

This affects clone instructions, GitHub navigation, and release links. Replace every such URL with the canonical repository URL; use `https://github.com/Indosaram/ferryx/releases` for releases.

### HIGH - README links to a nonexistent LICENSE file

`README.md:99` links to `./LICENSE`, but no project LICENSE file exists. Either add the stated dual-license text at that path or remove the link and avoid claiming a file users cannot open.

## Non-blocking findings

### MEDIUM - Pages does not automatically rebuild for consumed UI source

`site/src/components/LiveFerryxDemo.tsx` imports UI source through the `@ui` alias, but `.github/workflows/deploy-pages.yml:7-9` only triggers on `site/**` and the workflow file. Add `ui/**` to the `push.paths` list so UI changes that affect the embedded demo rebuild and redeploy Pages.

### LOW - Delivery evidence records a nonexistent test command

`docs/reference/FERRYX_DOCUMENTATION_DELIVERY_EVIDENCE.md:22` says `bun run ui:test` passed. The reproducible command is `bun run --cwd ui test`.

## Review lane summary

| Lane | Verdict | Notes |
| --- | --- | --- |
| Goal and constraint verification | PASS, but contradicted by direct command reproduction | It did not validate the root quick-start commands it described. |
| Hands-on QA | PASS with warnings | Validated Pages, archive build, and root cleanup; identified missing LICENSE. |
| Documentation/workflow quality | FAIL | Reported the broken quick-start commands, missing LICENSE, incomplete trigger paths, and stale evidence command. |
| Security | PASS with advisory | Confirmed stale `ferryx/ferryx` links pose a broken-link/hijack risk. |
| Context and history | PASS with warnings | Confirmed missing LICENSE and stale site repository links. |

## Verified passes

- Root cleanup is real: the reviewed archive contains `README.md` as the only root Markdown file.
- Repository homepage and Pages API point to `https://indosaram.github.io/ferryx/`.
- Pages workflow `32617394722` successfully built and deployed the reviewed revision.
- The live Pages endpoint returns HTTP 200 and renders Ferryx content.
- The workflow uses frozen Bun lockfiles and least-privilege Pages permissions.

## Required remediation order

1. Correct all live site repository, clone, and release URLs to `Indosaram/ferryx`.
2. Correct README quick-start commands and remove or resolve the missing LICENSE link.
3. Correct the delivery-evidence test command.
4. Add `ui/**` to the Pages workflow trigger paths.
5. Rebuild the site, run the corrected README commands from a fresh archive, deploy Pages, and repeat this review.
