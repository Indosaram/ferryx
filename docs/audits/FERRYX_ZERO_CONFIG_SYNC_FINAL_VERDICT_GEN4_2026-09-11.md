# Ferryx zero-config relay sync — Gen4 final verdict

Date: 2026-09-11  
Scope: `8e25c0f9-7c81-41b6-bac7-4cea47133d9a`  
Requested revision: `050e8a6572a47af39179b258e63ee4a964b9667e`  
Branch: `remote-connectivity`  
Later externally supplied revision observed and exercised: `35e237ab6572d323f3f047eab2b3d64fd9964e3d`

## VERDICT: REQUEST CHANGES — NOT APPROVED

**The relay-generation repair and claim-to-dispatch binding are credited. The single daemon-owned pairing condition is not fully closed.** The new integration loses a requested View permission, treats an explicit daemon refusal as permission to start a competing relay owner, and retains a dead coordinator after relay shutdown. These are concrete findings in the new integration, not a demand to finish the already-disclosed direct-WebSocket, persistence, audience or physical-machine test work.

The relevant changes in `93dc006`, `b335d9c`, `fcd0f48`, and `050e8a6` were reviewed in that order against the Gen3 report. Three new opt-in observations independently reproduced the integration defects. The CLI observation invokes the actual built executable; relay, reverse client, gateway and daemon request handling use production code. No production endpoint, identity, token or terminal was used.

## Disposition of the three Gen3 conditions

| Condition | Gen4 disposition |
| --- | --- |
| Separate client attempts from relay-owned generations | **Closed for the reported defect.** `RegisteredPairing.control_generation` is mandatory and relay-stamped. Claim uses it, and idempotent registration requires the same authenticated control generation. |
| Preserve the validated generation through claim-to-dispatch | **Closed for the reported boundary.** The claim returns its generation; pairing exchange passes `Some(control_generation)` through `proxy_http`; `open_session_channel` rejects a mismatch with 410 while holding the channel lock. |
| Route real GUI/CLI pairing through one retained daemon authority | **Partially implemented, not closed.** The happy path now uses the daemon coordinator. The reproduced permission, error-fallback and stopped-coordinator cases below remain. |

Source anchors for the generation repairs are `src-tauri/src/remote/relay_server.rs:109–119`, `:627–633`, `:652–658`, `:714–733`, `:760–793`, and `:1149–1164`. Non-pairing paths pass `None`. The permanent socket regressions independently passed **2/2**. Their 409 responder proves successful relay forwarding, not actual gateway token issuance; the separate View-permission observation below does exercise actual token issuance.

The Windows auth-path source correction is also credited: `src-tauri/src/remote/auth.rs:71–72` uses `canonical_remote_dir()`. This review did not run Windows. The new literal-path test is useful resolver coverage, but its helper calls `resolve_canonical_remote_dir` directly rather than invoking `canonical_auth_path`; it does not independently mutation-test that production delegation. That test limitation is not a claim that the source repair is broken.

## B1 — High: a requested View pairing issues a Control bearer

**Exact primary location:** `src-tauri/src/daemon/server.rs:1893–1901`, specifically the relay call at **1901**.  
**Permission hard-code:** `src-tauri/src/remote/auth.rs:403`.  
These files/locations are unchanged between the requested commit and the later revision observed in this review.

The daemon computes `perm` from `RemoteCreatePairingCode.permission`, but only passes it to the local fallback. In the relay branch it invokes `generate_pairing(Duration::from_secs(60))`, whose capability registration has no permission argument. `register_pairing_capability` stores `default_permission: DevicePermission::Control`. `exchange_pairing_code` subsequently copies that permission into the issued device.

### Concrete authorization failure

A caller requests `Some(DevicePermission::View)` through the supported daemon/GUI IPC API and shares the resulting PIN with a recipient intended to be view-only. Redeeming it through the public relay returns a live **Control** device and bearer. The recipient receives stronger privileges than the pairing request authorized. The gateway uses that permission for terminal input admission at `src-tauri/src/remote/server.rs:1195–1212`.

Independent observation:

