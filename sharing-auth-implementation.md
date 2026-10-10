# Secure Session Sharing & Gateway Authorization Implementation

## Overview & Scope
Implements self-hosted session share issuance, atomic exchange, permission-gated join, live expiry deadline cancellation, and instant revocation in Axum gateway. Addresses parent reviews: single authority transaction, watch `send_replace` retention, bounded registry pruning, saturating TTL arithmetic, consistent `>=` expiry checks, fail-closed socket tickets, and deterministic barrier tests. Preserves UI-worker owned tests. Shared repo sources are strictly READ ONLY; all forward edits derive strictly from dirty baseline `9c07425a0f901f76a7c4fb813e161dd2628f7410` in `/tmp/ferryx-superlogical-sharing-state`.

## Artifact & File Hashes (SHA-256)
- `sharing-auth-forward.patch`: `276844fc7836d1d85d0852e0bdea73eeba5c4a7149363fc48dc0ae136616428b`
- Baseline `src-tauri/src/remote/mod.rs`: `f2a030886da63f36bf16dad837402508463583e8c88e6273632472d4e01e4827`
- Baseline `src-tauri/src/remote/server.rs`: `a4a1a188ee93f26eff5560ffe1a4c660d9f11c87f0d19ab35b3c795da9e08a1a`
- Baseline `src-tauri/src/remote/state.rs`: `6ed1f6f21b5f9ea7f0915ea2273f4b0c7d720ee32e54084db126807ff1cbacd2`
- Work `src-tauri/src/remote/mod.rs`: `ae29a1daca3db88c70eb1e7e1c3856dd6fd56c89eb33dfe0da27c88ebc239b8b`
- Work `src-tauri/src/remote/presence.rs`: `0aa53d0c0ac514f540b054964588ec6c2fb5285713b6648334934c2caaf1db20`
- Work `src-tauri/src/remote/server.rs`: `d537daa1ad6fc17ce35530d1f724f621fb5741b55b1caa3d2fc4699da8f4455b`
- Work `src-tauri/src/remote/share_token.rs`: `ff7f4886fa020f7febac86c46ab7ab7ced5557b5cbab30a4e1ee3a7568ebae3b`
- Work `src-tauri/src/remote/state.rs`: `8e22c57501848d6d7f561a33778d2c4f63db79831753024e5bf311ae4db99fa3`

## Security Flow Matrix & Concurrency Invariants
| Flow | Route / Path | Mechanism | Invariant / Security Enforcement |
|---|---|---|---|
| Issue | POST `/api/v1/shares` | `create_share_handler` | 128-bit CSPRNG, 60s exchange TTL, clamped TTL (1s..30d) with saturating arithmetic. |
| Single Transaction | POST `/api/v1/shares/exchange` | `exchange_share_handler` | Removes code, checks tombstone, inserts active grant in 1 atomic step. |
| TTL Consistency | Exchange / Join | `PendingExchange.grant_expires_at` | Binds fixed creation expiry; exchange does not extend or reset claimed lifetime. |
| Anti-Resurrection | Authority State | `revoked_share_ids` Tombstones | Tombstone recorded on revoke; exchange fails with `RevokedToken` if revoked while pending. |
| Live Expiry Cancellation | Terminal & Events WebSockets | `while_authorized(..., deadline)` | Bounded `sleep_until(deadline)` terminates active PTY & events streams at `>= expiry`. |
| Presence Cleanup | WebSocket Teardown | `PresenceGuard::drop` | Dropping expired socket triggers presence leave & push event with zero sleeps. |
| Fail-Closed Tickets | POST `/api/v1/socket-ticket` | `issue_socket_ticket` | `revocation_receiver` maps errors directly to 401 Unauthorized; checks `*rx.borrow()`. |
| Fresh Record Selection | GET `/api/v1/shares/{id}` | `get_share_handler` | Uses presented validated grant on share auth; selects fresh canonical record on owner auth. |
| No-Referrer | All join/share paths & `/join` | `standard_security_headers` | `Referrer-Policy: no-referrer`, `Cache-Control: no-store` prevents code leak. |
| View / Write Gating | WS `/api/v1/terminal/{id}` | `ws_terminal_handler` | View role drops binary PTY input, blocks resize, forbids share issuance. |
| Worktree Jail | Gateway router | Root jail verification | Share token grants zero fs/worktree API capability; cannot leak repository paths. |

## Authority & Dependency Contract
- Upstream `writer-paste-implementation.md` remains blocked. Backend routes provide explicit authority guards.
- Runtime execution is UNRUN per policy; all cargo/bun tests are delegated to sole executor `st_01a0f80d` (`8b3`).

## Authored Tests (UNRUN)
- `remote::share_token::tests::test_csprng_token_length_and_uniqueness`
- `remote::share_token::tests::test_constant_time_comparison`
- `remote::share_token::tests::test_route_issue`
- `remote::share_token::tests::test_route_join`
- `remote::share_token::tests::test_route_reuse`
- `remote::share_token::tests::test_route_expiry`
- `remote::share_token::tests::test_route_revoke`
- `remote::share_token::tests::test_route_referrer`
- `remote::share_token::tests::test_route_viewwrite`
- `remote::share_token::tests::test_route_forgedidentity`
- `remote::share_token::tests::test_route_concurrentconsume`
- `remote::share_token::tests::test_atomic_revoke_exchange_race_barrier_deterministic`
- `remote::share_token::tests::test_checked_ttl_bounds_and_overflow_safety`
- `remote::share_token::tests::test_issue_join_ttl_response_consistency_no_extension`
- `remote::share_token::tests::test_live_share_expiry_deadline_cancellation_and_presence_cleanup`
- `remote::presence::tests::*`
- `remote::server::tests::test_sharing_auth_route_matrix`

## Verification & Targeted Remote Execution Command
- `diffapplycheck baseline only`: Verified via `git apply --check` (Exit: 0) and `patch -p1 --dry-run` (Exit: 0).
- Absolute report path: `/Volumes/T9-Mac/project/ferryx/sharing-auth-implementation.md`
- Absolute patch path: `/Volumes/T9-Mac/project/ferryx/sharing-auth-forward.patch`
- Remote test command for executor `st_01a0f80d` (`8b3`):
  `cargo test --manifest-path src-tauri/Cargo.toml --lib remote::share_token::tests remote::presence::tests remote::server::tests::test_sharing_auth_route_matrix`
