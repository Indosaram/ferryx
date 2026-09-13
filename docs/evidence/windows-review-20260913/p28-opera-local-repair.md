# P28 Opera device suggestion: local repair receipt

2026-09-13; child task `st_01a098f0`. BOUNDED-UI-02 / P28 device suggestion
extension. Local source regression repaired; **actual Windows Opera runtime
acceptance remains pending**. This is macOS arm64 Vitest/jsdom evidence,
not an owned Windows browser or live server acceptance receipt.

## Ownership and minimal repair

- Start-of-task Git survey showed `ui/src/remote/deviceIdentity.ts` clean;
  the new test and this report were absent. Foreign changes remained read-only.
- Read root/UI instructions, TypeScript reference, the P28 addendum, parser,
  real PairingPage, storage helper, RemoteApp QR caller, existing pairing test,
  runner configuration and setup. The existing pairing validator was not edited.
- Only these three allocated paths were written, using `apply_patch`.
- Moved the existing Opera regex branch before generic Chrome/Firefox/Safari.
  Edge and iOS-specific precedence remain unchanged. No regex, authentication,
  caller, storage or network behavior was otherwise changed.
- Both PIN pairing (initial state and blank-name fallback) and QR pairing consume
  the shared parser. The regression executes the real PIN pairing caller; QR
  behavior was traced in source, not separately executed.

## Registered RED/GREEN

The task identifies this exact invocation as C002-registered; no command
expansion or new registration was performed by this child:

```sh
bun run --cwd ui test src/remote/deviceIdentity.opera.test.ts
```

Runner: `vitest run --maxWorkers=1`, Vitest 3.2.7, jsdom.
Final test source was unchanged between the following RED and GREEN runs.

| Phase | Local start | Exit | Result | Duration |
| --- | --- | --- | --- | --- |
| Intended RED, before production edit | 13:06:51 | 1 | 1 file failed; 4 failed, 4 passed (8 tests) | 770 ms |
| GREEN, after branch move | 13:07:17 | 0 | 1 file passed; 8 passed (8 tests) | 585 ms |

Captured intended RED output:

```text
identifies 'Opera' when its Windows UA contains compatibility tokens
expected 'Windows - Chrome' to be 'Windows - Opera'

submits 'Windows - Opera' when 'Opera' pairing uses the 'unedited' name
Expected the element to have value: Windows - Opera
Received: Windows - Chrome
Expected request body:
{"code":"654321","deviceName":"Windows - Opera","installationId":"p28-owned-installation"}
Received request body:
{"code":"654321","deviceName":"Windows - Chrome","installationId":"p28-owned-installation"}
Number of calls: 1
```

Blank fallback produced the same wrong submitted field. The edited-name case
failed only its independent initial-display assertion: the actual custom name
was preserved. Edge/Chrome parser and real pairing controls passed. Soft initial
display assertions deliberately allow the real request and callback chain to
finish even during RED; submitted-field assertions are not bypassed.

Earlier development runs at 13:05:55 and 13:06:26 both exited 1 with 3 failed /
5 passed, proving parser and submitted-field failures. A nonunique patch was
rejected between them with no file change. Before the final RED, the fixture
was strengthened with independent initial-display expectations and explicit
signal deadlines; the final RED above is the comparison baseline.

## Regression surface and cleanup

- Three explicit Windows UA parser cases: OPR+Chrome+Safari, Edge+Chrome+Safari,
  and Chrome+Safari. Expected identities do not derive from parser output.
- Five real mounted PairingPage form scenarios: those three unedited names,
  a distinct edited Opera name, and whitespace fallback.
- Only fetch is mocked at the network boundary; navigator UA is an explicit
  platform fixture. Real React state, form submit, installation-ID lookup,
  JSON request construction, Response parsing and onPaired dispatch execute.
- Request and onPaired signals are subscribed before submit; response release
  is explicitly controlled. Both signals are awaited inside act with 1000 ms
  failure deadlines, cleared in finally. There are no sleeps or polling.
- Before release, callback count is zero and submission remains disabled.
  After release, exact single callback token/metadata and exact single POST
  URL, method, headers, code, deviceName and installationId are asserted.
- Every pairing scenario uses finally to unmount its owned DOM, restore fetch
  and navigator mocks, and restore the previous canonical installation-ID key
  (or remove only that key if originally absent). Other storage is untouched;
  seeding the canonical key prevents legacy migration or random generation.
- All final GREEN scenarios completed through their exact callback and cleanup
  paths. No live fetch, IPC, GUI, server, daemon, SSH, installation, branch,
  worktree, commit, push, release or build was performed. Broader validation is
  left to the lead's combined verification rather than expanding this command.

## Review and diagnostics

- LSP: no diagnostics for either changed TypeScript file after the fix.
- Scoped `git diff --check`: clean; production diff reviewed and final test
  source reread. Production change is two lines moved, not new behavior layers.
- Pure LOC: parser 49, new regression 89; both below 200. Report is prose.
- Single responsibility: device suggestion / device suggestion regression.
- Boundary purity: existing UA input classification remains at its boundary;
  test uses typed fetch and a real serialized request/Response, not domain mocks.
- Variant discrimination: regex precedence, not tagged-union discrimination.
- No new type assertions, any, non-null assertions, suppression or defensive
  production checks. Test Promise resolver initialization is synchronous;
  its initial throwing function avoids an untyped/non-null escape hatch.
- Helpers are reused (three deferred signals, two bounded signal waits), have
  at most one parameter, and introduce no production abstraction. No negative
  naming, parameter bloat, redundant destructive verification or new logging.
- The RED proves the behavior is regression-locked before the production edit.
  Uncommitted work remains in a shared moving tree and can be affected by other
  sessions. Only the allocated parser/test/report are owned by this child.

## Outstanding acceptance

Run owned actual Windows Opera pairing to observe the correct unedited name
displayed and submitted, then a distinct edited name retained. This local
receipt does not close C001/C002/C003 or certify live Windows runtime behavior.
