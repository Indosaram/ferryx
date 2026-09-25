# Lane 1 — comment-only edits in `src-tauri/src/daemon/server.rs` (2026-09-24)

Scope: exactly two doc-comment edits in `src-tauri/src/daemon/server.rs` — no code lines changed,
no other repo file written by this lane.

## Tooling status first (read before trusting anything below)

This lane's `eval`/shell tool was permission-denied (2 consecutive rejections: one 3-command batch,
then one single read-only `grep`), so `sed -n`, `grep -n`, and `git diff --stat` could NOT be run
here. All evidence below is from read-only anchored file reads (`read` with explicit 1-indexed
offsets), which give exact line numbers because the first output line equals the requested offset,
plus a file-length delta check. Commands left for the designated verify node:

```sh
grep -n "always finds a token that is already written" src-tauri/src/daemon/server.rs   # expect no hits
sed -n '875,880p' src-tauri/src/daemon/server.rs                                        # edit 1, new text
sed -n '969,976p' src-tauri/src/daemon/server.rs                                        # edit 2, doc block + new paragraph
git diff --stat src-tauri/src/daemon/server.rs                                          # expect exactly this one file
```

## Edit 1 — doc block above `publish_transport_rendezvous_internal`

Old text (was lines 875-879; anchored pre-edit read at `offset=875`):

    /// Publishes this boot's loopback rendezvous pair in the order a reader depends on: the previous
    /// boot's token is dropped first, this boot's token is written second, and the port is published
    /// last. A port on disk is therefore never ahead of the credential beside it: a reader that finds
    /// a port always finds a token that is already written, so its first attempt can only fail on a
    /// port a previous boot left behind.

New text (now lines 875-880; anchored post-edit read at `offset=875`):

    /// Publishes this boot's loopback rendezvous pair in the order a reader depends on: the previous
    /// boot's token is dropped first, this boot's token is written second, and the port is published
    /// last. The ordering guarantees that this boot's port is never on disk before this boot's token.
    /// It does not extend to a port a predecessor left behind: while this boot is republishing, a
    /// reader can still find that port with no token beside it, an interval that is harmless because
    /// the daemon rejects a credential it did not mint and `DaemonClient` re-reads both halves once.

Why: the old sentence overclaimed ("always finds a token that is already written"). During
publication the predecessor's port can still be on disk with no token beside it (old token removed
first, this boot's token written second, port last). The second paragraph is unchanged and still
names `remove_stale_socket_after_lock` (now lines 881-886).

Post-edit structure of this doc block: paragraphs at 875-880, 882-886, 888-890; blank `///` comment
lines at 881 and 887; `#[cfg(any(not(unix), test))]` at 891; `fn publish_transport_rendezvous_internal`
at 892.

## Edit 2 — doc block above `publish_agent_state_rendezvous`

Old text (was lines 968-969; anchored pre-edit read at `offset=968`):

    /// Publishes the ingress endpoint as one record renamed into place: a reader can never observe
    /// this boot's port beside a previous boot's token, or the reverse.

New text (now lines 969-976; new paragraph at 972-976; anchored post-edit read at `offset=963`):

    /// Publishes the ingress endpoint as one record renamed into place: a reader can never observe
    /// this boot's port beside a previous boot's token, or the reverse.
    ///
    /// `fs::rename` replaces an existing destination on every target: Unix `rename(2)`, and Windows
    /// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (see `library/std/src/sys/fs/windows.rs`),
    /// matching the `std::fs::rename` documentation's "replacing the original file if `to` already
    /// exists". A Windows reader does not block the replace: std opens files with a default share
    /// mode of `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.

Post-edit: `#[cfg(any(not(unix), test))]` at 977, `fn publish_agent_state_rendezvous(` at 978, the
`fs::rename(&staged, ...)` call at 985 — all code lines byte-identical to pre-edit.

Platform claims were verified in the installed toolchain sources BEFORE writing that paragraph:

- `/Users/indo/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/std/src/sys/fs/windows.rs`
  — `pub fn rename(old: &WCStr, new: &WCStr)` calls `c::MoveFileExW(old.as_ptr(), new.as_ptr(),
  c::MOVEFILE_REPLACE_EXISTING)`.
- same file — `OpenOptions::new()` sets `share_mode: c::FILE_SHARE_READ | c::FILE_SHARE_WRITE |
  c::FILE_SHARE_DELETE`, and `File::open_native` passes `opts.share_mode` to `CreateFileW`, so a std
  reader's open handle does not block the replace.
- `.../library/std/src/fs.rs` — `pub fn rename` docs contain, verbatim: "Renames a file or directory
  to a new name, replacing the original file if `to` already exists."

## Verification evidence (no shell)

- Anchored post-edit read `offset=875, limit=20` returned the new paragraph on lines 875-880, with
  `#[cfg]` at 891 and the fn signature at 892 — all counted directly from the returned lines.
- Anchored post-edit read `offset=963, limit=26` returned the new paragraph on lines 972-976, with
  `#[cfg]` at 977, the fn at 978, and the `fs::rename` call at 985.
- File-length delta: pre-edit the read footer said "5900 more lines ... offset=1010" => 6909 lines;
  post-edit it says "6022 more lines ... offset=895" => 6916 lines. Delta = exactly +7 lines
  (edit 1: 5 -> 6 lines = +1; edit 2: 2 doc lines -> 8 doc lines = +6). The size arithmetic matches
  the intended insertions exactly.
- The phrase "always finds a token that is already written" is absent from both edited blocks in the
  post-edit reads; a file-wide grep could not be executed in this lane (shell denied) — run the
  `grep -n` command above for the file-wide claim.
- `lsp_diagnostics` could not run (LSP daemon request timeouts, 3 attempts); the change is
  comment-only and the anchored reads show every code line intact.

## Files written by this lane

- `src-tauri/src/daemon/server.rs` — the two comment edits above (only these).
- `.omo/evidence/cross-platform-audit-fixes-2026-09-24/lane1-comment-edits.md` — this log.
