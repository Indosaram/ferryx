# A14 native IPC ambiguity repair

## Delivered

Production change is confined to `src-tauri/src/daemon/client.rs:445-472`.
The operation boundary captures the request ID for the six explicit mutation
variants before moving the request into IPC. Service/transport failures and
unexpected reply variants preserve that ID and set ambiguity only for mutations.
Typed `PairedHostOperationError` responses remain unchanged. Error codes remain
unchanged. Operation lookup is explicitly read-only despite having a request ID.
The existing generic ServiceError conversion is intentionally untouched.

The serialized request cap remains 32 KiB (`daemon/client.rs:410`); the outer
timeout remains 35 seconds (`daemon/client.rs:423`). No retries were introduced.
Conservative assumption: the existing ServiceError transport does not expose
delivery stage, so all boundary failures of mutations are treated as possibly
delivered, including failures before submission. Read-only failures remain clean.

## Budget finding and limitation

The aggregate claim is real, not refuted:

- `paired_host/client.rs:273` gives reqwest 40 seconds.
- `paired_host/client.rs:308-311` independently wraps mutation HTTP in a 40-second
  timeout and reads in a 10-second timeout.
- `paired_host/client.rs:346` performs capability HTTP first;
  `:368-397` then performs journal HTTP before `:398-405` submits mutation HTTP.
- `daemon/server.rs:1904-1906` awaits MachineClient execution directly; the IPC
  connection's disappearance is not a cancellation signal for that execution.
- The native client still stops waiting at 35 seconds, potentially before even
  one 40-second mutation attempt completes.

The minimal boundary repair makes this earlier return reconcilable rather than
claiming a definite failure. The virtual-clock test observes actual operation
receipt at the UDS peer, advances 35 seconds while keeping the peer open, and
awaits the client's completion signal before closing the peer. RED lost identity;
GREEN preserves it. This proves native timeout behavior deterministically, not a
real remote commit after 35 seconds. The 35-second timeout still truncates caller
waiting; removing that truncation while retaining the required unchanged outer
timeout is not claimed. No inner HTTP budget was shortened to hide the mismatch.

## Evidence

`A14-ambiguity-RED.log`:

```
A14 mutation error=ClientError { code: "PAIRED_HOST_UNAVAILABLE", machine_error: None, request_id: None, ambiguous: false } cleanup=true
mutation receipt followed by transport failure: ClientError { code: "PAIRED_HOST_UNAVAILABLE", machine_error: None, request_id: None, ambiguous: false }
test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 1018 filtered out; finished in 0.01s
EXIT_CODE=101
```

`A14-ambiguity-GREEN.log`:

```
A14 native_35s_deadline error=ClientError { code: "PAIRED_HOST_UNAVAILABLE", machine_error: None, request_id: Some("3941b9de-b16d-4d9a-ae0a-118f90fd91f4"), ambiguous: true } peer_still_in_flight=true cleanup=true
A14 all_read_only_non_ambiguous=true lookup_id_not_mutation=true cleanup=true
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1018 filtered out; finished in 0.02s
EXIT_CODE=0
```

The first mutation case and timeout assertions are the same failing-first tests.
After RED, DTO fields for later mutation cases (unreached after the first RED
assertion) were corrected before GREEN. All six variants then executed against
EOF, malformed JSON and an unexpected typed reply. No sleeps or polling.

`A14-ambiguity-paired-host-suite.log` records the exact required command once:

```
test paired_host::client::tests::real_machine_catalog_replay_checks_digest_and_preserves_metadata ... ok
A13 paused_authenticated_http=true subscribed_gate=true stale_repair_rejected=true forgotten_tombstone_rejected=true other_host_preserved=true listener_refused=false root_removed=true
thread 'paired_host::service::native_transport_tests::paused_authenticated_response_cannot_adopt_after_repair_or_forget' (2527278) panicked at src/paired_host/service.rs:90:9:
assertion failed: refused
test result: FAILED. 26 passed; 1 failed; 0 ignored; 0 measured; 994 filtered out; finished in 0.49s
EXIT_CODE=101
```

This is a separate existing cleanup test failure, not the ambiguity defect.
The aggregate's recorded failure was also separate: see
`A14-aggregate-reverify-01-rust.log:164-172`, where the catalog test failed at
`client_tests.rs:368` on `project.metadata.git_root.is_none()`. That test passed
here with GIT_CEILING_DIRECTORIES set to the isolated temporary root, preventing
fixture directories inside this worktree from inheriting its Git repository.
No catalog or A13 service test was edited, weakened, skipped, or retried.
The exact required suite is therefore **not green**; that requirement remains
blocked by the unrelated A13 cleanup failure. Its root cause is not proven here.

`A14-ambiguity-check.log` records the exact required cargo check:

```
warning: `ferryx` (lib) generated 19 warnings (run `cargo fix --lib -p ferryx` to apply 2 suggestions)
Finished `dev` profile [unoptimized] target(s) in 35.42s
EXIT_CODE=0
```

Diagnostics returned no diagnostics for all three changed Rust files (the final
test-file diagnostics call succeeded after one freshness timeout). The runner
passed `bash -n`. Existing compiler warnings were preserved. RED also captured
a relocated-toolchain rust-objcopy missing-libLLVM warning; an internal relative
symlink in the disposable copied toolchain repaired that tooling warning before
GREEN, with no production or Cargo configuration change.

## Files changed by this task

- `src-tauri/src/daemon/client.rs` (only operation boundary)
- `src-tauri/src/paired_host/mod.rs` (register test module)
- `src-tauri/src/paired_host/native_ambiguity_tests.rs` (new)
- `docs/evidence/paired-daemon/A14-ambiguity-RED.log`
- `docs/evidence/paired-daemon/A14-ambiguity-GREEN.log`
- `docs/evidence/paired-daemon/A14-ambiguity-paired-host-suite.log`
- `docs/evidence/paired-daemon/A14-ambiguity-check.log`
- `docs/evidence/paired-daemon/A14-ambiguity-run.sh`
- `docs/evidence/paired-daemon/A14-ambiguity-report.md`

The runner records the execution environment; its disposable toolchain/cache and
fixture roots are removed after validation, so it requires re-provisioning those
paths for replay. Cargo updated debug/check artifacts under this worktree's
`src-tauri/target`; no release build, deployment or commit occurred. Toolchain
and registry were copied read-only from installed tooling into the worktree;
compilation and tests used those copies and isolated HOME/runtime/data/temp paths.
No canonical daemon, real HOME data, running PTY, or protected repair file was
modified. No PTY was created. Test-owned UDS peers were joined and temporary
roots explicitly closed before assertions. The suite's A13 test logged its root
removed even though its listener-refusal assertion failed.
