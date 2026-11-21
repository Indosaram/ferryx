# Account Auto-Pairing — Plan v4: delete Connect, registration and Needs Grant

**Status:** Proposal, revised across two adversarial review rounds plus a scope correction.
Not implemented.
**Date:** 2026-11-15
**Supersedes:** v1 (rejected), v2 (approve with changes), v3 (right defect, wrong scope — kept a
manual retry the requirement forbids)

**Scope in one line:** signing in to the account *is* the connection. No enrollment step, no
per-machine Connect button, no resting "Needs Grant" row.

## 1. What the reviews changed

Two adversarial rounds returned REJECT then APPROVE WITH CHANGES. v3 is built on the corrected
facts. v2's own new errors are listed in the second table — they matter because v2 shipped them
as confident claims.

**v1 errors, corrected:**

| v1 claim | Reality | Evidence |
| --- | --- | --- |
| "the daemon holds no standing account credential" | It does when this machine enrolled: `account-session.json`, owner-only | `account/enroll_client.rs:19-22` |
| "no automatic grant+redeem exists below the UI" | It does, without any UI — but wired to hub migration only | `hub_runtime/holder_link.rs:364`, `:201-202`, `:312-341` |
| epoch-based eligibility needs no daemon change | `HostView` carries neither the enrollment epoch nor an E2E flag | `paired_host/inventory.rs:79-103` |
| `shell` 401/403 proves the route exists | `shell` is not in the admitted route table at all | `relay_server.rs:3539` (`allowed_http_route`) |
| `requestGrant` is rate-limited per account | `issue_grant` has no limiter; `pairing_rate_limited` is the relay's per-IP **failure** lockout | `account/service.rs:1566-1640`, `relay_server.rs:2187-2283` |

**v2 errors, corrected here:**

| v2 claim | Reality | Evidence |
| --- | --- | --- |
| Outcome A opens agent control via the account-mark bypass | The bypass is only in `http_machine_peer`, scoped to machine-to-machine supervisor routes; agent control/voice/supervisor-UI all use `http_device` → `permits()` | `agent_control_service.rs:1159`, `:1134-1152`, `:1289-1663` |
| "voice/supervisor routes inherit the bypass" | They do not | `voice_api.rs:65,77,98`, `supervisor_api.rs:69,86,99,110` |
| "agent control needs a new desktop-owner marker" | No new policy is needed: `permits()` returns true for every token-valid device when `enabled` and `restricted` is `None` | `policy.rs:161-174`, `agent_control_service.rs:1140-1149` |
| The daemon already has the credential (as a general fact) | Only when the machine enrolled; plain sign-in writes localStorage only, and sign-out never deletes the file | `AccountSignIn.tsx:85,147,233`; `accountSession.ts:445-466` |
| "older daemons keep manual Connect" after a protocol bump | A bump makes the new UI refuse every paired-host call against an old daemon | `daemon/client.rs:2328-2329`, `:860-870` |
| Trigger 2: `ACCOUNT_SESSION_CHANGED_EVENT` reaches the daemon | It is a DOM `CustomEvent`; the daemon never sees it | `accountSession.ts:433-443` |

## 2. Problem

There must be **no registration or connection step in the GUI at all.** Signing in to the account
is the connection. Nothing that resembles "add this machine", "enroll this computer", "Connect"
or a per-machine button may remain.

Today the user has to perform two separate manual acts:

1. **Enrollment.** Onboarding's `AccountStep` asks for a code and calls `issueCode` then `enroll`
   (`AccountStep.tsx:74-78`), which writes `account-session.json` on this machine
   (`ipc/account.rs:55-60`). A plain sign-in does **not** do this — it writes localStorage only.
2. **Connect.** A per-machine row button in Settings calls `handleConnectAccountMachine`
   (`RemoteSection.tsx:386`, wired at `:713`), which is the only redeemer of grants
   (`:410`).

