# Multi-host remote input latency baseline

The opt-in loopback client-transport soak completed end to end. Four simulated
hosts returned 4,011,582 measured echoes over 60,000.068208 ms. Aggregate
p50 was 0.051875 ms, p90 0.062458 ms, p95 0.103292 ms, p99 0.194167 ms,
and maximum 60.834625 ms. No latency threshold is asserted.

Scope: production WebSocketTerminalTransport input-to-application-echo over
independent simulated loopback gateways, not SSH/PTY/Rust gateway/rendering.
See README.md for method, assumptions, portability, existing attach-readiness
limitation, and why this approach won over extending the Rust PTY fixture.
Only new files in this remote test area were written; no shared helpers or
production code were changed, and no live Ferryx process was signaled.
Changes are committed on the `sa-tests` branch.

## Run from repository root

```sh
bun ./ui/src/remote/input-latency-soak/soak.mjs --hosts 4 --duration-ms 60000 --warmup 100
bun test ./ui/src/remote/input-latency-soak/soak.test.mjs
```

## Verbatim baseline stdout (exit 0)

```json
{
  "schemaVersion": 1,
  "measurement": "WebSocketTerminalTransport.write-to-application-echo",
  "topology": "independent loopback Bun echo gateways; no SSH, PTY, Rust gateway or renderer",
  "loadModel": "closed-loop; one outstanding 64-byte input per host; concurrent hosts",
  "percentileMethod": "nearest-rank",
  "startedAt": "2026-09-12T16:04:32.301Z",
  "requestedDurationMs": 60000,
  "durationMs": 60000.068208000004,
  "hostCount": 4,
  "warmupSamplesPerHost": 100,
  "livenessTimeoutMs": 5000,
  "environment": {
    "platform": "darwin",
    "arch": "arm64",
    "release": "25.6.0",
    "cpu": "Apple M4 Max",
    "bun": "1.4.0"
  },
  "latencyMs": {
    "sampleCount": 4011582,
    "min": 0.024292000001878478,
    "p50": 0.05187500000465661,
    "p90": 0.062458000000333413,
    "p95": 0.10329200000342098,
    "p99": 0.19416700000147102,
    "max": 60.83462499999996
  },
  "hosts": [
    {
      "hostId": "host-0",
      "latencyMs": {
        "sampleCount": 1002896,
        "min": 0.024292000001878478,
        "p50": 0.05170899999939138,
        "p90": 0.06229199999506818,
        "p95": 0.10200000000259024,
        "p99": 0.19050000000061118,
        "max": 42.319917000000714
      }
    },
    {
      "hostId": "host-1",
      "latencyMs": {
        "sampleCount": 1002896,
        "min": 0.02524999999877764,
        "p50": 0.05195799999637529,
        "p90": 0.06266600000162725,
        "p95": 0.10379199999806588,
        "p99": 0.1937499999985448,
        "max": 60.811041999999816
      }
    },
    {
      "hostId": "host-2",
      "latencyMs": {
        "sampleCount": 1002895,
        "min": 0.025084000000788365,
        "p50": 0.05195800000001327,
        "p90": 0.06258399999933317,
        "p95": 0.10366599999542814,
        "p99": 0.19529099999635946,
        "max": 60.83462499999996
      }
    },
    {
      "hostId": "host-3",
      "latencyMs": {
        "sampleCount": 1002895,
        "min": 0.025207999999111053,
        "p50": 0.05195800000365125,
        "p90": 0.06229200000234414,
        "p95": 0.10370800000055169,
        "p99": 0.19708300000093004,
        "max": 60.82250000000022
      }
    }
  ]
}
```

## Failing-first proof

RED was captured after writing the harness tests, before writing the runner.
No production change was needed. This is a missing-harness failure, not a
claim that a pre-existing latency regression was reproduced.

### RED (verbatim)

```text
bun test v1.4.0 (34cbb9a40)

ui/src/remote/input-latency-soak/soak.test.mjs:

# Unhandled error between tests
-------------------------------
error: Cannot find module './soak.mjs' from '/Users/indo/code/project/orca-lite-wt/sa-tests/ui/src/remote/input-latency-soak/soak.test.mjs'
-------------------------------


 0 pass
 1 fail
 1 error
Ran 1 test across 1 file. [9.00ms]
exit_code=1
```

### GREEN (verbatim; one post-implementation test run)

```text
bun test v1.4.0 (34cbb9a40)

ui/src/remote/input-latency-soak/soak.test.mjs:
(pass) remote input latency measurement > uses nearest-rank percentiles without changing the samples [1.30ms]
(pass) remote input latency measurement > measures every host through real sockets, excluding warmup [9.71ms]
(pass) remote input latency measurement > rejects invalid run configuration before creating hosts [0.13ms]

 3 pass
 0 fail
 18 expect() calls
Ran 3 tests across 1 file. [24.00ms]
exit_code=0
```

## Verification

Language-server diagnostics: no errors or warnings in either source file;
one informational conversion hint in soak.mjs (bounded may be async).
The runner was built with Bun and the built entry point executed with two hosts,
zero duration and one warmup echo each; bundle-smoke.json records two measured
echoes, one per host. Generated build output was removed after verification.

Build command:

```sh
bun build ./ui/src/remote/input-latency-soak/soak.mjs --target=bun --outfile=./ui/src/remote/input-latency-soak/soak.bundle.mjs
bun ./ui/src/remote/input-latency-soak/soak.bundle.mjs --hosts 2 --duration-ms 0 --warmup 1
```

Verbatim build output:

```text
Bundled 2 modules in 34ms

  soak.bundle.mjs  9.77 KB  (entry point)

```

