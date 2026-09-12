# A04 independent verification - generation 5 final candidate

## Verdict: ACCEPTED for the scoped independent A04 review; parent final acceptance required

Reviewed 2026-09-12, checkpoint `2026-09-12T07:45:07Z`, on Darwin arm64 in `/Users/indo/code/project/orca-lite-wt/herdr-wave1`, HEAD `a2534ff4e125a7577a9dc1dee6cb598b3cf94c9a`. No criterion-blocking finding remains for the original A04 backend extraction. This is not approval of subsequent packets, Windows/native features, or the complete multi-host plan.

The current producer reports completed parent checks, not a blocked request. I independently ran the exact focused real IPC/HTTP fixture once on the final source and observed the original PTY PID/CWD, shared authority, successful HTTP identity correlation and teardown. The generation-4 rejection below is historical: its visibility defect is corrected and its missing execution evidence now exists. `A04-parent-green-result.json`, not `A04-green-command-request.json`, is the current execution receipt.

## Source and evidence identity

These eight SHA-256 values were independently measured before execution and compared again after execution; all match the parent manifest. Paths are relative to `src-tauri/src/`.

| Source | SHA-256 |
| --- | --- |
| `daemon/server.rs` | `5789ef53877df3b97107e1dd0293342de1e595ab7dfacf42095312d9c9b27c91` |
| `daemon/mod.rs` | `f62cac476e47a45f3a6dce95311d194820fd59f15411fe8a18ce54df5328812b` |
| `daemon/workspace_service.rs` | `714a8e4dbceec57aba00aad01bcdc5a23bcaf41284f0b88c75cfa60797930af6` |
| `daemon/session_service.rs` | `1bbd593f77fc575c5b2a717913a30d4db3631df02956ce1e2eb89980c024ac7c` |
| `daemon/a04_shared_services_tests.rs` | `57ed927b9c2fafa157e5f3123fa886b3758b98dc9794be8d37a4b880c67031ba` |
| `remote/state.rs` | `15f3fffbc551b035beb522c7d3db314406cefb9a1f3bbdcb0fd714b4f9aa7af4` |
| `remote/backend.rs` | `9e4c9049421a7cfdcaf0d539c42af5857abc8dd8a768e21fbec3526c34fcd71c` |
| `daemon/remote_ssh_gateway_qa.rs` | `689858c905782aaaead195016d1b38140a6bd673f4bf6bb5cfc5260eae521abc` |

Additional measured identities:

- Full approved plan: `0ad21b3583024a2c920b2bf328ece1dca4c7116b1e577e37dd5c7fdb2e172a2a`.
- `A04-parent-green-result.json`: `9fd7f9b05bf742c153acd3207d0e7700947fdb647d429ee2b6354cdc5c3b78d1`.
- `A04-implementation.md`: `d1f7651984c967ae48a136b8bda9bd3a9d0ec89dd464c97403e76fcf1f8ba5f6`.
- Previous report preserved below: `0ce949442b72aa8ae01ce508cecbf7f31f83a26851b15441d4617ca063a3cebc`.

Chronology qualification: parent focused4 and remote212 ran at server hash `c35dcaaefa147415733fdeb9e7196fc27e31fc3b4ac0472392edf317b83941b5`, before removal of an unused import. They are not falsely labeled runs on the final eight-file manifest. The final independent focused run below closes the original runtime identity requirement on current hashes. The parent's later local34, SSH4, handover3, real SSH1 and headless binary exit0 receipts supersede historical pending notes. No new unrelated suite or mutation was run here.

## Original requirement disposition

| Requirement | Evidence and disposition |
| --- | --- |
| One set of shared handles for IPC/HTTP | PASS. `server.rs:797-916` constructs one PTY manager, terminal service, router and registry. `state.rs:322-331` derives the gateway registry and backend from the session authority before publication. Fixture `:58-64` asserts session/workspace/backend Arc identity; actual HTTP returned the original IPC session/workspace in the independent run. Equal revisions alone are not treated as identity proof. |
| Move domain logic, not duplicate or unused wrappers | PASS. Complete tracked diff removes spawn/cache/claim/metadata/canonicalization implementations from server and moves them into the two services. Server spawn `:924-936` and registration `:1913-1914` are adapters; Deref preserves moved internal callers. `session_service.rs:714` is the single daemon local `spawn_in_worktree` call. HTTP backend `backend.rs:80-112` actually uses the authority, delegates transports to the original router and projects authoritative metadata. Existing baseline handle sharing was not itself the missing extraction. |
| Registration mutation gate and root/cwd admission | PASS for current daemon writers. Workspace service `:17-48` serializes registration/unregistration and retains absolute/canonical/project-root checks. Session service `:462-501,638-719` serializes unregister with spawn and resolves the registered target under the workspace gate; canonical custom cwd must satisfy both manager boundary and resolved worktree containment. The independent IPC scenario rejected unregistered spawn and parent-root escape, then successfully spawned in the registered child. Desktop `ipc/project.rs:177-218,271-292` mutates its separate UI registry and sends daemon IPC; it is not a bypass of the daemon gate. Raw registry handles remain available to trusted code; type-enforced exclusivity is not claimed. |
| Service-less constructors fail closed | PASS at the present capability-gated boundary. All six gateway constructor paths converge on `new_with_paths_backend`, setting `machine_services: None`; only daemon installation supplies services. Parent focused construction test passed. `remote/server.rs:1980-2027` advertises empty capabilities and authenticates the unavailable machine routes before 503 `MACHINE_SERVICE_UNAVAILABLE` (403 for mirror). Parent remote212 includes `a03_absent_machine_service_is_private_and_unavailable`, whose actual response is decoded and checked for 503/no-store/typed error at `:2347-2364`. That test calls the handler directly, not TCP; independent TCP execution here covers legacy GET sessions, not a new machine CRUD API. The stubs also remain unavailable with services installed, deliberately pending later packets. |
| No strong authority ownership cycle | PASS for the construction graph. Gateway owns services; service has no server/gateway reference and uses Weak handover (`session_service.rs:148-162`). This breaks the handover callback -> gateway -> service -> handover cycle visible at `server.rs:885-903`. Parent focused weak-upgrade drop test passed for service, workspace, gateway, handover and terminal. Live task lifetime is not equated with that empty-construction test. |
| Headless without AppHandle | PASS. Service fields and fixture construction require no AppHandle. The independent child ran real UDS handling, Axum HTTP/auth and a PTY without creating a desktop or starting canonical daemon entry points. Linked Tauri dependencies are not an AppHandle requirement. |
| Sync I/O threading | PASS for the moved authority. IPC registration and unregister filesystem work use `run_blocking`; local canonicalization, shell resolution, PTY creation and metadata/claim publication run in the blocking closure. The owned async spawn guard moves into that closure, so cancellation cannot release admission before ownership publication; no parking-lot gate crosses await. Resume probing now precedes registry resolution, but no machine spawn route is enabled. Constructor config/auth I/O and the direct synchronous registration adapter remain synchronous as before. SSH network/persistence boundaries are preserved. |
| Local/SSH/legacy handover semantics and callers | PASS within backend scope. `proxy.rs:585-849` retains SSH inventory validation, generation fencing, cursor forwarding and legacy-peer routing; the new backend forwards every transport operation to it. SSH startup/store/platform rejection, immutable request persistence and restore cursor reset moved intact. Service metadata overlays local/SSH details only when available, leaving legacy metadata intact. Unregister remains the existing explicit destructive operation, not future non-destructive A07 unregister. Parent local34, remote212, survival4, handover3 and real SSH1 passed. `server.rs:744` now unconditionally re-exports normalization for `ipc/agents.rs:319`; headless binaries check exit0 supports non-test caller visibility. |
| Real deterministic runtime and cleanup | PASS for normal runtime plus recorded failure paths. Exact independent evidence follows. No mocks replace UDS/TCP/auth/PTYs. Response-based negative assertions are not timing-only absence. Fixture cancellation/deadline channels are established before spawn and fired at the acknowledged-PTY seam; the 30-second timer is a deadline, not readiness polling. Parent focused4 includes six cleanup injections and original-child reap receipts. |

