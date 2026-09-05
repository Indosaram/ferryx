# Isolated scope QA driver execution receipt

2026-09-05, child hephaestus / st_01a070f6. Executable harness delivered;
product scenario acceptance remains BLOCKED. This is not sections 5-8 completion.

## Lead follow-up: executable API boundary and test-running self-test

The first revision did not constitute a complete scenario harness. This revision
adds `ferryx-scope-api.mjs` and `ferryx-scope-boundary.test.mjs`; it still does **not**
claim full scenario automation. The historical receipts below remain historical.

`--self-test` now actually spawns Bun's test runner for the three boundary tests,
records literal argv/stdout/stderr/exit code, and fails if that child fails. Its
real execution on this revision collected 3 passing tests / 11 assertions. There
is no recursive self-test invocation and no presupplied test report acceptance.

New RED: `bun test --no-preload ./scripts/qa/ferryx-scope-qa.test.mjs` produced
6 pass / 1 fail: expected `commands.some(argv includes test && exitCode === 0)`
true, received false. New API wire tests then caught an actual missing auth header
(expected `Bearer fixture-token-123456789`, received null); Bun stdin writes were
corrected to write before end. No tests were skipped or weakened.

Final GREEN command:

```sh
bun test --no-preload ./scripts/qa/ferryx-scope-qa.test.mjs ./scripts/qa/ferryx-scope-boundary.test.mjs
```

Result: 10 pass / 0 fail / 43 assertions / 381ms. `node --check` on all scripts and
the same scoped tsc command below exited 0. LSP remains unavailable.

Explicit QA fixture manifest input may now set:

```json
{"integration":{"base":"http://127.0.0.1:EPHEMERAL_PORT/"}}
```

Replace EPHEMERAL_PORT with the actual isolated router port. This is a binding,
not evidence. Ports 5173/43821, credentials in URLs, redirects, proxy use, foreign
hosts and paths are rejected. Before sending a token the driver actually requests
`/api/v1/health` and requires HTTP 200 plus `X-Ferryx-QA-Fixture: <manifest.id>`.
The integration fixture router must emit that header; the existing product router
does not yet do so. An absent/wrong header stops after one unauthenticated request.
This is a namespace guard on an explicitly trusted loopback fixture, not protection
against a malicious local process that can read the same-user private manifest.

After identity verification it reads only private `$QA_ROOT/token`, sends bearer
authentication through curl stdin (never argv), and actually GETs sessions,
workspace/state, agents and hosts. Push additionally GETs push/vapid-public-key.
Every response records URL/status/redacted headers and body size; API bodies are
omitted to prevent personal transcript leakage. Full fixture-page bodies remain
recorded. Missing routes return `QA_API_ROUTE_UNAVAILABLE`, not a synthesized pass.
The wire fixture proved five real requests, no token on health, auth on subsequent
requests, a real 404, redaction, and identity mismatch stopping before token read.

Self-test cleanup on this revision: two listeners closed; root
`/private/tmp/ferryx-qa.OKZmaY`, id `e37ef03e-1b6e-4311-8d34-3eb9e9c6d6f6`, removed;
no foreign PIDs signalled and lead SSH untouched. HTTP fixture used ports 52132/52131.

Precisely still unimplemented: browser selector actions/screenshots/native pixel
assertions; authenticated mutation lifecycle and retained-event subscription;
external product CLI invocation; SSH bridge sever/reconnect; provider callbacks
and history resume; mobile/physical device interactions and push delivery. No
private Bun.WebView profile API was verified (the attempted public documentation
endpoint returned HTTP 404); constructing it without isolation is not authorized.
Direct product binary launches remain prohibited by the assignment. The read-only
API binding is executable now, but no production fixture binding was supplied or
used. Existing sessions/workspace routes were source-verified; agents/hosts are
planned routes and may correctly fail. Imported reports or device JSON can never
turn any scenario green: final product acceptance still fails closed.

## Delivered paths

- `scripts/qa/ferryx-scope-qa.mjs`: executable Bun entry; strict CLI, JSON report,
  real curl invocation, response headers/body/hash, action log and cleanup receipts.
- `scripts/qa/ferryx-scope-fixtures.mjs`: private root/manifest validation, seeded
  fixture directories, explicit cleanup, fail-closed scenario acceptance gates.
- `scripts/qa/ferryx-scope-page.mjs`: two ephemeral loopback HTTP listeners serving
  element/canvas/WebGL/image/cross-origin iframe/click-count fixtures.
- `scripts/qa/ferryx-scope-qa.test.mjs`: behavioral guards and real subprocess/HTTP
  integration tests. These are script tests, not UI Vitest tests.