Both must disappear. The user's words: *"if you log in via the relay, pairing must happen
immediately"* and *"there should be no registration process in the GUI"*.

The observable failure: a machine enrolled in the account reaches the relay and reports
`online: true`, but the desktop has no device token for it, so every authenticated route is
refused and the row sits at **Needs Grant** until someone clicks Connect.

## 2a. What "remove the process" means concretely

| Removed | Replaced by |
| --- | --- |
| `AccountStep` code entry + `issueCode`/`enroll` | sign-in alone; the daemon derives its identity from the account session |
| Settings per-row **Connect** button (`RemoteSection.tsx:713`) | nothing — the row is simply usable |
| `Needs Grant` as a user-visible state | absent; a machine is either usable or shows why not |
| Manual retry affordance as the primary recovery | automatic retry on the events in §5 |

`needsMachineGrant` may remain as an internal transient value while a pass is in flight, but it
must never be the resting state of an online machine, and it must never require a click.

## 3. The automatic path already exists — in Rust, for a different feature

`hub_runtime/holder_link.rs` is the standby-supervisor link. When a standby hub runs, `grant_on`
resolves the account bearer from the identity directory, lists the account's machines, requests a
`machine`-scope grant, and calls `redeem_grant` with no user involved. It is the single best
precedent in the tree for what this plan wants, and v1 named the wrong one
(`useAccountWorktrees.ts:239`, which is web-client-only and keeps tokens in memory).

**Settled (§3a): `grant_on` reaches at most one machine, and only through hub machinery.**
`holder_link` is reached through the supervisor-move path: `supervisor_move.rs:382 pair_to_holder`
calls `holder_link::LINK.holder_host`, whose callers are `hub_runtime.rs:2464
follow_holder_pairing_with` and the standby pull (`:3058`). A plain desktop *does* start a hub
after readiness (`cli.rs:4527-4531`) unless `FERRYX_HUB=off` (`hub_runtime.rs:4442`), and on an
enrolled standby `follow_holder_pairing` does pair — but only to the lease holder, and only when
the role names a holder other than this machine (`hub_runtime.rs:2436`, `:2447-2455`). So nothing
in account login, app start, or the desktop's remote path pairs the account's other machines.
The reconciler is therefore not redundant; it is the missing piece.

## 3a. Settled: `grant_on` is hub-only, so the reconciler is required

| Path | Pairs account machines? |
| --- | --- |
| Desktop account login | no |
| App start, Settings open | no |
| `pairing_bootstrap` / remote attach | no |
| Hub standby pulling a holder's checkpoint | **yes, the holder only** (`hub_runtime.rs:2464`, `:3058`) |

So the corrected picture is: the daemon has the credential only when *it* enrolled (§5), and the
redeem implementation is complete (`service.rs:943`). A working automatic grant+redeem exists but
is **wired to hub migration only**, for at most one host. Account login never invokes it. That is
the real root cause — narrower than v1 claimed and different from what v1 said.

## 4. Why the layer must change

v1 put the reconciler in a React hook. The corrected facts argue against that:

- The daemon already owns the account credential and already redeems grants (§3). A UI loop would
  be a second owner of the same responsibility.
- Account machines live only in `RemoteSection`'s local state (`RemoteSection.tsx:93-149`);
  `App.tsx` has no machine list. `ACCOUNT_SESSION_CHANGED_EVENT` is dispatched
  (`accountSession.ts:431-443`) but **nothing listens for it**. v1 assumed app-scope wiring existed.
- A second webview exists (`native_terminal/surface_host.rs:8923`), so two reconcilers can run
  concurrently. Two `redeem_grant` calls both pass `pair/exchange`, registering **two devices** on
  the target; the second fails `StaleGeneration` (`service.rs:1113-1115`) and leaves an orphaned
  Control device behind.
- A UI reconciler does nothing when the app is closed or the machine is headless.

