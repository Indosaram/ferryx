# Lead process-output redaction verification

Date: 2026-09-08. The lead completed this increment after the QA-only lane
returned without a production implementation.

## Failing-first evidence

Invalid newline escaping in the interrupted test fixtures was corrected before
accepting any RED result. Syntax errors were not regression evidence.

`node --test scripts/release-hosts.test.mjs`, monitor
`mon_HA9YN59VSF3NAVNE` / `bash_28`, exited 1:

- 33 tests, 30 passed, 3 failed, no skips.
- The failures reported raw **synthetic** secret leakage on successful exit,
  nonzero exit, and inherited-environment execution.
- Child programs used fake keys/tokens and wrote values across stream chunks.
  No real credential value was used as test data.

`node --test --test-name-pattern='reports non-EPIPE' scripts/release-hosts.test.mjs`,
monitor `mon_MKM59J6KZR05G5Q7` / `bash_29`, exited 1 with
`Missing expected rejection`. A real child received an injected EIO input
stream error, which the old implementation silently ignored.

## Implementation and GREEN

The module exports `redactProcessOutput(text, effectiveEnv)` and snapshots the
actual environment used for each child. Nonempty private-key, password, secret,
token, and Apple API-key values are masked before stdout, stderr, command labels
or errors are exposed. Both raw and JSON-escaped forms are covered. Output bytes
are joined before decoding/redaction, preserving stream-boundary correctness.
An explicitly supplied empty environment does not inherit redaction values.

Non-EPIPE input errors terminate the owned child and reject only after close.
The test confirms the child has exited before the error is delivered.

`node --test scripts/release-hosts.test.mjs`, monitor
`mon_0Y9FTWC40TJZRZSM` / `bash_30`, exited 0:

```text
tests 34
pass 34
fail 0
cancelled 0
skipped 0
todo 0
duration_ms 3539.911583
```

These tests execute real Node children for output, error, input and timeout
behavior. They do not prove remote release builds or credential usability.

## Operational constraint

Tauri signer help can display inherited signing environment defaults. Run such
diagnostics with signing variables removed from the child environment; do not
rotate keys or mutate the user's global environment. The file to sign is
positional: `cargo tauri signer sign <FILE>`; `-f` denotes a private-key path.

## Cleanup

All child processes and test fixtures completed or were removed by test cleanup.
The non-EPIPE regression explicitly proves child termination. No real credential,
installed app, daemon, remote filesystem or global configuration was changed.