- This report. No production/configuration/manifest files modified.

## Executable usage

Run from the repository root:

```sh
bun scripts/qa/ferryx-scope-qa.mjs --self-test
bun scripts/qa/ferryx-scope-qa.mjs --prepare --scenario foundation
```

`--prepare-only` aliases `--prepare`. Preparation reports the canonical private
root, `manifest.json`, `env.json`, and literal cleanup command. It retains files
explicitly, starts no background process, and does not export or alter the parent
environment. Two repositories contain distinct sentinel files but are intentionally
**not Git initialized/committed**: the task prohibits staging/commits. An integrator
with fixture-only commit authorization must add that step and record the actual
Git invocations. Neither provider stores nor credentials are copied.

Supply the exact canonical root printed by prepare (`/private/tmp/ferryx-qa.*` on
this machine), not a personal directory or symlink alias:

```sh
bun scripts/qa/ferryx-scope-qa.mjs --scenario foundation --root "$QA_ROOT" --evidence-dir docs/evidence/ferryx-scope/foundation
bun scripts/qa/ferryx-scope-qa.mjs --cleanup --root "$QA_ROOT"
```

Choose `design`, `mobile`, or `push` literally for other scenarios. Roots must be
owned, private, direct children of canonical `/tmp`, with this harness's manifest
and non-symlink private child directories. No caller-supplied process IDs, tokens,
URLs, commands or cleanup hooks are adopted. Reports use exclusive UUID filenames
and never overwrite previous evidence. An explicit root remains caller-owned;
scenario-created listeners close in `finally`, and self-test-created roots are
removed there. Abrupt SIGKILL cannot produce a receipt; use the printed explicit
cleanup command for a retained preparation. No signal handlers target foreign PIDs.

Exit 0 means only `prepared`, `cleaned`, or `self-test-passed`; the latter explicitly
sets `productVerified:false`. All product scenarios currently exit 2 (`blocked`),
with named missing observables. Supplying `{status:"passed"}` or screenshots cannot
override that gate. Push additionally requires physical background-delivery proof.

## Exact RED / GREEN evidence

The initial baseline exported callable root/evidence validators that incorrectly
accepted inputs. Tests collected and failed on behavior, not missing imports:

```text
bun test --no-preload ./scripts/qa/ferryx-scope-qa.test.mjs
rejects an unsafe root before accessing resources:
  Expected promise that rejects; Received promise that resolved
rejects claimed success without actual observations:
  Received function did not throw; Received value: { status: "passed" }
rejects push evidence without a physical-device receipt:
  Received function did not throw; Received value: { status: "passed" }
0 pass / 3 fail / 3 expect() calls / exit 1
```

An earlier identical test target without `--no-preload` hit the tool's 20-second
timeout after the Bun version banner; that run is not RED evidence. A later patch
attempt failed on a nonunique match and made no edits; it was corrected against a
fresh read, not silently treated as implemented.

After implementation the original three tests passed. Final expanded suite:

```text
bun test --no-preload ./scripts/qa/ferryx-scope-qa.test.mjs
7 pass / 0 fail / 31 expect() calls / 339.00ms / exit 0
```

Coverage includes unsafe personal roots, symlink children, false scenario success,
missing physical push evidence, real self-test subprocess/HTTP/cleanup, all four
scenario nonzero gates, and prepare-to-cleanup execution. No fixed sleeps or polling.

```sh
for file in scripts/qa/ferryx-scope-*.mjs; do node --check "$file" || exit; done
ui/node_modules/.bin/tsc --allowJs --noEmit --target esnext --module esnext --moduleResolution bundler --skipLibCheck scripts/qa/ferryx-scope-*.mjs
```

Both exited 0 without output. These are JavaScript parse/module checks, **not strict
checkJs type coverage**. LSP is unavailable by parent-established tooling failure;
no retry. No Rust changes or Cargo invocation, no duplicate target cache/build,
and no UI build/dist race introduced. Full product build gates remain integrator-owned.

## Actual self-test and cleanup receipt

`bun scripts/qa/ferryx-scope-qa.mjs --self-test` exited 0 on Bun 1.4.0,
Darwin 25.6.0 arm64. It actually invoked:

```text
curl --fail-with-body --silent --show-error --max-time 5 --dump-header - http://127.0.0.1:63152
exitCode: 0; stderr: ""
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Cache-Control: no-store
```

The JSON stdout included literal full response body and headers, body SHA-256
`12186dce0304825f2ef47a89ed882fe79437645f29879b80756cd1ebd5586c48`,
action `fixture.listen`, and secondary iframe origin `http://127.0.0.1:63151`.
This is a fixture HTTP result, never product gateway evidence.

