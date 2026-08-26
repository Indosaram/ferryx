# Browser CLI startup reactor regression

Date: 2026-08-26

## Issue

Starting the Ferryx desktop app could abort during the synchronous Tauri `setup`
callback with:

```text
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

`start_browser_cli_server` was binding a `tokio::net::UnixListener` before the
Tokio runtime context existed.

## Fix

`src-tauri/src/ipc/browser_cli.rs` now:

1. Binds the Unix socket with `std::os::unix::net::UnixListener` during setup.
2. Marks that listener nonblocking.
3. Converts it to `tokio::net::UnixListener` inside `tauri::async_runtime::spawn`
   before accepting browser CLI requests.

The Unix socket remains private (`0700` parent directory and `0600` socket file).

## Automated evidence

- The plain-thread regression test reproduces the old panic without a Tokio
  reactor and now starts the server successfully.
- That test sends a real `{"command":"list"}` request over the Unix socket and
  requires the expected empty list response.
- The existing Tokio Unix-socket list round-trip test also passes.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 272 passed.
- `cargo build --manifest-path src-tauri/Cargo.toml`: passed.

## Manual desktop verification

Desktop UI automation is intentionally not used. On a local desktop:

1. Launch Ferryx normally.
2. Confirm it opens without the `there is no reactor running` panic or process
   abort.
3. In a separate terminal, run:

   ```bash
   src-tauri/target/debug/ferryx browser list
   ```

   Expect JSON array output. It may be empty when no Browser tabs exist.
4. Create one Browser tab in Ferryx, rerun the command, and confirm the returned
   JSON includes that tab's browser ID.

