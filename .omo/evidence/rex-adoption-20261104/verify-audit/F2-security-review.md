# F2 independent security review

Head: `35c984684b60c40dd6d29cd8703270fe86c30580`; base: `d59508fa9`.
Verdict: **REJECT**.

This is the source/diff-review part of F2 only. No builds, tests, browser checks, remote sessions, or production daemon operations were run by this leg. Static assertions are explicitly not F3 QA. The read-only source assertion command exited 1; see `static-review.log`. All locations below refer to the integration head.

## Blocking findings

### SEC-1 HIGH: remote write/resize variants bypass the new cross-pane gate (T5/T12)

`src-tauri/src/daemon/pane_grants.rs:124-195` enumerates `Write` and `Resize`, but not `RemoteWrite`, `RemoteResize`, or `RetryRemoteSession`; its wildcard at line 195 treats them as reads. `server.rs:7175-7242` accepts those variants and writes/resizes the target session or forwards to a legacy peer. Target generation is a routing incarnation, not caller authorization. A classified pane can therefore mutate another remote/paired pane without its directional grant. The request protocol exposes these at `protocol.rs:878-890`. `AbortHandover` is also absent from the admin set despite the rollback handler at `server.rs:9101`; audit all mutating variants, not just the named examples.

Smallest repair: add explicit session write/control gates for remote mutations and admin for lifecycle mutations, plus an exhaustive mutation-classification regression table. Verify denied pane-to-remote/paired writes through the real socket, then grant and retry.

### SEC-2 HIGH: an enabled plugin can bypass all manifest/grant restrictions via the owner socket (T30)

`plugins/runtime.rs:37-49` spawns an ordinary same-uid process directly from the daemon and only reports its PID to a logging observer. It never inserts a plugin identity into the pane process registry. `server.rs:6670-6703` constrains stdio dispatch, but the plugin can ignore stdio and connect directly to the public same-uid daemon socket/Windows pipe. `supervisor_peer.rs:465-527` finds no pane ancestor for a daemon child and classifies it Owner; `pane_grants.rs:347` then grants all mutations. This defeats the promised Pane-like restrictions, even for a manifest requesting only `render`.

Smallest repair: register plugin processes and descendants as plugin-scoped restricted identities on every transport before any request can be accepted, remove them on reap, and prohibit Owner classification for these identities. Test a real enabled render-only executable opening the ordinary daemon transport and attempting Write/Shutdown/grant creation. Merely testing the stdio `authorize` helper cannot catch this bypass.

### SEC-3 MEDIUM: failed peer identification becomes fail-open (T5/T12)

`windows_pipe.rs:86-93` returns Unknown if process identity cannot be obtained or the blocking task fails. `server.rs:6647-6648` explicitly allows Unknown through every new gate except OwnerOnly, and `pane_grants.rs:350-355` does the same. The control channel calls this policy at `control_server.rs:450-454`, so unknown peers may stop/retire the daemon too. The planned exception is legacy token-authenticated Windows TCP, not failed identification on process-aware transports.

Smallest repair: deny Unknown at the production transport boundary; keep in-process trusted calls explicit rather than sharing the failure sentinel. Preserve only the documented TokenAuthenticated compatibility exception until T13. Fault-inject peer identity failure and assert FORBIDDEN without handler side effects.

## Required deviation and build-green risk review

- **T30 iframe deviation: BLOCKED, not approved.** `ui/src/components/PluginPane.tsx:29-31` uses `sandbox=""`, no allow-scripts or allow-same-origin, `srcDoc`, and no-referrer. `plugins/mod.rs:98-105` prepends `default-src 'none'`, plugin-only image/style origins, base-uri/form-action none. This is a plausible scriptless opaque-origin boundary, but is not the isolated native webview specified by the plan. Only mac/win/linux native-surface checks can discharge the orchestrator's provisional condition: no app internals, no parent DOM access, failed Tauri invoke, and blocked foreign-origin fetch. A WindowProxy for `window.parent` exists normally; prove parent DOM/API inaccessibility, not literal absence of the parent property. No such runtime evidence was produced by this leg. Plugin resource URLs remain unserved; text-only clock does not prove those paths.
- **Headless Windows supervisor: MEDIUM fidelity risk.** `daemon/supervisor.rs:620-625` changes the headless decision to `managed_externally = false`, bypassing package identity/store-marker detection that the desktop updater performs. A packaged headless CLI may attempt Task Scheduler lifecycle ownership contrary to the MSIX contract. Smallest repair: move the OS/package ownership predicate into a headless-capable shared module. A native packaged CLI observable is owed; no claim of a baseline reproduction.
- **No-active-pane layout: compatibility risk, not proven security bug.** `ui/src/lib/layoutSync.ts:233,239` throws on null active pane. This is preferable to serializing a null numeric node, but valid legacy/transient empty layouts need real UI coverage. Do not infer success from compilation. Also `isLayoutIntentAction` at 300-306 omits TOGGLE_PANE_EXPANDED while daemon `layout.rs:104,507` supports it; expansion persistence is incomplete at the frontend boundary.
- **GUI harness feature gates:** `Cargo.toml:49-99` now excludes desktop/native harnesses from headless targets. Correct feature isolation is not evidence those harnesses compile or execute in default-feature builds; all-target/default and real UI gates remain owned by host legs.

