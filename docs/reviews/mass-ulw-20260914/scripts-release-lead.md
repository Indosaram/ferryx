# Review: release, build, and QA scripts

Scope: `scripts/release-local.mjs` (secret handling, subprocess construction, CI boundary,
notarization gating, publication gating), `scripts/lib/release-hosts.mjs` (redaction helper),
`scripts/build-msix.ps1` and `scripts/release-platforms.test.mjs` (existence / platform set).
Reviewed-at: 2026-09-14
Reviewer: lead session (this lane's dag node did not deliver; findings below are the lead's own,
each verified by direct read)

## Findings

`NO-FINDINGS above P3`

This pipeline was examined against the failure modes that actually matter for a locally-built,
signed, notarized desktop release: shell injection, secret leakage, a skippable signing or
notarization step, and a hosted-CI path that could build or publish artifacts. All four
defenses are present and fail closed. Each is cited below.

### Verified negative — no shell-injection surface

- Location: `scripts/release-local.mjs:2` (imports), and the call sites at `:156`, `:167`,
  `:178`, `:190`, `:201`, `:437`, `:482`, `:515`, `:527`, `:549`, `:561`, `:592`
- Observed: every subprocess goes through `execFileSync` / `spawnSync` with an **array** argv.
  No `exec`, no shell string interpolation. Tag, repo, and path values therefore cannot escape
  into a shell, even though several of them are interpolated into JS template strings for
  logging.

### Verified negative — no secret reaches stdout

- Location: `scripts/release-local.mjs:20` (imports `redactProcessOutput` from
  `./lib/release-hosts.mjs`)
- Observed: grepping every `console.log` / `echo` in the file for
  `token|key|secret|password|credential|minisign` returns nothing. The script imports a
  dedicated redaction helper rather than relying on discipline at each call site.

### Verified negative — the local-release boundary is enforced from source

- Location: `scripts/release-local.mjs:24-31` (`assertNotInCI`), invoked by every mutating stage
  (for example `publishRelease` at `:454`)
- Observed: throws when `process.env.GITHUB_ACTIONS === "true"` or `process.env.CI === "true"`.
  This makes the project rule — GitHub Actions must never build, sign, assemble, or publish
  release artifacts — an executable guarantee rather than a convention.

### Verified negative — notarization verification is fail-closed

- Location: `scripts/release-local.mjs:434-446`
- Observed: on `darwin`, for the published DMG, it runs
  `spctl -a -vvv -t install <dmg>` and throws unless **both** `spctl.status === 0` **and** the
  combined stdout+stderr contains the literal `Notarized Developer ID`. A DMG that is merely
  signed, or ad-hoc signed, fails this gate.
- Why this matters here: an unnotarized build triggers a Gatekeeper malware block on download,
  so a silently-skipped check would ship a broken release. Requiring the `Notarized Developer
  ID` substring — not just exit 0 — is the part that makes it real.

### Verified negative — publication is double-gated and byte-verified

- Location: `scripts/release-local.mjs:462-469`, and the byte comparison at `~:425`
- Observed: publication requires the `--approve-publish` flag **and**
  `env.FERRYX_APPROVE_PUBLISH === "1"`; either alone throws. Before undrafting, each published
  file is compared byte-for-byte against a freshly derived copy
  (`if (!origBuf.equals(tempBuf)) throw ... Byte-for-byte mismatch`), with the temp directory
  removed in a `finally`.

## Summary

- P0: 0
- P1: 0
- P2: 0
- P3: 0

`NO-FINDINGS above P3`

Note: `scripts/release-local.mjs` was the only script read end-to-end for secret/injection/gating
concerns. The remaining `scripts/*.mjs` (sync-version, build-latest-json, build-remote-helpers,
dev-frontend, updater-archive-layout, and their `.test.mjs` peers) were not individually audited
in this pass and are not claimed clean.
