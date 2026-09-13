# R9 eager-upload actual-wire diagnosis

Task `st_01a09812`, 2026-09-13, Darwin arm64. Scope: private owner/router and diagnostic client only. **No production transport fix or whole-plan acceptance is claimed.**

## Disposition

The eager-upload failure is real, remains reproducible with the current production router, and **does not require connection pooling**. This packet adds the previously missing fresh-client reset receipt. It also records a reset on a demonstrably reused connection. Successful server response writes and successful client HTTP receipt are different facts: in every observed reqwest failure the server had already written the complete correct rejection, but reqwest returned a body-write transport error instead of an HTTP response.

No incorrect authorization, oversized-body acceptance, malformed rejection envelope, or missing no-store header was demonstrated in this scope. All nine staged cases in each execution received the correct complete HTTP response without a read error. The 2,097,153-byte eager body is outside the approved 65,536-byte machine JSON input limit. The inspected plan does not require the server to finish receiving such an upload, unboundedly drain it, or guarantee HTTP response delivery despite a concurrent socket failure. Therefore this is **a demonstrated eager-client delivery limitation of early rejection, not evidence authorizing a blanket server-drain repair**. The stronger claim "every eager upload receives 401/403/413" is false in this environment and must not be represented as satisfied, even for the 65,537-byte unauthorized raw cases.

Recommendation: retain the bounded early-rejection implementation and the staged test strategy. Preserve transport errors as transport errors; do not retry mutations or relabel resets as HTTP rejection. No nonconflicting production repair is requested for R9 on this evidence. A product decision to guarantee eager invalid-body response delivery would be a new, explicitly bounded HTTP lifecycle/client interoperability scope, not the closed deterministic-test repair.

## Approved contract and requirement classification

Authority read: `/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md`, D04 (133-141), error contract (299-315), security/resource bounds (441-449), and A14 acceptance at 635. Historical reconciliation: `WAVE1-resume-acceptance-gaps.md` item 4/Q8. Historical R3 artifacts were read only in `herdr-wave1`.

| Requirement | Classification and evidence |
| --- | --- |
| Owner-issued machine scope plus Control; mirror cannot elevate itself | **Proven for fixtures.** Both grants are issued through the actual owner's pairing action and exchanged normally; returned scope and Control permission are checked. No forged bearer, mutable persisted grant, or body-injection middleware. Full pairing-product acceptance is outside this probe. |
| Authorization before domain work; unauthorized body must not be required for refusal | **Proven at the relevant HTTP boundary.** Mirror/anonymous staged requests declare each original body length and send zero body bytes; complete 403/401 returns. Source `workspace_api::admit_until` authorizes before project body extraction. This is not a new exhaustive revocation/filesystem side-effect audit. |
| Machine JSON input capped at 64 KiB; bounded-body failure maps to 413 | **Proven for the exercised project POST/DELETE routes.** Full 65,537-byte eager machine POST without early FIN returns 413. Staged machine requests send exactly 65,537 bytes with original Content-Length (including 2,097,153), then read 413. No full 2 MiB server ingestion is claimed or required. |
| Exact status, structured error, nonretryable, UUID requestId, no-store | **Proven for all nine staged wire cases and all delivered reqwest responses.** Validators check status/code, Content-Length framing on raw responses, no-store, retryable=false, UUID, string message, object details. Expected codes: `PAYLOAD_TOO_LARGE`, `MACHINE_ACCESS_REQUIRED`, `UNAUTHORIZED`. No-store is the inherited R9 boundary requirement; plan line 315 supplies the status mapping. |
| Every eager invalid upload receives an HTTP response | **Violated as a stronger transport proposition, not an explicit approved complete-upload guarantee.** Raw read resets, body-write errors, and one zero-byte early-FIN response are retained. They are never counted as receiving 401/403/413. Correct server writes do not override client failure. |
| Pooling is required for the reset | **Disproved.** First wire execution has fresh-client DELETE mirror `BodyWrite/ConnectionReset`; final execution has a reused-connection POST mirror reset. Pool reuse is established by connection IDs, not inferred from client configuration. |
| Pooling causes or fixes the failure | **Unproven; not supported by these outcomes.** First execution: fresh 7/9 valid, pooled 9/9. Final: fresh 8/9 valid, pooled 8/9. These are diagnostic observations, not controlled statistical rates. |
| Reset occurs before the server emits rejection bytes | **Disproved for observed reqwest failures.** Complete successful server write precedes each failure. For raw resets, client read buffers also contain the complete rejection before the read error. Exact kernel packet/ACK ordering remains unverified. |
| Production native client handling, valid-body requests under arbitrary timing, relay/TLS, Linux/Windows | **Genuinely unverified here.** This is direct loopback HTTP/1.1 using reqwest 0.12.28, axum 0.8.9, hyper 1.11.1 from the locked manifest. It is not A14, relay, platform, or release acceptance. |

