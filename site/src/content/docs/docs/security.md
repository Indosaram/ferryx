---
title: Security and Operations
description: "Ferryx security model: what the relay can see, agent control defaults, machine grants, the shared trust root, data retention, the updater trust chain, and the limits of the hosted relay."
lastUpdated: 2026-10-24
---

This page describes what you trust when you use Ferryx, and what the hosted relay at `relay.ferryx.dev` does and does not promise. To report a vulnerability, use the private channel in [`SECURITY.md`](https://github.com/Indosaram/ferryx/blob/main/SECURITY.md).

## Encryption model

Ferryx reaches a machine in one of three ways: on the local network (the daemon's own gateway), over a direct QUIC path between two Ferryx machines, or through a relay.

- **Direct path:** end-to-end encrypted between the two machines. The relay only carries the signed offer/answer exchange and is the fallback when the direct path breaks.
- **Account attach sessions:** a Noise handshake runs between the client and the machine before any gateway byte, so the relay forwards ciphertext. The relay marks these sessions `opaque` and never parses their frames.
- **Relay-proxied web and paired-host traffic (`/host/{id}/...`):** this is the part that is changing.

| | Before | After the end-to-end change |
| --- | --- | --- |
| What the relay forwards | Plain HTTP and WebSocket bytes between the browser or paired desktop and the machine's gateway (TLS ends at the relay) | Ciphertext only; requests and terminal streams travel inside the same Noise tunnel that account attach uses |
| What the relay operator could read | Terminal input and output, API requests and responses for that session | Session metadata only: machine ID, timing and byte counts |
| How a client opts in | Not possible | Updated clients ask for an opaque session (`?e2e=1`) when the device has an attach key |
| Older clients | Plaintext | Keep working over plaintext for one release, with a deprecation warning logged by the relay; then plaintext is refused |
| Devices paired by PIN | Plaintext | Get a typed `REPAIR_REQUIRED` and a re-pair prompt, because they have no attach key |

Until the end-to-end change ships in a release and the relay runs it, treat relay-proxied browser and paired-host sessions as readable by whoever operates the relay. Use the direct path, the local network, or [your own relay](/docs/self-hosted-relay/) if that matters to you.

## Agent control is off by default

Agent control lets a paired device with a Control grant type into your terminals and drive agents (`ferryx-cli agents --host`). On a fresh install it is off. Turn it on in **Settings > Remote Access > Agent control**, where you can also exclude single devices and read the last 50 audit records. The choice is stored per machine in `<data>/remote/agent-control.json` (mode 0600). An unreadable file means off.

An install upgraded from a build where agent control was always on keeps it on once, if a device already holds a machine Control grant, so a working setup is not silently broken. You can turn it off from the same card.

## A machine grant means code execution

Pairing a machine is not phone mirroring. A machine Control grant permits browsing raw filesystem paths and executing programs as the daemon's OS user. Only pair machines and desktops you control. Mirror grants keep their active-desktop-session restrictions and path redaction.

## The relay host is part of the trust root

Machines enrolled to the same account trust each other's direct-path keys through a record signed by the account service. In production the account service runs inside `ferryx-relay`, so the account signing key is not independent of the relay host: whoever controls that host's account key can attest a direct-path key for any machine of the account owner.

If that is not acceptable:

- **Pin keys manually.** Exchange keys out of band with `ferryx direct-trust identity` and `ferryx direct-trust provision`. A manual pin always wins over an account-attested key, and a revoked machine is never re-pinned automatically.
- **Disable auto-trust.** Set `FERRYX_DIRECT_AUTO_TRUST=0` (also `false`, `off` or `no`) on a machine to turn off account-attested pinning on that side. The default is on.
- **Pin the account key.** `FERRYX_RELAY_ACCOUNT_PUBLIC_KEY` overrides the key a machine otherwise pins from `/api/account/v1/public-key` at enrollment.

The full rules, including key precedence and rotation, are in [`docs/direct-peer.md`](https://github.com/Indosaram/ferryx/blob/main/docs/direct-peer.md).

## Self-hosting the relay

Running your own `ferryx-relay` moves the relay and the account trust onto infrastructure you operate, and keeps your traffic and account data off `relay.ferryx.dev`. Self-hosted relays are free on every plan. See the [self-hosted relay guide](/docs/self-hosted-relay/).

## Data retention

The desktop app sends no telemetry, and sessions, workspaces and preferences stay on your machines. The relay and the account service keep machine ownership records, billing state, your email address and hashed login codes; most of it is kept until account removal, and peer IPs used for rate limiting stay in memory only. There is no self-service account deletion: send a request through the channel in `SECURITY.md` and the operator deletes the records by hand. The itemized list, with retention for each item, is in [Relay and account data](/privacy/#relay-and-account-data).

## Updater trust chain

- The app checks for updates at launch and then once an hour, only from `https://github.com/Indosaram/ferryx/releases/latest/download/latest.json` (`plugins.updater.endpoints` in `src-tauri/tauri.conf.json`). Installs managed by a package manager skip this check.
- Every update archive must carry a minisign signature that verifies against the public key built into the app (`plugins.updater.pubkey`). An update that fails verification is not installed.
- The same key signs `SHA256SUMS` on every GitHub release, so you can verify a manual download:

```bash
minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm
shasum -a 256 -c SHA256SUMS --ignore-missing
```

Releases are built on operator-controlled machines, not hosted CI; each one ships a `provenance.json` and CycloneDX SBOMs. See [Release provenance](https://github.com/Indosaram/ferryx/blob/main/SECURITY.md#release-provenance).

## Hosted relay limits

`relay.ferryx.dev` is run by one maintainer on a best-effort basis, with no SLA. Plainly:

- **Single host.** One primary host serves all traffic and is the only writer of account data. A read-only warm standby can carry relay tunnels during a failover, but sign-in, enrollment, grants and billing changes wait for the primary. There is no multi-region deployment.
- **No SLA.** There is no uptime commitment, no service credit, and no guaranteed response time for outages. Plan for the relay being unavailable: a self-hosted relay removes the dependency, and the local network does not need it at all.
- **Region.** The primary runs in South Korea and is reached through Cloudflare's network. Expect higher latency from other continents.
- **Status.** Measured uptime of the hosts is public at [status.ferryx.dev](https://status.ferryx.dev). It reports what happened; it is not a promise.
- **7-day grace cutoff.** If an account goes overdue or over capacity, a 7-day grace period starts. If it is still not in good standing after 7 days, all account-based remote connections, relay and direct, stop at the deadline until billing or machine count is fixed. See [Pricing & Plans](/docs/pricing/).