```text
real DaemonServer View request
  -> relay-ACKed PIN
  -> real public relay exchange
  -> real reverse client and gateway
  -> HTTP 200, device.permission = control
  -> issued bearer validates as DevicePermission::Control
```

The positive control invokes the same actual daemon handler's local branch with View and obtains a View device. The observation does not merely inspect a synthetic ACK or trust a client-displayed label.

**UI boundary:** `ui/src/lib/tauri.ts:755–756` and `src-tauri/src/ipc/remote.rs:247–264` support/forward View requests. The current Settings button at `ui/src/components/settings/RemoteAccessSection.tsx:176` explicitly requests `control`; this report does **not** claim that the current screen exposes a view-only selector or that every ordinary Settings click escalates privilege. The reproduced defect is the supported API's permission contract.

**Required correction:** carry the requested permission into coordinator capability creation and the single-use auth record, or explicitly reject unsupported View requests. Never silently promote View to Control. Add actual daemon-IPC-to-gateway exchange regressions for both permissions.

## B2 — High release/availability blocker: an explicit daemon error starts a competing owner

**Exact requested-commit location:** `src-tauri/src/main.rs:250` (`.ok()`), leading to standalone construction at `:277–286`. `git blame` against `050e8a6` was used to pin the line reference.  
**Later correction credited:** external `35e237a` adds a socket-existence check but retains the error-erasing `.ok()` and the same standalone branch.

`remote_create_pairing_code` returns errors for explicit daemon responses as well as transport failures. Converting the entire result to `Option` does not distinguish “no daemon answered” from “the daemon answered and refused this request.”

### Concrete ordinary failure, independently reproduced

1. The daemon's existing relay connection registers a first PIN and its coordinator is Ready.
2. Invoke the actual CLI with the supported syntax `ferryx pair --generate-pin` (the parser also accepts `ferryx pair generate`).
3. The real daemon request handler returns `Invalid pairing transition: Ready -> Registering` for the second request.
4. The CLI discards that explicit response, prints `No running daemon answered; pairing standalone from this process instead.`, authenticates another relay control connection with the same machine identity, and registers another PIN.
5. Redeeming the daemon's previously Ready PIN now returns **404**, because the CLI replaced its authenticated control generation.

The observation waits for the CLI to print a newly relay-registered PIN before checking the original PIN. It does not infer replacement merely from a missing message or timeout. A synthetic IPC handshake supplies compatible metadata to avoid unrelated binary-upgrade side effects; the actual CLI request is forwarded to the real `DaemonServer` handler and its real rejection is returned. The test establishes control-owner replacement and invalidation, not successful redemption through the standalone CLI's gateway.

This directly violates the original single-authority acceptance condition. No foreign machine key, guessed PIN, traffic flood or victim impersonation is needed; an ordinary second pairing request is enough.

At `050e8a6`, `DaemonClient` could additionally spawn a daemon on demand, so it was not a pure “running daemon” probe. The later socket-existence check narrows that behavior and is credited. It is not evidence that a socket corresponds to a responding relay authority, and it does not repair the reproduced explicit-error case.

**Required correction:** preserve the daemon result. An explicit daemon rejection must remain an error, not trigger standalone ownership. Separate successful daemon pairing, daemon refusal, unavailable daemon, and uncertain transport outcome. Restrict standalone fallback to the supported genuinely-unowned case; do not weaken relay owner/generation checks to hide the competition. Add a subprocess regression proving that a second request/refusal does not create a second relay owner or invalidate the first PIN.

## B3 — Medium: stopping relay leaves a dead coordinator selected for local pairing

**Exact locations:**

- `src-tauri/src/remote/server.rs:2054` publishes the coordinator into shared state.
- `src-tauri/src/remote/server.rs:1887–1895` stops/aborts the relay but does not clear that shared coordinator.
- `src-tauri/src/daemon/server.rs:2235–2246` transitions to Off without clearing it.
- `src-tauri/src/daemon/server.rs:1897–1909` selects the coordinator solely because the option remains `Some`.

### Concrete availability failure, independently reproduced

Start relay mode with the production gateway starter, stop its handle, execute the daemon's actual Off configuration transition, then request pairing through daemon IPC. The request goes to the old coordinator and returns **`Relay registration channel closed`**, rather than the previously supported local pairing response. Clearing **only** the stale coordinator makes the same View pairing request succeed and issue a View device.

