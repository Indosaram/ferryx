# Ferryx Commercial Billing Operations Runbook

This runbook documents the operational procedures, configuration requirements, database maintenance,
and failure recovery policies for the commercial billing and managed relay subscription system in
`ferryx-relay` and `ferryx-account`.

---

## 1. Architectural Overview and Deployment Modes

Ferryx services require an explicit deployment mode declared via environment variable on startup.
The binary does not provide CLI flags for deployment modes; any unrecognized CLI flags fail with exit status 2.

### 1.1 Deployment Mode Rules

| Mode | Environment Variable Setting | Characteristics |
| :--- | :--- | :--- |
| **Self-Hosted** | `FERRYX_DEPLOYMENT_MODE="selfhost"` | Billing routes (`/api/account/v1/billing/*`), Lemon Squeezy webhooks, machine limits, and suspension checks are completely disabled. All endpoints return `404 Not Found` for billing routes. All accounts and connected daemons operate perpetually free with unlimited machines. |
| **Commercial** | `FERRYX_DEPLOYMENT_MODE="commercial"` | Requires Lemon Squeezy configuration (`FERRYX_LS_*`). If Lemon Squeezy configuration is missing or incomplete, the service starts in fail-closed mode: free accounts remain functional, but paid surfaces (`checkout`, `quantity`, `webhook`) return `503 Service Unavailable (BILLING_UNCONFIGURED)`. |

Cross-platform environment variable configuration:
- **Linux / macOS**:
  ```bash
  export FERRYX_DEPLOYMENT_MODE="commercial"
  ```
- **Windows PowerShell**:
  ```powershell
  $env:FERRYX_DEPLOYMENT_MODE = "commercial"
  ```
- **Windows CMD**:
  ```bat
  set FERRYX_DEPLOYMENT_MODE=commercial
  ```

---

## 2. Plan Catalog, Pricing, and Lemon Squeezy Variants

### 2.1 Exact Plan Tiers and Capacity Specs

The official Ferryx plans and approved prices are defined as follows:

| Plan Key | Target Audience | Base Capacity | Price (Monthly) | Price (Annual) | Extra Host Packs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Free** (`free`) | Personal developers | 1 registered machine | $0 | $0 | None |
| **Pro** (`pro_monthly` / `pro_annual`) | Power users | 10 registered machines | $5 / month | $50 / year | +5 machines per pack ($2/mo or $20/yr) |
| **Team** (`team_monthly` / `team_annual`) | Organizations (min 2 seats) | 10 machines / seat (pooled) | $7 / seat / month | $70 / seat / year | +5 machines per pack ($20/yr, annual only) |

### 2.2 Configured Lemon Squeezy Variants: Five Variants Contract

While earlier planning text referenced 4 variants, the production implementation strictly configures
**5 Lemon Squeezy variant IDs** across 8 required `FERRYX_LS_*` environment variables.

#### Why Five Configured Variants?

1. **Pro Base Variants (2)**:
   - `FERRYX_LS_VARIANT_PRO_MONTHLY`
   - `FERRYX_LS_VARIANT_PRO_ANNUAL`
   - *Pro Host Pack Mapping*: Pro host packs do **not** use a separate Lemon Squeezy variant ID.
     The Pro plan in Lemon Squeezy is configured with **graduated pricing** on a single subscription item.
     The subscription item wire quantity is calculated as `quantity = 1 + host_packs`:
     - Unit 1 represents the base plan ($5/mo or $50/yr for 10 machines).
     - Units 2+ represent additional host packs ($2/mo or $20/yr per 5 machines).
2. **Team Base Variants (2)**:
   - `FERRYX_LS_VARIANT_TEAM_MONTHLY`
   - `FERRYX_LS_VARIANT_TEAM_ANNUAL`
   - *Team Base Seat Mapping*: The Team plan uses **standard seat-based pricing**, where wire `quantity = seats` (minimum `TEAM_MIN_SEATS = 2`).
     Each seat contributes 10 machines to an organization-wide pooled quota (`machine_limit = max(seats, 2) * 10 + 5 * host_packs`).