## Reproducer and observation method

Owned artifact: `src-tauri/examples/r9_eager_upload_probe.rs`, auto-discovered by Cargo without manifest/module/dependency changes. Run from this worktree root:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUSTC_WRAPPER= \
CARGO_TARGET_DIR="$PWD/src-tauri/target" \
cargo run --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --example r9_eager_upload_probe
```

The executable is a bounded diagnostic, not a green/red transport acceptance test. **Exit 0 means diagnostic completion and cleanup only.** It emits all observed errors and continues the fixed matrix, never retries a failed request. Exact failing cells may vary because the race itself is the subject of investigation; no assertion requires timing luck or a particular reset count. The two executions were a first diagnosis and a distinct FIN-control expansion, not retry-until-pass runs.

Before any library object is constructed, a supervisor execs its private child with HOME, runtime, data, sessions, XDG config/data/cache, TMPDIR and cwd beneath an owned `src-tauri/target/r9-private-*` root. The child verifies these prefixes before constructing the runtime/owner. The owner is `DaemonServer::new_with_paths` with real shared machine services. Its private config mode is set to LocalNetwork **without starting the gateway** to avoid the pairing command's Off-to-public-relay auto-start branch. Owner pairing commands run over a bounded in-memory duplex connection, are joined, and exchange the resulting PINs normally. Only the manually bound port-0 loopback listener serves HTTP. No daemon discovery, UDS listener, relay connection, desktop, PTY or Git command is used by the probe.

The only listener adaptation wraps the actual TCP stream's AsyncRead/AsyncWrite methods. It delegates scalar and vectored socket writes unchanged, counts inbound reads without copying request headers or bodies, and records successful response bytes, socket errors, shutdown, drop, and connection identity. It does not replace requests, authorize, synthesize responses, proxy, or drain. Events are accumulated in bounded memory and printed after the runtime joins; no live logging IO is inserted into the transport. A mutex adds observation overhead, so these are not timing-performance measurements.

Raw TCP clients simultaneously write and read using `tokio::join!`. Full eager source buffers contain exactly 65,537 or 2,097,153 bytes, not a declared length with an empty injected body. Successful client `write` counts mean accepted by the local socket, **not bytes consumed by the server**. Larger eager attempts are interrupted by refusal: final raw 2 MiB cases accepted only 556,120-1,194,023 bytes locally before write failure. No artifact mislabels this as complete 2 MiB transmission.

The final fixed 45-case matrix is three route/size combinations times three grants times five modes:

- POST `/api/v1/workspace/projects`, 65,537 bytes.
- POST `/api/v1/workspace/projects`, 2,097,153 bytes.
- DELETE `/api/v1/workspace/projects/missing`, 2,097,153 bytes.
- Grants: machine-Control, mirror-Control, anonymous.
- `raw-eager`: full upload attempt, leave write side open until response/end of concurrent attempt.
- `raw-eager-fin`: full upload attempt, half-close immediately after successful full write; if write fails first, no successful FIN is claimed.
- `raw-staged`: machine sends limit+1; unauthorized sends zero; all declare original length and half-close only after response.
- `reqwest-fresh`: new client, no idle pool, no proxy, no redirects, HTTP/1 only.
- `reqwest-pooled`: new client with pooling; await complete health response, then issue one eager mutation on that origin. The health completion is the event boundary. Every final pooled case actually reused its prime connection.

Raw reads retain response bytes even when the next read resets. `complete_valid_http_bytes=true` is a byte/framing fact only. `received_http_without_read_error=false` and `transport_and_http_valid=false` preserve the failure. A full response followed by a reset is **not** accepted as a successful request. Raw response capture stops at EOF/error with an 8 KiB bound. IO deadlines are 10 seconds, scenario 120 seconds, child supervisor 150 seconds, and external Cargo monitor 600 seconds. No fixed sleep, polling delay, mutation retry, or server drain is used.

## Actual results

Complete raw outcomes are retained in the logs; JSON strings preserve CRLF and all error messages without request secrets. These are application socket-boundary wire observations, not a privileged kernel packet capture.

### Final matrix (`R9-eager-upload-resume-final.log`)

`R` means read reset after complete correct response bytes; `W` means upload failed; neither is success. `EOF0` means no response bytes. Plain statuses mean complete response and successful raw upload where applicable.

| Route/size/grant | raw eager | raw eager FIN | staged | reqwest fresh | reqwest reused |
| --- | --- | --- | --- | --- | --- |
| POST 65,537 machine | 413 | EOF0 | 413 | 413 | 413 |
| POST 65,537 mirror | R (403 bytes) | 403 | 403 | 403 | 403 |
| POST 65,537 anonymous | R (401 bytes) | 401 | 401 | 401 | 401 |
| POST 2,097,153 machine | W (413 received) | R+W (413 bytes) | 413 | 413 | 413 |
| POST 2,097,153 mirror | R+W (403 bytes) | R+W (403 bytes) | 403 | 403 | BodyWrite/reset54 |
| POST 2,097,153 anonymous | R+W (401 bytes) | R+W (401 bytes) | 401 | BodyWrite/brokenPipe32 | 401 |
| DELETE 2,097,153 machine | R+W (413 bytes) | R+W (413 bytes) | 413 | 413 | 413 |
| DELETE 2,097,153 mirror | R+W (403 bytes) | R+W (403 bytes) | 403 | 403 | 403 |
| DELETE 2,097,153 anonymous | R+W (401 bytes) | R+W (401 bytes) | 401 | 401 | 401 |

All 9 staged cases valid; 16/18 reqwest responses valid, 2 transport failures. Final listener accepted/dropped 45/45 sockets. Pooled connection IDs 5,10,15,20,25,30,35,40,45 each wrote both the health 200 and the intended rejection on the same socket. Thus the failing connection 25 was actually reused, not just pool-enabled.

### Exact causal boundary receipts

First execution (`R9-eager-upload-resume-wire.log`), fresh DELETE mirror, connection 31:

- seq 441: new server accept; no preceding request on that connection.
- seq 444: `server_write Ok(335)`, complete `HTTP/1.1 403 Forbidden`, no-store, Content-Length 162, structured MACHINE_ACCESS_REQUIRED.
- seq 445: further server read; seq 446/447: successful shutdown and socket drop.
- seq 448: client failure, verbatim:

```text
reqwest::Error { kind: Request, url: "http://127.0.0.1:49311/api/v1/workspace/projects/missing", source: hyper_util::client::legacy::Error(SendRequest, hyper::Error(BodyWrite, Os { code: 54, kind: ConnectionReset, message: "Connection reset by peer" })) }
```

Final execution, reused POST mirror, connection 25: health write seq 368, prime completion 369, complete rejection write 371, successful shutdown 373, drop 374, client error 375:

```text
reqwest::Error { kind: Request, url: "http://127.0.0.1:49599/api/v1/workspace/projects", source: hyper_util::client::legacy::Error(SendRequest, hyper::Error(BodyWrite, Os { code: 54, kind: ConnectionReset, message: "Connection reset by peer" })) }
```

Final fresh anonymous POST, connection 29: complete 401 write seq 427, shutdown/drop 429/430, client failure 431:

```text
reqwest::Error { kind: Request, url: "http://127.0.0.1:49599/api/v1/workspace/projects", source: hyper_util::client::legacy::Error(SendRequest, hyper::Error(BodyWrite, Os { code: 32, kind: BrokenPipe, message: "Broken pipe" })) }
```

The first execution also had the same fresh anonymous POST broken-pipe failure (port 49311, seq 325), preserved verbatim in its log. The first version's `raw-eager` mode used immediate post-upload FIN; in the final source that exact boundary is named `raw-eager-fin`, and the keep-open eager mode is added. No production source changed between these modes.

## Mechanism: observed versus inferred

Source chain inspected: `register_project_boundary` / `unregister_project_boundary` -> `workspace_api::admit` / `admit_until` -> `project_body` -> Axum Bytes extraction under `DefaultBodyLimit::max(MACHINE_JSON_MAX_BYTES)`. Authorization failure drops the unconsumed request; limit failure drops the remaining body and constructs `machine_error`. The latter emits the exact status, no-store and ErrorEnvelope. These are ordinary production paths, not test middleware.

Locked hyper 1.11.1 `proto/h1/dispatch.rs:225-285` observes dropped body receivers and calls `poll_drain_or_close_read`; `proto/h1/conn.rs:858-880` attempts a cheap read then closes if the body is still incomplete. `dispatch.rs:450-454` maps write/flush IO failure to BodyWrite. This source plus observed successful rejection writes, incomplete upload counts, socket shutdown/drop, and subsequent client write errors is consistent with early response/close racing an ongoing eager upload. No speculative malformed-response or authentication failure is needed to explain the observations.

**Not proved:** the exact Darwin kernel path that creates each RST, TCP segment/ACK ordering, whether the reqwest parser had already consumed the response before surfacing BodyWrite, or the probability under an uninstrumented listener. Server `poll_write Ok(n)` establishes local socket acceptance, not remote parser receipt. Raw reset cases independently show client-side complete response bytes before error, but that cannot be substituted for a reqwest response.

The machine 65,537 immediate-FIN case produces zero server response writes and zero client bytes in both executions. The final keep-open equivalent returns 413. Hyper's HTTP/1 builder documents `half_close=false` by default (`server/conn/http1.rs:258-266`). This is a separate EOF/half-close sensitivity consistent with that default, not the same as a 2 MiB BodyWrite reset; internal task cancellation was not instrumented. No half-close support guarantee was found in the approved plan.

## Verification, unrelated failure, and cleanup

- LSP before initial compile reported unlinked-file (tooling limitation). After Cargo discovery, both subsequent Rust diagnostics calls returned **no diagnostics**. No settings were changed.
- Initial Cargo attempt exited 101 because the example tried to call crate-private `AuthManager::create_scoped_pairing_code` (`E0624`). Full `R9-eager-upload-resume-run.log` retained. Fixed only the owned example to use the public owner protocol action; no production visibility change or auth bypass.
- First wire and final expanded Cargo run/build both exited 0, with inherited library warnings preserved. Probe emitted transport failures despite successful diagnostic exit.
- Related test ran once with the same locked/no-default-features build environment: `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote::server::tests::r3_http_boundary_contract -- --exact --nocapture`. **Exit 101, 0 passed / 1 failed**, full output in `R9-eager-upload-resume-related-test.log`. Its 24 wire/18 injected-body checks and six classification checks completed, then the legacy `/api/v1/sessions` assertion failed at `server.rs:2531`:

```text
assertion `left == right` failed
  left: "{\"error\":{\"code\":\"UNAUTHORIZED\",\"message\":\"UNAUTHORIZED\",\"retryable\":false,\"requestId\":\"8e37b79f-8dd8-42e5-999e-49078ee8ef92\",\"details\":{}}}"
 right: "Missing auth token"