The parent-authorized eighth-file SSH QA extension is test-only. `remote_ssh_gateway_qa.rs:57-94` now reads advertised Connected generation and sends typed `RemoteWrite`, preserving the bounded actual-output assertion. The full diff changes no production SSH fence. The earlier raw-binary mismatch failure remains in `A04-extracted-real-ssh-failure.log`; parent attributes it through source comparison, not an unmodified-baseline runtime. The later real SSH success is accepted as runtime evidence, not erased by the earlier failed attempt.

## RED provenance and restoration

Read the actual logs and parent receipts, not just their requested commands:

- Split authority: `A04-red-shared-authority.log` exits101 with `HTTP cannot see original IPC session and workspace`. Owner83825/PTy83858 had valid IPC/OS cwd; successful HTTP200 returned `[]`, registry revisions1/0. This is a controlled second real authority, not a claim the original production baseline was split.
- Unregistered admission mutation: `A04-red-unregistered-g2.log` exits101 with `unregistered spawn admitted`; original PTY96743 and owner96723 reaped, root removed.
- Cwd admission mutation: `A04-red-cwd-g2.log` exits101 with `cwd escape admitted`; original PTY97881 and owner97839 reaped, root removed.
- `A04-parent-guard-red-result-g2.json` pins mutated server hashes `5b9f9f83bca75f3d31dc14e80043685ccacf9987885bc143390a3dce3cbd96aa` and `fe7e9d69b5c43555372b5d0e00b9c6e33b13e5f8e44e97047114aa4fbdef8f3a`, and original/restored hash `668e1710b738c5de019ec447c36bbcf075852f6256c0946d3a47f6838a190345`. Those are pre-extraction identities, not final source hashes. Current moved source contains both guards; independent GREEN exercised both rejection assertions. No mutation was introduced or rerun by this verifier.

## Exact independent runtime execution and observations

No monitor tool is exposed in this child. Used background bash with blocking `wait` and an explicit completion notification, not busy polling. Tool timeout180 seconds; run completed in approximately three seconds. Exact invocation from the worktree:

```sh
(env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::a04_shared_services_tests::a04_shared_authority_runtime -- --exact --nocapture & job=$!; printf 'A04 INDEPENDENT cargo_pid=%s\n' "$job"; wait "$job"; result=$?; printf 'A04 INDEPENDENT completed exit_code=%s\n' "$result"; exit "$result")
```

Cargo PID37405; warm build finished0.71s; executed `src-tauri/target/debug/deps/ferryx_lib-a8f661fd9c0ae1d6`. Exit0, one selected outer test and its same-selector child each passed, zero failures/ignored, 890 filtered out. Outer runtime1.63s, child1.61s. All command output was read. Compiler emitted16 existing warnings, unsuppressed: eight unnecessary unsafe blocks in `macos_file_drop.rs` (79,80,102,105,122,123,124,169); unused `token` at `remote/auth.rs:1399`; unused `scroll_daemon_client` at `lib.rs:915`; dead `app` at `ipc/notifications.rs:28`; unused constant at `native_terminal/sys/constants.rs:56`; unused `no_auth_query` at `remote/server.rs:2597`; and unused WriterLeaseGuard/items/acquire method in `worktree/manager.rs:81,88,333`. There was no compile/test error or new suppressed warning.

Runtime output, preserving the actual identity and cleanup values:

```text
A04 ROOT path=/private/tmp/a04-v5JoVL
A04 AUTHORITY session=0x10ad51340 workspace=0x10ad51030 backend=Pointer { addr: 0x10ad51340, metadata: DynMetadata(0x1076af1c8) }
A04 OWNED owner_pid=37427 session=579f6b4c-7b72-4048-aa6a-440d31cf3dfb pty_pid=37480 root=/private/tmp/a04-v5JoVL
A04 LIVE owner_pid=37427 pty_pid=37480 ipc=/private/tmp/a04-v5JoVL/fixture.sock http=127.0.0.1:64880 metadata={"session":{"cols":80,"cwd":"/private/tmp/a04-v5JoVL/project","endSequence":null,"rows":24,"running":true,"sessionId":"579f6b4c-7b72-4048-aa6a-440d31cf3dfb","startSequence":null,"workspaceId":"a04","worktree":null},"type":"describeSessionOk"} process_cwd=Some("/private/tmp/a04-v5JoVL/project")
A04 HTTP status=200 sessions=[{"running":true,"sessionId":"579f6b4c-7b72-4048-aa6a-440d31cf3dfb","title":null,"workspaceId":"a04","worktreeLabel":"project"}] registry_ipc=1 registry_http=1
A04 CLEANUP listener=IPC joined=true
A04 CLEANUP listener=HTTP joined=true
A04 CLEANUP session=579f6b4c-7b72-4048-aa6a-440d31cf3dfb pty_pid=Some(37480) reaped=true
A04 CLEANUP listeners_joined=true socket_removed=true closed=1 injection=None errors=[]
A04 CLEANUP child_pid=37427 reaped=true exit=exit status: 0
A04 CLEANUP private_root_removed=true
A04 INDEPENDENT completed exit_code=0
```

HTTP does not expose literal PID/CWD fields. Identity is established by its original session/workspace ID, retained original PTY handle, handshake owner PID and IPC plus OS CWD, not invented HTTP fields. The shell is fixture `/bin/sh`, working under its private project, not the desktop repository. Fixture sets private HOME/FERRYX_DATA_DIR/FERRYX_RUNTIME_DIR, removes relay credentials, uses `.no_proxy()` loopback HTTP and ephemeral port, and never calls canonical daemon startup.

Immediately after child/PTY reap and command completion, independent Python `os.kill(pid, 0)` returned ProcessLookupError for37405,37427,37480; `Path.exists()` returned false for the root and socket. Numeric cleanup: one owned PTY reaped, one owner child reaped, two listeners joined, zero cleanup errors, zero remaining reported processes, zero remaining root/socket. These checks supplement original-handle wait/reap, not replace it with elapsed-time absence.

## Parent execution receipts inspected, not rerun

| Artifact | Completed result |
| --- | --- |
| `A04-extracted-focused-green.log` | exit0,4 passed; construction/drop plus normal/six injected runtime paths |
| `A04-extracted-session-green.log` | exit0,34 passed |
| `A04-extracted-remote-green.log` | exit0,212 passed |
| `A04-extracted-ssh-survival-green.log` | exit0,4 passed |
| `A04-extracted-handover-green.log` | exit0,3 passed |
| `A04-extracted-bins-check.log` | exit0,CLI/relay headless check, not a test count |
| `A04-extracted-real-ssh-green.log` | exit0,1 outer actual isolated SSH test |

Parent normal runtime identifies owner14409, PTY14482, session `7a6bc87c-ada3-4715-adec-29d0ee9399c3`, cwd `/private/tmp/a04-gskjP3/project`, HTTP200 same session/workspace, both revisions1, both listeners joined and original PTY reaped. Its manifest records all13 focused reported PIDs and all7 private roots absent. Cleanup injection intentionally retains exactly one close error while reaping both owned PTYs; it is asserted, not swallowed. Real SSH receipt exposes helper31418 reaped and both `qa-new-tab`/`qa-split-restore` cwd `/private/tmp/fx99lxCL/remote project's space`; parent reports both roots absent. First successful SSH child's captured stdout is not printed, so its numeric PID is not independently visible; outer success proves its assertions, not a fabricated per-child PID receipt.

## Executed static checks and review boundaries

`git diff --check` exited0. Exact source hash check:

```sh
shasum -a 256 src-tauri/src/daemon/{server.rs,mod.rs,workspace_service.rs,session_service.rs,a04_shared_services_tests.rs,remote_ssh_gateway_qa.rs} src-tauri/src/remote/{state.rs,backend.rs}
git diff -- src-tauri/src/daemon/mod.rs src-tauri/src/daemon/server.rs src-tauri/src/daemon/remote_ssh_gateway_qa.rs src-tauri/src/remote/backend.rs src-tauri/src/remote/state.rs
rg -n 'RemoteGatewayState::new|DaemonServer::new|handle_register_workspace\(|handle_unregister_workspace\(|normalize_process_cwd|workspace_registry\.(register|unregister)|\.spawn_in_worktree\(|with_machine_services' src-tauri/src src-tauri/tests
```

Post-runtime exact cleanup/source recheck:

```python
import os,hashlib,json
from pathlib import Path
for pid in [37405,37427,37480]:
    try: os.kill(pid,0); print(f'pid={pid} absent=false')
    except ProcessLookupError: print(f'pid={pid} absent=true')
for p in ['/private/tmp/a04-v5JoVL','/private/tmp/a04-v5JoVL/fixture.sock']:
    print(f'path={p} absent={not Path(p).exists()}')
m=json.loads(Path('docs/evidence/paired-daemon/A04-parent-green-result.json').read_text())
for p,h in m['sourceSha256'].items():
    print(p,hashlib.sha256(Path(p).read_bytes()).hexdigest()==h)
```

All eight comparisons printedTrue. Additional read-only inventory: `pwd`, worktree `git status --short`, evidence `ls`, `wc -l` on all eight source files, `git rev-parse HEAD`, UTC date and evidence/plan hashes. Searches traced constructors/callers across source and integration tests, daemon registry/spawn sites, unavailable responses and router transport delegation. Direct reads covered full plan (including continuation after truncation), full new services/fixture/backend/mod/SSH QA, state constructor family and snapshot handling, server construction/adapters plus complete tracked extraction diff, proxy transport implementation, desktop registry/IPC separation, HTTP unavailable handler/test and the implementation/parent manifests/logs. All eight changed sources were inspected; this is not a claim every unchanged line in the3883-line server or1164-line state file was reread or every discovered caller test executed. No source-string assertion substitutes for the independently executed fixture.

## Cleanup, limits and end state

Only this report is edited; the previous report is retained verbatim below. No source mutations, commits, unrelated suites, production network writes or canonical daemon/desktop actions were performed. Cargo used only the explicitly authorized existing private warm target; fixture-created runtime files were removed by its owner. No owned resource remains requiring cleanup.

No original A04 blocker remains. Deliberate boundaries: no machine CRUD/catalog/directory/journal implementation is claimed; those capabilities remain off. Windows compilation/runtime, native desktop/default-feature acceptance and complete-plan release acceptance are not claimed. Empty-construction drop proof does not prove every live task's destruction. Cancellation injections target the acknowledged-PTY seam, not cancellation inside blocking spawn; external SIGKILL/deadlock cleanup is not guaranteed. These disclosed limits are not silently converted into missing later-packet obligations or invented runtime proof. Parent retains final acceptance authority.

---

# Historical generation-4 report (preserved verbatim)

# A04 independent verification - generation 4 production candidate

## Verdict: NOT ACCEPTED

Inspected 2026-09-12, UTC checkpoint `2026-09-12T07:08:53Z`, worktree `/Users/indo/code/project/orca-lite-wt/herdr-wave1`, HEAD `a2534ff4e125a7577a9dc1dee6cb598b3cf94c9a`. Parent final acceptance remains required. No later packet is approved.

The production extraction now exists and all seven current Rust hashes match `A04-green-command-request.json`. However, that artifact is a request, not a completed GREEN manifest: all four exit codes are null and all four requested extraction logs are absent. The producer explicitly reports pending execution. Under the generation-4 gate, no Cargo, alternate unit signal, or real fixture was run by this verifier. Historical prerequisite GREEN is not extraction GREEN. There is also a concrete non-test caller visibility regression and a gap in the requested regression selection, described below.

### Current-source findings

1. **Blocking caller visibility regression: non-test agent discovery still uses the old exported path.** `ipc/agents.rs:319`, inside the ordinary `antigravity_session_id` function, calls `crate::daemon::server::normalize_process_cwd`. `ipc/mod.rs:1` includes agents without a test gate. The extraction moved the function to `daemon/session_service.rs:20`; `server.rs:744-745` re-exports it as `pub(crate)` only under `#[cfg(test)]`. The non-test server's private `use super::session_service::*` is not a crate-visible re-export. Thus the existing non-test caller no longer has a visible function at its old path. This is a source-established visibility defect, not an observed compiler diagnostic (Cargo was not run). Preserve the crate-visible compatibility re-export in non-test builds, or migrate that caller to the new crate-visible definition. The requested library unit tests compile with `cfg(test)` and can conceal this defect; the requested binary check is essential.
2. **Requested regressions do not execute the moved SSH/handover blast radius.** The session command selects `daemon::server::tests::`, but `server.rs:2287-2292` declares `ssh_survival_tests` and `remote_ssh_tests` as sibling modules, not descendants of `tests`. The `remote::` selector does not select those daemon modules either. Moved SSH persistence/watch/restore and helper-home plumbing therefore get compiled in the lib test harness but not exercised by those two filters. Handover tests likewise are outside that selector. Parent evidence must distinguish these omissions from passing Local/session tests and use safe existing isolated coverage for the moved paths; do not run live SSH against a developer host. No Windows runtime or default/native build evidence is supplied.

No corrected generation-3 test defect is reopened here. Current fixture propagates IPC JoinSet failures, retains original PTY handles outside the cancellable scenario, asserts injected cleanup counts, reports listener success accurately, and runs emergency kill plus reap off-thread. The historical checkpoints below remain unchanged for provenance and are not current findings.

### Requirement disposition against the actual candidate

| A04 requirement | Current disposition |
| --- | --- |
| One shared authority backs IPC and HTTP | Static support. `DaemonServer::new_with_paths` creates one terminal service/router/registry, constructs the session/workspace authorities before publication, and installs the session authority as the HTTP backend through `with_machine_services`. That method derives both registry and workspace service from the same session authority rather than trusting unrelated inputs. Fixture asserts session/workspace/backend Arc identity. Current runtime proof is missing. |
| Move spawn/canonicalization/provider claims, not unused wrappers | Static support. Server diff removes those implementations and their state, retaining a spawn adapter and registration adapter; service owns request fingerprints/cache, metadata, claims, validated local spawn, close/cleanup and SSH persistence. HTTP backend delegates transport to the original router and adds authoritative metadata projection. This improves domain authority beyond the baseline's existing shared handles. There is one daemon `spawn_in_worktree` call, in the new session service; no new raw HTTP spawn or shell path exists. |
| All daemon registration/unregistration uses mutation gate | Static support for present production callers. IPC registration uses `run_blocking` and workspace `register`; unregister delegates through Deref to the session service, takes the spawn lock and calls gated workspace unregister off-thread. Local validated spawn holds the workspace gate through spawn/publication. Desktop `ipc/project.rs:290` unregisters its separate frontend registry only after daemon IPC revocation; it is not a discovered daemon gate bypass. Public raw registry handles remain available to trusted code; this is not type-enforced exclusivity. |
| Registered roots and validated cwd | Static support. Workspace registration retains absolute/canonical-directory and canonical repository-root checks. Session spawn resolves a registered manager, canonicalizes custom cwd, applies `canonical_allowed_path`, and checks containment in the resolved worktree/root before the lower PTY guard. No requested unregistered fallback or cwd-filter mutation remains in the moved implementation. Parent guard RED/restoration evidence is valid historical prerequisite evidence, not current GREEN. |
| Service-less constructors fail closed | All six constructor paths converge on `new_with_paths_backend`, which sets `machine_services: None`; only daemon installation supplies it. A04 construction test covers three entry points; the shared initialization covers the others statically. Existing `remote/server.rs:1968-2027` authenticates capabilities, emits an empty machine capability list, and returns 503 `MACHINE_SERVICE_UNAVAILABLE` for Machine-Control on the two placeholder routes (403 otherwise). These remain unconditional stubs even with services present, appropriate before A06/A09. No executed current HTTP503 proof; no unimplemented route is claimed working. |
| No strong service/server/gateway cycle | Static support for construction graph. Handover commit callback owns gateway; gateway owns session authority; authority's reverse handover edge is Weak. Services do not hold DaemonServer or gateway. New drop test checks session/workspace/gateway/handover/terminal weak upgrades after server drop. It is unrun and covers empty construction, not live-session task destruction. |
| No AppHandle required headlessly | Service fields and actual fixture construction contain no AppHandle requirement. Fixture builds real UDS handling, Axum TCP/auth and PTYs without running desktop or canonical daemon startup. Current headless execution remains unverified. |
| Sync I/O threading | Local canonicalization/Git manager validation, shell resolution, PTY spawn and ownership publication now run inside `run_blocking`; owned async spawn guard moves into that closure and workspace parking-lot gate is not held across await. Registration/unregister IPC work is off-thread. Resume probing occurs before registry resolution now, unlike baseline; this is not evidence of new HTTP admission because no machine spawn API is enabled. Gateway constructor config/auth I/O and direct synchronous registration adapter remain synchronous as before. SSH network operations retain their original async path and persistence blocking boundaries. Cancellation during blocking spawn is still not runtime-tested. |
| Local / SSH / legacy handover | Static transport preservation: new backend forwards recovery, generations, attach cursor, input, resize and signal to SessionRouter; router still validates SSH inventory and routes exact legacy peers, without Local fallback. SSH startup/store/platform checks, immutable request persistence and restore cursor reset are moved, not replaced. Metadata projection now uses service cwd/worktree while leaving absent legacy metadata unchanged. Unregister retains existing explicit destructive close semantics, not future A07 non-destructive unregister. Runtime preservation is unverified; see regression selector defect above. |
| Scoped RED and restoration | Supported by completed parent artifacts and actual scoped assertion output. Split-authority RED shows successful HTTP with empty sessions rather than setup failure. Guard REDs show the precise admission failures. Parent manifest records restoration of baseline server hash. Current source visibly restores both guards in their new location; current hashes necessarily differ after extraction. No repeat mutations authorized or performed. |
| Original IPC/HTTP session, PID/CWD, teardown GREEN | NOT ESTABLISHED for this candidate. Required final logs and parent manifest absent, and independent runtime launch barred at this request-only checkpoint. HTTP legacy list projects session/workspace identity, not PID/CWD fields; fixture correlates that ID to the original PTY handle and IPC/OS cwd. No claim of a literal HTTP PID field is made. |
| Deterministic real-surface test | Static support. No transport mocks hide integration. HTTP absence is checked in a completed authenticated response, not inferred by waiting. Cancellation/deadline triggers use pre-established oneshots at the acknowledged original-PTY seam. Thirty-second sleep is an explicit scenario deadline, not a readiness/absence wait. Listener joins and requests are bounded. External SIGKILL/deadlock cleanup and cancellation during blocking spawn remain disclosed limitations. |

