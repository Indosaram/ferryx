# Owner Luau runtime

Scripting is off by default. Set `{"scripting":{"enabled":true}}` in
`<data>/settings.json`, preserving any other settings. The daemon reads this at
startup. Only the owner's `init.luau` is loaded:

- Linux and macOS: `$XDG_CONFIG_HOME/ferryx/init.luau`, defaulting to
  `$HOME/.config/ferryx/init.luau`.
- Windows: `%APPDATA%\ferryx\init.luau`.

No current-directory or repository file is considered. These paths intentionally
do not use the macOS application-support data directory.

```lua
ferryx.after(100, function()
    ferryx.log("tick")
end)
```

`after(milliseconds, callback)` is one-shot, returns a timer identity, accepts a
positive delay, and refuses more than 64 pending timers. Register another timer
inside the callback for repetition. `log(string)` appends to `<data>/script.log`;
read it with `ferryx-cli script log`. Script errors are appended there too. An
initialization error discards its pending timers but does not stop the daemon.

The VM has a 64 MiB allocator limit and a 50 ms per-invocation thread CPU budget,
enforced by the Luau interrupt callback. The dedicated thread holds no daemon
request loop. Host disk operations use the IPC blocking pool. Dynamic loading,
host libraries, and protected calls that could catch the interrupt are absent.

Timer identities combine the source SHA-256 with registration order. Claims in
`<data>/script-ledger.json` are durable before effects, giving at-most-once rather
than transactional exactly-once execution: a crash after the claim may lose an
effect. A reload suppresses the same timer inside its period. Initialization log
effects are deduplicated for the same source and call order across reloads.
Changing the source gives its effects new identities. The ledger is retained
across Windows idle restart and carried in the Unix handover decision record.
Commit waits for a running callback, pauses predecessor execution, and snapshots
the ledger; a failed decision with no durable record resumes the predecessor.

The successor reads the committed ledger before starting its own worker. It
reloads source rather than transferring VM objects. Only `after` and `log` are
provided here; broader host bindings belong to todo 24.

No build, test, QA, or mutation command was run during implementation, per the
implement-first instruction. The integration verification must resolve the new
Cargo dependencies and update Cargo.lock before running locked build gates.