The fixture uses an ephemeral loopback gateway port instead of the workstation's fixed production port. It invokes the real gateway stop operation and real daemon Off transition. No browser click sequence or physical network interface transition is claimed as executed. Source inspection shows that starting a non-relay listener also does not replace or clear the stale option, so selecting local/Tailscale mode does not itself repair this retained state.

**Required correction:** manage publication and removal with the owning gateway/relay lifecycle, including stop and failed startup, and prevent an old handle's cleanup from clearing a newer owner. The local/relay decision must reflect current active configuration/ownership, not just a leftover `Option`. Add stop/disable and relay-to-non-relay pairing tests.

## Verification and test-quality boundaries

| Independently run check | Observed result |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::` | Exit 0. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test relay_pairing_generation_regression -- --test-threads=1` | **2 passed, 0 failed**; exit 0. |
| `cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_gen4_audit -- --ignored --nocapture --test-threads=1` | **3 passed, 0 failed**; all three are defect-presence observations, not approval checks. |
| `bun run --cwd ui test src/remote/` | Exit 0. |
| `bun run --cwd ui build` | Exit 0. |

The committed dispatch test exercises `open_session_channel` directly with a stale generation and has a matching-generation wait control. Its source is non-vacuous with respect to the dispatch guard. The submitter's historical red-before-green mutations were not independently repeated here; their disclosure is not substituted for this review's executed evidence.

New audit source: `src-tauri/tests/zero_config_gen4_audit.rs`. It is explicitly opt-in and labels every `observes_*` test as a defect observation. Run serially because the fixture temporarily overrides environment variables. All identities, stores, sockets and network endpoints are isolated inside temporary workspace directories/loopback. The actual CLI child is killed and reaped after the observation.

The first two audit invocations used the unsupported CLI spelling `pair generate-pin`; the CLI probe therefore timed out while the other two observations passed. The harness was corrected to `pair --generate-pin`, after which all three observations passed. Those initial timeouts are **not** product-defect evidence. Builds also encountered shared build-directory contention; no unrelated process was killed.

Representative successful evidence IDs: generation regressions `02225ff5-02a0-4d3e-9ffc-f9a148a0cfe4`; Gen4 observations `1ffd1065-4973-4d7d-9453-8c0e14cebe90`; remote Rust suite `651443ee-1d35-4074-9d7f-8888a01f3f16`; UI suite `048af7ed-8087-4e60-bd62-c7b6c9ffc0ef`; UI build `fbed4271-f07c-4a21-af37-17bb79abf348`. The structured completion result records the final revision-fresh verification after this report was written.

### Revision provenance

The worktree initially matched HEAD `050e8a6` but contained **89 pre-existing modified files**. Those changes were preserved. The worktree subsequently advanced externally through `c96e6ff` to `35e237a`; this reviewer made neither commit. The pinned CLI reference above comes from committed `050e8a6`. The primary daemon/server/auth files involved in B1 and B3 were unchanged; the newer CLI socket guard leaves B2's explicit-error behavior intact. Runtime observations were performed in the supplied mutable worktree, not represented as tests of a clean immutable `050e8a6` checkout.

Only this Gen4 report and the new opt-in audit test file were authored by this reviewer. The existing Gen3 report was not overwritten. No product fix, commit, deployment, Windows run, real browser-engine run, physical two-machine run, or real-user terminal operation was performed.

The disclosed direct-WebSocket query credentials, cross-process persistence/permission/fsync limits, constant signing audience and limited F10 lifecycle coverage remain explicit limitations. They are not relabeled as new reproduced exploits or used as moving approval gates in this round. The reported SSH/sshd failures were outside the focused commands and were not independently repeated.

## Final disposition

**Approve the closure of the Gen3 counter-mix-up and claim-to-dispatch findings; do not approve the complete zero-config relay pairing integration at `050e8a6`, or infer that `35e237a` closes it.** Repair B1–B3 and verify the actual daemon permission, CLI-refusal and stopped-owner paths. The present release hold is supported by concrete authorization and availability failures, not by the absence of a browser engine or two physical machines in F10.