### Source and artifact identity

Independently measured SHA-256; source paths below are relative to `src-tauri/src/`.

| Artifact | SHA-256 |
| --- | --- |
| `daemon/server.rs` | `d67480ecbecaa4ec32f69df7a4ef5b28044d8ea9aa74d1b8fca7248a0b6effe1` |
| `daemon/mod.rs` | `f62cac476e47a45f3a6dce95311d194820fd59f15411fe8a18ce54df5328812b` |
| `daemon/workspace_service.rs` | `714a8e4dbceec57aba00aad01bcdc5a23bcaf41284f0b88c75cfa60797930af6` |
| `daemon/session_service.rs` | `1bbd593f77fc575c5b2a717913a30d4db3631df02956ce1e2eb89980c024ac7c` |
| `daemon/a04_shared_services_tests.rs` | `57ed927b9c2fafa157e5f3123fa886b3758b98dc9794be8d37a4b880c67031ba` |
| `remote/state.rs` | `15f3fffbc551b035beb522c7d3db314406cefb9a1f3bbdcb0fd714b4f9aa7af4` |
| `remote/backend.rs` | `9e4c9049421a7cfdcaf0d539c42af5857abc8dd8a768e21fbec3526c34fcd71c` |
| `A04-green-command-request.json` | `4fc88189e1713eba59412b39556d46ac82e602b08fc2042cf8d09ab066a00139` |
| `A04-implementation.md` | `4a78a4bcd1f4e5afe7b00d0405d726f299cd7e76352bb1df8fef96719f898680` |
| `A04-parent-cleanup-result-g3.json` | `aa06c0016ad9ace5ef747a92af82c4cb5c7cabf0fc7678905a837d060e454310` |
| `A04-parent-guard-red-result-g2.json` | `56ccd5edfe815b0ff580d2e525d582d4cb924b137d77d6b217ff1e5575efae88` |
| Approved full plan | `0ad21b3583024a2c920b2bf328ece1dca4c7116b1e577e37dd5c7fdb2e172a2a` |
| Historical report suffix preserved below | `8809bbb3b3963f319cf0af41d7a5c127451a8bbe2aae6746ee954f179c0fbacc` |

### Real-surface observations: inherited, not independently rerun

- `A04-red-shared-authority.log:458-482`: owner 83825, PTY 83858; IPC/OS cwd `/private/tmp/a04-DkgEaQ/project`; HTTP200 with `[]`, revisions 1/0; `HTTP cannot see original IPC session and workspace`, exit 101 in parent receipt.
- `A04-red-unregistered-g2.log:123-150`: `unregistered spawn admitted`, child 96723, PTY 96743 reaped, child exit 101, root removed. `A04-red-cwd-g2.log:123-150`: `cwd escape admitted`, child 97839, PTY 97881 reaped, child exit 101, root removed. These are scoped behavioral RED failures, not compilation failures.
- `A04-guards-restored-green.log:127-145`: owner 638, original PTY 683, session `d08882f2-007f-4c7f-a68b-864cf7e9c82e`, socket `/private/tmp/a04-OXW5Lc/fixture.sock`, HTTP `127.0.0.1:52528`; matching IPC/OS cwd `/private/tmp/a04-OXW5Lc/project`, same session/workspace in HTTP200, revisions 1/1; listener joins, PTY reap, child wait and private-root removal. Parent receipt independently reports PIDs/roots absent.
- `A04-cleanup-reviewed-g3.log:123-225` and parent manifest record normal plus six injected scenarios passing, correct cleanup counts and only the intended one close error. These receipts pin fixture `24d8e8f0...` and baseline server `668e1710...`, not this extraction. Detailed historical appendices are not duplicated.

No new original PID, CWD, listener teardown or resource absence was observed by this verifier. Process inventory was read only to avoid concurrent Cargo; it showed unrelated mahoquot Cargo jobs, not an A04 run. No process was signalled. A present-day PID check could encounter PID reuse and would not replace original-handle reap receipts.

### Exact executed checks and inspection boundaries

Shell commands were read-only with 10-second bounds; none were long-running. `git diff --check` returned exit 0. Hash/manifest inspection returned seven MATCH lines, four absent log paths and four null exit codes. The decisive commands were:

```sh
cd /Users/indo/code/project/orca-lite-wt/herdr-wave1
 git diff --check
 git rev-parse HEAD
 date -u '+%Y-%m-%dT%H:%M:%SZ'
 rg -n 'mod agents|cmd_.*agent|cfg' src-tauri/src/ipc/agents.rs src-tauri/src/ipc/mod.rs
 rg -n 'RemoteGatewayState::new|DaemonServer::new|handle_register_workspace\(|handle_unregister_workspace\(|normalize_process_cwd|workspace_registry\.(register|unregister)|\.spawn_in_worktree\(|MachineServices|with_machine_services' src-tauri/src src-tauri/tests
 rg -n 'worktree_path|describe_session|impl RemoteSessionBackend|set_.*callback|on_commit|remote_state' src-tauri/src/daemon/proxy.rs src-tauri/src/remote/server.rs src-tauri/src/daemon/handover.rs
 rg -n 'remote_ssh_tests|remote_ssh_qa|ssh_survival_tests|mod tests|workspace_registry\(' src-tauri/src/daemon/server.rs src-tauri/src src-tauri/tests
 rg -n 'A04 |Error:|test result:|panicked' docs/evidence/paired-daemon/A04-{red-shared-authority,red-unregistered-g2,red-cwd-g2,guards-restored-green,cleanup-reviewed-g3}.log
 python3 - <<'PY'
import pathlib,json,hashlib
r=json.loads(pathlib.Path('docs/evidence/paired-daemon/A04-green-command-request.json').read_text())
for p,w in r['sourceSha256'].items():
 h=hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest();print(p,h,'MATCH' if h==w else 'MISMATCH')
for c in r['commands']:print(c['log'],pathlib.Path(c['log']).exists(),c['exitCode'])
for p in [*pathlib.Path('docs/evidence/paired-daemon').glob('A04*json'),pathlib.Path('docs/evidence/paired-daemon/A04-implementation.md'),pathlib.Path('docs/evidence/paired-daemon/A04-independent-verification.md'),pathlib.Path('/Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md')]:print(p.name,hashlib.sha256(p.read_bytes()).hexdigest())
PY
```

