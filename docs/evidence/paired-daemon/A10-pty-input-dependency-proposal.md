# Portable PTY input dependency coordination

Proposed exact shared-file delta (not yet applied):

- src-tauri/Cargo.toml: `portable-pty = "0.9"` becomes
  `portable-pty = { version = "0.9", path = "vendor/portable-pty" }`.
- src-tauri/Cargo.lock: existing portable-pty0.9.0 record keeps name/version and
  dependencies; remove registry source/checksum only. No dependency upgrades.

New private vendor/portable-pty contains upstream0.9.0 source and licenses.
Adapter changes: Windows ConPTY input pipe is a byte-mode nonblocking named pipe;
MasterPty exposes an optional duplicated Windows input handle. Existing blocking
writer semantics are preserved by its blocking wrapper. New machine seam writes
nonblocking with cancellation/deadline owned by the future; no submitted
overlapped write or synchronous writer worker survives drop.

Native Windows facility was contacted read-only successfully on maho-win. New
staging root will be independent of Q4. Jobs<=2. No socket/event integration edits.