3. **Team Host Pack Variant (1)**:
   - `FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL`
   - *Separate Variant Requirement*: Lemon Squeezy subscriptions only support a single quantity axis per subscription.
     Because the base Team subscription quantity represents seats, extra team host packs cannot be combined on that quantity axis.
     Furthermore, because monthly $2 add-ons would incur Lemon Squeezy's minimum processing fee (50¢ per transaction = 25% overhead),
     team host packs are sold **exclusively as an annual subscription** with wire `quantity = host_packs`.

---

## 3. Environment Variable Configuration

### 3.1 Production Environment Variables

All commercial deployments must configure the following exact environment variable names in their service definitions:

| Variable Name | Required | Description |
| :--- | :--- | :--- |
| `FERRYX_DEPLOYMENT_MODE` | Yes | Must be set to `commercial`. |
| `FERRYX_ACCOUNT_ORIGIN` | Yes | Canonical public HTTPS URL of the account service (e.g. `https://relay.checka.cc`). |
| `FERRYX_ACCOUNT_DATA_DIR` | No | Directory holding the SQLite store (`account-store.sqlite3`). If unset, defaults to `~/.ferryx/account` on Unix/macOS (`$HOME/.ferryx/account`) and `%USERPROFILE%\.ferryx\account` on Windows (`src-tauri/src/account/origin.rs`). |
| `FERRYX_LS_API_KEY` | Yes | Lemon Squeezy API key (Bearer token). |
| `FERRYX_LS_STORE_ID` | Yes | Lemon Squeezy store ID (numeric string). |
| `FERRYX_LS_WEBHOOK_SECRET` | Yes | Secret string used for verifying `X-Signature` HMAC-SHA256 headers. |
| `FERRYX_LS_VARIANT_PRO_MONTHLY` | Yes | Variant ID for Pro Monthly plan. |
| `FERRYX_LS_VARIANT_PRO_ANNUAL` | Yes | Variant ID for Pro Annual plan. |
| `FERRYX_LS_VARIANT_TEAM_MONTHLY` | Yes | Variant ID for Team Monthly plan. |
| `FERRYX_LS_VARIANT_TEAM_ANNUAL` | Yes | Variant ID for Team Annual plan. |
| `FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL` | Yes | Variant ID for Team Annual Host Pack add-on. |
| `FERRYX_LS_TEST_MODE` | No | Sandbox test mode flag: `0`/`false` for production, `1`/`true` for Lemon Squeezy sandbox. |
| `FERRYX_LS_API_BASE_URL` | No | Optional API base URL override (e.g. `http://127.0.0.1:port/v1`). Dedicated to local test/staging loopback mock fixtures to completely isolate automated testing from the real Lemon Squeezy provider; defaults to `https://api.lemonsqueezy.com/v1` in production. |
| `FERRYX_BILLING_CLOCK_OFFSET_SECS` | Debug Only | Clock offset in seconds. **Ignored in release builds** (`#[cfg(debug_assertions)]` only). |

### 3.2 Systemd Drop-in Unit Example (Linux Production)

Production daemons running under systemd use environment drop-ins with strict permissions (`0600`).
Never store production secrets in world-readable unit definitions or shell history.

Create `/etc/systemd/system/ferryx-account.service.d/billing.conf`:

```ini
[Service]
# Set explicit deployment mode
Environment="FERRYX_DEPLOYMENT_MODE=commercial"
Environment="FERRYX_ACCOUNT_ORIGIN=https://relay.checka.cc"

# Load sensitive Lemon Squeezy API credentials and variant IDs from a protected file
EnvironmentFile=/etc/ferryx/billing.env
```

Create `/etc/ferryx/billing.env` (permissions: `chmod 0600 /etc/ferryx/billing.env`):