Additional executed inventory commands: `pwd`; `git -C /Users/indo/code/project/orca-lite-wt/herdr-wave1 status --short`; `ls /Users/indo/code/project/orca-lite-wt/herdr-wave1/docs/evidence/paired-daemon`; `ps -axo pid,ppid,command | grep -E 'cargo|a04|A04|orca.*daemon' | grep -v grep`; `wc -l src-tauri/src/{daemon/{server.rs,mod.rs,workspace_service.rs,session_service.rs,a04_shared_services_tests.rs},remote/{state.rs,backend.rs}}`; `for f in docs/evidence/paired-daemon/A04*json; do printf '\n%s\n' "$f"; python3 -m json.tool "$f"; done`; `git diff --stat`; `git diff -- src-tauri/src/remote/backend.rs src-tauri/src/remote/state.rs src-tauri/src/daemon/mod.rs`; `rg -n '^[-+]([^+-]|$)' <(git diff -- src-tauri/src/daemon/server.rs)`. Relative commands ran in the named worktree.

Direct reads covered the full approved plan (1-387 then 388-end after truncation), full new services, full fixture/backend, implementation and existing historical report; server construction/adapters 710-1059, state constructors/snapshot 270-609, router 580-829, current HTTP capability/stubs 1940-2039, agent caller 270-359 and desktop unregister 230-309. Server extraction was reviewed through its complete changed-line diff, not claimed fully reread across all unchanged 3884 lines. Constructor/caller search covered src and integration tests; those callers were not all executed. Source findings are not type/build/runtime verification.

Report validation: Markdown LSP diagnostics were requested with severity `all`; the tool reports no configured `.md` server, so diagnostics are unavailable. A read-only Python read-back split the report at the historical generation-3 marker and SHA-256 verified the suffix as `8809bbb3b3963f319cf0af41d7a5c127451a8bbe2aae6746ee954f179c0fbacc`, matching the original report exactly. It also recomputed every requested source hash (all unchanged) and rechecked the four candidate logs (all absent). These are artifact checks, not prose-pinning tests or runtime substitutes.

### Cleanup, blockers and end state

Only this report is edited. Prior reports are retained once, verbatim, below. No source mutations, other worktree changes, commits, Cargo/build processes, desktop/canonical daemon actions, network writes, PTYs, listeners, private runtime roots or private build targets were created by this verifier. No verifier-owned resources require cleanup. No prose tests are added.

A04 is NOT ACCEPTED because the non-test caller regression remains in the exact submitted hashes, completed candidate GREEN/source manifest is absent, relevant SSH/handover execution is not established, and the required independent isolated real fixture/PID/CWD/teardown proof is consequently blocked. Parent owns monitored execution of the exact commands in the pinned request; this report neither substitutes baseline signals nor authorizes acceptance before that manifest and final independent proof.

---

# Historical generation-3 checkpoint (preserved verbatim)

# A04 independent verification - generation 3 cleanup checkpoint

## Verdict: NOT ACCEPTED

Inspected `2026-09-12T04:55:20Z`, worktree `/Users/indo/code/project/orca-lite-wt/herdr-wave1`, HEAD `a2534ff4e125a7577a9dc1dee6cb598b3cf94c9a`.

This is the requested cleanup-harness prerequisite review, **not an A04 candidate review or runtime approval**. The producer reports `INCOMPLETE A04 - test cleanup implementation ready for parent execution`, with `productionEdited: false`. Its two requested logs and `A04-parent-cleanup-result-g3.json` are absent. No Cargo or fixture was launched: completed parent GREEN execution and its source manifest remain prerequisites for independent runtime verification. Parent final acceptance remains required; no later packet is approved.

### Changed cleanup harness: findings

The complete current 455-line fixture was read, together with the command request and the production close chain (`DaemonServer::handle_close` -> `TerminalService::close_session` -> PTY manager close and original session kill/reap). Static improvements over the historical fixture are real: listener handles are retained immediately after startup; setup and assertions are inside the cleanup boundary; IPC clients are drained before PTY enumeration; close errors accumulate rather than skipping siblings; original PTY handles survive registry removal; the wrapper waits for its owner before removing its root. Cancel/deadline injections use oneshot channels created before the real spawn, then trigger at the acknowledged PTY barrier. They do not depend on a fixed sleep to reach that barrier. Actual UDS, Axum/TCP, authentication and PTYs remain in use, not mocked transport.

Actionable evidence defects remain:

1. **Unexpected IPC task failures are only printed, not propagated.** The IPC drain matches `Ok(())` but sends every other join result to `eprintln!`, then returns the listener loop result. A client panic can therefore coexist with a successful listener join and an empty cleanup-error list. Accumulate/return those failures so an injection label cannot conceal a real handler failure. This is narrower than claiming such a panic occurred; none was executed here.
2. **Most injected paths do not assert their expected PTY cleanup count.** Only `Cleanup` asserts `closed == 2`. `Scenario`, `Cancel`, and `Deadline` accept `injected && result.is_err()` without asserting one original PTY was enumerated/reaped. Setup injections similarly do not assert zero. Original handles are retained only after cleanup enumeration; the scenario's original handle is dropped with its future. A session disappearing from the registry before enumeration could leave `closed=0` and still pass these assertions. The command request's required parent reconciliation of every `A04 OWNED` PID against cleanup receipts is therefore essential, not optional. Retaining ownership inventory outside the cancellable future and asserting counts/identity would make the tests themselves enforce this claim.
3. **Listener receipts overstate success on error.** After `join_listener` returns an error, the fixture still prints `joined=true`, and later prints `listeners_joined=true`. The error is retained and normally fails the test, so this is not a hidden passing error by itself; a parent must inspect exit status and the full error list, not accept the boolean receipt alone. Distinguish task termination from successful graceful completion in receipts.

The request correctly discloses residual boundaries: no independent bound on emergency OS wait, no guarantee after external SIGKILL/deadlock, and no cancellation-during-blocking-spawn proof. Inspection additionally shows emergency `session.kill()` executes directly on the async worker; only `wait_and_reap` is sent through `run_blocking`. Thus the request's wording that emergency “kill/wait_and_reap runs off-thread” is too broad. Normal production close has bounded TERM/KILL/reader phases, but that is not a whole-fixture cleanup guarantee. These observations do not authorize production cleanup changes in this checkpoint.

### Source and request identity

Independently measured SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| `daemon/a04_shared_services_tests.rs` | `e358d6d5635ba587c5468bad9c3fe283e7f0b1fb115c7c008ee0c9a7fff799b2` |
| `daemon/server.rs` | `668e1710b738c5de019ec447c36bbcf075852f6256c0946d3a47f6838a190345` |
| `daemon/mod.rs` | `6fe8266dc2526588252328487b25738791e29b80c8161e3a1079f9909ad49b0f` |
| `remote/state.rs` | `a07b344a9443d0d2bbc3375ceec8dcb04ab6cdf3c60661517656878b97327760` |
| `remote/backend.rs` | `98765bee3536676b3e9505fd567ac7651927fe9a89e7d9495e3349f6cfdea8da` |
| `A04-cleanup-command-request-g3.json` | `d390cfcf922ae218c0d3fa8ba6664e26fd286f78d1009ac3eb67d0b79cf5066a` |
| Approved plan | `0ad21b3583024a2c920b2bf328ece1dca4c7116b1e577e37dd5c7fdb2e172a2a` |
| Prior report, preserved below | `ab65483f90c79f6041bb464c83a1ca5a304b8654495d5f4f92079451bc1e0bbe` |

Source paths in the table are under `src-tauri/src/`. Python independently parsed the request and matched both declared source hashes. The historical split-authority RED fixture hash (`2b87c076...`) **does not match this changed harness**; its server hash still matches. Preserve that RED as historical scoped evidence, not a GREEN manifest for the changed test. No repeat RED was launched or requested here.

### Real-surface observations and outstanding A04 proof

No new real surface was exercised. Read parent log lines 450-end show historical owner PID `83825`, PTY `83858`, IPC metadata and OS cwd `/private/tmp/a04-DkgEaQ/project`, HTTP 200 with `[]`, and registry revisions 1 versus 0. The scoped assertion `HTTP cannot see original IPC session and workspace` failed; parent receipt records exit 101 and cleanup. This is a meaningful historical split-authority RED, not a compile/setup failure, not independently observed current process state, and not successful IPC/HTTP identity proof.

