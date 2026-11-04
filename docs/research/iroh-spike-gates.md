# iroh direct-attach spike: frozen gates

Frozen: 2026-11-04, before any spike code was written (todo 26 of
`.omo/plans/rex-adoption-20261104.md`). These gates do not change after
measurement starts. The result doc (`iroh-spike-result.md`) reports one
measured number and one GO/NO-GO per gate. The feature ships only if all four
gates pass AND the owner approves the production relay change in-session.

Scenario: remote terminal attach desktop <-> desktop, two hosts picked by
`scripts/pick-build-host.sh` (one Linux, one Windows), same pair for both
transports.

| Gate | Statement | Measurement | Pass condition |
| --- | --- | --- | --- |
| (a) latency | p95 terminal round-trip latency improves by at least 30% vs the current relay hop on the same two hosts | `scripts/qa/iroh-rtt.mjs`: 1,000 keystroke echo round trips over each transport, p95 in ms | `irohP95Ms <= 0.70 * relayP95Ms` |
| (b) size | Release binary size increase is below 30 MB | size of `ferryx-cli` built with `--release --no-default-features` with and without `--features iroh-direct` | `sizeDeltaMb < 30` |
| (c) PIN pairing | PIN pairing keeps working unchanged alongside | existing PIN pairing tests pass unchanged with `--features iroh-direct` enabled; no pairing source file is edited | all pass, zero edits to pairing code |
| (d) no new service | No new account or third-party service is required | runtime assertion test: endpoint has zero address-lookup services and only the self-hosted relay URL; packet capture during UDP-blocked fallback shows no `*.iroh.network` / n0 hosts | test green and capture has zero n0 hits |

Fixed constraints during the spike:

- Cargo feature `iroh-direct`, off by default, never in default, headless or
  release builds.
- iroh 1.4: `Endpoint::builder(presets::Minimal)` with `RelayMode::Custom`
  holding only the self-hosted relay (`https://relay.ferryx.dev/relay` in the
  product config; an isolated test hostname for the spike). No n0 relays, no
  pkarr, no DNS address lookup.
- iroh-relay is built from a pinned iroh clone at the 1.4 tag on the test host
  (`cargo build --profile optimized-release -p iroh-relay --features server`)
  and runs on an isolated test host only. Production relay changes need
  explicit owner approval and are out of scope.
- License must be MIT or Apache-2.0 for every new crate (recorded, not a gate).
- Memory (RSS of both peers during the RTT run) is recorded, not a gate.
