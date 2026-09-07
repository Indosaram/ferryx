# Windows target teardown repair phase

## Scope and independence

D6 can be reproduced at the actual `WindowsCompositorTarget` destructor with a
real child HWND. The lead read its current constructor/destructor and LSP
symbols. Target teardown is private to `platform/windows.rs`; a module-local
Windows test can exercise it without adding a public test API or editing the
host/IPC files currently owned by the Wayland worker.

Run a separate G008 workflow: `windows-repair` (`deep`, native platform logic)
followed by `windows-verify` (`deep`, native contract verification).
Allowed source write scope in the isolated repair worktree is
`src-tauri/src/native_terminal/platform/windows.rs` only, including its
Windows-only test module. An unavoidable wider interface change requires
coordination before editing. No host, IPC, renderer, Linux, UI, manifest or
vendor changes. The worker must not stage or commit; the lead serializes Git
index operations after the other producers finish.

## Faithful native RED

The proposed integration target from diagnosis is replaced before implementation
by a module-local test at the actual private destructor seam:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows::tests::child_is_destroyed_on_owner_thread_when_detached_by_worker -- --exact --nocapture
```

On a real Windows runner, create the actual child class/HWND and actual
`WindowsCompositorTarget` on a pumping owner thread. Register destruction
observation before moving/dropping the target on a worker. Capture
`WM_NCDESTROY` and its thread ID. Use explicit queued-message/event ordering to
decide failure, not fixed sleeps or successful polling. A bounded deadline may
only fail a stuck test. Clean up any surviving window on its owner before
reporting the RED assertion.

This must exercise the production target destructor, not merely demonstrate
that generic cross-thread `DestroyWindow` fails. A missing toolchain, zero
matched tests or child-creation failure is not the required RED.

## Minimal repair contract

Destroy the target on its owner thread without synchronously waiting under the
host mutex. Preserve surface-before-target drop order, pointer transparency and
existing initial-hidden/reveal behavior. Check same-thread destruction and
already-destroyed-parent behavior as adjacent cases. Do not add a general thread
dispatcher, a registry, or a host/IPC refactor unless the actual API contract
requires it and the lead coordinates the overlapping scope.

## Windows runner and evidence boundary

Read-only preflight already passed:

- `maho-win` is reachable through existing SSH authentication.
- `cargo --version` reports `cargo 1.97.0`.
- `C:/Users/sook/ferryx-winbuild/orca-lite` is at
  `e2a19066fe36f126d62ffecc952f3dc0b5f3258a`, not the local reviewed base.

Use an isolated, uniquely owned Windows test checkout/staging area; existing
remote source, installed apps and user sessions remain untouched. Survey dirty
state, disk and running build processes before staging. Do not clean or replace
foreign caches. Transfer only an explicitly identified source snapshot and
owned test/repair delta, record hashes, and preserve exact base identity.
This is temporary native API validation, not a remote app rollout.

Only `cargo test` runs test binaries. No installed/release/debug Ferryx app is
launched directly. SSH Session 0 can prove native API/test behavior, but cannot
prove a visible user desktop window. Real Windows tab/overlay screenshots remain
separately pending for aggregate acceptance.

Write `windows-repair.md`, `windows-verification.md`, raw RED/GREEN logs, source
hashes and cleanup receipts under the rendering report/evidence directories.
Every owned HWND, process and remote staging artifact must have a cleanup
receipt; preserve the local repair diff for later verified integration.
