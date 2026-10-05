# Linux verbatim retrieval — daemon_handover_transfer_contract list-gate compile failure

Host: linux (indo@100.91.254.71, hostname `indo`)
Command: `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list`
cwd: /home/indo/ferryx-pane-completion/source-21dea3c0
Raw native exit: 101
Candidate at time of run: d97233c1 delta (pre-6c69715f)
Log: pass3/linux/logs/handover-list.log (lines 629-636)
Record: pass3/linux/rust-commands.jsonl entry id=handover-list, selectionStatus=COMPILE_FAILED, status=RAN_FAILED

## Verbatim

```
warning: `ferryx` (lib) generated 75 warnings (run `cargo fix --lib -p ferryx` to apply 19 suggestions)
error[E0063]: missing field `local_split` in initializer of `DaemonRequest`
   --> tests/daemon_handover_transfer_contract.rs:109:28
    |
109 |             .send_request(&DaemonRequest::Spawn {
    |                            ^^^^^^^^^^^^^^^^^^^^ missing `local_split`

For more information about this error, try `rustc --explain E0063`.
error: could not compile `ferryx` (test "daemon_handover_transfer_contract") due to 1 previous error
warning: build failed, waiting for other jobs to finish...
```

## Consequence recorded

The dependent gate `handover` (the same target with `--nocapture --test-threads=1`) was recorded
`NOT_RUN_BLOCKED` with reason `prerequisite compile/list gate failed for configuration handover`
and causeLog `pass3/linux/logs/handover-list.log`.

## Extracted facts

- Site: `tests/daemon_handover_transfer_contract.rs:109:28` (`DaemonRequest::Spawn` initializer)
- Missing field: `local_split`
- Error code: `E0063`
- Raw native exit: 101; selected count: null (COMPILE_FAILED)
- This is the *list* gate, so the run gate was never reached — NOT_RUN_BLOCKED, never a pass.
