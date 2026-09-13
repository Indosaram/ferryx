# A24 staged rollout and rollback runbook

Status: **partial automated rehearsal; AC12 is NOT signed off**. A23 and the native/platform release matrix remain prerequisites. No deployment was performed.

## Non-negotiable safety boundary

Never restart, kill, upgrade, replace, or terminate an unrelated developer/user `ferryx --daemon`. It owns live PTYs. Never use broad process-name cleanup. A feature rollback is not permission to terminate a daemon, delete remote sessions, reset pairing, erase credentials, or rewrite paired targets as Local/SSH.

All installation, release build, signing, packaging, deployment, native desktop operation and live-session steps below are **HUMAN ONLY, NOT EXECUTED BY A24**. Use disposable isolated accounts/machines, explicit runtime/data/config/session roots, and dedicated test credentials. Record the exact owner PID, socket, data roots, binary revision, daemon epoch, grant scope and capability list before each action. If ownership cannot be proved, stop. Never point fixtures at a user's HOME or canonical socket.

## Automated evidence and limitations

- `A24-RED.log`: old admission survives a failed negotiation, demonstrated by four behavioral failures before the fix.
- `A24-GREEN.log`: passing adapter and retirement tests plus required library check.
- `A24-rust-current.log`, `A24-check.log`, `A24-ui-build.log`: full command results.
- `A24-concurrent-build.log`: preserved compile interruption and A16 half-applied proxy errors; not passing evidence.
- Adapter tests model wire-version combinations through the actual desktop adapter with an injected native command response. They do not run archived binaries or relay services.
- Retirement uses actual in-process HandoverManager instances with injected actions and retained request guards. No PTYs, daemon processes or real sockets are created. An unrelated owner remains Active, receives no abort, and retains fixture route/credential bytes. This demonstrates request drain, NOT live-session drain or cross-process survival.

## Staged rollout: HUMAN ONLY

Proceed one stage at a time. Preserve backward compatibility until the entire canary matrix passes. Record errors and stop expansion on any unsupported route, rejected required query field, schema loss, scope promotion, wrong epoch, duplicate spawn or unrelated-owner change.

1. **Baseline and backups.** On dedicated accounts, inventory old/new component versions and credential-store locations. Keep a pre-v3 v2 backup, the intact current v3 file, and a hash/size inventory. Record unrelated fixture daemon identity and session targets without changing it. Check the A23 gate; do not enable a feature because a UI bundle is new.
2. **Relay compatibility first.** Install the compatible relay on a disposable service. Exercise old mirror client traffic and new machine HTTP/WS paths. Verify route forwarding and query preservation, especially `daemonEpoch`, `afterSequence`, `path`, `includeHidden`; verify forbidden routes stay denied. Intentionally place the new remote/client behind an old relay: route/query rejection must produce explicit incompatibility, not an empty successful directory result or a less-secure alternate route. Keep the machine feature off for that host on failure.
3. **Remote daemon services second.** Install only on dedicated service accounts. Verify machine-purpose PIN, identity, explicit capabilities, and unchanged mirror permissions. Test newer local against a peer with fewer capabilities and an older epoch: supported API epoch values remain identities, not version ordering; unsupported operations fail closed. An unsupported API version must be refused. Verify an old mirror token never acquires machine scope. Preserve project/session catalogs and credential stores.
4. **Local daemon/native proxy third.** Install only to isolated local roots. Verify inventory and proxy capabilities independently; a false or absent `pairedDaemonProxyV1` keeps paired attachment/creation unavailable. Test new desktop with old local daemon: Local/SSH remain usable but a paired target stays paired and disabled. No automatic daemon replacement. For a specifically owned handover, subscribe to its exact completion signals before committing; retain the old instance while sessions/operations drain. Verify the original target epoch and process identity survive. Never interpret a gateway's new epoch as permission to recreate a session.
5. **Desktop canary last.** Enable `pairedDaemonProjectsV1` only after all lower-layer gates pass, first internally on disposable roots, then in a small opt-in cohort. Test register/browse/worktrees, native menu/tab/split routing, disconnect/reconnect, revocation, restart and lost-create-response/no-duplicate-spawn. Keep an unrelated fixture daemon and Local/SSH panes present; prove their owners and data are unchanged. Exercise an older desktop against a future unknown capability: explicit safe refusal, no fallback or mirror branch.
6. **Expand only after sign-off.** Attach native macOS evidence and Linux headless evidence (plus required Windows coverage or explicit exclusion), archived-version matrix, and rollback artifacts. Never count mocked wire responses as actual binary compatibility.