```bash
# Lemon Squeezy API & Store Configuration
FERRYX_LS_API_KEY=YOUR_LEMON_SQUEEZY_API_KEY_HERE
FERRYX_LS_STORE_ID=YOUR_STORE_ID_HERE
FERRYX_LS_WEBHOOK_SECRET=YOUR_WEBHOOK_SECRET_HEX_HERE

# Production Variant IDs (5 variants)
FERRYX_LS_VARIANT_PRO_MONTHLY=111111
FERRYX_LS_VARIANT_PRO_ANNUAL=222222
FERRYX_LS_VARIANT_TEAM_MONTHLY=333333
FERRYX_LS_VARIANT_TEAM_ANNUAL=444444
FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL=555555

# Store Sandbox Mode (false for live production, true for test sandbox)
FERRYX_LS_TEST_MODE=false
```

Reload and restart service:
```bash
sudo systemctl daemon-reload
sudo systemctl restart ferryx-account
```

---

## 4. Lemon Squeezy Webhook Integration

### 4.1 Webhook Endpoint & Signature Verification

- **Endpoint URL**: `https://relay.checka.cc/api/account/v1/billing/webhook`
- **Signature Header**: `X-Signature`
- **Algorithm**: Raw HMAC-SHA256 computed over the unparsed request body bytes (`req.body.to_vec()`) using `FERRYX_LS_WEBHOOK_SECRET`, compared using constant-time verification.
- **Failures**: Signature mismatches return `401 Unauthorized (WEBHOOK_SIGNATURE_INVALID)` and record an unauthenticated rejection audit in `payment_events`.

### 4.2 Complete Supported Webhook Event List

The adapter explicitly processes **11 subscription and invoice event names**:

1. `subscription_created`: New subscription purchased; initializes or upgrades entitlement.
2. `subscription_updated`: Subscription plan, billing interval, status, or quantity modified.
3. `subscription_cancelled`: Customer cancelled subscription; remains active until `ends_at` timestamp.
4. `subscription_resumed`: Cancelled subscription resumed prior to expiration.
5. `subscription_expired`: Subscription reached end of billing cycle without renewal; downgrades to Free.
6. `subscription_paused`: Subscription paused; transitions status to `paused`.
7. `subscription_unpaused`: Paused subscription resumed.
8. `subscription_payment_success`: Renewal payment succeeded; updates authoritative renewal facts.
9. `subscription_payment_failed`: Renewal payment failed; account enters `past_due` status.
10. `subscription_payment_recovered`: Overdue payment successfully collected; clears overdue state.
11. `subscription_payment_refunded`: Payment refunded or charged back.

*Note on Unhandled Events*: Any other event (such as `order_created` or `license_key_created`) returns `200 OK {"status": "ignored"}` and records an ignored audit marker.

### 4.3 Refund and Chargeback Handling: State Reconciliation vs. Invented Statuses

When a `subscription_payment_refunded` event arrives:
1. The payload type is a `subscription-invoice`.
2. The account service does **not** invent an artificial `"refunded"` subscription status (which does not exist in Lemon Squeezy or in our `SubscriptionStatus` enum).
3. The service calls `client.fetch_authoritative_subscription(&event)` to query the Lemon Squeezy API (`GET /v1/subscriptions/{id}`) for the true, authoritative subscription state.
4. The authoritative status returned by Lemon Squeezy (such as `cancelled`, `expired`, `past_due`, or `active`) is recorded in `subscriptions`.
5. Entitlement evaluation (`evaluate_owner_and_persist`) immediately recomputes the owner's capacity and grace period status.

### 4.4 Idempotency, Out-of-Order Delivery, and Replay Protection

1. **SHA-256 Event Deduplication**: Every incoming webhook body is hashed (`event_key = sha256_hex(raw_body)`). If the key already exists in `payment_events` with an applied outcome, the endpoint immediately short-circuits with `200 OK {"status": "duplicate"}` without re-fetching from Lemon Squeezy or re-mutating state.
2. **Out-of-Order Timestamp Defense**: Lemon Squeezy webhooks carry `attributes.updated_at` (ISO 8601). If a delayed webhook arrives with `updated_at < existing.ls_updated_at`, the outdated payload is ignored (`WebhookDecision::Ignored`) and the more recent state on disk is preserved.
3. **Manual Webhook Replay**: If a webhook delivery failed due to network disruption or temporary server maintenance, navigate to the Lemon Squeezy Webhooks Dashboard and trigger **Resend**. The endpoint will verify the signature, process the event, and return `applied`.