Measurement caveat: this was a shared Apple M4 Max workstation with concurrent
coding work; the short bundle build and smoke run overlapped the soak start.
Windows/Linux execution and the full application build were not performed.

## Mutation proof (added by the supervising session)

The RED above is a missing-harness failure, as stated. The objective's substitute clause for
test-only lanes additionally asks for a mutation proof: force the exact regression, capture
the assertion failing, revert, capture green. That proof is recorded here.

Mutation applied to `soak.mjs` (the nearest-rank selector, off-by-one):

```diff
-  const rank = (percentile) => sorted[Math.ceil(percentile * sorted.length) - 1];
+  const rank = (percentile) => sorted[Math.floor(percentile * sorted.length)];
```

RED with the mutation present (exit 1) — the assertion fails and names the drift:

```
-   "p50": 5,
+   "p50": 6,
(fail) remote input latency measurement > uses nearest-rank percentiles without changing the samples [0.28ms]
(pass) remote input latency measurement > measures every host through real sockets, excluding warmup
(pass) remote input latency measurement > rejects invalid run configuration before creating hosts
 2 pass
 1 fail
```

GREEN after reverting (exit 0):

```
 3 pass
 0 fail
Ran 3 tests across 1 file. [8.00ms]
```

This establishes the percentile assertion is not vacuous. It pins exact values over a
shuffled ten-sample set (`[7,3,10,1,9,4,6,2,8,5]` → p50 5, p90 9, p95 10), asserts the input
array is not reordered in place, and asserts `summarize([])` throws. Only p50 moved under the
mutation, and the suite caught it.

The source file was restored from an in-memory copy and verified byte-identical; no
`git restore`/`git checkout --` was used.

## C4 isolation demonstrated at runtime (added by the supervising session)

The isolation claim was previously supported by reading the fixture code — `.env_clear()`
before re-adding only `PATH`/`FERRYX_*`/`HOME`/temp vars, a `tempfile` runtime dir, and
protocol-only shutdown. Reading proves intent; it does not prove what the process did. So the
live runtime directory was snapshotted around an actual run.

Before and after `cargo test --test daemon_duplicate_prevention`:

```
/tmp/rorca-501 (the LIVE daemon's runtime dir)
  srw-------  0     Sep 13 07:55:43  agent-state.sock
  srw-------  0     Sep 13 07:55:43  daemon.sock
  -rw-------  0     Sep 13 07:55:43  daemon.lock
  -rw-------  18    Sep 13 07:55:43  handover_routes.json
  ...
  daemon.sock  inode=2324915031  mtime=1789253743  size=0

byte-identical before vs after : true
daemon.sock inode / mtime      : unchanged
```

What the run itself reported:

```
isolated runtime=/var/folders/zh/.../T/fx-dup-ubOiPp/run  data=/var/folders/zh/.../T/fx-dup-ubOiPp/data
spawned isolated daemon pid=68140
spawned isolated daemon pid=68159
reaped isolated daemon pid=68159 status=exit status: 1     <- the duplicate, correctly refused
reaped isolated daemon pid=68140 status=exit status: 0
cleanup verified: children reaped and /var/folders/zh/.../T/fx-dup-ubOiPp removed
test second_daemon_refuses_already_locked_socket_directory ... ok   (exit 0, 2.25s)
```

Checks at the same moment:

| Check | Result |
|---|---|
| Live `/tmp/rorca-501` listing byte-identical | true |
| Live daemon PID / start time | 1010, Sun Sep 13 07:55:43 2026 — unchanged |
| `ferryx --daemon` processes alive afterwards | 1 (both spawned children retired) |
| Leftover `fx-dup-*` fixtures | 0 |
| Test output ever mentions `/tmp/rorca-501` | false |

The last row matters most: the child cannot reach the live endpoint because `env_clear()`
removes any variable that would point at it, and the captured output confirms the string never
appears. The duplicate daemon exits 1 by losing a lock race inside the fixture directory, not
by any signal — consistent with the source having zero `kill`/`SIGTERM`/`pkill` calls.

## Mutation proofs for the other two tests (added by the supervising session)

The mutation proof above covers the soak harness only. The objective's substitute clause
asks to force **the exact regression** for the test-only lane, so each of the three tests
needed its own. These two were missing and are recorded here.

### 2. `terminal_selection_survival_regression`

Forced the exact historical bug named in the test's own doc comment: `line_text_at`
re-installs its temporary line selection onto the terminal handle, clobbering the user's
active selection.

Mutation in `src-tauri/src/native_terminal/selection.rs::line_text_at`:

    + install_selection(handle, &selection)?;
      let mut ordered = GhosttySelection::default();

GREEN before: `test result: ok. 1 passed; 0 failed`
RED with mutation (exit 101):

    assertion `left == right` failed: active word selection must survive line inspection
      left: "first line has user-selected target text to copy"
     right: "target"

The left value is the whole row - precisely the clobbering this test exists to prevent.
Reverted; source restored byte-identical.

### 3. `daemon_duplicate_prevention`

Forced the exact regression: downgrade the exclusive advisory lock to a shared one, which
permits multiple holders, so a second daemon is no longer refused.

Mutation in `src-tauri/src/daemon/server.rs:576`:

    - libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
    + libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB)

GREEN before: `test result: ok. 1 passed; 0 failed` (2.08s)
RED with mutation (exit 101): `second_daemon_refuses_already_locked_socket_directory ... FAILED`
Reverted; source restored byte-identical.

Live-daemon safety across both runs: `/tmp/rorca-501/daemon.sock` mtime `1789253743`
identical before and after; test output never mentions `/tmp/rorca-501`; daemon PID 1010
(started Sun Sep 13 07:55:43 2026) unchanged.
