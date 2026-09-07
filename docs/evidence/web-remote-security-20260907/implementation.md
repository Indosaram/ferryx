# Web remote security implementation evidence

## Scope and isolation

- Base: detached worktree at `37272f5`, `/Users/indo/code/project/ferryx-web-remote-security-20260907`.
- Production changes: `src-tauri/src/remote/auth.rs` and `server.rs` only.
- Tests: `tests.rs` includes narrowly scoped `security_tests.rs`, `security_socket_tests.rs`; `auth.rs` includes `auth_security_tests.rs`.
- No commits, main-tree edits, desktop interaction, live daemon connection, or production auth/config mutation. Existing remote tests create isolated disposable PTYs/daemon fixtures; they do not contact the production daemon.
- Read project/root and src-tauri AGENTS rules from the main checkout (the pinned worktree has no AGENTS files), plus programming/Rust README, async Tokio, concurrency, Axum, and type-state references.
- Build target: `src-tauri/target-security`, APFS copy-on-write clone of the macOS debug cache. Cargo/Zig output stayed in the isolated target. Vendored Ghostty source is symlinked read-only from the main checkout; its build script directs caches/output into OUT_DIR. No release Cargo build.
- Parent owns the QA example, UI build, and independent manual evidence. The implementation agent did not edit or launch that fixture. No manual-QA or reviewer approval is claimed here.

## Repairs

### 1. Unauthenticated static path escape

The request path is percent-decoded once and parsed before joining to disk. Dot components, backslashes, Windows drive/ADS prefixes, NUL, malformed escapes, residual nested escapes, and network-root paths are rejected with 400 on every host. Asset-root discovery runs through `ipc::run_blocking`; canonicalization and file reads are asynchronous. Both requested files and SPA `index.html` fallback are canonicalized and checked against the canonical asset root. Out-of-root symlink targets return 404. Reads use the canonical checked path.

Normal assets, encoded spaces in asset names, `/`, and arbitrary SPA navigation continue to return the asset/index content. Inventory and selection code are unchanged.

Threat boundary: this protects an untrusted HTTP caller, including pre-existing escaping symlinks. It assumes the application asset tree is locally trusted, not concurrently rewritten by a hostile local process between canonicalization and open; this is not an OS capability-directory sandbox.

### 2. Already-open sockets and revoke/upgrade races

`AuthManager` owns a per-device latched watch signal. Subscription holds the device lock before the signal-registry lock, matching revocation's lock order. A token validated just before revocation cannot register a new uncancelled socket afterward. The signal is retained across the HTTP upgrade callback gap and removed from the registry on device deletion; existing receivers retain cancellation.

Event/raw/grid socket lifetimes run under biased revocation cancellation, including initial snapshots and pending sends. Terminal attachment/initial resize is also cancellable and returns 401 when revoked before upgrade. Raw/grid send, input, focus and grid-writer pumps are locally owned futures, not detached spawned tasks: cancellation drops pending backend futures and every socket half. Other devices have independent signals. Durable token serialization and permission DTOs are unchanged.

Already-completed operations/bytes handed to the transport before revocation cannot be undone. The patch stops further socket work on cancellation; it does not retroactively undo PTY input already accepted before the revocation boundary.

### 3. View permission management

View callers may revoke themselves, but cannot revoke any other ID (403, including unknown IDs). Control callers retain device management; authorized unknown targets return 404, successful deletion returns 204. Existing tokens for unaffected devices remain valid.

### 4. Server-side pairing budget

One global five-failure budget covers the current 60-second pairing window. PIN lookup, single-use removal and failure accounting share one lock. Both invalid and expired attempts spend budget; all exchanges after exhaustion, including correct PINs, return HTTP 429 JSON `{"code":"pairing_rate_limited"}`. Neither creating/rotating PINs nor successful pairing resets the budget. No client IP or forwarded header is trusted, so changing source identity cannot multiply attempts.

At window expiry, old window PINs are discarded and a fresh budget is available. A PIN created late in a window therefore has less than 60 seconds remaining (never more); callers must request a fresh PIN for the next window. Exhaustion deliberately denies even valid pairing until the window rolls over. The budget is ephemeral with the already-ephemeral PINs; persisted paired devices/tokens remain compatible.

## RED before production fixes

All commands ran from the isolated worktree with:

```sh
export CARGO_TARGET_DIR="$PWD/src-tauri/target-security"
export CARGO_BUILD_JOBS=4
```

Default features include `native-terminal`; no `--no-default-features` was used.