---

## 5. Subscription Quantity Modifications and Deferred Charges

### 5.1 Mechanics: PATCH `/v1/subscription-items/{item_id}`

When a customer changes seats or host packs via `POST /api/account/v1/billing/quantity`:
1. The server identifies the subscription item ID (`sub.item_id`, obtained from `first_subscription_item.id`).
2. The server issues a `PATCH` request to `https://api.lemonsqueezy.com/v1/subscription-items/{item_id}` with payload:
   ```json
   {
     "data": {
       "type": "subscription-items",
       "id": "{item_id}",
       "attributes": {
         "quantity": new_quantity,
         "disable_prorations": true,
         "invoice_immediately": false
       }
     }
   }
   ```
3. **Deferred Charge**: With `disable_prorations: true` and `invoice_immediately: false`, the customer is **not** charged immediately. The financial charge for the updated quantity is deferred to the next billing renewal date.
4. **Immediate Entitlement Update**: Once the provider acknowledges the quantity modification (`update_and_read_back`), the new quantity facts are reconciled into `AccountStore` immediately (`reconcile_subscription_from_provider`). The customer's machine limit and seat pool expand right away without waiting for the next renewal invoice.

---

## 6. Seven-Day Grace Period and Complete Remote Cutoff Policy

### 6.1 Invariants

- **Duration**: Exactly 7 days (`GRACE_SECS = 604_800` seconds).
- **Trigger**: Starts the first moment an account is evaluated as `OverLimit` (machines registered exceed limit) or `PastDue` (unpaid renewal invoice).
- **Grace Deadline**: `grace_ends_at = grace_started_at + 604_800`.
- **Enrollment Guard Rule**:
  - In `src-tauri/src/account/billing/entitlement.rs:163`, `may_enroll_new = remote_allowed && snapshot.machines_used < limit`.
  - If an account is in grace due to `OverLimit` (`machines_used > limit`), `snapshot.machines_used < limit` is false, so enrolling an additional machine is rejected immediately (`402 PLAN_LIMIT_REACHED`).
  - If an account is in grace solely due to `PastDue` payment delinquency while still strictly under its plan machine limit (`machines_used < limit`), existing machines remain connected and `may_enroll_new` evaluates to `true` until the capacity limit is reached or the grace deadline expires (`Stopped`).

### 6.2 Scope of Suspension: Remote-Only Cutoff

> **Implementation Phase Status**:
> Phases 1-5 (entitlement model, SQLite store/migration, deployment modes, Lemon Squeezy checkout/webhooks/quantities, issuance enforcement for enroll/grant/attach, the 60-second relay suspension sweeper, host direct-route leases and billing notice emails) are implemented on the `feat/relay-monetization` branch and pass the account, relay and direct-route test suites on the Windows verification host (source-pinned gate, Phase 5 gate3).
> Staging end-to-end scenarios run against a Lemon Squeezy stub; a real Lemon Squeezy sandbox checkout has not been exercised yet and requires sandbox credentials.

When the grace deadline expires (`now >= grace_ends_at`), the owner enters `Stopped` status:

1. **Relay Connections**:
   - The relay sweeper task running every 60 seconds evaluates active connections.
   - For stopped owners, all active WebSocket relay sessions are cleanly terminated with WebSocket close reason `REMOTE_SUSPENDED`.
   - Control tunnel admissions (`bind_machine_key`) are refused.
   - Session attachment (`attach_session_handler`) returns `402 Payment Required (REMOTE_SUSPENDED)`.
2. **Direct QUIC Tunnels & Host Leases**:
   - Host daemons require a signed entitlement lease (`POST /api/account/v1/billing/lease`).
   - The account service calculates `lease_expiry = min(now + 86400, grace_ends_at)`.
   - When stopped, the lease endpoint refuses issuance (`402 REMOTE_SUSPENDED`), and direct QUIC connection offers (`direct_offer_handler`) are rejected.
