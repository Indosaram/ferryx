# Phase 4 — Todo 15: Billing Operations Runbook & Pricing Documentation

- Worktree: `/Volumes/T9-Mac/project/ferryx-relay-monetization`
- Branch / base: `feat/relay-monetization` @ `016936d4`
- Status: **documentation and operational runbook landed; no source code modified; nothing executed locally — no cargo, no bun build, no tsc, no test, no server process, no git commit. Every command below is UNRUN.**
- Implementation note: Phase 1–3 backend and UI contracts landed. Task 8 (issuance gate enforcement) contains active unverified compilation errors; Tasks 9, 10, and 14 belong to Phase 5. Neither Phase 4 nor Phase 5 are claimed as verified shipping code.
- Plan: `.omo/plans/relay-monetization.md` Todo 15 (Closes GAP-11).

---

## Changed and Added Files (Exact Revisions)

| Path | Status | Lines | Purpose |
| :--- | :--- | :--- | :--- |
| `docs/billing-operations.md` | NEW | 338 | Operational runbook for Ferryx commercial billing, Lemon Squeezy integration, 5-variant mapping, systemd drop-ins, SQLite maintenance, grace period full stop, and disaster recovery. |
| `docs/account-service.md` | MODIFIED | +7 | Added cross-link section connecting `ferryx-account` service operations directly to `docs/billing-operations.md`. |
| `site/src/content/docs/docs/pricing.mdx` | MODIFIED | +3 / -2 | Corrected Quantity FAQ: distinguished deferred next-renewal financial charges from immediate local entitlement / machine capacity expansion upon validated provider acknowledgment / webhook. |
| `.omo/evidence/relay-monetization/task-15.txt` | NEW | 63 | Environment variable cross-check ledger verifying all 14 configuration variables against production code locations. |

---

## Source Verification & Evidence Summary

### 1. Lemon Squeezy Variant Contract: Five Variants vs. Four Variants
- **Observed Source Contract**: `src-tauri/src/account/billing/lemonsqueezy.rs:145-154` defines exactly 5 variant configuration fields:
  1. `FERRYX_LS_VARIANT_PRO_MONTHLY`
  2. `FERRYX_LS_VARIANT_PRO_ANNUAL`
  3. `FERRYX_LS_VARIANT_TEAM_MONTHLY`
  4. `FERRYX_LS_VARIANT_TEAM_ANNUAL`
  5. `FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL`
- **Explanation**:
  - Pro host packs do **not** use a separate variant because Pro uses graduated pricing on the primary subscription item (`quantity = 1 + host_packs`).
  - Team base uses standard seat-based pricing (`quantity = seats`, min 2 seats). Extra team host packs cannot share that quantity axis (Lemon Squeezy subscriptions only have one quantity axis) and cannot be billed monthly due to minimum fee thresholds (50¢ on a $2 charge = 25% overhead). Therefore, team host packs require a separate annual variant.
  - The legacy text in `relay-monetization.md` mentioning 4 variants was updated by the fixed contract to 5 variants.

### 2. Supported Webhook Event Types & Refund Handling
- **Observed Source Contract**: `src-tauri/src/account/billing/lemonsqueezy.rs:26-38` defines `SUPPORTED_EVENTS` with 11 event names:
  `subscription_created`, `subscription_updated`, `subscription_cancelled`, `subscription_resumed`, `subscription_expired`, `subscription_paused`, `subscription_unpaused`, `subscription_payment_success`, `subscription_payment_failed`, `subscription_payment_recovered`, `subscription_payment_refunded`.
- **Refund & Chargeback Policy**:
  - The codebase does **not** invent an artificial `"refunded"` subscription status.
  - On `subscription_payment_refunded` (an invoice event), the account service queries Lemon Squeezy via `client.fetch_authoritative_subscription(&event)` (`GET /v1/subscriptions/{id}`) to obtain the true authoritative state (`cancelled`, `expired`, `past_due`, etc.) and reconciles the store and grace period accordingly.

### 3. Immediate Capacity vs. Deferred Billing Charges
- **Observed Source Contract**: `src-tauri/src/account/billing/routes.rs:662-756` and `lemonsqueezy.rs:188-212`.
  - When updating quantities via `POST /api/account/v1/billing/quantity`, the adapter PATCHes `/v1/subscription-items/{item_id}` with `disable_prorations: true` and `invoice_immediately: false`.
  - The customer is **not** charged immediately; the financial billing charge is deferred to the next renewal date.
  - However, immediately upon provider acknowledgment (`update_and_read_back`), the server reconciles the new quantity into the database (`reconcile_subscription_from_provider`) and updates the owner's entitlement and machine limits right away.
  - `site/src/content/docs/docs/pricing.mdx` was updated to accurately reflect this behavior.

### 4. SQLite Schema & Safe Maintenance
- **Observed Source Contract**: `src-tauri/src/account/store_sqlite/schema.rs:186-193` defines `billing_states`:
  - Columns: `owner_key TEXT PRIMARY KEY, grace_started_at INTEGER, stopped_at INTEGER, last_notice TEXT`.
  - Grace deadline is strictly `grace_started_at + 604_800` (`GRACE_SECS`).
- **Safe Rollback**:
  - `account-store.sqlite3` is the authoritative store. When initialized, legacy `account-store.json` is imported once and renamed to `account-store.json.migrated-<ts>`.
  - Overwriting `account-store.sqlite3` with an old JSON file is prohibited as it destroys all user registrations, machine records, payments, and subscriptions created post-migration.
  - The account service maintains no persistent in-memory store cache (`service.read` loads freshly from SQLite every call; `service.mutate` runs inside `BEGIN IMMEDIATE` transactions in `store_sqlite/mod.rs:59-92`). Manual targeted SQL updates using bounded busy timeouts take effect on the next read without stopping or restarting running daemons.

### 5. Constraints from Pre-Implementation Reviews & Unimplemented Phase 5 Features
- As mandated by `.omo/ulw-execute/direct-lease-preimplementation-review.md` and `.omo/ulw-execute/notices-preimplementation-review.md`, Task 10 (host direct lease) and Task 14 (billing notice emails) are treated strictly as unimplemented future constraints and never claimed as already shipped features.
- Furthermore, the automated relay suspension sweeper (Task 9) is Phase 5 work and is documented in the runbook only as an operational target requirement, not as verified shipping code.
- Staging instructions explicitly note that `FERRYX_LS_API_BASE_URL` is for local loopback test fixtures separate from the live Lemon Squeezy provider, and warn against claiming live checkout URL generation without valid provider credentials.

---

## Verification & Execution Commands (All UNRUN)

All verification commands are documented below for remote execution on `maho-win` or designated test infrastructure. **No commands were run locally.**

```bash
# UNRUN: Documentation verification in docs/
# Check that all environment variable names match the codebase
cat .omo/evidence/relay-monetization/task-15.txt

# UNRUN: Astro documentation site build (run on maho-win)
# bun run --cwd site build
```