| Log | Command/test filter | Observed RED (exit 101) |
| --- | --- | --- |
| `red-http.log` | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::tests::security -- --nocapture` | Six failures before HTTP/auth fixes: raw traversal returned `PRIVATE_SENTINEL` (200 rather than 400); symlink escape returned `SYMLINK_SENTINEL` (200 rather than 404); encoded traversal returned 200; View revoked Control (204 rather than 403); unknown other-device View returned 404 rather than 403; 20 parallel invalid PIN requests all returned 400 instead of exactly five 400 and fifteen 429. |
| `red-sockets.log` | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::tests::security::sockets -- --nocapture` | Seven failures before socket fix: events/raw/grid delivered post-revoke output; raw/grid pending input sockets never closed within the bounded deadline; raw/grid revocation during blocked attachment returned 101 instead of 401. |
| `red-upgrade.log` | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::tests::security::sockets::revoke_before -- --nocapture` | Three failures before socket fix: events/raw/grid authorized responses deliberately held before the upgrade callback still emitted their initial snapshot after revocation. |

The first socket build invocation hit the tool's 150-second compilation timeout before any test output; it was rerun with a 600-second command budget. The recorded socket RED contains actual behavioral failures, not a compiler/timeout substitute for RED.

## GREEN and final verification

| Log/result | Command | Result |
| --- | --- | --- |
| `green-http.log` | First HTTP filter above after HTTP fixes | 6 passed, 0 failed |
| `green-security.log` | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::tests::security -- --nocapture` | 16 passed, 0 failed, native grid enabled |
| `remote-tests.log` | `cargo test --manifest-path src-tauri/Cargo.toml --lib remote:: -- --nocapture` | **85 passed, 0 failed, 0 ignored**; includes six additional auth concurrency/clock tests, persisted auth, native mirror/grid, real PTY control, daemon ownership, focus-change disconnect, and focus-independent registered-project inventory |
| LSP diagnostics | All six changed/new Rust files, severity all | No diagnostics found on final wave (earlier freshness requests timed out/cancelled; those were not counted as clean) |
| `cargo-check.log` | `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0, default/native-terminal features |
| `example-build.log` | `cargo build --manifest-path src-tauri/Cargo.toml --example web_remote_security_qa` | Exit 0; parent's runnable public-API fixture compiled but was not launched by implementation agent |
| `related-integration-tests.log` | `cargo test --manifest-path src-tauri/Cargo.toml --test scoped_control --test clean_dev_resource_contract` | Resource contract 1/1 passed; scoped control 4 passed, **1 unrelated existing failure**, command exit 101 |
| Diff hygiene | `git diff --check --ignore-submodules=all` | Exit 0 |

The new tests use real loopback HTTP/router and raw WebSocket framing, not source/prose checks. Socket tests replace only the PTY boundary with a real output hub plus exact Notify-gated backend futures; Ghostty grid parsing is real. Response-layer middleware holds the actual 101 response to deterministically exercise the upgrade gap. Reads and notification waits use bounded deadlines, not sleeps/polling. Pending-input destruction is asserted after socket closure as an explicit lifecycle barrier. The full remote suite separately exercises real disposable PTYs and the parent owns independent real-surface manual QA.

Auth tests use simultaneous OS threads and barriers to prove exactly five failure slots under contention, concurrent single-use exchange, a boundary clock advanced without sleeps, expiry/replay rejection, successful pairing without resetting failures, persisted View credentials, and late revocation subscription rejection.

## Unrelated failures and warnings

The extra scoped-control target fails:

```text
control::lease::rejects_competing_controller_until_expiry_or_revoke ... FAILED
called Result::unwrap_err() on an Ok value:
ControlLease { target: TargetRef { host_id: "h", owner_id: "o", epoch: Epoch(1),
backend_session_id: "s" }, device_id: "two", lease_id: "lease-two", expires_at_ms: 120 }
```

`src/ferryx_scope/control/lease.rs` unconditionally inserts/replaces a lease, while its test expects ControlConflict for another controller. It does not use this patch's auth or socket implementation. The file is byte-identical to `37272f5`: baseline and worktree SHA-256 both `5c34443223fc35f8dd04b12030a66d81a519369b634e546728d9b6eaa502831d`. No unrelated fix, test deletion or skip was made.

Cargo reports seven existing warnings outside changed files: notification `Manager` import in non-test builds (native input `super_key` in test builds), unnecessary mutable font buffer, unused notification app field, unused PTY `wait_and_reap`, and unused writer-lease guard/items/acquisition method. Complete output is retained in the logs; warnings were not suppressed.

All implementation changes remain uncommitted. The generated target and dependency symlinks are local build setup, not patch files. Parent manual QA and the final single gate review remain independent obligations.