3. **PTY and Local Preservation (Zero Session Loss)**:
   - Host daemon PTY master processes, running terminal sessions, local workspaces, agent pipelines, and LAN connections are **never killed or disturbed**.
   - No daemon process is ever terminated. Only remote relay and direct tunnel gateways are suspended.

---

## 7. Database Administration: SQLite Storage & Safe Rollback

### 7.1 SQLite Schema Structure

Account data is stored in `account-store.sqlite3` using WAL mode. The billing tables are:

```sql
CREATE TABLE IF NOT EXISTS billing_states (
    owner_key TEXT PRIMARY KEY,
    grace_started_at INTEGER,
    stopped_at INTEGER,
    last_notice TEXT
);

CREATE TABLE IF NOT EXISTS payment_events (
    event_key TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    applied_at INTEGER,
    outcome TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subscriptions (
    subscription_id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL,
    org_id TEXT,
    plan_key TEXT NOT NULL,
    seats INTEGER NOT NULL,
    host_packs INTEGER NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    ends_at INTEGER,
    ls_customer_id TEXT,
    ls_updated_at INTEGER NOT NULL,
    manage_url TEXT
);
```

### 7.2 Manual Grace Extension Procedure

If an operator needs to manually grant additional grace time for an enterprise customer or resolving a billing dispute, execute a targeted SQL transaction.

**Rules**:
- `ferryx-account` does **not** keep a persistent in-memory store cache across requests. In `src-tauri/src/account/service.rs` and `src-tauri/src/account/store_sqlite/mod.rs:59-92`, every read (`service.read`) executes a fresh transaction on `account-store.sqlite3`, and every mutation (`service.mutate` / `AccountStore::mutate_transaction`) begins a `rusqlite::TransactionBehavior::Immediate` (`BEGIN IMMEDIATE`) transaction on SQLite, loads the store, mutates it, serializes updates via `codec::save_store_to_transaction`, and commits.
- **Never terminate or restart running daemons**: Because each request reads freshly from SQLite, manual transaction updates take effect immediately on the next operation without stopping or restarting the running service.
- **Always take a WAL-safe backup before writing**.
- Target one verified owner key (`owner_key = '<checked_owner>'` matching a raw user ID `usr_...` or organization key `org:...`).
- Preserve `last_notice` unless intentionally resetting notice delivery.
- Set `grace_started_at` to the desired timestamp (`new_grace_ends_at - 604800`) and clear `stopped_at = NULL`.
- Always set a bounded busy timeout and execute within `BEGIN IMMEDIATE` ... `COMMIT`.

```bash
# 1. Take a safe online backup before writing
sqlite3 /var/lib/ferryx/account/account-store.sqlite3 \
  ".backup /var/backups/ferryx/account-backup-pre-grace-ext-$(date +%Y%m%d%H%M%S).sqlite3"

# 2. Execute transactional update with bounded busy timeout
sqlite3 /var/lib/ferryx/account/account-store.sqlite3 <<EOF
PRAGMA busy_timeout = 5000;
BEGIN IMMEDIATE;

-- Verify existing owner record
SELECT owner_key, grace_started_at, stopped_at, last_notice
FROM billing_states
WHERE owner_key = 'usr_1234567890abcdef';

-- Update grace timestamp and clear stopped status
UPDATE billing_states
SET grace_started_at = strftime('%s', 'now'),
    stopped_at = NULL
WHERE owner_key = 'usr_1234567890abcdef';

-- Verify updated state before commit
SELECT owner_key, grace_started_at, stopped_at, last_notice
FROM billing_states
WHERE owner_key = 'usr_1234567890abcdef';

COMMIT;
EOF
```

### 7.3 Safe Rollback Instructions: Never Overwrite Newer SQLite with Stale JSON

When `ferryx-account` initializes for the first time, it performs a one-time migration of `account-store.json` into SQLite and renames the file to `account-store.json.migrated-<timestamp>`.