Both service files and `A04-implementation.md` remain absent. The tracked server diff still only registers the Unix test module. Remaining original requirements are: moved shared workspace/spawn/canonicalization/provider-claim authority (not unused wrappers or duplicated paths); registry mutation gate and all creation callers; guard-removal RED with restoration receipts; service-less constructor 503/no capability; ownership graph without strong cycle or AppHandle; constructor/caller blast radius and Local/SSH/legacy handover preservation; sync-I/O threading review of the eventual extraction; completed focused/remote and CLI/relay checks; parent GREEN source manifest; independent isolated/private-target headless IPC/HTTP PID/CWD and teardown verification. These are **not established** by a passing cleanup prerequisite, even if the parent later obtains one. Full historical requirement assessments below are retained as historical findings, not regenerated candidate review.

### Exact checks executed in this generation

All shell calls were read-only, with 10-second bounds. Commands executed:

```sh
pwd
git -C /Users/indo/code/project/orca-lite-wt/herdr-wave1 status --short
ls -lt /Users/indo/code/project/orca-lite-wt/herdr-wave1/docs/evidence/paired-daemon
cd /Users/indo/code/project/orca-lite-wt/herdr-wave1 && date -u '+%Y-%m-%dT%H:%M:%SZ' && git rev-parse HEAD && git diff -- src-tauri/src/daemon/server.rs && shasum -a 256 src-tauri/src/daemon/{server.rs,mod.rs,a04_shared_services_tests.rs} src-tauri/src/remote/{state.rs,backend.rs} docs/evidence/paired-daemon/A04* /Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md && find docs -name A04-implementation.md && rg -n 'async fn handle_close|fn close_session|fn wait_and_reap|fn kill\(|fn is_reaped' src-tauri/src/{daemon/server.rs,terminal} && ls src-tauri/src/daemon/*service*
cd /Users/indo/code/project/orca-lite-wt/herdr-wave1 && python3 - <<'PY'
import json,hashlib,pathlib
p=pathlib.Path('docs/evidence/paired-daemon/A04-cleanup-command-request-g3.json')
r=json.loads(p.read_text())
for name,want in r['sourceSha256'].items():
 actual=hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest()
 print(name, 'MATCH' if actual==want else 'MISMATCH')
for name in ['src-tauri/src/daemon/workspace_service.rs','src-tauri/src/daemon/session_service.rs',r['resultFile'],*[c['log'] for c in r['commands']]]:
 print(name, 'PRESENT' if pathlib.Path(name).exists() else 'ABSENT')
PY
```

All returned exit 0. `find` returned no implementation report; Python reported both source hashes MATCH and both service files, result manifest, and two requested logs ABSENT. Direct reads: full approved plan (1-387, 388-647, 648-end), full current fixture, full existing report, cleanup request, parent generation-2 audit and RED receipt, RED log 450-end, `server.rs:2670-2714`, `terminal/service.rs:202-end`, `terminal/pty.rs:334-448`, and `terminal/session.rs:190-304`. Hashing unchanged production files is identity verification, not a renewed full implementation audit. No compilation, test, rustfmt or runtime result is claimed for this generation.

Report validation: `lsp_diagnostics` with severity `all` returned no configured Markdown server, so diagnostics are unavailable. A read-only Python check read the written report, split at the historical generation-2 heading, and computed SHA-256 of the retained suffix: `ab65483f90c79f6041bb464c83a1ca5a304b8654495d5f4f92079451bc1e0bbe`, exactly matching the original report. The same check counted 455 fixture lines and confirmed the new verdict heading. No prose tests were added.

### Cleanup and boundary

Only this report was edited, preserving both prior reports verbatim below. No source edits/mutations, other worktree changes, commits, Cargo/build processes, daemons, desktop processes, listeners, PTYs, runtime roots or production network writes were made by this verifier. No long-running checks needed monitoring. No verifier-owned runtime resources require cleanup. Parent commands remain unexecuted here by explicit generation-3 direction. A04 remains NOT ACCEPTED pending the prerequisite results and the original complete GREEN candidate/evidence.

---

# Historical generation-2 checkpoint (preserved verbatim)

# A04 independent verification - generation 2

## Verdict: NOT ACCEPTED

Inspection time: `2026-09-12T04:43:34Z`. Worktree: `/Users/indo/code/project/orca-lite-wt/herdr-wave1`. HEAD: `a2534ff4e125a7577a9dc1dee6cb598b3cf94c9a`.

**The producer has supplied a blocked parent-command request, not a GREEN candidate. Independent runtime verification is therefore prohibited by the generation-2 gate and was not launched.** `A04-guard-red-command-request-g2.json` explicitly reports `blocked-on-required-guard-red-not-green-ready`, `productionEdited: false`, and a setup/teardown and timeout-orphan safety blocker. `A04-implementation.md` was not found anywhere in the worktree. Both proposed service files remain absent. Parent final acceptance remains required; this report approves no later packet.

The earlier report below is preserved verbatim as historical evidence, not treated as review of a new candidate. Its assertion that all A04 logs/fixtures were missing was accurate for its earlier snapshot but is superseded: a fixture, split-authority RED log, parent RED receipt, and two command requests now exist. The production extraction is still absent. The only tracked server diff is the four-line test-module inclusion.

### Current source and evidence identity

SHA-256, independently executed with `shasum -a 256`:

| Artifact | SHA-256 |
| --- | --- |
| Approved plan | `0ad21b3583024a2c920b2bf328ece1dca4c7116b1e577e37dd5c7fdb2e172a2a` |
| `src-tauri/src/daemon/server.rs` | `668e1710b738c5de019ec447c36bbcf075852f6256c0946d3a47f6838a190345` |
| `src-tauri/src/daemon/mod.rs` | `6fe8266dc2526588252328487b25738791e29b80c8161e3a1079f9909ad49b0f` |
| `src-tauri/src/daemon/a04_shared_services_tests.rs` | `2b87c0765970140a90a08e31af182008ba9f351de2d979b1ae8a4c22522970b4` |
| `src-tauri/src/remote/state.rs` | `a07b344a9443d0d2bbc3375ceec8dcb04ab6cdf3c60661517656878b97327760` |
| `src-tauri/src/remote/backend.rs` | `98765bee3536676b3e9505fd567ac7651927fe9a89e7d9495e3349f6cfdea8da` |
| `A04-guard-red-command-request-g2.json` | `79f2d361c564f32d003789cd261dfde79e8719b1b456695b58baed93e7f364ba` |
| `A04-parent-red-result.json` | `7f4ad961655cd53dbe957cfb06202a90f7e5bb97c99683d54e66cc94407a17bb` |
| `A04-red-command-request.json` | `2799e2d59a2e2f5d9cad3cf9a9a82bde9f1cf1b7f407b8069812f443415c665b` |
| `A04-red-shared-authority.log` | `61b19446aff7cdac85703d937bed14c4f48f63e66983d460395fa895c80e672f` |
| Historical report before this update | `62037ba14f03db961566d3d079e5a7df45ba18bf766ed65940d3b6c4e538822a` |

The parent split-authority RED receipt's two source hashes match the current server and fixture exactly. No GREEN source manifest exists. The split-authority mutation is an environment-selected test branch, not a production source mutation. No completed guard-mutation/restoration receipt is present; requested mutations are not evidence of executed or restored mutations.

### Scoped requirement disposition

