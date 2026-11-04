# Todo 14 implementation handoff

- Branch: rex/t14. Implementation and tests only; no build, test, mutation, QA,
  host lease, or CI command was executed, per the implement-first instruction.
- `git diff --check` passed during source inspection. This is not compile evidence.
- server.rs and protocol.rs are named integration points in todo 14. Their edits
  are additive: owner preference request, capability, callback sink, and test.
- Clipboard permission is checked against the actual local PTY registry before
  callback delivery and again before desktop event publication. SSH and paired
  sessions cannot acquire permission from the global setting.
- Clipboard reads return without replying, which the vendored header explicitly
  specifies as denial. The daemon suppresses parser PTY replies.
- The owner preference is an atomic durable JSON record beside the persistent
  daemon lock, read on startup, including successor startup and Windows restart.
  Cwd remains parser-owned and travels in Ghostty's existing VT seed formatter.
- Notifications and clipboard effects are transient, not retained for reconnect.
- The per-session two-second limiter uses process-local monotonic time; its last
  timestamp is not transferred in handover. A successor can deliver an effect
  within two seconds of its predecessor's last effect. This remains a plan-wide
  state-survival gap requiring integration follow-up.
- The vendored clipboard callback normalizes OSC 52, 1337 and 5522 to one request
  without a protocol discriminator. This implementation gates all standard,
  single text/plain writes identically. It never grants remembered permissions
  and does not acknowledge 5522 writes. Strict OSC-52-only acceptance cannot be
  distinguished from this callback alone and needs a parser API decision.
- Acceptance command (not run):
  `cargo test --manifest-path src-tauri/Cargo.toml --lib -- native_terminal::osc_effects notification`
- Additional authored tests include `terminal::fuzz_tests::osc_effects`, daemon
  stream publication, settings persistence, and the settings toggle.
- The assigned worktree is retained for the parent integrator; no other
  worktrees, branches, production processes, or canonical files were modified.