**Decision: the reconciler belongs in `PairedHostService`.** It is the component that already
owns the inventory, the generation fence, and the redeem implementation. The UI becomes a
display surface.

## 5. Design — daemon-side reconciler

**Prerequisite: sign-in must make the daemon hold the credential, with no enrollment act.**

`account-session.json` is written only when *this machine is enrolled*: `ipc/account.rs:55-60`
(reached from `enrollThisMachine`) and the CLI (`cli.rs:2546`, `:3247`). A plain **sign-in** writes
localStorage only (`AccountSignIn.tsx:85,147,233` → `storeAccountSessionToken`,
`accountSession.ts:445-453`).

**Sign-in must write the daemon session directly.** It already has everything enrollment needs:
`issueCode` + `enroll` are an account-API round trip that a valid session token can drive
(`AccountStep.tsx:74-78`). The reconciler therefore calls the same enrollment internally the
first time it sees a signed-in session with no credential file, and the user never sees a code.

Worse, **sign-out never deletes the file.** Every `ACCOUNT_SESSION_FILE` occurrence is a write or
a read; there is no removal. `RemoteSection` sign-out clears localStorage only
(`accountSession.ts:455-466`). A daemon reconciler reading that file would keep minting grants
with a 30-day bearer **after the user signed out.**

Required, and part of this plan:

- One IPC command pair that owns the file: save on sign-in, **delete on sign-out and on
  `UNAUTHORIZED`**. This also becomes the single token-clearing owner §12 asks for.
- On first signed-in run without a credential file, enroll silently and write it. No code, no
  panel, no confirmation step.
- Resolve the origin from `StoredAccountSession.account_origin` (`enroll_client.rs:26-29`), not
  from the enrollment record.
- Treat **signed-in-but-not-enrolled as a transient state that resolves itself**, never a state
  that asks the user to act.

```
PairedHostService::reconcile_account_machines()
  credential = ensure_session_file()      // silent enroll on first run
  for each machine in AccountService::list_machines()   // requires §6 fields
    if not eligible (see §7) -> record reason, continue
    if single_flight(machine_id) held -> continue
    redeem_grant(...)                                    // reuses service.rs:943 unchanged
```

Triggers — event-driven only, no timers. **A DOM event cannot reach the daemon**, so trigger 2
from v2 is replaced:

1. daemon start, once a session is readable
2. an explicit `paired_host_reconcile` IPC command, called by the UI whenever it *already*
   refreshes the account machine list: sign-in, window focus (`RemoteSection.tsx:358-364`). The
   daemon de-duplicates internally, so repeated calls are cheap.
3. a `RepairRequired` result from any operation against the target

Bounded: one machine at a time per pass, at most one in-flight redeem per machine id.

## 6. Required daemon change: expose what eligibility needs

`HostView` must gain two additive fields. Without them the reconciler cannot tell a usable host
from a broken one — this is the defect that makes v1 unimplementable.

```rust
// paired_host/inventory.rs, HostView
#[serde(default, skip_serializing_if = "Option::is_none")]
pub enrollment_epoch: Option<String>,
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub e2e: bool,
```

Follow the `published_origins` precedent (`inventory.rs:96-102`): defaulted and omitted when absent.

**Do not bump `DAEMON_PROTOCOL_VERSION`.** v2 said to, copying that precedent, but that field's
comment explains the bump was needed because a *client* preedating the field would reject the
projection; the relevant mechanism here is different. `HostView` is `deny_unknown_fields`
(`inventory.rs:78`), so an old client rejecting a new field is a real risk — but a bump makes the
new UI refuse **every** paired-host call against an old daemon (`daemon/client.rs:2328-2329`,
mismatch at `:860-870`), taking the whole inventory dark until the daemon restarts. That is a
worse outcome than the field problem it solves.

