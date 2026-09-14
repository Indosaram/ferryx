# A14 start and A10 follow-through

Full goal remains incomplete: 16 of 48 tracked tasks closed, 32 open.
No full A06-A14 acceptance is inferred from scoped tests.

## Active work

- A14 run `dag_eee987ec-f28d-4b90-948b-8466717d9c74`:
  `native-client` and `ui-adapter` run in parallel, followed by
  `native-integration`, then one `aggregate-verifier`.
  Definition and shared command contract are in `A14-composition-contract.md`.
  Native producer task is `st_01a09920`. No A15 runtime node was started.
- `st_01a098de` continues actual owner metadata IPC recovery, truthful partial
  state and event-reset reliability. It retains metadata-specific ownership.
- `st_01a09914` continues real relay PTY reset repair.
- `st_01a09922`, `a10-saturated-socket-final`, owns actual saturated machine
  WebSocket input/controller isolation proof and minimal fixes if reproduced.
  It owns only controller/input blocks in remote/server.rs and
  daemon/session_service.rs, new tests/machine_input_cancellation.rs and
  scoped support/evidence. Metadata and relay owners were kept separate.

## Parent observations

Read actual `handle_machine_terminal_socket`: it holds the global controller
map mutex across cancellable PTY input. New socket admission takes the same
mutex before fencing the prior generation. Potential consequences are stalled
unrelated panes and delayed replacement. The bounded reader queue may also
delay observing Close while input is saturated. These are source hypotheses,
not reproduced failures yet. The new worker must capture real kernel/WS RED
and prove cancellation, generation authority and sibling responsiveness.

Earlier portable input and final-close workers are completed and evicted.
Their work is retained; they were not duplicated or restarted. Existing
separate-process retirement and parent input checks remain scoped evidence.

Parent attempted the full current `machine_owner_handover` test under monitor
`mon_69HX9YWDNPVF96TV`, bash_136, with private env-i supervisor, umask 077,
existing target, jobs 2 and no canonical paths. This run did not reach a test:
A14 declared client/projects modules before their files existed, producing
E0583 and exit 101. Log: `A10-owner-parent-current.log`.
The producer was notified to keep module declaration gaps short.
`/tmp/a10-owner-final.9yPtjf` and its empty child directories were removed with
rmdir. No behavioral failure or successful owner coverage is claimed.
Rerun after producer readiness, not continuously against moving source.

Markdown LSP is not configured, so the A14 contract has no LSP diagnostic
result. No new language server or dependency was installed for a prose file.
Existing UI PushClient failures and all composed platform gates remain open
as documented in their prior reports. Source and evidence are uncommitted.
