# iroh direct-attach spike: result

Todo 26 of `.omo/plans/rex-adoption-20261104.md`. The gates were frozen beforehand in
`iroh-spike-gates.md` and were not changed after measurement. Evidence:
`.omo/evidence/rex-adoption-20261104/task-26-*` (GitHub Actions runs 41797844187,
41804593311 and 41818311262 on ubuntu-24.04).

## What was built

- Cargo feature `iroh-direct` (off by default). `cargo tree -e normal` on a default build shows
  0 iroh crates. A default-feature `ferryx-cli` answers `iroh-probe` with "not a command".
- `src-tauri/src/remote/iroh_direct.rs`:
  - The endpoint is built with `Endpoint::builder(presets::Minimal)`,
    `RelayMode::Custom` holding only the self-hosted relay URL, `clear_address_lookup()`, and a
    DNS resolver with `FallbackMode::Never`, so it never falls back to public resolvers.
  - n0 relay, pkarr and DNS host names are refused at parse time.
  - On the iroh stream it runs the existing Noise attach handshake (`attach_crypto` /
    `establish_session`), so device authorization is the same as on every other attach path.
- Runtime assertion test `a_bound_endpoint_has_no_address_lookup_and_only_the_self_hosted_relay`.
  It checks that the endpoint has 0 address-lookup services, holds the self-hosted relay, and
  holds none of n0's production or staging relays. It was mutation-checked twice and went red both
  times (run 41804593311):
  - with `RelayMode::Default`;
  - with the `PkarrPublisher::n0_dns()` publisher added.
- Hidden `ferryx-cli iroh-probe` (only in `iroh-direct` builds) and `scripts/qa/iroh-rtt.mjs`.
  They measure 1,000 keystroke echo round trips over the current relay hop (`/tunnel/opaque`)
  and over iroh, between two peers.
- `rustls` exact pin moved from 0.23.43 to 0.23.46, because iroh 1.4 requires >= 0.23.46. Cargo
  resolves optional dependencies too, so this pin applies to every build, not only `iroh-direct`.

## Gates

| Gate | Pass condition | Measured | Verdict |
| --- | --- | --- | --- |
| (a) latency | `irohP95Ms <= 0.70 * relayP95Ms` | Linux netns pair, 1,000 keystrokes: relay hop p95 **2.258 ms**, iroh (direct path selected) p95 **2.313 ms**, a change of **-2.4%** (worse) | **NO-GO** |
| (b) size | `sizeDeltaMb < 30` | release `ferryx-cli`, default features, x86_64 Linux: 68,055,464 B without vs 77,442,440 B with `iroh-direct` = **+8.95 MiB** | **GO** |
| (c) PIN pairing | existing pairing tests pass unchanged with `iroh-direct`; no pairing file edited | `cargo test --features iroh-direct --lib -- pin pairing remote::session_transport remote::attach_router remote::iroh_direct remote::iroh_probe --skip ssh::bridge`: 137 passed, 0 failed; the same filter at the base: 130 passed (the 7 extra are the new spike tests); no pairing source file in the diff | **GO** |
| (d) no new service | 0 address-lookup services, only the self-hosted relay, no n0 traffic | runtime assertion test green 5/5 runs, red under both mutations; feature graph has 0 `pkarr` entries (`rg -c "n0\|pkarr"` = 27, all `n0-error`/`n0-future`/`n0-watcher`/`n0-dns-resolver` utility crates); tcpdump of both QA runs: 0 DNS queries, 0 n0 host names, every unicast destination is a peer or the self-hosted relay host | **GO** |

Licenses: `iroh-direct` adds 63 crates. All are MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib,
Unicode or CC0, and none is (L)GPL (`task-26-licenses.txt`).

The self-hosted relay is iroh-relay, built from github.com/n0-computer/iroh at tag v1.4.1 with
`cargo build --profile optimized-release -p iroh-relay --features server`. It ran in `--dev`
mode on an isolated test network only. Production `relay.ferryx.dev` was not touched.

## Latency (gate a)

Setup on one ubuntu-24.04 GitHub runner (run 41818311262):
- Two peers in separate network namespaces on a bridge, with netem adding 1 ms each way on
  every peer link.
- The relays sit on the bridge host: ferryx-relay `/tunnel/opaque` for the current hop, and
  iroh-relay `--dev` for iroh.
- Both transports carry the same Noise attach handshake and 1-byte frames, 1,000 sequential
  keystroke echoes.

| Run | p50 | p95 | Path | Peer max RSS |
| --- | --- | --- | --- | --- |
| relay hop (`/tunnel/opaque`) | 2.183 ms | 2.258 ms | relay | 22.4 MB |
| iroh, UDP allowed | 2.202 ms | 2.313 ms | direct (selected path is IP) | 35.7 MB |
| iroh, UDP between peers blocked | 2.272 ms | 2.369 ms | self-hosted relay | 35.4 MB |

In this topology the relay host is one bridge hop from both peers, so the "direct" path saves
nothing. The 30% gate is not met. Direct attach could only beat the relay when the relay is far
from both peers (relay.ferryx.dev vs two machines on one LAN or nearby). That case was not
measured: the Linux <-> maho-win pair named in the todo was not run, because the owner's
verify-once rule stopped all further QA legs. The frozen gate is judged on what was measured,
and on that the result is NO-GO.

Failure scenario: with UDP between the peers dropped by iptables, iroh fell back to the
self-hosted relay (`irohDirect: false`) and finished all 1,000 echoes. The capture of that run
(`task-26-tcpdump-failure.log`) has no DNS queries and no `*.iroh.network`, `iroh.link` or n0
hosts. Every unicast packet went to 10.26.0.1, 10.26.0.2 (the peers) or 10.26.0.254 (the relay
host); the rest is IPv6 link-local neighbour discovery.

## Memory

The iroh peer's max RSS is 35.5 MB against 22.4 MB for the relay-hop peer (+13 MB). Recorded
only; this is not a gate.

## Decision

**NO-GO: do not ship `iroh-direct`.** Gate (a) fails. Gates (b), (c) and (d) pass. The feature
stays in the tree as an off-by-default spike so the far-relay case can be measured later
without rebuilding the harness. It must not be enabled in default, headless or release builds.
No production relay change is proposed.

Todo 25 results do not change this:
- Item 1 is CONFIRMED: Rex's local GUI<->server transport is HTTP + WebSocket over a Unix socket.
  That is a local transport and says nothing about remote paths.
- Item 3 is INCONCLUSIVE: Rex's web client seems to be served only on a tailnet listener. That
  points to a third-party overlay (Tailscale), which the owner has already ruled out (tsnet
  dropped), not to a self-hosted QUIC relay.
- Neither result is evidence that a direct path beats Ferryx's relay hop.
- Re-open only with a measurement where the relay is remote from both peers, and only if that
  run clears 30% at p95.

## Not measured

- The Linux <-> maho-win pair (RUN@win) was stopped by the verify-once rule. The probe and
  `scripts/qa/iroh-rtt.mjs` already support it through `--dial-prefix "ssh maho-win"`.
- Real NAT traversal across the internet.