Instead: gate the fields behind a capability string the client checks before reading them, and
confirm that an unknown *response* field is tolerable on the wire path in use. If it turns out
`deny_unknown_fields` makes that impossible, the bump is unavoidable — and then §13 must say the
update **forces a daemon restart**, not that old daemons keep working.

Note the trap the review found: `HostEndpoint.generation` is the **inventory generation**
(`inventory.rs:617-625`), not the enrollment epoch, and `mergePairedHosts` already conflates them
(`remoteModel.ts:69`). Eligibility must key on the new field, never on `generation`.

## 7. Eligibility

All must hold:

1. `online === true`
2. `machineId` is non-empty, `enrollmentEpoch` is present, and `machineId` is **not this
   desktop's own machine id** (`ipc/remote.rs:285-288` — `list_machines` returns the owner's own
   machine too; pairing it to itself mints a Control device on its own gateway and adds a
   duplicate row)
3. the target is not revoked — the reconciler lives in Rust, so it reads `AuthStatus::Revoked`
   (`inventory.rs:455-459`), **not** the UI's `pairedHostInventory.ts:412` that v2 cited
4. no in-flight redeem for this machine id
5. the grant scope is `machine`, never `mirror` — a mirror device is refused on every project
   route (`workspace_api.rs:293`, `filesystem.rs:481`, `repo_discovery.rs:454`)
6. **skip only when the host is usable.** Usable = stored with a matching `enrollmentEpoch`
   **and** `e2e == true`. A `paired` row without an attach key sends relay traffic in plaintext;
   a default relay refuses it with `426 E2E_REQUIRED` (`relay_server.rs:3258-3265`) which the
   client maps to `REPAIR_REQUIRED` (`client.rs:803`) while the row still reads `"paired"`.

## 8. Agent control: default policy, no new marker

v2 chose outcome A (`requester_machine` proof) and was wrong: the account-mark bypass lives only
in `http_machine_peer` (`agent_control_service.rs:1159`), whose doc comment scopes it to
"same-account machine-to-machine supervisor routes only". Its callers are `observe`
(`supervisor_api.rs:39`), `request` (`:342`), `release-to` (`:652`), `checkpoint` (`:673`) and
`fence` (`agent_control_service.rs:1098`). A would have handed this desktop `release-to` (stop the
hub, release the lease), `checkpoint` (an archive that "carries secrets",
`supervisor_api.rs:665-669`) and `fence` — more power, none of the requirement.

**Decision: redeem plain (outcome B) and set the target's existing policy to its permissive
default. No new marker, no new policy type, no change to the `permits()` call sites.**

`AgentControlPolicy::permits` (`policy.rs:161-174`) is:

```rust
if !self.enabled { return false; }                       // the only real gate
if self.excluded.contains(device_id) { return false; }   // per-device escape hatch
match self.restricted {
    None => true,                                        // no allowlist => every device
    Some(allowed) => allowed.contains(device_id),
}
```

So `enabled = true` with `restricted = None` admits every device that already holds a valid
token — which is exactly the set of devices legitimately paired to that machine. Authentication
runs first: `http_device` calls `validate_token` **before** `permits()` (`:1140-1149`), so an
unpaired or revoked token never reaches the policy at all. "All devices" therefore means the
account owner's own paired devices, not a widened door.

Implementation: the reconciler sets `enabled = true` and leaves `restricted` as `None`, through
the existing `set_enabled` (`policy.rs:186-188`). The per-device `excluded_device_ids` escape
hatch is unchanged (`ipc/agent_control_settings.rs:43-54`), so an owner who wants a device out
keeps that ability without touching the allowlist design.

Routes this opens, all through `http_device` → `permits()`: agent-control sessions, events, send,
answer, stop (`agent_control_service.rs:1289-1663`), voice (`voice_api.rs:65,77,98`), supervisor
snapshot/panel/start/pause (`supervisor_api.rs:69,86,99,110`), the mutation re-check
(`http_reauthorize`, `:1204-1215`) and the owner event stream (`machine_events.rs:296-298`).