| Requirement | Generation-2 finding |
| --- | --- |
| Separate authority RED | **Supported by parent runtime artifacts, not independently rerun.** The fixture selects a second real `DaemonServer` for HTTP under `A04_SPLIT_AUTHORITY`; its actual HTTP response is an empty list while IPC has a session. The exact scoped assertion fails, rather than a compile/setup failure. |
| Unregistered-root and cwd-bypass RED | **Not established.** The fixture contains rejection assertions, and the split-authority run reached later assertions, but no executed guard-removal failures are supplied. Latest request explicitly blocks on these mutations. |
| Shared domain authority and moved spawn/canonicalization/claims | **Not implemented.** Both proposed service modules are missing. Current constructor still shares the existing terminal router and registry; the server diff adds only the test module. Existing sharing is not extraction. |
| Workspace mutation gate and all creation paths delegated | **Not established; no service candidate.** Historical source findings remain baseline findings, not a migrated-authority audit. |
| Service-less constructor 503 | **No new runtime proof.** Current gateway constructor family has backend/registry parameters but no machine-services parameter. A04 fixture tests mirror-authenticated GET sessions, not the missing-services machine-operation distinction. Historical unconditional placeholders cannot establish a working production service. |
| No strong cycle / no AppHandle headlessly | **Candidate absent.** Inspected constructor passes shared router/registry handles; fixture directly constructs a server without AppHandle. Parent RED is headless evidence for the baseline fixture only, not a new ownership graph or destruction proof. |
| Constructor/caller blast radius | **Blocked before candidate review.** No production caller migration exists in the inspected diff; a full new-service caller audit would be premature. Historical caller inventory is retained below, not promoted to verification. |
| Local / SSH / legacy handover semantics | **Unverified for A04.** Fixture covers one Local shell, no SSH or legacy handover. No extraction exists to assess for preservation. |
| Sync I/O threading | **Unverified for extraction.** Current gateway construction reads persisted configuration synchronously. Fixture does synchronous filesystem setup and process-cwd probing inside async scenario execution. No new service threading boundary is available to review. Historical spawn concerns remain unverified baseline observations. |
| Real IPC/HTTP PID/CWD equality and teardown GREEN | **Blocked.** Parent RED gives IPC/OS identity and deliberately empty HTTP; it does not supply successful cross-surface identity. No independent launch or GREEN receipt exists. |
| Determinism and integration fidelity | **Partial static evidence only.** Fixture uses real UDS handling, TCP/Axum router, auth and PTY, not mocked integration. Absence is asserted against a completed HTTP response, not inferred from elapsed time. Reads/listener joins have bounded timeouts and no fixed sleeps are present. Setup/cleanup robustness remains explicitly blocked. |

### Real-surface evidence inspected (parent run only)

`A04-red-shared-authority.log:458-482` records owner PID `83825`, PTY PID `83858`, socket `/private/tmp/a04-DkgEaQ/fixture.sock`, HTTP `127.0.0.1:63100`, session `4ac61709-11d9-475e-9269-21a31e1ca373`, and matching IPC/OS cwd `/private/tmp/a04-DkgEaQ/project`. HTTP returned status 200 with `sessions=[]`, registry revisions IPC `1` / HTTP `0`. The failure is exactly:

```text
Error: HTTP cannot see original IPC session and workspace
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 887 filtered out
```

The parent receipt reports exit 101. The log records PTY reaped, listeners joined, socket removed, fixture child reaped with exit 101, and private root removed. The receipt additionally reports both PIDs absent. These are inspected producer/parent artifacts, not newly observed process state. No timing-only absence check is accepted here as independent cleanup proof.

The fixture source explains the safety blocker: some setup fallible operations occur after IPC task spawn but before the cleanup block; cleanup uses early-return `?`/`ensure!`; outer timeout kills/reaps the fixture child without tracking every separate-session PTY. Thus the successful cleanup of this one assertion failure does not establish cleanup for setup failure or timeout. The latest command request acknowledges this exact limitation. No unsafe substitute launch was attempted.

### Exact generation-2 checks executed

All shell calls were read-only and bounded to 10 seconds. The following reproduces the executed commands (absolute paths are intentional):

```sh
pwd
git -C /Users/indo/code/project/orca-lite-wt/herdr-wave1 status --short
ls -l /Users/indo/code/project/orca-lite-wt/herdr-wave1/docs/evidence/paired-daemon/*A04*
ls /Users/indo/code/project/orca-lite-wt/herdr-wave1/docs/evidence/paired-daemon
find /Users/indo/code/project/orca-lite-wt/herdr-wave1 -name A04-implementation.md
wc -l /Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md
ls -l /Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/src/daemon/{server.rs,mod.rs,workspace_service.rs,session_service.rs,a04_shared_services_tests.rs} /Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/src/remote/{state.rs,backend.rs}
cd /Users/indo/code/project/orca-lite-wt/herdr-wave1 && date -u '+%Y-%m-%dT%H:%M:%SZ' && git rev-parse HEAD && shasum -a 256 src-tauri/src/daemon/{server.rs,mod.rs,a04_shared_services_tests.rs} src-tauri/src/remote/{state.rs,backend.rs} docs/evidence/paired-daemon/A04* /Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md
rg -n 'A04|panicked|test result|FAILED|cleanup|HTTP|registry|PTY|pid' /Users/indo/code/project/orca-lite-wt/herdr-wave1/docs/evidence/paired-daemon/A04-red-shared-authority.log
git -C /Users/indo/code/project/orca-lite-wt/herdr-wave1 diff -- src-tauri/src/daemon/server.rs
```

`find` produced no report path. `ls -l` returned exit 1 because both proposed service files do not exist. `wc -l` reported 832 newline characters; the read tool exposed 833 logical lines. The full approved plan was read in ranges 1-387, 388-647, and 648-end after its initial read reached the byte limit. Additional direct reads: complete historical report, latest guard command request, parent RED result, full 143-line fixture, `server.rs:890-989`, and `remote/state.rs:273-442`. The RED log was inspected through the exact filtered command above, not claimed read in full. Backend/mod identity was measured, not reread as a new candidate audit.

The latest requested command, **not executed by this verifier**, is:

```sh
CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::a04_shared_services_tests::a04_shared_authority_runtime -- --exact --nocapture
```

That request is for parent-monitored guard mutations and expressly requires cleanup hardening first. It is not permission for this verifier to launch a GREEN/private-target rerun. No Cargo, tests, build, daemon, desktop, or network operation was launched. Report-file diagnostics were requested with `lsp_diagnostics` (severity `all`); the tool reported no LSP server configured for `.md`, so Markdown diagnostics are unavailable. There were no long checks to monitor. No prose-pinning test was added.

### Cleanup, blockers, and boundary

This verifier created no processes, listeners, target directories, private roots, or PTYs, made no network production writes, and touched no canonical daemon or desktop. The only write is this report. No source mutations, other worktree edits, cleanup deletions, or commits were made. Pre-existing tracked/untracked artifacts were left intact.

Acceptance is blocked by (1) missing service extraction and producer implementation report, (2) pending guard RED execution and restoration receipts, (3) acknowledged fixture cleanup safety gaps, and (4) absent completed parent GREEN execution/source hashes and independent safe real-surface proof. The existing parent split-authority RED is meaningful but insufficient. Generation-2 instructs stopping at this blocked-command state, so no candidate review or alternate runtime signal is substituted.

---

# Historical generation-1 report (preserved verbatim)

# A04 independent verification

## Verdict: NOT ACCEPTED

Inspected 2026-09-12 (UTC check: `2026-09-12T04:27:18Z`) in `/Users/indo/code/project/orca-lite-wt/herdr-wave1`, HEAD `a2534ff4e125a7577a9dc1dee6cb598b3cf94c9a`.

The A04 candidate and producer evidence were unavailable at inspection. Neither `src-tauri/src/daemon/workspace_service.rs` nor `src-tauri/src/daemon/session_service.rs` exists. `docs/evidence/paired-daemon/A04-implementation.md` and A04 logs are absent. No producer-focused fixture invocation, RED receipt, GREEN receipt, restoration manifest, or PID/CWD/teardown receipt is available. This is an explicit rejection of the submitted snapshot, not a claim that an unseen producer implementation failed a runtime test. Parent final acceptance remains required; no subsequent packet is approved.

The complete approved plan was read (833 lines, in two reads because the first response reached the byte limit). D02 and A04 require extraction of shared domain authority, not just existing terminal-handle sharing. Missing evidence alone blocks acceptance, and current source independently confirms the extraction is absent.

## Source identity

SHA-256 measured with `shasum -a 256`:

| Artifact | SHA-256 |
| --- | --- |
| Approved `.omo/plans/ferryx-herdr-cloud-multi-host-plan.md` in canonical checkout | `0ad21b3583024a2c920b2bf328ece1dca4c7116b1e577e37dd5c7fdb2e172a2a` |
| `src-tauri/src/daemon/server.rs` | `fa92980b92102b8799346be88c4ff03affd26a0503f09b6fbd07fccefe32031c` |
| `src-tauri/src/daemon/mod.rs` | `6fe8266dc2526588252328487b25738791e29b80c8161e3a1079f9909ad49b0f` |
| `src-tauri/src/remote/state.rs` | `a07b344a9443d0d2bbc3375ceec8dcb04ab6cdf3c60661517656878b97327760` |
| `src-tauri/src/remote/backend.rs` | `98765bee3536676b3e9505fd567ac7651927fe9a89e7d9495e3349f6cfdea8da` |
| Both proposed service files; A04 producer report/logs | Missing; no hash possible |

