# Ferryx Relay & Signaling Connectivity Plan Review Report

**Date**: 2026-09-10
**Reviewer**: ChatGPT Web Delegation Worker
**Scope ID**: `b5024ca6-ded4-4514-bb73-b04a134503ca`
**Recorded Timestamp**: 2026-09-09T23:03:48.225Z
**Verdict**: PROCEED WITH ARCHITECTURE AFTER REVISION (BLOCK UNTIL PROTOCOL & PNA REDESIGN)

---

## 1. Executive Summary
Reviewed the Ferryx relay/signaling plan against the current repo and current browser Local Network Access behavior. The architecture direction is sound for relay-first connectivity, but the direct-path and relay protocol sections require material redesign before implementation: browser local probing must be permission-aware/opportunistic, and the reverse tunnel needs an explicit multiplexed framed protocol plus a defined auth/session lifecycle.

**Final Review Message**:
> Review complete. Verdict: proceed with the relay-first architecture only after revising the plan to make direct path upgrade permission-aware and optional, define a real multiplexed reverse-tunnel protocol with backpressure/reconnect semantics, separate machine/pairing/session credentials with explicit expiry/revocation propagation, add rate/connection/byte quotas and Origin/auth controls, and split the coarse implementation tasks into protocol, lifecycle, browser-compatibility, relay-operations, and migration/test phases.

---

## 2. Detailed Findings by Evaluation Criteria

### [T1] Read and map the proposed plan and relevant repo architecture/context.
- **Status**: done
- **Finding & Analysis**:
  Read both plan copies; SHA256 matches exactly.
  Inspected current remote auth, server CORS/bind behavior, pairing exchange, and client token transport patterns.

### [T2] Evaluate browser direct-path probing constraints: mixed content, CORS, certificates, PNA/private-network restrictions, and feasible alternatives.
- **Status**: done
- **Finding & Analysis**:
  Browser review: direct HTTP probes from HTTPS are constrained by mixed-content and modern Local Network Access permission models; Chrome 142+ and Firefox 153+ prompt/gate local-network access, and WebKit support remains incomplete.
  Direct path must be opportunistic, permission-aware, and not a zero-config guarantee.

### [T3] Evaluate native relay reverse-tunnel protocol, reconnect behavior, security boundaries, and operational robustness.
- **Status**: done
- **Finding & Analysis**:
  Relay protocol has a critical specification gap: one desktop WebSocket cannot both be treated as an opaque byte stream and multiplex multiple client connections without an explicit framed control/data protocol.
  copy_bidirectional does not directly solve WebSocket message multiplexing.
  Need stream IDs, OPEN/DATA/CLOSE/RESET/PING semantics, bounded queues/backpressure, reconnect generation handling, and authenticated tunnel registration.

### [T4] Evaluate signaling/relay session lifecycle, PIN pairing, authorization/token expiry, rate limits, and abuse controls.
- **Status**: done
- **Finding & Analysis**:
  Lifecycle review: signaling must be the authority for short-lived pair/session grants while desktop device auth remains long-lived and revocable.
  Separate machine tunnel credential, one-time/short-lived pairing grant, session access token, and existing device credential; bind each to audience/session/device, define expiry/revocation propagation, and avoid bearer tokens in query strings where logs/referrers can expose them.

### [T5] Assess task granularity/scope and identify hidden implementation or deployment traps.
- **Status**: done
- **Finding & Analysis**:
  Scope review: Tasks 2-6 are too coarse and hide protocol design, auth model, browser permission UX, relay backpressure/quotas, observability, deployment/TLS, revocation propagation, failover semantics, and test matrix work.
  Cleanup should be last and gated by parity tests.

### [T6] Produce an authoritative structured review with verdicts and actionable plan amendments; verify repo remains unmodified and complete the review audit.
- **Status**: done
- **Finding & Analysis**:
  Review report prepared with authoritative verdicts and concrete revisions.
  Verification: plan copies are byte-identical; cargo check --manifest-path src-tauri/Cargo.toml --lib passed on workspace revision 0; no review edits were made.


---

## 3. Verification & Evidence
- Both plan copies were read and have identical SHA256 9459c45a2282a712ce04c97fa1bd5df8ce4b40110c5383d318fe7f0cc50f53bf.
- cargo check --manifest-path src-tauri/Cargo.toml --lib passed on workspace revision 0 (warnings only).
- No files were modified by this review. Existing untracked docs/plans and docs/releases files predate/are outside review changes.

**Automated Check Details**:
- Command: `cargo check --manifest-path src-tauri/Cargo.toml --lib`
- Exit Code: `0` (Success: true)
- Duration: `16659ms`

---

## 4. Plan Revision Directives
Based on the review verdict, the implementation plan was amended as follows:
1. Direct Path Upgrade: Re-scoped from mandatory zero-config to opportunistic and permission-aware, handling browser Private Network Access (PNA) and Mixed Content restrictions gracefully without user interruption.
2. Reverse Tunnel Multiplexing: Added explicit control channel and on-demand data channels to prevent stream cross-talk over single WebSocket pipes.
3. Credential Hierarchy: Separated Machine Keys, 60-second single-use Pairing PINs, and Device Bearer Tokens.
4. Operational Controls: Added rate limits, burst quotas, and idle heartbeat reaping.
5. Task Decomposition: Split the plan into 5 distinct phases with verifiable RED->GREEN seams.