## Reviewed boundaries without a new blocking finding

- **T6:** `remote/security_headers.rs` normalizes CSP connection origins, has no bare ws/wss sources, sends frame-ancestors/XFO/referrer/nosniff, keeps HSTS in the TLS layer, and guards upgrades before dispatch. Both routers call `harden` (`remote/server.rs:8274`, `relay_server.rs:4455`). Attach/control auth failure paths close 1008/AUTH_FAILED. Existing private-LAN gateway CSP reachability risk in the ledger remains a host QA obligation; not a security approval of unexercised authenticated flows.
- **T12 pipe construction:** owner SID-only protected DACL, first-instance binding, remote-client rejection, noninheritable handles, and client PID lookup are present (`windows_pipe.rs:48-93`). Legacy TCP bypass is explicitly retained for the release-gated T13 and is not mislabeled as effective full Windows same-uid enforcement.
- **T14:** OSC callbacks copy FFI payloads, cap clipboard bytes, strip controls/bidi, rate-limit events, always deny reads, and are opt-in/muted on replay (`native_terminal/osc_effects.rs:56-145`). Server output sink checks local PTY ownership (`server.rs:3984-4001`). Broader 1337/5522 clipboard gating is stricter than plan, not an escalation. Process-local throttle reset is transient, not a persistent permission loss.
- **T18:** signed exact manifest verification before parsing/policy, floor/halt fail-closed, second-fetch JSON equality before offering/downloading (`ipc/updater.rs:399-413,433-500,653-675,725+`), release-only fixed endpoints, and external-management early gate are present. A signed older manifest replay does not defeat the default no-downgrade comparator, but the halt switch is not an anti-replay scheme. Fetch body currently has no byte cap (`:512-523`), a hardening concern rather than a demonstrated exploit. The endpoint QA test still mutates process-global environment (`:1393-1395`); parallel isolation remains owed, not a claimed flaky baseline.
- **T23:** Luau uses a dedicated thread; io/os/package/debug/load/require/pcall removed; sandbox and 64 MiB memory limit; 50 ms thread CPU interrupt; 64 timers; owner config path only and setting gate (`script/vm.rs:36-76`, `store.rs:51-76`). Timer claims are persisted before callback, execution is frozen for handover. Owner-authored host exec deliberately allows argv execution with timeout; it is not a project import evaluator. Host output/resource bounds and native watchdog tests still need execution. No sandbox escape was executed or claimed.
- **T30:** full-manifest approvals require renewed consent after argv/capability change; unknown stdio requests deny by default; bounded lines and restart max 3 exist. Malformed manifests fail closed in list/load. These positive controls do not repair SEC-2's second transport.

## Failure receipt and classification

Exact command: `python3 .omo/evidence/rex-adoption-20261104/verify-audit/static-review.py`.
Exit: **1**. Raw log: `static-review.log`.
First failing assertion: `remote terminal writes and resizes are explicitly pane-gated`, `pane_grants.rs:195`.
Other failing source assertions: unidentified process fail-open, missing plugin process classification, literal leafId guardrail. These are source-level findings, not runtime reproductions. They are new policy/extension defects in the integration diff, not classified baseline failures: the new grant/plugin implementations do not exist in base. The pre-existing ancestry escape described by `supervisor_peer.rs` (setsid + detach + reparent) remains an acknowledged baseline limitation and is not counted as a new finding. No baseline command failure was independently reproduced in this no-build leg.

F2 approval requires repairing the blocking findings and host QA evidence for the iframe deviation. No product edits made.