## Rollback order: HUMAN ONLY

Prefer disabling exposure and a compatible forward fix over binary/schema downgrade.

1. **Disable desktop exposure and new mutations first.** Turn off the paired feature for canaries. Retain paired projects/layouts with a disabled explanation. Detach only explicitly selected proxies; do not close remote sessions or convert targets to Local/SSH. Verify unrelated panes and remote processes remain unchanged.
2. **Desktop downgrade, if explicitly required.** Stop only the designated UI writers to the isolated workspace file; do not stop its unrelated daemon. If a daemon is also a writer and cannot safely be quiesced, abort schema downgrade. Preserve current v3 bytes and credential stores. Validate the pre-v3 backup is valid v2 and belongs to this account; restore it explicitly for the old parser. Never feed v3 to an old writer, overwrite v3 with an empty old snapshot, or merge schemas blindly. Keep new paired state for re-upgrade. Verify old desktop launch/read/write and then re-upgrade recovery using the retained v3 copy, with writers stopped at each transition.
3. **Local proxy rollback.** Keep paired creation/attachment disabled. Remove only owned proxy attachments. Roll back only the dedicated instance after safe handover/drain has been proved; if it still owns sessions, leave it running until their explicit drain. Verify credentials, host generations and target mappings survive. Never force process exit as a rollback shortcut.
4. **Remote service rollback.** Disable machine admission and close only its machine control sockets without deleting PTYs or catalogs. Inspect residual sessions and obtain explicit permission before closing any disposable session. Preserve the old owner while draining. Verify unaffected fixture daemon identity, epochs and session state before/after. Do not downgrade incompatible durable state in place; retain snapshots and use a compatible service instead.
5. **Relay rollback last.** Only after dependent machine features are disabled and required traffic no longer needs new routes. Verify legacy mirror clients still work, old grants retain their scope, and new clients see explicit incompatibility. Do not silently strip security-relevant query fields to make old routes accept requests.

At every step record old/new version, negotiated capabilities, precise typed error, affected target identity, unchanged unrelated-owner evidence, durable-state hashes and cleanup receipts. Stop rollback if any prerequisite is missing; preserving a running owner is safer than forcing a downgrade.

## Required compatibility/sign-off table

| Combination | Required result | A24 evidence/status |
| --- | --- | --- |
| New desktop, old local daemon | Paired disabled, Local/SSH continue, no forced restart | HUMAN: actual old local daemon not run |
| New remote daemon, old relay | Explicit rejected route/query compatibility gate | HUMAN: old relay not run |
| New local/desktop, older remote epoch and fewer capabilities | Retain host/identity; unsupported operation refused before dispatch | Automated adapter test: epoch `1`, empty capability list, `UNSUPPORTED_CAPABILITY` |
| New local/desktop, unsupported older protocol | `INVALID_REQUEST`; revoke previous admission | Automated adapter test: API version `0` |
| Older desktop adapter, future capability | `UNSUPPORTED_CAPABILITY`; no subsequent operation dispatch | Automated adapter test: known capability plus `futureV9`; archived desktop HUMAN |
| Peer changes machine or downgrades to mirror | `CROSS_HOST_RESULT` / `UNSUPPORTED_CAPABILITY`; no stale admission | Automated adapter test |
| Old mirror/mobile client, new daemon/relay | Existing mirror projection and active-session rules; no machine access | HUMAN: actual legacy mobile not run |
| Offline host / UI feature disabled | Retain projects/layouts; no unexpected queued mutation or target conversion | HUMAN: native surface not exercised |
| UI downgrade with v2 backup | Writers stopped, v3 preserved, old parser never rewrites v3 | HUMAN: schema rollback not executed |
| Owned daemon rollback while unrelated daemon runs | Retire only after session and request drain; unrelated process/data unchanged | Request-drain seam passes; live-session/process proof HUMAN |

Sign-off remains withheld until each HUMAN row has attached results and A23 plus required native/Linux evidence is complete.