```

  This is outside the example's compilation path: A09-owned `list_sessions` now authenticates through `authenticate_machine_request`, whose missing-token failure is structured; the inherited assertion still expects legacy text. Parent/A09 owns reconciliation of this exact router/test contract. No existing test was edited, skipped or retried. The related test uses its own existing TempDir and reports cleanup; unlike the diagnostic child it was not exec-isolated with private HOME/runtime environment, so its run is not claimed as an isolation proof. That verification-command isolation omission is retained explicitly rather than covered by the probe's cleanup claim.

- `R9-eager-upload-resume-source.json` records matching before/after hashes across the final run/test for the probe, server/router, workspace API, auth, daemon server and Cargo.lock. This is a bounded identity receipt, not a claim about every concurrently owned file.
- First probe child PID 98064 and final child PID 1467 were waited/reaped with exit 0. Both emit graceful listener joined, connection refused, runtime joined, and root removed. First 36/36 and final 45/45 accepted sockets have socket-drop receipts. No PTY was spawned. A post-run process check found none of those child/Cargo PIDs; no `r9-private-*` target root remained. Related test also logs `listener_joined=true connection_refused=true private_root_removed=true no_pty=true` after its assertion failure.

Owned files are only this report, the new example, and `R9-eager-upload-resume-*` diagnostic evidence. No production source, dependency, existing test, UI, other worktree or global config was edited. No commit, release, deployment or desktop action occurred. R9's eager-delivery uncertainty is now narrowed to a demonstrated client/transport limitation with concrete wire boundaries; broader acceptance remains with the parent.