There is no A04 GREEN manifest against which to compare these hashes. No RED mutations were made by this verifier; producer mutation restoration cannot be established.

## Requirement assessment

| Requirement | Current evidence and disposition |
| --- | --- |
| One authority backing IPC and HTTP | **Not established for A04.** `DaemonServer::new_with_paths` (`server.rs:919-957`) already creates one terminal service/router and clones that router and workspace registry into the gateway. This is the baseline explicitly recognized in the plan, not evidence of service extraction. |
| Move spawn/canonicalization/provider claims rather than duplicate them | **Not implemented in inspected snapshot.** `handle_spawn` remains on `DaemonServer` (`server.rs:2388` onward), including idempotency, registry target resolution, cwd canonicalization, shell resolution, provider claim conflict checks, actual spawn, metadata publication and lifecycle cleanup. `daemon/mod.rs` contains neither service module. Gateway state has no optional `Arc<MachineServices>`. |
| Workspace mutation authority | **Not implemented as shared service.** Registration still writes `self.workspace_registry.register(...)` directly around `server.rs:2300`; unregister directly calls `self.workspace_registry.unregister(...)` at 2310 and closes owned sessions. No extracted shared workspace mutation gate is present in the inspected code. Durable catalog behavior itself belongs to A05 and is not required here. |
| RED: separate registry/service cannot see IPC-created session | **Unverified / blocking.** No A04 fixture or failing assertion log. Existing constructor accepts separately supplied backend and registry, but source inspection is not the required IPC/HTTP RED proof. |
| RED: unregistered root and bypassed cwd rejected | **Source guards exist; scoped RED unverified.** `handle_spawn` resolves a registered target, requires existing directory cwd, canonicalizes it, applies `canonical_allowed_path`, and checks the resolved worktree/root boundary. No executed A04 mutation demonstrates these assertions catch a bypass. |
| Constructors without services return 503 / advertise no machine capability | **Baseline source support only.** All six gateway constructors converge on `new_with_paths_backend` (`state.rs:319-438`). `remote/server.rs:1967-2027` advertises an empty capability list and sends Machine-Control requests to unconditional `MACHINE_SERVICE_UNAVAILABLE` placeholders for directory listing and session POST; other scopes receive 403. This does not prove a production constructor with real shared services works or that the missing-services distinction survives extraction. No runtime check executed here. |
| No strong ownership cycle | **A04 unverified.** Existing constructor supplies shared data handles rather than `Arc<DaemonServer>` to the gateway; its desktop event closure captures a broadcast sender. Proposed services are absent, so their ownership graph cannot be accepted. No leak/drop fixture executed. |
| No Tauri AppHandle needed headlessly | **A04 unverified.** Inspected constructor/spawn/backend paths take no AppHandle; gateway desktop sink is optional. No A04 headless fixture exists to independently exercise the claimed extraction. |
| Constructor/caller blast radius | **No candidate to validate.** Constructor searches locate daemon production/test construction, gateway tests, security tests, SSH tests, machine-auth tests, resize tests, relay tests, and backend-injection socket tests. `RemoteSessionBackend for TerminalService` still supplies terminal operations, with `workspace_id: None` in local detail. No extracted service constructor/caller migration exists. This is a surface inventory, not a full compatibility pass. |
| Local / SSH / legacy handover semantics | **Not regression-tested.** Existing spawn rejects SSH/local routing mismatches and local shell override for SSH, validates daemon-configured SSH inventory, and delegates SSH resolution through `run_blocking`. Existing construction uses `SessionRouter`; handover commit stops the old gateway listener and clears active selection. Unregister can close a legacy peer session. No A04 runtime evidence establishes preservation across extraction. |
| Synchronous I/O threading | **Unresolved for A04.** Existing spawn runs provider resume resolution off-thread, but cwd `exists`, `is_dir`, canonicalization, shell resolution and spawn remain inside the async method. Existing gateway snapshot rebuilding uses `spawn_blocking` (`state.rs:501-550`). No candidate exists to assess whether extraction preserves/improves this boundary. These are source observations, not newly introduced regressions. |
| Real headless fixture: shared PID/CWD across IPC/HTTP and cleanup | **Blocked.** Producer command, fixture source and evidence are unavailable. No equivalent result is inferred from prior A03/V06 reports or unit tests. |
| Determinism / integration fidelity | **Unverified.** No A04 tests available to check event-before-action ordering, bounded waits, absence assertions, or mocks. No sleeps, polling, mock substitute, or fabricated runtime signals were used by this verifier. |

## Exact executed checks

All shell checks were read-only, with 10-second bounds. Working directory for the following relative paths was `/Users/indo/code/project/orca-lite-wt/herdr-wave1`.

```sh
git status --short
ls -la docs/evidence/paired-daemon
rg -n 'workspace_service|session_service|A04|fixture' docs/evidence/paired-daemon
ls src-tauri/src/daemon/*service* docs/evidence/paired-daemon/A04*
rg -n 'MachineServices|DaemonWorkspaceService|DaemonSessionService|new_with|pub fn new|spawn_terminal|handle_spawn|register_unique_root|MACHINE_SERVICE_UNAVAILABLE|machine_services' src-tauri/src/daemon/{server.rs,mod.rs} src-tauri/src/remote/{state.rs,backend.rs,server.rs}
git rev-parse HEAD
shasum -a 256 src-tauri/src/daemon/{server.rs,mod.rs} src-tauri/src/remote/{state.rs,backend.rs}
rg -n 'RemoteGatewayState::new|impl RemoteSessionBackend|struct DaemonRemoteSessionBackend|workspace_registry\.(register|unregister)|fn validate_spawn_cwd|fn resolve_spawn|fn claim|AppHandle' src-tauri/src/daemon/server.rs src-tauri/src/remote src-tauri/src/state.rs
shasum -a 256 /Users/indo/code/project/orca-lite/.omo/plans/ferryx-herdr-cloud-multi-host-plan.md
date -u '+%Y-%m-%dT%H:%M:%SZ'
```

Observed command limitations/errors: `ls` reported both service and A04 evidence globs absent. The final `rg` reported `src-tauri/src/state.rs` does not exist; its other requested paths were searched successfully. This missing extra search path is not treated as proof of anything about that file. Initial git status contained only untracked `docs/evidence/paired-daemon/WAVE1-prerequisites.json`; no tracked candidate modifications were reported.

Direct file reads inspected: full approved plan; complete `remote/backend.rs` and `daemon/mod.rs`; `remote/state.rs:250-719`; `daemon/server.rs:850-1014` and `2300-2689`; `remote/server.rs:1940-2039`. Evidence-directory listing and search exposed prior A03/V04/V06 artifacts, not A04 producer artifacts. Prior logs were not rerun or relabeled as A04 proof.

No build, test, language-server validator, fixture, CLI daemon, desktop, or network operation was executed. The exact required fixture invocation cannot be supplied because the producer has not supplied it in the inspected artifacts. Running an invented command or older fixture would not resolve that blocker. No long-running checks needed monitoring.

## Real-surface observations and cleanup

No new runtime surface was exercised: there is no independently observed A04 daemon PID, PTY PID, cwd, IPC session identity, HTTP identity, or teardown event to report. Baseline source observations above are explicitly not runtime proof.

The verifier launched no persistent process, opened no listener, allocated no isolated runtime/build root, and performed no production network writes. No canonical daemon or desktop was started, stopped, restarted, or inspected through control APIs. No source changes, mutations, commits, or cleanup deletions were made. The only intended write is this report; the pre-existing prerequisites file is left untouched. There are no verifier-owned runtime resources to clean up, and no claim is made about producer cleanup.

## Blocking acceptance conditions

1. The inspected worktree has no A04 shared-service implementation.
2. Producer `A04-implementation.md`, scoped RED/GREEN logs, source hashes/restoration evidence, and a safe focused real-fixture invocation are missing.
3. Independent real IPC/HTTP PID/CWD identity and teardown proof therefore cannot be produced.

Each unmet requirement is rejected or explicitly unverified above. No replacement implementation or later-packet approval is authorized by this report.