**CRITICAL DATA SAFETY WARNING**:
- **NEVER** delete `account-store.sqlite3` and rename `account-store.json.migrated-*` back to `account-store.json`!
- Doing so will permanently destroy all user sign-ups, machine registrations, payment records, and subscriptions created since the initial migration.
- If `account-store.sqlite3` contains existing records (`user_count > 0 || machine_count > 0`), the migration logic intentionally refuses to import `account-store.json` and immediately renames it to prevent accidental data overwrites.

#### Correct SQLite Backup and Safe Restoration

Restoring a database backup is a **destructive point-in-time recovery**. Any records created after the backup timestamp will be lost.

**Restoration Procedure**:
1. **Explicit Operator Approval Required**:
   Confirm that reverting to the backup's timestamp and incurring point-in-time data loss of subsequent registrations, payments, and machine enrollments is explicitly approved.
2. **Online Database Backup (Routine)**:
   ```bash
   sqlite3 /var/lib/ferryx/account/account-store.sqlite3 ".backup /var/backups/ferryx/account-backup-$(date +%Y%m%d%H%M%S).sqlite3"
   ```
3. **Safe Restoration Steps**:
   ```bash
   # Step A: Stop account service
   sudo systemctl stop ferryx-account

   # Step B: Preserve current active database for emergency rollback before overwriting
   mv /var/lib/ferryx/account/account-store.sqlite3 /var/lib/ferryx/account/account-store.sqlite3.pre-restore-$(date +%Y%m%d%H%M%S)
   rm -f /var/lib/ferryx/account/account-store.sqlite3-wal /var/lib/ferryx/account/account-store.sqlite3-shm

   # Step C: Copy verified SQLite backup into place
   cp /var/backups/ferryx/account-backup-TARGET.sqlite3 /var/lib/ferryx/account/account-store.sqlite3
   chmod 0600 /var/lib/ferryx/account/account-store.sqlite3

   # Step D: Restart account service
   sudo systemctl start ferryx-account
   ```

---

## 8. Staging Verification Commands and Cleanup

For staging validation on a test host without production credentials:

```bash
# ------------------------------------------------------------------------------
# 1. Environment Setup (Dedicated staging directories and loopback mock provider)
# ------------------------------------------------------------------------------
export STAGING_DIR="/tmp/ferryx-billing-staging"
mkdir -p "$STAGING_DIR"

export FERRYX_DEPLOYMENT_MODE="commercial"
export FERRYX_ACCOUNT_ORIGIN="http://127.0.0.1:43822"
export FERRYX_ACCOUNT_DATA_DIR="$STAGING_DIR"
export FERRYX_MAIL_DIR="$STAGING_DIR/mail"
mkdir -p "$FERRYX_MAIL_DIR"

# Test credentials and sandbox flags
export FERRYX_LS_API_KEY="test_api_key_bearer"
export FERRYX_LS_STORE_ID="12345"
export FERRYX_LS_WEBHOOK_SECRET="test_webhook_secret_hex"
export FERRYX_LS_VARIANT_PRO_MONTHLY="101"
export FERRYX_LS_VARIANT_PRO_ANNUAL="102"
export FERRYX_LS_VARIANT_TEAM_MONTHLY="201"
export FERRYX_LS_VARIANT_TEAM_ANNUAL="202"
export FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL="301"
export FERRYX_LS_TEST_MODE="true"
export FERRYX_LS_API_BASE_URL="http://127.0.0.1:9999/v1"

# ------------------------------------------------------------------------------
# 2. Launch Services (UNRUN locally - run on designated test/staging host)
# ------------------------------------------------------------------------------
# Start ferryx-account in background:
# ferryx-account &
# ACCOUNT_PID=$!

# ------------------------------------------------------------------------------
# 3. Authentication & Normal Account Operations
# ------------------------------------------------------------------------------
# Request magic link:
# curl -s -X POST "$FERRYX_ACCOUNT_ORIGIN/api/account/v1/login/request" \
#   -H "Content-Type: application/json" \
#   -d '{"email": "operator@example.com"}'

# Extract login code from test mail directory:
# LOGIN_CODE=$(cat "$FERRYX_MAIL_DIR"/* | grep -oE 'code=[a-zA-Z0-9_-]+' | cut -d= -f2 | head -n 1)

# Consume login code to receive session token:
# SESSION_TOKEN=$(curl -s -X POST "$FERRYX_ACCOUNT_ORIGIN/api/account/v1/login/consume" \
#   -H "Content-Type: application/json" \
#   -d "{\"code\": \"$LOGIN_CODE\"}" | jq -r '.token')

# Check baseline entitlement (Free plan):
# curl -s -X GET "$FERRYX_ACCOUNT_ORIGIN/api/account/v1/billing/entitlement" \
#   -H "Authorization: Bearer $SESSION_TOKEN" | jq .

# ------------------------------------------------------------------------------
# 4. Webhook Ingestion Verification
# ------------------------------------------------------------------------------
# Payload simulating subscription_created:
# WEBHOOK_BODY='{"meta":{"event_name":"subscription_created","custom_data":{"user_id":"test_user_id"}},"data":{"type":"subscriptions","id":"sub_100","attributes":{"store_id":"12345","customer_id":"cust_1","variant_id":"101","status":"active","quantity":1,"created_at":"2026-09-30T12:00:00Z","updated_at":"2026-09-30T12:00:00Z","test_mode":true}}}'
# WEBHOOK_SIG=$(echo -n "$WEBHOOK_BODY" | openssl dgst -sha256 -hmac "$FERRYX_LS_WEBHOOK_SECRET" | cut -d' ' -f2)

# Post webhook:
# curl -s -X POST "$FERRYX_ACCOUNT_ORIGIN/api/account/v1/billing/webhook" \
#   -H "Content-Type: application/json" \
#   -H "X-Signature: $WEBHOOK_SIG" \
#   -d "$WEBHOOK_BODY" | jq .

# ------------------------------------------------------------------------------
# 5. CLI Verification
# ------------------------------------------------------------------------------
# ferryx account plan --origin "$FERRYX_ACCOUNT_ORIGIN"
# ferryx account plan --json --origin "$FERRYX_ACCOUNT_ORIGIN"

# ------------------------------------------------------------------------------
# 6. Cleanup Staging Artifacts
# ------------------------------------------------------------------------------
# kill "$ACCOUNT_PID" || true
# rm -rf "$STAGING_DIR"
```

---

## 9. Operational Incident Response & Troubleshooting

| Symptom | Cause | Remediation |
| :--- | :--- | :--- |
| `503 Service Unavailable (BILLING_UNCONFIGURED)` | `FERRYX_DEPLOYMENT_MODE="commercial"` but one or more of the 8 required `FERRYX_LS_*` variables are empty or missing (`config.is_configured()` is false). Paid routes fail closed before any external provider network call. | Check `/etc/ferryx/billing.env` to ensure all 8 required Lemon Squeezy environment variables are set. |
| `502 Bad Gateway (BILLING_PROVIDER_ERROR)` | Lemon Squeezy configuration variables are present, but the API key/store ID is invalid, upstream Lemon Squeezy returns an HTTP error, or network connectivity failed during checkout creation or subscription item quantity PATCH. | Verify Lemon Squeezy API credentials and store permissions. If running staging/test suites, ensure `FERRYX_LS_API_BASE_URL` points to an active loopback mock fixture. |
| `401 Unauthorized (WEBHOOK_SIGNATURE_INVALID)` | Webhook secret in Lemon Squeezy dashboard does not match `FERRYX_LS_WEBHOOK_SECRET`. | Verify secret hex string in Lemon Squeezy settings and reload service. |
| Customer reports "over limit" immediately after upgrade | Checkout completed but webhook has not yet arrived. | Check Lemon Squeezy webhook logs for delivery errors, or trigger manual resend. |
| Customer cannot enroll second machine on Free plan | Account is on Free tier (`machine_limit = 1`). | Customer must either unregister the first machine or upgrade to Pro via Settings > Billing. |