The three dangerous peer routes stay closed because this plan never issues a
`requester_machine` proof: `release-to`, `checkpoint`, `fence` remain unreachable to a
desktop-paired device.

## 9. One redeem path

v1 kept the JS-tunnel legacy branch. Drop it for account grants.

`RemoteSection.tsx:396` does `capabilities().catch(() => null)` and falls through to the legacy
path on **any** error — daemon restart, timeout. That path mints a grant, redeems it in a JS
tunnel, registers a new device, then usually fails at `migrate` (§7.6), leaving a spent token, an
orphaned Control device, and nothing stored. An automatic caller would hit this silently.

Rules:

- E2E path only.
- If `capabilities()` throws or reports E2E false, record `DAEMON_UPDATE_REQUIRED` and make **no**
  grant request.
- Keep the legacy migrate route for importing old browser credentials only. It is not an account
  grant path.

## 10. Failure classes

Park only what cannot succeed by retrying. `retryable` is already authoritative in
`service.rs:1214-1233`.

| Class | Codes | Behavior |
| --- | --- | --- |
| Terminal | `UNAUTHORIZED`, `WRONG_RELAY`, `MACHINE_GRANT_REQUIRED`, `INVALID_PIN`, `revoked`, epoch mismatch after one list refresh | park for the session, surface reason |
| Retryable | `RATE_LIMITED`, `SERVICE_UNAVAILABLE`, `TIMEOUT`, `HOST_UNAVAILABLE` | one new attempt per **observed event** (§5 triggers) — never a timer |
| Unsupported daemon | E2E capability absent | `DAEMON_UPDATE_REQUIRED`, no grant requested |

`online` means "seen in the last 300 s" (`account/service.rs:49`), so a machine that just went
away still looks online. That is why retryable failures key on events, not on the `online` flag
alone.

Orphaned grants are self-healing: `GRANT_TTL` is 600 s (`account/service.rs:32`). An
`invoke` in flight cannot be cancelled, so the reconciler must never promise "persists nothing on
abort" — only "issues no further requests".

Every re-redeem registers a new device and does not revoke the old one. Forgetting the previous
`deviceId` drops only the local record: no device-revoke route is admitted by the relay
(`relay_server.rs:3568-3629`), so devices still accumulate on the target. Say that plainly rather
than claiming a fix.

## 11. Concurrency

- Single-flight per host id inside `PairedHostService::redeem_grant`, not in the caller. The
  caller-side map in v1 did nothing across processes.
- One boot-time pass; no periodic sweep.
- Sequential across machines to keep relay and gateway load low (not for rate-limit reasons —
  that mapping in v1 was wrong).

## 12. UI: delete the affordances

- **Remove the per-row Connect button** (`RemoteSection.tsx:713`) and
  `handleConnectAccountMachine` (`:386`). Nothing replaces them; the row becomes usable on its own.
- **Remove the enrollment step** from onboarding. `AccountStep` stops asking for a code and stops
  calling `issueCode`/`enroll` (`:74-78`); signing in is the whole step.
- `needsMachineGrant` may be rendered only while a pass is in flight. If it renders, the flow is
  broken — treat a resting Needs Grant row as a defect, not a UI state to document.
- `RemoteSection` consumes the daemon's reconciler state; it never drives redeeming.
- Single owner for clearing the session token on `UNAUTHORIZED` — `RemoteSection.tsx:133-137`
  and `:160-163` already do it locally; consolidate into the §5 owner.
- A per-row reason from a sanitized diagnostic code, reusing `recordInventoryDiagnostic`
  (`pairedHostInventory.ts:52`). This is the only user-facing recovery surface: it explains, and
  the daemon retries on its own.
- Setting `remote.autoPairAccountMachines`, default on. It exists so a user can **stop** the
  behavior; it is not a step they must take to start it.

## 13. Migration

