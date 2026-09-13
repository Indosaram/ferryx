# Parent aggregate verification: UI wave (A17, A18, A19, A22)

Single aggregate check after the four parallel UI packets landed, run by the
parent, not by the children.

## Result

```
NODE=v22.22.3
Test Files  1 failed | 220 passed (221)
     Tests  3 failed | 4470 passed (4473)
✓ built in 2.60s
AGG_BUILD_EXIT=0
```

UI build: **exit 0**. Suite: **4470 of 4473 passing**.

## The 3 failures are pre-existing and unrelated

All three live in `ui/src/features/ferryx/push/client.test.ts`:

- `accepts only same origin exact task links` - expected the task URL, got `null`
- `denied permission never subscribes` - expected `denied`, got `enabled`
- `server unsubscribe precedes local removal ...` - resolved `'disabled'` instead of rejecting

Chain of proof, in the order the parent established it:

1. `git diff --stat HEAD -- ui/src/features/` is **empty**. No packet touched the
   push feature.
2. `client.test.ts` imports only `vitest` and `./client`, so no changed UI module
   is reachable from it.
3. The one global that *could* have leaked was real and was checked, not assumed:
   `ui/src/test/setup.ts` IS modified and applies to every test file. Its whole
   diff is wrapping the `Element.prototype.matches` patch in
   `if (typeof Element !== "undefined")`, which only guards non-DOM environments.
4. Decisive: the same file was run in the **canonical tree**
   `/Users/indo/code/project/orca-lite`, which contains none of this work:
   `Test Files 1 failed (1) / Tests 3 failed (3)`. Identical failure.

Conclusion: pre-existing breakage in the push notification permission/origin
logic. Out of scope for this plan, not introduced by this wave, and deliberately
not "fixed" here.

## Runner trap worth recording

An earlier parent run of the same tests reported
`TypeError: storage.getItem is not a function` and looked like an A17 regression.
It was not. The isolation harness
`docs/evidence/paired-daemon/A12-session-metadata-run.sh` starts its command with
`env -i PATH=/Users/indo/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin ...`, which
discards any PATH the caller exports, so `node` resolved to
`/opt/homebrew/bin/node` = **v25.8.0**. Node 22 lives at
`/Users/indo/.local/bin/node` (v22.22.3). That storage error is the known Node 25
runner artifact.

Rule: that harness is for Rust/daemon isolation only. UI suites must be run
without it, exporting
`PATH=/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin`.

## What this does and does not mean

It means the four UI packets integrate without regressing the suite or the build.
It does **not** mean AC01/AC03/AC07/AC11 are accepted. Each child reported its own
gaps, which remain open:

- A17: paired descriptor persistence, exact-target reattach, remote restart/expiry,
  legacy-owner handover, real restart exercises, Rust corrupt-row quarantine.
- A19: AC03/AC11 not fully proven - SSH connection-status display, a distinct
  refresh-error stale indicator, native event delivery, desktop layout/session QA.
- A22: settings-to-Add-Project navigation left disabled pending A18 integration;
  real layout rollback, Linux pairing, native desktop QA unproven.
- A18: human desktop QA unverified.
