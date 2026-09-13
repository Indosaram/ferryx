# Portable input candidate: native Windows GREEN; composed acceptance OPEN

Task st_01a0983c. Native Windows cancellation is implemented, not an unsupported
stub. This report does not close A10 socket budgets, owner epoch routing, or
whole-platform acceptance. Exact candidate hashes: A10-input-portable-source.json.

## Source and dependency delta

Own files: terminal/session.rs, terminal/pty.rs, new terminal/windows_input.rs,
tests/pty_input_cancellation.rs, new tests/pty-input-native/{Cargo.toml,Cargo.lock,native.rs},
private src-tauri/vendor/portable-pty. Shared Cargo.toml changes portable-pty0.9 to
the private path; the existing lock record drops only its registry identity.
Proposal was recorded before shared edits in A10-pty-input-dependency-proposal.md.
No global Cargo cache/dependency source was changed. Upstream0.9 source/license
was copied into the private adapter. Native harness has a separate captured lock;
it does not establish full application dependency equivalence on Windows.

Adapter: ConPTY input uses byte-mode PIPE_NOWAIT named pipe, duplicated input
handle exposed by MasterPty. Existing blocking Write retries zero-progress writes
and retains blocking Local behavior. New Windows async input uses immediate
WriteFile attempts; no overlapped IRP or input worker survives future drop.
One-ms retry timer is cancellable. 64KiB admission and10s deadline include gate
waiting. Closing state and closed writer are rechecked at each actual syscall;
state lock covers syscall admission against begin_closing. Unix close-state
check similarly covers the nonblocking write. Parent-requested Unix EOF loop
now rejects zero reads rather than spinning.

Integration signature remains `PtySession::write_input_cancellable(&[u8]) ->
Result<(), PtyError>` (async). Caller must hold its generation authority and drop
the future on disconnect/revoke. Already accepted bytes cannot be retracted;
never retry a partially failed frame. No session service/proxy/socket/event
integration source was edited in this continuation.

## Windows native evidence

Private root `C:\Users\sook\ferryx-a10-input-01a0983c`, not Q4's root. Jobs2,
private HOME/USERPROFILE/TEMP/TMPDIR/FERRYX runtime/data/sessions/XDG/APPDATA,
private target, original Cargo/Rustup homes. No desktop/clipboard/canonical
daemon. The minimal native crate imports exact production session.rs and its
Windows input module, real private portable-pty, real ConPTY child; only metrics
and the enclosing error enum are supplied by the harness.

Exact command (inside private root):
`cargo --config qa.toml test --locked --manifest-path src-tauri/tests/pty-input-native/Cargo.toml --test native -- --nocapture`
qa.toml sets build.rustc-wrapper="" and jobs=2. Native command runs beneath a
60s process-tree watchdog (90s for blocking RED). Final output final2 log has
two passing libtest entries: ONE independent scenario plus child helper no-env
return. Do not count the helper as another saturation proof.

Actual behavioral RED: same command with A10_BLOCKING_RED=1, log
A10-input-windows-blocking-RED2.log. Real nonreading child PID24512, accepted
8,388,608 bytes before blocking16MiB legacy production write;100ms bound observed
blocked=true. Explicit control release allowed writer join and child counted
25,165,825 bytes including sentinel. Assertion then failed; original child
reaped, reader/drain joined. This RED exercises retained synchronous production
behavior AFTER adapter creation, not a falsely claimed pre-edit Windows run.

Final GREEN A10-input-windows-final2.log: original PID17480, private HOME CWD,
8,159,232 accepted saturation bytes; after dropping pending async input and
deadline operation, received8,159,233 including fresh sentinel. Original PID
unchanged. Closed-state async input denied. No cancelled bytes observed in this
run. Predicate permits a previously accepted prefix, as the contract must;
it does not promise kernel bytes are retractable. Queued operation10s deadline
was checked with controlled clock at11s. It was not an exact10s boundary test.
The pending first operation holds input gate while the second times out.

Conhost drains input independently of child reads. A bounded10s saturation
workload fills that queue; its transient zero return alone is NOT treated as
stable saturation. The actual production future is explicitly polled pending
before drop. Fixture uses real TCP control events for child readiness/release.

## Failures retained, not mislabeled

Build attempts1/2: sccache inherited config and CLI quoting failures; attempt3:
missing serde in standalone harness. Fixed private qa config and harness dep.
All are build/tool failures, not behavioral RED.
Runtime1: missing cursor-position reply prevented ConPTY startup; cleanup reader
timed out. Runtime2: transient pipe saturation assumption failed. Runtime3:
small timed write completed while conhost drained. Runtime4 hung awaiting exact
clock boundary; SSH supervisor timed out180s, then exact private executable
process tree24416/20056 was identified and terminated. Runtime5 new stage markers
localized wait after CLOCK_ADVANCED;60s watchdog terminated its owned process
tree. Runtime6 and final/final2 passed after explicitly polling timeout at11s.
First blocking RED attempt incorrectly repolled completed JoinHandle; corrected
with a genuinely saturating16MiB write and conditional join. Logs are all
A10-input-windows-{build-attempt*,run-attempt*,blocking-RED*,final*,timeout-cleanup}.
No failed assertion was skipped or removed merely to turn GREEN.

## Cleanup and current composed gate

Final Windows proof reaped original child, joined reader/drain and dropped
listener/handles. A10-input-windows-cleanup.log confirms OWNED_PROCESSES=0 and
ROOT_REMOVED=True. Q4 root and processes were not targeted. No commits.

Changed-source Unix command:
`cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_input_cancellation -- --nocapture`
exit101 in A10-input-portable-Unix.log, blocked by concurrent out-of-scope
machine_owner_socket.rs unresolved super::server/type inference and unmatched
MachineSessionDetail/MachineGateway variants in daemon/client.rs. No terminal
compile failure is reported there, but no composed Unix pass is claimed. Did not
edit that active lane or repeat unchanged build. Historical Unix PID32647 proof
remains scoped to its earlier source, not current acceptance.

Final production LSP requests were cancelled by language server; new Unix fixture
returned no errors. Native Windows compiler passed its exact source inclusion.
Full app Windows/Unix build, exact10s edge, saturated timeout rather than queued
timeout, exhaustive Local/SSH compatibility and aggregate review remain OPEN.
This is a portable implementation candidate with actual native proof, not full
acceptance or permission to advertise terminalStreamV1.