- Existing `paired` hosts without an attach key are **repaired** on the next pass (`e2e == false`
  ⇒ eligible). Migration is therefore part of the reconciler, not a one-off script.
- Existing `mirror` rows are re-redeemed at `machine` scope.
- Older daemons: no reconciler and no new `HostView` fields, so auto-pairing is impossible.
  The Connect button is **still gone** — do not reintroduce it for old daemons, because that
  re-establishes the process this plan removes. Those rows show `DAEMON_UPDATE_REQUIRED` and the
  app offers a daemon update as the only path. This holds **only if §6 avoids a protocol bump**;
  see that section before relying on it.

## 14. Tests

Rust (`redeem_grant`, reconciler):

1. self machine id excluded — no grant requested
2. `e2e == false` host is re-redeemed; `e2e == true` with matching epoch is skipped
3. epoch compared against `enrollmentEpoch`, never `generation`
4. legacy branch never taken; E2E-absent daemon yields `DAEMON_UPDATE_REQUIRED` with zero grant
   requests (assert the mock grant endpoint was never called)
5. retryable code → exactly one further attempt on an event, none without one
6. terminal code → parked, no further attempts on any event
7. the reconciler and the holder link racing for the same host produce **one** redeem and **one**
   device registration — model `reconcile_account_machines` plus `holder_host`, not two
   reconcilers (one daemon has one)
8. after redeem, an authenticated relay call (`GET workspace/projects`) returns 200 — the only
   test that proves "usable", not just "paired"
9. sign-out deletes the credential file and stops all further grant minting
10. a signed-in-but-not-enrolled desktop still reconciles (no `requester_proof` needed)
11. after pairing, the reconciler sets the target's policy to `enabled = true` with
    `restricted == None`, and an agent-control route then answers — assert the route, not the flag
12. an explicitly excluded device is still refused (`policy.rs:163-165`)
13. `release-to`, `checkpoint` and `fence` still refuse a desktop-paired device — the plan must
    not open the peer routes as a side effect

UI (mocked commands):

14. reconciler state renders as `paired` with no click
15. **no Connect control renders** for an account machine — assert the affordance is absent, not
    that clicking it works
16. **onboarding has no code-entry step**; a signed-in session alone completes the step
17. a terminal failure renders its reason and needs no user action to recover on the next event

All async waits block on the inventory `pair` event or the mocked promise. No sleeps.

## 15. Out of scope

Relay route table, `pair/exchange` semantics, grant TTL, epoch validation rules, and the retired
PIN flow are unchanged. The web client keeps `useAccountWorktrees`. The CLI gains nothing.

## 16. Evidence index

Corrected citations (v1's are superseded where they disagree):

- Daemon account credential: `account/enroll_client.rs:19-22`; written at `ipc/account.rs:55-60`,
  `cli.rs:2546-2549`, `cli.rs:3244-3249`
- Existing automatic grant+redeem: `hub_runtime/holder_link.rs:364`, `:201-202`, `:312-341`,
  bearer at `:450-465`
- `HostView` fields: `paired_host/inventory.rs:79-103`; additive-field precedent `:96-102`;
  generation `:617-625`
- Redeem implementation: `paired_host/service.rs:943`; retryable mapping `:1214-1233`;
  stale-generation `:1113-1115`
- Route admission: `relay_server.rs:3539` (`allowed_http_route`), E2E gate `:3258-3265`,
  per-IP failure lockout `:2187-2283`, `ADMISSION_WINDOW` `:655`
- Agent-control policy: `agent_control/policy.rs:108-116`, `agent_control_service.rs:1134-1149`
- UI redeem call site: `RemoteSection.tsx:410`; E2E block `:396-423`; legacy block `:427-466`;
  `catch(() => null)` at `:396`
- Manual-only diagnosis: `redeemGrant` appears only in `RemoteSection.tsx`, the TS bridge, and
  type declarations
