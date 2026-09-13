# A11 relay-owned runtime proofs pass; composed build NOT READY

Task st_01a098a9, macOS arm64, worktree herdr-resume-01a097f8. No production change was needed in this continuation. Inherited relay implementation and all original tests, including Windows shell fixtures and historical failures, are retained. Only relay_pairing_generation_regression.rs and new A11-followthrough-owned-* evidence were authored here.

## Delivered proofs

The fully isolated final execution passed all eight relay integration tests and the exact-boundary expired-ticket real WS test. The existing real RelayClient/data channel was used, not a replacement tunnel. Capabilities and created targets agree with the actual gateway identity. One create response is held after the actual completed201 is materialized, before relay response headers are written; the requesting task is aborted and joined, then operation lookup reconciles the original target. Identical retry retains it and the single-session inventory. This is a genuinely withheld response, not merely an acknowledged retry.

Two registered real roots create distinct PTYs. Final successful identity: machine 141976f3-d489-4e8f-8de7-be4231f8bd8a, owner epoch 1789268154950, raw session a4f97b83-401d-4a84-b41a-8530cc7ecb13, original PID25846; sibling PID25917. Shell-assembled PID/PWD markers match the private project and second-root paths. Original target/PID remain unchanged through replay. DELETE reaps the original while the sibling survives, then sibling DELETE reaps it.

New replay assertions use actual shell printf output through PTY -> gateway -> reverse data tunnel -> relay WS. A marker establishes cursor108, subsequent output is recovered through a fresh-ticket suffix attachment, then 1MiB of real shell output evicts retained history. Reattachment with the old cursor receives an explicit gap and the retained shell marker. No direct hub publication establishes these new proofs. The original synthetic hub suffix/gap tests remain separately intact. This is retained-history overflow, not slow-consumer budget or OS firewall proof.

Existing assertions cover input, resize103x37 acknowledged by ordered ping/pong and actual PTY size, armed trap/interrupt response, stale owner epoch rejection, malformed fields, replayed tickets and cross-machine/path denial. Expiry uses injected exact current-time boundary and real WS401, with consumed ticket record; no sleeps.

New failure-containment test deliberately panics with two live relay PTYs after lost-response reconciliation and the second PID/PWD marker. Cleanup closes each backend ID, asserts each held original PtySession is_reaped, joins gateway/relay runtime threads and removes the private root before rethrowing the exact injected panic. The test requires that exact panic, so cleanup assertion failures cannot masquerade as intended containment. The log prints pid=None after reap because the reaped handle no longer exposes PID; it is not a numeric PID receipt for the first failing-scenario shell. No claim retroactively repairs the historical timed-out run.

## Commands and results

Reproducible runner: `python3 docs/evidence/paired-daemon/A11-followthrough-owned-run.py`.
Exact argv, environment, before/after owned hashes, exits and supervisor removal: A11-followthrough-owned-isolated.json. Unabridged output: A11-followthrough-owned-isolated.log.

All Cargo commands use --locked, absolute src-tauri/Cargo.toml, --no-default-features, private existing target, jobs2, dev/test debug0, incremental0, empty wrapper, normal Cargo/Rustup homes. Before library initialization the runner strips ALL inherited FERRYX variables and supplies private HOME/runtime/data/session/XDG/TMP and agent-state socket beneath /tmp/a11-followthrough-clean-q3xaiv19. The supervisor is removed in finally. Loader path explicitly selects the private target's libghostty-vt directory.

- cargo test --test relay_pairing_generation_regression -- --nocapture --test-threads=1: exit0, 8 passed, none skipped.
- cargo test --lib remote::relay_server::tests::a11_expired_ticket_wire_boundary -- --nocapture: exit0, 1 passed.
- cargo check --bin ferryx-cli --bin ferryx-relay: exit101, outside-scope concurrent composition error below.
- Final clean-environment build not run after check failure. Earlier owned-final.log records check/build exit0 on earlier composition, not approval of the later bytes.
- LSP on changed integration fixture and inherited relay_server: no diagnostics. git diff --ignore-submodules=all --check: exit0.

Earlier baseline/real/final logs are preserved. Their supervisors had private main directories but inherited FERRYX_AGENT_STATE_SOCKET and FERRYX_SESSION_ID; that audit defect motivated the corrected final isolation run. Those runs are NOT the final isolation receipt; no claim of complete canonical-state exclusion is inferred from them.

## Exact external blocker

Final cargo check fails E0061 at remote/workspace_api/worktrees.rs:459 and :48: worktree_committed is called with three arguments, but concurrent daemon/workspace_service.rs:99 now requires fourth argument &machine_protocol::Worktree. These are outside relay ownership. No workaround or edit was made. The previous malformed journal attribute is fixed by its owner and did not block these tests. Final composed check/build and stable aggregate remain NOT READY until the worktree/event owner finishes this API composition. Do not relabel this compiler failure behavioral RED or a relay regression.

## Owned source hashes

- relay_server.rs: 0ccad7f19422b34ac8bedf3f3077b20af06b52370d52f8b12bd117116c541e57 (inherited, not edited here).
- relay_client.rs: 386abd5a2c71c0889912d6790b74bbf77352e2b75fab7d9bf0ac3f48a8786c0e (unchanged).
- relay_pairing_generation_regression.rs: 5813c51e43b8c51eb222136de7f4afa44d93641c08959f7ab3eff1f5fb195a6c.

Before/after hashes match for these owned files during the isolated run. Concurrent outside-source mutation means this is NOT immutable aggregate provenance.

## Claim boundaries

The client fixture receives only relay HTTP/WS origins; only the reverse client receives the direct gateway address. This proves test-client relay routing, NOT OS-enforced firewall exclusion, A14 native-client negotiation, native menus/rendering, final Linux/Windows composition, deployed relay compatibility, release readiness or full AC acceptance. terminalStreamV1 capability enablement and event completeness remain outside scope. The historical allowlist/body tests prove controlled transport for all admitted methods; they are not actual Git mutation tests. No gateway/service/journal/PTY/Cargo/UI edits, capability enablement, remote host writes, desktop launches, commits, deployments or releases were performed. All changes remain uncommitted.
