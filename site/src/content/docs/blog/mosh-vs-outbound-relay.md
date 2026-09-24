---
title: "Mosh vs Outbound-Relay Remote Terminals"
description: "Mosh rewrites the transport; outbound relays change who dials whom. Compare both for reaching NAT'd dev machines without inbound SSH exposure."
---

**Mosh vs Outbound-Relay Remote Terminals.** These solve different layers of the same reachability problem: Mosh replaces SSH's TCP session with a UDP state-sync protocol so a session *roams* across network changes — but still requires you to reach the host directly, UDP ports included. An outbound relay inverts the dial direction: your dev machine dials *out* to a relay, clients dial the same relay, and nothing on the host is ever inbound-addressable — so NAT, firewalls, and hotel networks stop being obstacles by construction, the dial direction Ferryx's paired remote also relies on. They compose as often as they compete: relay for reachability, Mosh-like transport for roaming once a path exists.

![mosh-vs-outbound-relay cover](/images/blog/mosh-vs-outbound-relay/cover.png)

## Transport differences

**Mosh** is a *protocol-level* replacement for SSH's stream. The client and `mosh-server` sync terminal state (screen contents) over encrypted UDP; lost packets repair via the next sync, IP changes re-key the session to the new address, and a prediction layer keeps typing feel local. What it preserves from SSH: you still initiate against a *specific host address on a specific port* — the standard reachability contract. What it changes: the session tolerates transport churn instead of dying with it.

**An outbound relay** is a *topology-level* change. The dev machine runs a client that maintains a persistent outbound connection to a relay (self-hosted on a VPS, or a hosted service); the browser/phone/client also connects to the relay; the relay splices the two streams together. There is no listening port on the dev machine at all — no forwarded port, no exposed sshd, no UDP hole to punch. The relay sees a splice (or, in privacy-preserving designs, ciphertext it cannot parse); the host's attack surface from the network is *zero inbound*, because no inbound exists.

The layer distinction is the whole comparison: Mosh changes *what happens to bytes on a path you already have*; the relay changes *whether you have a path at all*. Evaluating either against the wrong question produces confident wrong answers — "Mosh doesn't help, my host has no open ports" (true, and beside the point: the relay question) and "the relay doesn't help, my sessions keep dropping" (true for drops *after* connection, which is Mosh's question).

One composition note: relays typically carry TCP streams, so plain SSH rides them natively — and SSH-over-relay already survives drops *if* sessions below it are owned (tmux/daemon), because the reattach is cheap. Roaming-without-reattach (Mosh's signature move) requires the relay or its transport to support connection migration, which is a per-relay capability question — verify rather than assume.

## NAT and firewall behavior

| Scenario | Mosh alone | Outbound relay |
| --- | --- | --- |
| Home NAT, no port forwarding | Fails — needs inbound UDP port + forwarding | Works — host dials out only |
| Corporate firewall, UDP blocked | Fails — Mosh cannot start | Works — single outbound TCP/WebSocket |
| CGNAT (no public IP at all) | Fails — nothing to address | Works — no address needed |
| Client on hostile/roaming network | Strong — session follows IP changes | Depends — stream may drop; reattach cheap if sessions owned |
| Host behind policy: "no inbound ever" | Violates policy | Compliant by construction |

The pattern in the table: Mosh fails the *first-mile* tests (everything requiring inbound reachability) and wins the *last-mile* test (client-side roaming). The relay inverts it — trivially passes first-mile (it only dials out), while last-mile smoothness depends on what transport runs inside the splice and whether your session layer absorbs reconnections. Teams in CGNAT apartments, locked-down offices, or compliance environments reach for the relay first; teams whose hosts are already reachable but roam between networks reach for Mosh first.

Ports and protocol allowances matter operationally: Mosh needs UDP allowed both directions plus `mosh-server` installed on the host; the relay needs only outbound 443-compatible connectivity — a property that also makes it survive captive-portal environments that redirect everything else.

## Security surface of each

**Mosh's surface** is conventional and well-audited: SSH authenticates the session (key exchange over the initial SSH channel), then Mosh's UDP carries the encrypted state-sync with its own key. Your exposure = an open UDP port on the host (scanned, rate-limited by nature of UDP) plus standard SSH-daemon hygiene on port 22. Single-hop, peer-to-peer, no third party in the path — the trust list is short and yours.

**The relay's surface** splits by design: (1) *reachability trust* — the relay is in the connection path; in a naive splice design it can see plaintext, so either end-to-end encryption inside the tunnel (relay sees ciphertext only) or a relay you self-host on your own VPS; (2) *authentication* — pairing flows (device grants, revocable) rather than host credentials, which is actually a *strength* over raw SSH: a stolen phone gets its pairing revoked instead of requiring key rotation across machines; (3) *blast radius* — because the host has zero inbound listeners, scanning, brute-forcing, and unpatched-daemon-CVEs simply do not apply to it. The relay becomes the internet-facing component — which is exactly why self-hosting or E2E-encrypting it is the responsible configuration.

The honest summary: Mosh keeps third parties out but keeps an inbound port in; the relay removes the inbound port but introduces a path component you must trust or own. Which risk you prefer depends on environment — home-lab self-hosters often prefer owning the relay; laptop-roamers on already-reachable hosts often prefer Mosh's directness. The [self-hosted relay post](/blog/self-hosted-relay-explained/) works the relay configuration side, and [remote vs SSH](/blog/remote-vs-ssh/) frames the adjacent comparison for full remote stacks.

![Mosh vs Outbound-Relay Remote Terminals illustration](/images/blog/mosh-vs-outbound-relay/body-1.png)

## FAQ

### Does Mosh work through a relay?

Only if the relay splices a TCP stream Mosh can ride — but Mosh's UDP transport then needs the relay to forward UDP too, which most TCP/WebSocket relays do not. In practice: SSH-over-relay is the common composition, and if sessions underneath are daemon/tmux-owned, reattach-on-drop approximates Mosh's survivability without needing UDP anywhere. Verify your relay's protocol support before assuming otherwise.

### Which is better for a machine behind CGNAT with no public IP?

The outbound relay — categorically. CGNAT means no inbound path exists at any layer (no port forward, no UPnP, no DDNS trick), which eliminates Mosh and every direct-SSH variant outright. The relay's dial-out design needs no address at all: your host connects to the relay, your client connects to the relay, the splice happens there. This is the single environment where the topology choice decides everything.

### Can I get Mosh-style network roaming through an SSH relay setup?

Roaming is a property of the *transport inside* the tunnel: SSH-over-relay reconnects when the stream breaks (cheap if sessions are owned below), but does not seamlessly migrate a live connection the way Mosh's UDP state-sync does. Some relay/transport implementations add their own migration (QUIC-based transports do); check whether yours does. Alternatively, once the relay makes the host reachable, Mosh itself can ride the path if UDP passes — composition restores roaming where policy allows it.

### Is a hosted relay a third party reading my terminal?

Only if you configure it that way — the design axis to check is end-to-end encryption inside the tunnel versus terminated-at-relay transport. E2E designs (relay splices ciphertext) leave the relay operator with metadata only; plaintext-splicing designs require trusting the operator, which is why self-hosting the relay on a VPS you control is the standard recommendation for the plaintext case. The [relay architecture post](/blog/self-hosted-relay-explained/) details both configurations.
