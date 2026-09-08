# Existing release tooling test evidence

Captured on 2026-09-08 against the existing working-tree scripts. No production or test edits preceded this run.

## Invocation

```sh
node --test scripts/sync-version.test.mjs scripts/build-latest-json.test.mjs scripts/release-workflow.test.mjs scripts/updater-archive-layout.test.mjs
```

Monitor: `mon_QYGQ9SKVKB5BG2N3`, terminal session `bash_1`.

## Captured result

```text
status: exited_1 exit_code: 1
tests 27
suites 0
pass 26
fail 1
cancelled 0
skipped 0
todo 0
duration_ms 465.312833

test at scripts/release-workflow.test.mjs:56:1
signatures and updater bundles are collected as release artifacts
AssertionError [ERR_ASSERTION]: The input did not match the regular expression /-name "\*-setup\.exe"/.
```

The workflow uses `-name "*.exe"` in its collection step, while the test demands the narrower installer pattern. This is an existing workflow/test mismatch, not a change-induced failure. The suite does not establish that local builds or downloaded installers work.

The other 26 tests passed, including version mapping/idempotence, malformed-tag handling, updater-platform selection, missing-signature warning behavior, and archive root/AppleDouble checks.

## Interpretation for hardening

- The missing-signature test deliberately expects exit 0 and a reduced platform set when another signed artifact exists. A green suite therefore does not establish a complete required release matrix.
- Manifest fixtures use opaque signature strings. These prove string transport, not cryptographic validity.
- Workflow tests currently require hosted release triggers and credentials. A local-only policy needs replacement behavioral policy coverage, not merely a green run of these existing expectations.
- This assessment adds no behavior and has no RED-to-GREEN production increment. The failure remains visible for the proposed implementation.

## Cleanup receipt

The monitor reports process exit 1. Test fixture creation is paired with `finally`/`rmSync` cleanup in the version, manifest, and archive test files. The workflow test only reads files. No server, browser, daemon, application, or remote build was started by this test command.
