# Phase 3 Task 7 Implementation Report

## Overview
Task 7 implementation approved according to `.omo/plans/relay-monetization.md`, `.omo/ulw-execute/phase3-root-review.md`, and root review criteria; awaiting remote build and test execution by root.

All work was completed strictly without local cargo build, local cargo test, local server execution, child subagents, or git commits. Remote verification by root is expected.

## Scoped Files and Status
- `src-tauri/src/account/billing/routes.rs`: Modified (2,156 lines, SHA256: 8725fc5e7bd13fc35e39fdb6767dc8e70f630bdce9daac4ed0f3aae3eabd0454). Balanced delimiters (parens=0, braces=0, brackets=0).
- `src-tauri/src/account/billing/routes_tests.rs`: Modified (3,038 lines, SHA256: f52b2cf0b359dc30bd2547e16d7e9aeee6052f2f4241c07da9a022ea7fbbc9bb). Balanced delimiters (parens=0, braces=0, brackets=0).
- `src-tauri/src/account/billing/mod.rs`: Maintained (exports `pub mod routes;`).
- `src-tauri/src/account/service.rs`: Maintained (merges billing routes, structured `ErrorBody` with `details`, self-host 404 isolation).
- `src-tauri/src/bin/account.rs`: Maintained (standalone account binary explicit deployment-mode wiring).

## Implemented Fixes and Changes

1. **Acceptance Page Same-Origin Issuer Binding and Canonical Token Contract** (`routes.rs`)
   - Aligned embedded `/org/accept` page with `accountSession.ts` storage and event contract:
     - Canonical storage keys: `CANONICAL_TOKEN_KEY = 'ferryx_remote_token_account'` and `TOKEN_ORIGIN_KEY = 'ferryx.account.tokenOrigin'`.
     - Validates issuer binding `localStorage.getItem(TOKEN_ORIGIN_KEY) === cleanOrigin(window.location.origin)` before reading `ferryx_remote_token_account`.
     - Removed all invented `ferryx.account.token` storage key usages.
     - On successful sign-in / poll approval (`status === 'approved'`), writes both canonical token and `tokenOrigin`, then dispatches canonical origin-only `ferryx:account-session` CustomEvent with `{ detail: { origin } }`.
     - On 401 unauthenticated response, clears both canonical `ferryx_remote_token_account` and `ferryx.account.tokenOrigin`, and dispatches `ferryx:account-session` event.

2. **Org Invite Replacement Semantics, Removal Revocation & Pool Grace Transitions** (`routes.rs`)
   - In `org_invite`, issuing a new invite for the same org and email replaces earlier unexpired invites for that address (`invite.expires_at > now && (invite.email != invite_email || invite.org_id != org_id)`).
   - In `org_remove_member`, revokes all outstanding invitations for the removed user's normalized email within that org during the removal transaction, preventing leftover tokens from being used to rejoin.
   - In `org_accept`, preserves existing `ROLE_OWNER` or `ROLE_ADMIN` membership role and returns the preserved role in the response.
   - In both `org_accept` and `org_remove_member`, immediately recomputes and persists the affected pool grace state transitions in the same mutate transaction using `evaluate_owner_and_persist`.

3. **Customer Portal Manage URL Redaction for Team Members** (`routes.rs`)
   - In `EntitlementResponse::from_evaluation`, checks if `evaluation.scope.org_id` is present and redacts `manage_url` to `None` for members without `ROLE_OWNER` or `ROLE_ADMIN`.
   - Protects customer portal bearer URLs from disclosure to plain team members while preserving access for owners and admins.

4. **Quantity Multi-Axis Reconciliation and Concurrent Demotion Defense** (`routes.rs`)
   - Pre-call check before subsequent axes strictly verifies that `current_scope.role == check_initial_scope.role` as well as requiring `ROLE_OWNER` or `ROLE_ADMIN`.
   - Transaction mutate check inside reconciliation loop strictly revalidates `current_scope.role == initial_scope.role` and role authorization before applying acknowledged provider facts.
   - If owner demotion or role change occurs while provider I/O is pending, the transaction rejects with typed `ORG_ROLE_REQUIRED` (403) and subsequent axis PATCH is never dispatched.
   - Authoritative provider responses acknowledged before later failure are reconciled locally without holding SQLite locks across HTTP requests.

5. **Webhook Duplicate Short-Circuit and Provider Outage Protection** (`routes.rs`)
   - Valid invoice webhook deduplication short-circuits authenticated duplicate events before calling `fetch_authoritative_subscription`.
   - Replay of an already-applied invoice returns `200 {"status":"duplicate"}` even during external provider outages, making zero additional GET requests.
   - HMAC signature is strictly verified prior to dedupe short-circuiting so tampered or invalid signatures always return 401.

6. **Deterministic Task 7 Tests and Provider Stub Improvements** (`routes_tests.rs`)
   - In `stub_get_subscription`, records `subscription_gets` before checking `fail_get` so provider GET requests/attempts are observable even during outages.
   - In `stub_patch_item`, records `item_patch_attempts` before checking `fail_patch` / `fail_second_patch`, providing proof of all attempted axis patches.
   - In `quantity_partial_second_patch_failure_reconciles_first_axis`, asserts exact attempted item IDs and quantities via `item_patch_attempts` (`[(ITEM_ID, 4), ("901", 3)]`) while verifying only the first was recorded in `item_patches`.
   - In `duplicate_invite_token_rejected_after_member_removal`, respects production invite replacement semantics (token1 is replaced and rejected with 400 `ORG_INVITE_INVALID`, token2 succeeds), then verifies that removing the member invalidates a leftover invite token for that member.
   - In `org_accept_page_serves_html`, asserts machine-consumed keys: `ferryx_remote_token_account`, `ferryx.account.tokenOrigin`, `ferryx:account-session`, and absence of `ferryx.account.token`.
   - In `quantity_reconciliation_rejects_demoted_owner_during_provider_io_and_skips_second_patch`, uses exact prearmed `oneshot` and `Notify` synchronization to demote the owner during in-flight provider I/O, asserting 403 `ORG_ROLE_REQUIRED` and verifying the second PATCH was never executed.
   - In `org_accept_preserves_existing_owner_or_admin_role_and_recomputes_grace`, verifies that existing admin role is preserved and org pool billing state is persisted.
   - In `concurrent_valid_webhooks_serialize_as_applied_and_duplicate`, exercises concurrent webhook requests with bounded timeout and validates exactly one `applied` and one `duplicate`.
   - In `team_member_entitlement_redacts_customer_portal_manage_url`, verifies that plain members receive `manageUrl: null` while owners receive the full URL.

## Verification Status
- Syntactic delimiter verification (parentheses, braces, brackets) passed on `routes.rs`, `routes_tests.rs`, `service.rs`, `mod.rs`, and `bin/account.rs`.
- All tests are deterministic with explicit async event gates and bounded timeouts (no sleep loops).
- Tests assert machine-consumed storage/event values, JSON fields, and status codes.
- Local cargo build/test/run: UNRUN (approved; remote verification by root on maho-win).
- Git commits: None (as instructed).
