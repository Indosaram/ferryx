# Remote input-to-echo latency soak

This is an opt-in measurement, not a CI performance gate. It exercises the existing
`WebSocketTerminalTransport` over real loopback HTTP/WebSocket connections to four
independent simulated remote hosts by default. Each host has its own ephemeral
listener, bearer token, single-use target-bound ticket and output identity.
No running Ferryx daemon, user session, SSH configuration or shared helper is touched.

## Run

From the repository root, with Bun installed (baseline: Bun 1.4.0):

```sh
bun ./ui/src/remote/input-latency-soak/soak.mjs --hosts 4 --duration-ms 60000 --warmup 100
bun test ./ui/src/remote/input-latency-soak/soak.test.mjs
```

The runner writes exactly one JSON object to stdout on success. Redirect stdout to
an explicitly chosen file to retain another measurement; do not overwrite the
committed baseline when comparing runs. Failures go to stderr and return nonzero.
The implementation uses portable Bun and Node APIs on macOS, Windows and Linux;
only macOS was executed here. There are no platform-specific shell or PTY commands,
extra dependencies, package changes, production changes, or shared-helper changes.

Options (all optional):

| Option | Default | Meaning |
| --- | ---: | --- |
| `--hosts` | 4 | Positive integer simulated host count |
| `--duration-ms` | 60000 | Nonnegative sustained measurement duration |
| `--warmup` | 100 | Nonnegative completed warmup echoes per host, excluded |
| `--timeout-ms` | 5000 | Positive per-operation liveness timeout, not a percentile gate |

Zero duration performs exactly one measured echo per host for deterministic
functional testing. Do not interpret that smoke run as a latency baseline.

## Measurement boundary and load

Start: immediately before the production transport's `write(sessionId, payload)`.
End: its `onOutput` callback receives the complete host-tagged application echo.
This includes production input encoding, WebSocket send/receive, loopback kernel
transport, the simulated host handler, and production output listener dispatch.
Ticket exchange, socket readiness and warmup finish before the measurement starts.
The application echo is prefixed by the server, so merely observing the outgoing
input cannot count as a sample. Every input includes host identity and sequence;
the same session ID is deliberately reused on all hosts to expose cross-host routing.

Hosts run concurrently with one outstanding 64-byte ASCII input per host, sending
the next input immediately after the matching echo. There are no sleeps, polling
loops or artificial network delays. Each output subscription is installed before
attach/write. A host-specific READY output proves socket readiness before input.
Each pending operation has a bounded timeout, and unexpected output fails the run
instead of being silently dropped or included as a latency sample. All created
listeners and sockets are cleaned up in `finally`.

This is **not** WAN latency, SSH encryption/helper/PTY latency, Rust gateway latency,
keyboard-to-pixel latency, or a browser/React rendering benchmark. The simulated
hosts share one Bun process, and production `RemoteTerminal` rendering is not used.
It is a closed-loop saturation baseline: it does not model human typing cadence,
network impairment, open-loop arrivals, or coordinated-omission correction.

The nearest-rank percentiles use unrounded milliseconds. `latencyMs` is the
sample-weighted aggregate, not an average of per-host percentiles; each host also
has its own distribution and count. `durationMs` includes the last outstanding
round trips but excludes setup, warmup, sorting, JSON serialization and cleanup.
Samples are retained in memory for exact percentiles, so memory grows with run
length and host throughput. Keep long exploratory runs bounded accordingly.

## Recorded baseline

`baseline.json` is verbatim stdout from the 60-second four-host run on Apple M4 Max,
Darwin 25.6.0 arm64, Bun 1.4.0, started 2026-09-12T16:04:32.301Z.

- Samples: **4,011,582**; duration: **60,000.068208 ms**.
- Aggregate p50: **0.051875 ms**, p90: **0.062458 ms**, p95: **0.103292 ms**,
  p99: **0.194167 ms**, max: **60.834625 ms**.
- 100 warmup echoes per host were excluded.
- This was a shared workstation, not an isolated performance machine. A small
  bundle build and two-host smoke run overlapped the beginning of the soak;
  sibling coding tasks were also active. Compare topology/runtime/load and
  workstation contention before attributing differences to regressions.

`REPORT.md` includes full verbatim baseline stdout and RED/GREEN captures.
`red.log` records the new test failing before the harness existed; `green.log`
records its single successful post-implementation run (3 tests, 18 assertions).
No production behavior was changed, so RED/GREEN proves the harness contract,
not a production latency defect or a performance improvement. Tests assert
machine-consumed statistics, sample counts, configuration validation and actual
multi-host transport completion, never prose or wall-clock percentile thresholds.
`build.log` records the independent Bun bundle build, and `bundle-smoke.json`
records execution of that built entry point. The generated bundle is not retained.

## Design choice and existing limitation

Two approaches were considered: reuse the Rust gateway/PTY socket test fixture,
or reuse the UI transport and ticket-authenticated fixture pattern. The second
won because it supplies portable simulated hosts using real sockets, needs no
shared test-module edits, and cannot interfere with live PTY/daemon ownership.
The existing direct-transport security tests establish the ticket pattern, and
remote gateway socket tests establish ephemeral listeners and bounded receives.

An existing out-of-scope limitation: `WebSocketTerminalTransport.attach()` resolves
before WebSocket open and `write()` silently returns while the socket is not OPEN
(`ui/src/lib/terminalTransport/remoteTransport.ts`). The soak explicitly awaits
READY output instead of treating attach completion as readiness. This behavior
was not changed. No shared helpers or other tests were modified.
