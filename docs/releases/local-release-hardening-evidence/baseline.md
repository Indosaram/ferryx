# Release hardening baseline

Captured before implementation changes on 2026-09-08.

```sh
node --test scripts/sync-version.test.mjs scripts/build-latest-json.test.mjs scripts/release-workflow.test.mjs scripts/updater-archive-layout.test.mjs
```

Monitor: `mon_FH2CN3S1VYVZQZ3A`, terminal `bash_9`.

```text
status: exited_1 exit_code: 1
tests 27
suites 0
pass 26
fail 1
cancelled 0
skipped 0
todo 0
duration_ms 692.859458

signatures and updater bundles are collected as release artifacts
AssertionError [ERR_ASSERTION]:
The input did not match the regular expression /-name "\*-setup\.exe"/.
```

The failed expectation describes a narrower installer collection rule than the
existing hosted release workflow's `*.exe` glob. The implementation retires that
producer rather than weakening the installer requirement. Replacement policy
tests must reject hosted release production, while receipt assembly must prove
precise artifact selection independently.

This is the baseline, not the failing-first proof for every new feature. Each
implementation lane records its own regression-specific RED and GREEN.

Cleanup: the test process exited. Existing filesystem fixture tests clean their
temporary directories in `finally`; no application, server, build or remote
process was started by this command.
