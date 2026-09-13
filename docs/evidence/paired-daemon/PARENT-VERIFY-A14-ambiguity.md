# Parent verification: native IPC mutation ambiguity repair

Child `st_01a0996f` reported completion with one failing test. The parent
re-ran and re-read everything before accepting.

## Parent's own run

```
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --lib paired_host:: -- --test-threads=1
test result: ok. 27 passed; 0 failed; 0 ignored; 994 filtered out
exit=0
```
Evidence: `A12-session-metadata-parent-ph-suite.{log,exit,cleanup}`.

The child reported `26 passed; 1 failed` on
`paused_authenticated_response_cannot_adopt_after_repair_or_forget`
(`service.rs:90`, `assertion failed: refused`, `listener_refused=false`).
It does **not** reproduce at `--test-threads=1`. Classification: a parallel
execution race over listener/port refusal, the same family as the previously
documented `192.168.0.34:<port>` bind race. Not a regression from this repair,
and the child correctly refused to edit that unrelated test.

## Source review (parent read the diff, not the summary)

`src-tauri/src/daemon/client.rs` now derives the mutation identity at the IPC
boundary:

```rust
let request_id = match &request.operation {
    Operation::RegisterProject   { request }     => Some(request.request_id.clone()),
    Operation::UnregisterProject { request, .. } => Some(request.request_id.clone()),
    Operation::CreateWorktree    { request }     => Some(request.request_id.clone()),
    Operation::DeleteWorktree    { request }     => Some(request.request_id.clone()),
    Operation::CreateSession     { request }     => Some(request.request_id.clone()),
    Operation::CloseSession      { request, .. } => Some(request.request_id.clone()),
    ...
};
...
ambiguous: request_id.is_some(),
request_id: request_id.clone(),
```

Exactly the six mutation variants carry a request id; the eight read-only
variants have none, so they stay non-ambiguous without a blanket flag. The
35 s outer timeout, the 32 KiB request cap and the existing error codes are
unchanged, which the parent confirmed in the diff.

Before the repair, `From<ServiceError> for ClientError` in
`paired_host/client.rs` funnelled every transport failure through
`Self::local(&e.code)`, which hard-set `request_id: None, ambiguous: false`.
The child's RED log captures precisely that:

```
ClientError { code: "PAIRED_HOST_UNAVAILABLE", machine_error: None,
              request_id: None, ambiguous: false }
test result: FAILED. 1 passed; 2 failed
```

and the GREEN log shows the repaired behavior:

```
request_id: Some("3941b9de-b16d-4d9a-ae0a-118f90fd91f4"), ambiguous: true
peer_still_in_flight=true cleanup=true
A14 all_read_only_non_ambiguous=true lookup_id_not_mutation=true
test result: ok. 3 passed; 0 failed
```

## Timeout finding (recorded, not silently dropped)

The child substantiated the aggregate review's "budget cut short" claim instead
of assuming it: `paired_host/client.rs:308-311` allows a 40 s mutation attempt
after the capability and journal reads, while `daemon/client.rs:423` stops
native waiting at 35 s. The early native return is **kept** (the outer timeout
was required to stay), but it now returns with reconciliation identity intact.
Still unproven: whether a remote commit actually lands after that deadline.

## Verdict

Accepted as scoped complete. This closes the A14 aggregate review's blocking
FAIL. It is not full-plan acceptance.