```json
{
  "loopbackServersClosed": 2,
  "root": "/private/tmp/ferryx-qa.j64GEW",
  "id": "701a0232-ada7-49f7-b1d1-87091e6803ea",
  "removed": true,
  "processesSignalled": [],
  "leadSshTouched": false
}
```

The subsequent 7-test run also exercised cleanup of each root/listener it created.
No desktop/default daemon, personal PTY, provider, SSH bridge, Chrome profile or
global config was touched. Lead SSH root `/tmp/ferryx-scope-ssh.Tody5Z` is untouched.

## Integration gates still to wire (not implemented or verified here)

1. Production fixture adapter: real router via `new_with_paths_backend`, private
   token store, authenticated ephemeral BASE, two real PTYs/two host owners,
   explicit readiness events and exact-target receipts. No manifest assertion can
   replace this adapter. Hook actual product observations into `validateEvidence`
   only with behavioral RED and real surface tests; it intentionally always blocks now.
2. Actual `/api/v1/health`, agents create/list/start/prompt/messages/wait/stop/
   decide/answer, hosts/workspaces and event subscriptions. External `scope` CLI
   execution is deliberately absent under the direct-product-launch prohibition.
   No fabricated product binary grammar is run. Authentication/error status/header/
   body redaction must precede recording real gateway responses.
3. Desktop computer-use bound to an already-proven isolated QA PID/window. Wire
   waiting-inbox/waiting-target, run-on/ssh-host/ssh-test/remote-reconnect,
   history-query/result/resume and design-toggle/area/preview/note/target/send.
   Retain focus-independent target, bridge-only disconnect/PID/epoch/replay and
   exact provider-authored resume evidence. The deterministic page alone proves none.
4. Bun.WebView is the default and is available (`typeof === "function"`), but
   private data-path isolation has not been proven. No browser was constructed.
   `screenshots:[]` is honest missing evidence, not a generated substitute. Wire
   screenshot bytes/hash, DOM/CSS bounds, native overlay-free crop and actual action
   logs once an isolated browser/native capture boundary exists. Do not claim HTML
   paint or a browser screenshot certifies real native-child capture.
5. Mobile host/workspace/task-create/agent-start/agent-stop/chat-mode/composer/send/
   attachment/approval-accept/decline/question-submit selectors, actual callbacks,
   IME/drafts/files, two-client request identity and ownership handoff. Must run with
   desktop closed on a physical phone, not merely a resized browser.
6. Push API vapid-public-key/subscribe/unsubscribe, trusted same-origin HTTPS,
   real Chrome via Playwright for push/browser traces (no dependency installation),
   and operator-attested model/OS/browser physical device. Require actual locked/
   background notification/time/event receipt for waiting and provider-confirmed
   completion, notification tap exact target, deny/unsubscribe/revoke/Gone/dedupe.
   No phone/HTTPS/provider callback was verified. Both iOS and Android remain gates.
7. Native capture/helper packaging on all three OSes, strict compiler/product
   builds using the lead's existing Cargo cache/queue, and theme/viewport/mask/DnD/
   reconnect regression matrix are not replaced by these script checks.

Review: modules have separate fixture, page, driver and test responsibilities;
external root/CLI inputs are guarded at the boundary; no error suppression or
claimed product passes. Source edits were apply_patch-only. Foreign dirty work was
preserved. Work remains uncommitted and can be affected by concurrent sessions.

## Lead correction: actual PTY runner verification

The producer Bun test command reproduced EBADF posix_spawn in the lead monitor environment, including a standalone child_process.spawnSync minimal test. stdin ignore/pipe and --no-preload did not resolve the Bun test-runner defect. No assertions were removed or skipped.

Moved the same 18 tests to existing Vitest under Node, and made child execution use node:child_process with stream collection. The executable remains runnable with Bun. The self-test now invokes the project Vitest boundary suite and asserts its actual 3-test pass output.

Lead command: `node ui/node_modules/vitest/vitest.mjs run --config scripts/qa/ferryx-scope-vitest.config.mjs && bun scripts/qa/ferryx-scope-qa.mjs --self-test --evidence-dir docs/evidence/ferryx-scope/harness-lead`. Monitor mon_A5W7R7NRCJ398HTB / bash_92 exited 0: 4 files, 18 tests passed; actual self-test HTTP curl and nested 3 tests exited 0. Fixture listeners closed and owned temporary root removed. LSP scripts/qa diagnostics: zero errors. Product scenario gates remain incomplete, not certified by this harness self-test.
