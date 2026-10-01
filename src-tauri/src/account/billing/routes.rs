//! Billing, lease and team HTTP routes (plan todo 7).
//!
//! Fixed API contract from `.omo/plans/relay-monetization.md`:
//!
//! ```text
//! GET  /api/account/v1/billing/entitlement            (account session)
//! POST /api/account/v1/billing/checkout               -> 200 {"url": "..."}
//! POST /api/account/v1/billing/quantity               -> 200 entitlement
//! POST /api/account/v1/billing/webhook                (X-Signature) -> 200 {"status": "applied|duplicate|ignored"}
//! POST /api/account/v1/billing/lease                  (machine Ed25519) -> 200 {"lease": "...", "expiresAt": n}
//! GET  /api/account/v1/org/members
//! POST /api/account/v1/org/invite
//! POST /api/account/v1/org/members/{user_id}/remove
//! POST /api/account/v1/org/accept
//! ```
//!
//! Deployment rules: a `selfhost` deployment never serves these routes (`404`), and a commercial
//! deployment without Lemon Squeezy configuration still answers for healthy Free accounts -
//! only the paid surfaces (`checkout`, `quantity`, `webhook`) fail closed with
//! `503 BILLING_UNCONFIGURED`.
//!
//! Every synchronous account-store operation runs through [`offload`], which is
//! `crate::ipc::run_blocking` carrying a nested `Result<_, ApiError>` so the structured error
//! (code, status, details) survives the thread hop and the Tokio reactor is never blocked.
//!
//! The webhook path follows the plan's ordering rule: the Lemon Squeezy request (authoritative
//! subscription fetch for invoice payloads) happens *outside* the store transaction, and the
//! transaction re-reads the store and re-validates the owner, the event key and the ordering
//! timestamp before it writes.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::entitlement::{
    evaluate, is_subscription_active, next_grace_started_at, BillingSnapshot, Entitlement,
    EntitlementStatus, PlanKey, SubscriptionStatus, TEAM_MIN_SEATS,
};
use super::lease;
use super::lemonsqueezy::{
    event_key, parse_event, parse_subscription_status, pro_subscription_quantity,
    team_hostpack_quantity, team_subscription_quantity, variant_to_plan, verify_signature,
    CheckoutPlanChoice, LemonSqueezyClient, LemonSqueezyConfig, LemonSqueezyError, LsEvent,
    LsPayloadKind, PlanVariantKind, SubscriptionDetails,
};
use crate::account::service::{AccountState, ApiError};
use crate::account::store::{
    normalize_email, random_token, token_hash, AccountStore, BillingStateRecord, MachineRecord,
    OrgInviteRecord, OrgMemberRecord, OrgRecord, PaymentEventRecord, SubscriptionRecord,
    UserRecord,
};

pub const KIND_BASE: &str = "base";
pub const KIND_HOSTPACK: &str = "hostpack";

pub const ROLE_OWNER: &str = "owner";
pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_MEMBER: &str = "member";

pub const WEBHOOK_SIGNATURE_HEADER: &str = "x-signature";

/// Webhook bodies are larger than the 4 KiB login-body limit the service router applies
/// globally, so the webhook route carries its own inner limit (an inner
/// `DefaultBodyLimit` overrides the outer one for that route only).
pub const WEBHOOK_MAX_BODY_BYTES: usize = 64 * 1024;

pub const LEASE_CLOCK_SKEW_SECS: u64 = 60;

pub const ORG_INVITE_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// Debug-only clock override used by the end-to-end QA in plan todo 16.
pub const DEBUG_CLOCK_ENV: &str = "FERRYX_BILLING_CLOCK_OFFSET_SECS";

/// Account clock for every billing decision, uniformly.
///
/// Debug builds may shift it with [`DEBUG_CLOCK_ENV`] (whole seconds, negative allowed); release
/// builds compile the offset to a constant zero, so a deployed service can never be shifted.
pub fn billing_now() -> u64 {
    apply_clock_offset(crate::account::store::now_secs(), clock_offset_secs())
}

#[cfg(debug_assertions)]
fn clock_offset_secs() -> i64 {
    std::env::var(DEBUG_CLOCK_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

#[cfg(not(debug_assertions))]
fn clock_offset_secs() -> i64 {
    0
}

pub(crate) fn apply_clock_offset(base: u64, offset_secs: i64) -> u64 {
    base.saturating_add_signed(offset_secs)
}

#[derive(Clone)]
pub struct BillingConfig {
    pub ls: LemonSqueezyConfig,
}

pub fn billing_routes(ls: LemonSqueezyConfig) -> Router<Arc<AccountState>> {
    let webhook = Router::new()
        .route("/api/account/v1/billing/webhook", post(billing_webhook))
        .layer(DefaultBodyLimit::max(WEBHOOK_MAX_BODY_BYTES));
    Router::new()
        .route(
            "/api/account/v1/billing/entitlement",
            get(billing_entitlement),
        )
        .route("/api/account/v1/billing/checkout", post(billing_checkout))
        .route("/api/account/v1/billing/quantity", post(billing_quantity))
        .route("/api/account/v1/billing/lease", post(billing_lease))
        .route("/api/account/v1/org/members", get(org_members))
        .route("/api/account/v1/org/invite", post(org_invite))
        .route(
            "/api/account/v1/org/members/{user_id}/remove",
            post(org_remove_member),
        )
        .route("/api/account/v1/org/accept", post(org_accept))
        .route("/org/accept", get(org_accept_page))
        .merge(webhook)
        .layer(Extension(Arc::new(BillingConfig { ls })))
}

/// Runs one synchronous account operation on a blocking thread, preserving `ApiError` unchanged.
async fn offload<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    crate::ipc::run_blocking(move || Ok(operation()))
        .await
        .map_err(|error| ApiError::internal(format!("account blocking task failed: {error}")))?
}

fn require_billing_enabled(state: &AccountState) -> Result<(), ApiError> {
    if state.deployment_mode.is_billing_enabled() {
        Ok(())
    } else {
        Err(not_found("not found"))
    }
}

fn require_ls_config(config: &LemonSqueezyConfig) -> Result<(), ApiError> {
    if config.is_configured() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "BILLING_UNCONFIGURED",
            "this deployment has no Lemon Squeezy configuration",
        ))
    }
}

fn ls_client(config: &LemonSqueezyConfig) -> Result<LemonSqueezyClient, ApiError> {
    LemonSqueezyClient::new(config.clone()).map_err(provider_error)
}

fn provider_error(error: LemonSqueezyError) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "BILLING_PROVIDER_ERROR",
        error.to_string(),
    )
}

fn bad_request(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", message)
}

fn not_found(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
}

fn conflict(code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, code, message)
}

fn org_role_required() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        "ORG_ROLE_REQUIRED",
        "a team owner or admin role is required",
    )
}

fn org_invite_invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "ORG_INVITE_INVALID", message)
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(&body).map_err(|error| bad_request(error.to_string()))
}

async fn session_user(
    state: &Arc<AccountState>,
    headers: &HeaderMap,
) -> Result<UserRecord, ApiError> {
    let state = state.clone();
    let headers = headers.clone();
    offload(move || crate::account::service::authenticate_user(&state, &headers)).await
}

async fn lookup_machine(
    state: &Arc<AccountState>,
    machine_id: &str,
) -> Result<MachineRecord, ApiError> {
    let state = state.clone();
    let machine_id = machine_id.to_string();
    offload(move || {
        let store = load_store(&state)?;
        store
            .machines
            .values()
            .find(|machine| machine.machine_id == machine_id)
            .cloned()
            .ok_or_else(|| not_found("no such machine on this account service"))
    })
    .await
}

fn load_store(state: &AccountState) -> Result<AccountStore, ApiError> {
    AccountStore::load(&state.data_dir).map_err(ApiError::internal)
}

/// The pool a user's remote access is governed by: the team pool while they are a member of an
/// org, otherwise their own user pool.
#[derive(Debug, Clone)]
struct OwnerScope {
    owner_key: String,
    org_id: Option<String>,
    owner_user_id: String,
    user_ids: Vec<String>,
    role: Option<String>,
}

fn member_key(org_id: &str, user_id: &str) -> String {
    format!("{org_id}:{user_id}")
}

fn org_scope(store: &AccountStore, org_id: &str, fallback_owner: &str) -> OwnerScope {
    let mut user_ids: Vec<String> = store
        .org_members
        .values()
        .filter(|member| member.org_id == org_id)
        .map(|member| member.user_id.clone())
        .collect();
    user_ids.sort();
    user_ids.dedup();
    if user_ids.is_empty() {
        user_ids.push(fallback_owner.to_string());
    }
    OwnerScope {
        owner_key: format!("org:{org_id}"),
        org_id: Some(org_id.to_string()),
        owner_user_id: fallback_owner.to_string(),
        user_ids,
        role: None,
    }
}

fn owner_scope_for_user(store: &AccountStore, user_id: &str) -> OwnerScope {
    let membership = store
        .org_members
        .values()
        .filter(|member| member.user_id == user_id && store.orgs.contains_key(&member.org_id))
        .min_by(|left, right| left.org_id.cmp(&right.org_id));
    match membership {
        Some(member) => {
            let mut scope = org_scope(store, &member.org_id, user_id);
            scope.owner_user_id = user_id.to_string();
            scope.role = Some(member.role.clone());
            scope
        }
        None => OwnerScope {
            owner_key: user_id.to_string(),
            org_id: None,
            owner_user_id: user_id.to_string(),
            user_ids: vec![user_id.to_string()],
            role: None,
        },
    }
}

fn owner_scope_for_subscription(
    store: &AccountStore,
    owner_user_id: &str,
    org_id: Option<&str>,
) -> OwnerScope {
    match org_id {
        Some(org_id) => org_scope(store, org_id, owner_user_id),
        None => OwnerScope {
            owner_key: owner_user_id.to_string(),
            org_id: None,
            owner_user_id: owner_user_id.to_string(),
            user_ids: vec![owner_user_id.to_string()],
            role: None,
        },
    }
}

fn machines_used(store: &AccountStore, scope: &OwnerScope) -> u32 {
    store
        .machines
        .values()
        .filter(|machine| scope.user_ids.iter().any(|id| id == &machine.owner_user_id))
        .count() as u32
}

fn plan_key_str(plan: PlanKey) -> &'static str {
    match plan {
        PlanKey::Free => "free",
        PlanKey::ProMonthly => "pro_monthly",
        PlanKey::ProAnnual => "pro_annual",
        PlanKey::TeamMonthly => "team_monthly",
        PlanKey::TeamAnnual => "team_annual",
    }
}

fn parse_plan_key(raw: &str) -> Option<PlanKey> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "free" => Some(PlanKey::Free),
        "pro_monthly" => Some(PlanKey::ProMonthly),
        "pro_annual" => Some(PlanKey::ProAnnual),
        "team_monthly" => Some(PlanKey::TeamMonthly),
        "team_annual" => Some(PlanKey::TeamAnnual),
        _ => None,
    }
}

fn subscription_status_str(status: SubscriptionStatus) -> &'static str {
    match status {
        SubscriptionStatus::Active => "active",
        SubscriptionStatus::OnTrial => "on_trial",
        SubscriptionStatus::PastDue => "past_due",
        SubscriptionStatus::Unpaid => "unpaid",
        SubscriptionStatus::Cancelled => "cancelled",
        SubscriptionStatus::Expired => "expired",
        SubscriptionStatus::Paused => "paused",
    }
}

fn entitlement_status_str(status: EntitlementStatus) -> &'static str {
    match status {
        EntitlementStatus::Ok => "ok",
        EntitlementStatus::OverLimit => "over_limit",
        EntitlementStatus::PastDue => "past_due",
        EntitlementStatus::Stopped => "stopped",
    }
}

fn kind_str(kind: PlanVariantKind) -> &'static str {
    match kind {
        PlanVariantKind::Base => KIND_BASE,
        PlanVariantKind::Hostpack => KIND_HOSTPACK,
    }
}

fn is_team_plan(plan: PlanKey) -> bool {
    matches!(plan, PlanKey::TeamMonthly | PlanKey::TeamAnnual)
}

fn record_is_active(record: &SubscriptionRecord, now: u64) -> bool {
    parse_subscription_status(&record.status)
        .map(|status| is_subscription_active(status, record.ends_at, now))
        .unwrap_or(false)
}

fn subscription_belongs_to(record: &SubscriptionRecord, scope: &OwnerScope) -> bool {
    match scope.org_id.as_deref() {
        Some(org_id) => record.org_id.as_deref() == Some(org_id),
        None => record.org_id.is_none() && record.owner_user_id == scope.owner_user_id,
    }
}

/// The subscription that decides the pool's plan: active first, then the most recently updated.
fn pick_base_subscription<'a>(
    records: &[&'a SubscriptionRecord],
    now: u64,
) -> Option<&'a SubscriptionRecord> {
    records
        .iter()
        .copied()
        .filter(|record| record.kind == KIND_BASE)
        .filter(|record| {
            matches!(parse_plan_key(&record.plan_key), Some(plan) if plan != PlanKey::Free)
        })
        .max_by(|left, right| {
            record_is_active(left, now)
                .cmp(&record_is_active(right, now))
                .then(left.ls_updated_at.cmp(&right.ls_updated_at))
                .then_with(|| left.subscription_id.cmp(&right.subscription_id))
        })
}

fn owner_subscriptions<'a>(
    store: &'a AccountStore,
    scope: &OwnerScope,
) -> Vec<&'a SubscriptionRecord> {
    store
        .subscriptions
        .values()
        .filter(|record| subscription_belongs_to(record, scope))
        .collect()
}

#[derive(Debug, Clone)]
struct OwnerEvaluation {
    scope: OwnerScope,
    snapshot: BillingSnapshot,
    entitlement: Entitlement,
    grace_started_at: Option<u64>,
    stopped_at: Option<u64>,
    manage_url: Option<String>,
}

fn evaluate_owner(store: &AccountStore, scope: &OwnerScope, now: u64) -> OwnerEvaluation {
    let records = owner_subscriptions(store, scope);
    let base = pick_base_subscription(&records, now);
    let (plan, status, ends_at, seats, base_packs, manage_url) = match base {
        Some(record) => (
            parse_plan_key(&record.plan_key).unwrap_or(PlanKey::Free),
            parse_subscription_status(&record.status).unwrap_or(SubscriptionStatus::Active),
            record.ends_at,
            record.seats,
            record.host_packs,
            record.manage_url.clone(),
        ),
        None => (PlanKey::Free, SubscriptionStatus::Active, None, 0, 0, None),
    };

    let pack_records: Vec<&SubscriptionRecord> = records
        .iter()
        .copied()
        .filter(|record| record.kind == KIND_HOSTPACK)
        .filter(|record| {
            matches!(parse_plan_key(&record.plan_key), Some(plan) if plan == PlanKey::TeamAnnual)
        })
        .filter(|record| record_is_active(record, now))
        .collect();
    let pack_records_total: u32 = pack_records.iter().map(|record| record.host_packs).sum();
    let host_packs = if is_team_plan(plan) {
        base_packs.saturating_add(pack_records_total)
    } else {
        base_packs
    };

    // If any contributing Team pack subscription is delinquent (PastDue or Unpaid),
    // aggregate that delinquency into the pool's effective status so that delinquency
    // on packs does not get ignored when the base plan is healthy or cancelled-but-paid-through.
    // If the base plan itself is already delinquent (PastDue/Unpaid), its delinquency takes precedence.
    let base_is_delinquent = matches!(
        status,
        SubscriptionStatus::PastDue | SubscriptionStatus::Unpaid
    );
    let base_is_active = is_subscription_active(status, ends_at, now);
    let status = if is_team_plan(plan) && !base_is_delinquent && base_is_active {
        let delinquent_pack = pack_records.iter().find(|record| {
            matches!(
                parse_subscription_status(&record.status),
                Some(SubscriptionStatus::PastDue | SubscriptionStatus::Unpaid)
            )
        });
        if let Some(delinquent) = delinquent_pack {
            parse_subscription_status(&delinquent.status).unwrap_or(status)
        } else {
            status
        }
    } else {
        status
    };

    let previous = store.billing_states.get(&scope.owner_key);
    let prev_grace = previous.and_then(|record| record.grace_started_at);
    let prev_stopped = previous.and_then(|record| record.stopped_at);

    let snapshot = BillingSnapshot {
        plan,
        seats,
        host_packs,
        status,
        ends_at,
        machines_used: machines_used(store, scope),
        grace_started_at: prev_grace,
    };
    let entitlement = evaluate(&snapshot, now);
    let violated = entitlement.status != EntitlementStatus::Ok;
    let grace_started_at = next_grace_started_at(prev_grace, violated, now);
    let stopped_at = if entitlement.status == EntitlementStatus::Stopped {
        prev_stopped.or(Some(now))
    } else {
        None
    };

    OwnerEvaluation {
        scope: scope.clone(),
        snapshot,
        entitlement,
        grace_started_at,
        stopped_at,
        manage_url,
    }
}

fn persist_owner_evaluation(store: &mut AccountStore, evaluation: &OwnerEvaluation) {
    let violated = evaluation.entitlement.status != EntitlementStatus::Ok;
    match store.billing_states.get_mut(&evaluation.scope.owner_key) {
        Some(entry) => {
            entry.grace_started_at = evaluation.grace_started_at;
            entry.stopped_at = evaluation.stopped_at;
        }
        None if violated => {
            store.billing_states.insert(
                evaluation.scope.owner_key.clone(),
                BillingStateRecord {
                    owner_key: evaluation.scope.owner_key.clone(),
                    grace_started_at: evaluation.grace_started_at,
                    stopped_at: evaluation.stopped_at,
                    last_notice: None,
                },
            );
        }
        None => {}
    }
}

fn evaluate_owner_and_persist(
    store: &mut AccountStore,
    scope: &OwnerScope,
    now: u64,
) -> OwnerEvaluation {
    let evaluation = evaluate_owner(store, scope, now);
    persist_owner_evaluation(store, &evaluation);
    evaluation
}

fn evaluate_and_persist(
    state: &AccountState,
    user_id: &str,
    now: u64,
) -> Result<OwnerEvaluation, ApiError> {
    state.mutate(|store| {
        let scope = owner_scope_for_user(store, user_id);
        Ok(evaluate_owner_and_persist(store, &scope, now))
    })
}

pub fn entitlement_for_user_in_store(
    store: &mut AccountStore,
    user_id: &str,
    now: u64,
) -> Entitlement {
    let scope = owner_scope_for_user(store, user_id);
    evaluate_owner_and_persist(store, &scope, now).entitlement
}

/// Entitlement governing `user_id`'s remote access (the team pool while they are a member),
/// with the grace transition persisted. Blocking: call it from a blocking context.
pub fn entitlement_for_user(
    state: &AccountState,
    user_id: &str,
    now: u64,
) -> Result<Entitlement, ApiError> {
    Ok(evaluate_and_persist(state, user_id, now)?.entitlement)
}

async fn evaluate_user(
    state: &Arc<AccountState>,
    user_id: &str,
    now: u64,
) -> Result<OwnerEvaluation, ApiError> {
    let state_clone = state.clone();
    let user_id_owned = user_id.to_string();
    let (evaluation, prev_status, owner_key) = offload(move || {
        state_clone.mutate(|store| {
            let scope = owner_scope_for_user(store, &user_id_owned);
            let prev_status = store
                .billing_states
                .get(&scope.owner_key)
                .map(|b| {
                    if b.stopped_at.is_some() {
                        EntitlementStatus::Stopped
                    } else if b.grace_started_at.is_some() {
                        EntitlementStatus::PastDue
                    } else {
                        EntitlementStatus::Ok
                    }
                })
                .unwrap_or(EntitlementStatus::Ok);
            let owner_key = scope.owner_key.clone();
            let evaluation = evaluate_owner_and_persist(store, &scope, now);
            Ok((evaluation, prev_status, owner_key))
        })
    })
    .await?;

    if evaluation.entitlement.status != prev_status {
        if let Err(error) = crate::account::billing::notices::maybe_notify(
            state,
            &owner_key,
            prev_status,
            evaluation.entitlement.status,
            now,
        )
        .await
        {
            tracing::warn!(
                %owner_key,
                %error,
                "failed to dispatch billing notice on status transition"
            );
        }
    }

    Ok(evaluation)
}

fn remote_suspended(evaluation: &OwnerEvaluation) -> ApiError {
    ApiError::new(
        StatusCode::PAYMENT_REQUIRED,
        "REMOTE_SUSPENDED",
        "remote access is suspended for this account",
    )
    .with_details(serde_json::json!({
        "plan": plan_key_str(evaluation.entitlement.effective_plan),
        "status": entitlement_status_str(evaluation.entitlement.status),
        "graceEndsAt": evaluation.entitlement.grace_ends_at,
        "stoppedAt": evaluation.stopped_at,
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementResponse {
    pub plan: PlanKey,
    pub status: EntitlementStatus,
    pub machine_limit: u32,
    pub machines_used: u32,
    pub seats: Option<u32>,
    pub host_packs: u32,
    pub grace_ends_at: Option<u64>,
    pub org_id: Option<String>,
    pub role: Option<String>,
    pub manage_url: Option<String>,
}

impl EntitlementResponse {
    fn from_evaluation(evaluation: &OwnerEvaluation) -> Self {
        let entitlement = &evaluation.entitlement;
        let effective_seats = is_team_plan(entitlement.effective_plan)
            .then(|| evaluation.snapshot.seats.max(TEAM_MIN_SEATS));
        // Team member responses must not disclose customer-portal bearer URLs; only owner and admin can manage billing.
        let manage_url = if evaluation.scope.org_id.is_some()
            && !matches!(
                evaluation.scope.role.as_deref(),
                Some(ROLE_OWNER) | Some(ROLE_ADMIN)
            ) {
            None
        } else {
            evaluation.manage_url.clone()
        };
        Self {
            plan: entitlement.effective_plan,
            status: entitlement.status,
            machine_limit: entitlement.machine_limit,
            machines_used: evaluation.snapshot.machines_used,
            seats: effective_seats,
            host_packs: evaluation.snapshot.host_packs,
            grace_ends_at: entitlement.grace_ends_at,
            org_id: evaluation.scope.org_id.clone(),
            role: evaluation.scope.role.clone(),
            manage_url,
        }
    }
}

pub async fn billing_entitlement(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<EntitlementResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let user = session_user(&state, &headers).await?;
    let evaluation = evaluate_user(&state, &user.user_id, billing_now()).await?;
    Ok(Json(EntitlementResponse::from_evaluation(&evaluation)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckoutBody {
    pub plan: String,
    #[serde(default)]
    pub seats: Option<u32>,
    #[serde(default)]
    pub host_packs: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckoutPlan {
    ProMonthly,
    ProAnnual,
    TeamMonthly,
    TeamAnnual,
    TeamHostpackAnnual,
}

fn parse_checkout_plan(raw: &str) -> Option<CheckoutPlan> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pro_monthly" => Some(CheckoutPlan::ProMonthly),
        "pro_annual" => Some(CheckoutPlan::ProAnnual),
        "team_monthly" => Some(CheckoutPlan::TeamMonthly),
        "team_annual" => Some(CheckoutPlan::TeamAnnual),
        "team_hostpack_annual" => Some(CheckoutPlan::TeamHostpackAnnual),
        _ => None,
    }
}

impl CheckoutPlan {
    fn is_team(self) -> bool {
        matches!(
            self,
            CheckoutPlan::TeamMonthly | CheckoutPlan::TeamAnnual | CheckoutPlan::TeamHostpackAnnual
        )
    }
}

async fn ensure_org_for_checkout(
    state: &Arc<AccountState>,
    user: &UserRecord,
    now: u64,
) -> Result<String, ApiError> {
    let state = state.clone();
    let user_id = user.user_id.clone();
    let org_name = user.email.clone();
    offload(move || {
        state.mutate(|store| {
            let scope = owner_scope_for_user(store, &user_id);
            if let Some(org_id) = scope.org_id {
                return match scope.role.as_deref() {
                    Some(ROLE_OWNER) | Some(ROLE_ADMIN) => Ok(org_id),
                    _ => Err(org_role_required()),
                };
            }
            let org_id = random_token();
            store.orgs.insert(
                org_id.clone(),
                OrgRecord {
                    org_id: org_id.clone(),
                    owner_user_id: user_id.clone(),
                    name: org_name.clone(),
                    created_at: now,
                },
            );
            store.org_members.insert(
                member_key(&org_id, &user_id),
                OrgMemberRecord {
                    org_id: org_id.clone(),
                    user_id: user_id.clone(),
                    role: ROLE_OWNER.to_string(),
                    joined_at: now,
                },
            );
            Ok(org_id)
        })
    })
    .await
}

async fn team_base_subscription_exists(
    state: &Arc<AccountState>,
    user_id: &str,
    now: u64,
) -> Result<bool, ApiError> {
    let state = state.clone();
    let user_id = user_id.to_string();
    offload(move || {
        let store = load_store(&state)?;
        let scope = owner_scope_for_user(&store, &user_id);
        let records = owner_subscriptions(&store, &scope);
        Ok(pick_base_subscription(&records, now)
            .and_then(|record| parse_plan_key(&record.plan_key))
            .map(is_team_plan)
            .unwrap_or(false))
    })
    .await
}

pub async fn billing_checkout(
    State(state): State<Arc<AccountState>>,
    Extension(config): Extension<Arc<BillingConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<CheckoutResponse>, ApiError> {
    require_billing_enabled(&state)?;
    require_ls_config(&config.ls)?;
    let user = session_user(&state, &headers).await?;
    let request: CheckoutBody = parse_json(body)?;
    let plan = parse_checkout_plan(&request.plan)
        .ok_or_else(|| bad_request(format!("unknown plan: {}", request.plan)))?;
    let now = billing_now();

    let choice = match plan {
        CheckoutPlan::ProMonthly | CheckoutPlan::ProAnnual => {
            if request.seats.is_some() {
                return Err(bad_request(
                    "the Pro plan has no seats; use hostPacks for additional machines",
                ));
            }
            let packs = request.host_packs.unwrap_or(0);
            if matches!(plan, CheckoutPlan::ProMonthly) {
                CheckoutPlanChoice::ProMonthly { packs }
            } else {
                CheckoutPlanChoice::ProAnnual { packs }
            }
        }
        CheckoutPlan::TeamMonthly | CheckoutPlan::TeamAnnual => {
            if request.host_packs.unwrap_or(0) != 0 {
                return Err(bad_request(
                    "team host packs are a separate annual subscription: use plan team_hostpack_annual",
                ));
            }
            let seats = request.seats.unwrap_or(TEAM_MIN_SEATS);
            if matches!(plan, CheckoutPlan::TeamMonthly) {
                CheckoutPlanChoice::TeamMonthly { seats }
            } else {
                CheckoutPlanChoice::TeamAnnual { seats }
            }
        }
        CheckoutPlan::TeamHostpackAnnual => {
            if request.seats.is_some() {
                return Err(bad_request(
                    "the team host pack subscription has no seats; change seats on the team subscription",
                ));
            }
            CheckoutPlanChoice::TeamHostpackAnnual {
                packs: request.host_packs.unwrap_or(1),
            }
        }
    };

    let org_id = if plan.is_team() {
        Some(ensure_org_for_checkout(&state, &user, now).await?)
    } else {
        None
    };
    if matches!(plan, CheckoutPlan::TeamHostpackAnnual)
        && !team_base_subscription_exists(&state, &user.user_id, now).await?
    {
        return Err(conflict(
            "TEAM_SUBSCRIPTION_REQUIRED",
            "a team subscription is required before buying host packs",
        ));
    }

    let client = ls_client(&config.ls)?;
    let url = client
        .create_checkout(choice, &user.user_id, org_id.as_deref(), Some(&user.email))
        .await
        .map_err(provider_error)?;
    Ok(Json(CheckoutResponse { url }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuantityBody {
    #[serde(default)]
    pub seats: Option<u32>,
    #[serde(default)]
    pub host_packs: Option<u32>,
}

struct QuantityTargets {
    scope: OwnerScope,
    base: Option<(PlanKey, String)>,
    hostpack: Option<String>,
}

async fn quantity_targets(
    state: &Arc<AccountState>,
    user_id: &str,
    now: u64,
) -> Result<QuantityTargets, ApiError> {
    let state = state.clone();
    let user_id = user_id.to_string();
    offload(move || {
        let store = load_store(&state)?;
        let scope = owner_scope_for_user(&store, &user_id);
        let records = owner_subscriptions(&store, &scope);
        let base = pick_base_subscription(&records, now).and_then(|record| {
            parse_plan_key(&record.plan_key).map(|plan| (plan, record.subscription_id.clone()))
        });
        let hostpack = records
            .iter()
            .filter(|record| record.kind == KIND_HOSTPACK)
            .max_by(|left, right| {
                left.ls_updated_at
                    .cmp(&right.ls_updated_at)
                    .then_with(|| left.subscription_id.cmp(&right.subscription_id))
            })
            .map(|record| record.subscription_id.clone());
        Ok(QuantityTargets {
            scope,
            base,
            hostpack,
        })
    })
    .await
}

pub async fn billing_quantity(
    State(state): State<Arc<AccountState>>,
    Extension(config): Extension<Arc<BillingConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<EntitlementResponse>, ApiError> {
    require_billing_enabled(&state)?;
    require_ls_config(&config.ls)?;
    let user = session_user(&state, &headers).await?;
    let request: QuantityBody = parse_json(body)?;
    if request.seats.is_none() && request.host_packs.is_none() {
        return Err(bad_request("provide seats or hostPacks"));
    }
    let now = billing_now();
    let targets = quantity_targets(&state, &user.user_id, now).await?;
    let (plan, base_subscription_id) = targets.base.clone().ok_or_else(|| {
        conflict(
            "SUBSCRIPTION_REQUIRED",
            "no subscription to change; start with a checkout",
        )
    })?;

    let client = ls_client(&config.ls)?;
    // Every fail-fast check and every quantity conversion happens before the first provider call,
    // so a request that mixes a valid axis with an invalid one cannot leave a half-applied change.
    let mut planned: Vec<(String, u32)> = Vec::new();
    match plan {
        PlanKey::Free => {
            return Err(conflict(
                "SUBSCRIPTION_REQUIRED",
                "no subscription to change; start with a checkout",
            ))
        }
        PlanKey::ProMonthly | PlanKey::ProAnnual => {
            if request.seats.is_some() {
                return Err(bad_request(
                    "the Pro plan has no seats; use hostPacks for additional machines",
                ));
            }
            if let Some(packs) = request.host_packs {
                planned.push((
                    base_subscription_id.clone(),
                    pro_subscription_quantity(packs).map_err(provider_error)?,
                ));
            }
        }
        PlanKey::TeamMonthly | PlanKey::TeamAnnual => {
            if !matches!(
                targets.scope.role.as_deref(),
                Some(ROLE_OWNER) | Some(ROLE_ADMIN)
            ) {
                return Err(org_role_required());
            }
            if let Some(seats) = request.seats {
                planned.push((
                    base_subscription_id.clone(),
                    team_subscription_quantity(seats).map_err(provider_error)?,
                ));
            }
            if let Some(packs) = request.host_packs {
                let hostpack_subscription_id = targets.hostpack.clone().ok_or_else(|| {
                    conflict(
                        "HOSTPACK_SUBSCRIPTION_REQUIRED",
                        "buy the annual team host pack before changing its quantity",
                    )
                })?;
                planned.push((
                    hostpack_subscription_id,
                    team_hostpack_quantity(packs).map_err(provider_error)?,
                ));
            }
        }
    }

    let user_id = user.user_id.clone();
    let ls = config.ls.clone();
    let initial_scope = targets.scope.clone();

    // Reconcile each validated successful provider response before later-axis failures
    // (never hold SQLite across HTTP). Revalidate current membership/role/owner scope
    // inside the mutate transaction before applying captured targets.
    // Also revalidate before any subsequent axis so role loss after the first call cannot
    // initiate a second PATCH.
    for (i, (subscription_id, quantity)) in planned.iter().enumerate() {
        if i > 0 && is_team_plan(plan) {
            let check_state = state.clone();
            let check_user_id = user_id.clone();
            let check_initial_scope = initial_scope.clone();
            offload(move || {
                let store = load_store(&check_state)?;
                let current_scope = owner_scope_for_user(&store, &check_user_id);
                if current_scope.owner_key != check_initial_scope.owner_key
                    || current_scope.org_id != check_initial_scope.org_id
                    || current_scope.role != check_initial_scope.role
                    || !matches!(
                        current_scope.role.as_deref(),
                        Some(ROLE_OWNER) | Some(ROLE_ADMIN)
                    )
                {
                    return Err(org_role_required());
                }
                Ok(())
            })
            .await?;
        }

        let details = update_and_read_back(&client, subscription_id, *quantity).await?;
        let reconcile_state = state.clone();
        let ls = ls.clone();
        let user_id = user_id.clone();
        let initial_scope = initial_scope.clone();
        let facts = SubscriptionFacts::from_details(&details);
        offload(move || {
            reconcile_state.mutate(|store| {
                let current_scope = owner_scope_for_user(store, &user_id);
                if current_scope.owner_key != initial_scope.owner_key
                    || current_scope.org_id != initial_scope.org_id
                {
                    return Err(conflict(
                        "SCOPE_CHANGED",
                        "account scope changed while provider request was in flight",
                    ));
                }
                if is_team_plan(plan)
                    && (current_scope.role != initial_scope.role
                        || !matches!(
                            current_scope.role.as_deref(),
                            Some(ROLE_OWNER) | Some(ROLE_ADMIN)
                        ))
                {
                    return Err(org_role_required());
                }
                if !reconcile_subscription_from_provider(store, &ls, &current_scope, &facts) {
                    tracing::warn!(
                        subscription_id = %facts.subscription_id,
                        "the provider acknowledged a quantity change that could not be reconciled locally; the webhook stays authoritative"
                    );
                }
                Ok(())
            })
        })
        .await?;
    }

    let eval_state = state.clone();
    let eval_user_id = user_id.clone();
    let evaluation = offload(move || {
        eval_state.mutate(|store| {
            let evaluation_scope = owner_scope_for_user(store, &eval_user_id);
            Ok(evaluate_owner_and_persist(store, &evaluation_scope, now))
        })
    })
    .await?;
    Ok(Json(EntitlementResponse::from_evaluation(&evaluation)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseBody {
    pub machine_id: String,
    pub ts: u64,
    pub sig: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaseResponse {
    pub lease: String,
    pub expires_at: u64,
}

pub async fn billing_lease(
    State(state): State<Arc<AccountState>>,
    body: Bytes,
) -> Result<Json<LeaseResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let request: LeaseBody = parse_json(body)?;
    let auth_now = crate::account::store::now_secs();
    let billing_now = billing_now();
    let response = billing_lease_core(&state, &request, auth_now, billing_now).await?;
    Ok(Json(response))
}

pub(crate) async fn billing_lease_core(
    state: &Arc<AccountState>,
    request: &LeaseBody,
    auth_now: u64,
    billing_now: u64,
) -> Result<LeaseResponse, ApiError> {
    if request.ts.abs_diff(auth_now) > LEASE_CLOCK_SKEW_SECS {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "MACHINE_SIGNATURE_STALE",
            "machine signature timestamp is outside the accepted window",
        ));
    }
    let machine = lookup_machine(state, &request.machine_id).await?;
    if !crate::remote::auth::verify_machine_signature(
        &machine.public_key,
        &machine.machine_id,
        request.ts,
        &request.sig,
    ) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "MACHINE_SIGNATURE_INVALID",
            "machine signature verification failed",
        ));
    }

    let evaluation = evaluate_user(state, &machine.owner_user_id, billing_now).await?;
    if !evaluation.entitlement.remote_allowed {
        return Err(remote_suspended(&evaluation));
    }
    let expires_at = lease::lease_expiry(billing_now, evaluation.entitlement.grace_ends_at);
    let owner_key = evaluation.scope.owner_key.clone();
    let machine_id = machine.machine_id.clone();
    let state = state.clone();
    let lease = offload(move || {
        let key = state.signing_key().map_err(|error| {
            ApiError::internal(format!("account signing key unavailable: {error}"))
        })?;
        lease::sign_lease(&key, &machine_id, &owner_key, expires_at).map_err(ApiError::internal)
    })
    .await?;
    Ok(LeaseResponse { lease, expires_at })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookResponse {
    pub status: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebhookAction {
    Duplicate,
    Ignored(&'static str),
    Applied,
}

impl WebhookAction {
    fn status(self) -> &'static str {
        match self {
            WebhookAction::Duplicate => "duplicate",
            WebhookAction::Ignored(_) => "ignored",
            WebhookAction::Applied => "applied",
        }
    }
}

#[derive(Debug, Clone)]
struct SubscriptionFacts {
    subscription_id: String,
    variant_id: String,
    quantity: u32,
    status: SubscriptionStatus,
    ends_at: Option<u64>,
    updated_at: u64,
    customer_id: String,
    manage_url: Option<String>,
}

impl SubscriptionFacts {
    fn from_event(event: &LsEvent) -> Result<Self, ApiError> {
        let status = event
            .status
            .ok_or_else(|| bad_request("subscription event carries no status"))?;
        Ok(Self {
            subscription_id: event.subscription_id.clone(),
            variant_id: event.variant_id.clone(),
            quantity: event.quantity,
            status,
            ends_at: event.ends_at,
            updated_at: event.updated_at,
            customer_id: event.customer_id.clone(),
            manage_url: event.manage_url.clone(),
        })
    }

    fn from_details(details: &SubscriptionDetails) -> Self {
        Self {
            subscription_id: details.subscription_id.clone(),
            variant_id: details.variant_id.clone(),
            quantity: details.quantity,
            status: details.status,
            ends_at: details.ends_at,
            updated_at: details.updated_at,
            customer_id: details.customer_id.clone(),
            manage_url: details.customer_portal_url.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct ExpectedOwner {
    owner_user_id: String,
    org_id: Option<String>,
}

enum WebhookDecision {
    Duplicate,
    Apply(ExpectedOwner),
}

fn resolve_event_owner(
    store: &AccountStore,
    event: &LsEvent,
) -> Result<ExpectedOwner, &'static str> {
    let org_id = event
        .custom_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let user_id = event
        .custom_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (org_id, user_id) {
        (Some(org_id), user_id) => {
            let org = store.orgs.get(org_id).ok_or("ignored:unknown_org")?;
            if let Some(user_id) = user_id {
                if org.owner_user_id != user_id {
                    return Err("ignored:owner_mismatch");
                }
            }
            Ok(ExpectedOwner {
                owner_user_id: org.owner_user_id.clone(),
                org_id: Some(org_id.to_string()),
            })
        }
        (None, Some(user_id)) => {
            if !store.users.contains_key(user_id) {
                return Err("ignored:unknown_user");
            }
            Ok(ExpectedOwner {
                owner_user_id: user_id.to_string(),
                org_id: None,
            })
        }
        (None, None) => {
            let existing = store
                .subscriptions
                .get(&event.subscription_id)
                .ok_or("ignored:unknown_subscription")?;
            Ok(ExpectedOwner {
                owner_user_id: existing.owner_user_id.clone(),
                org_id: existing.org_id.clone(),
            })
        }
    }
}

fn decide_webhook(
    store: &AccountStore,
    key: &str,
    event: &LsEvent,
    authoritative_updated_at: Option<u64>,
) -> Result<WebhookDecision, &'static str> {
    if store
        .payment_events
        .get(key)
        .is_some_and(|record| !record.outcome.starts_with("rejected:"))
    {
        return Ok(WebhookDecision::Duplicate);
    }
    let owner = resolve_event_owner(store, event)?;
    if let Some(existing) = store.subscriptions.get(&event.subscription_id) {
        if existing.owner_user_id != owner.owner_user_id || existing.org_id != owner.org_id {
            return Err("ignored:owner_mismatch");
        }
        if let Some(updated_at) = authoritative_updated_at {
            if updated_at < existing.ls_updated_at {
                return Err("ignored:stale");
            }
        }
    }
    Ok(WebhookDecision::Apply(owner))
}

fn derive_quantities(plan: PlanKey, kind: PlanVariantKind, quantity: u32) -> (u32, u32) {
    match (plan, kind) {
        (PlanKey::ProMonthly | PlanKey::ProAnnual, _) => (0, quantity.saturating_sub(1)),
        (PlanKey::TeamMonthly | PlanKey::TeamAnnual, PlanVariantKind::Base) => (quantity, 0),
        (_, PlanVariantKind::Hostpack) => (0, quantity),
        (PlanKey::Free, _) => (0, 0),
    }
}

/// Sends the new quantity to Lemon Squeezy and reads the subscription back, so every caller
/// reconciles the quantity the provider acknowledged instead of assuming a local value. A failed
/// provider call is surfaced, never ignored.
async fn update_and_read_back(
    client: &LemonSqueezyClient,
    subscription_id: &str,
    quantity: u32,
) -> Result<SubscriptionDetails, ApiError> {
    client
        .update_subscription_quantity(subscription_id, quantity)
        .await
        .map_err(provider_error)?;
    client
        .get_subscription(subscription_id)
        .await
        .map_err(provider_error)
}

fn subscription_record_from_facts(
    facts: &SubscriptionFacts,
    owner_user_id: &str,
    org_id: Option<&str>,
    plan: PlanKey,
    variant_kind: PlanVariantKind,
) -> SubscriptionRecord {
    let (seats, host_packs) = derive_quantities(plan, variant_kind.clone(), facts.quantity);
    SubscriptionRecord {
        subscription_id: facts.subscription_id.clone(),
        owner_user_id: owner_user_id.to_string(),
        org_id: org_id.map(str::to_string),
        plan_key: plan_key_str(plan).to_string(),
        seats,
        host_packs,
        kind: kind_str(variant_kind).to_string(),
        status: subscription_status_str(facts.status).to_string(),
        ends_at: facts.ends_at,
        ls_customer_id: Some(facts.customer_id.clone()).filter(|value| !value.is_empty()),
        ls_updated_at: facts.updated_at,
        manage_url: facts.manage_url.clone(),
    }
}

/// Writes provider facts the account service just acknowledged (a quantity change) into the stored
/// subscription. The record keeps its owner pool and its ordering stamp never moves backwards;
/// `false` means the webhook stays authoritative (missing record, another owner's record, or facts
/// older than what is stored).
fn reconcile_subscription_from_provider(
    store: &mut AccountStore,
    config: &LemonSqueezyConfig,
    scope: &OwnerScope,
    facts: &SubscriptionFacts,
) -> bool {
    let Some(existing) = store.subscriptions.get(&facts.subscription_id) else {
        return false;
    };
    if !subscription_belongs_to(existing, scope) || facts.updated_at < existing.ls_updated_at {
        return false;
    }
    let Some((plan, variant_kind)) = variant_to_plan(&facts.variant_id, config) else {
        return false;
    };
    let owner_user_id = existing.owner_user_id.clone();
    let org_id = existing.org_id.clone();
    store.subscriptions.insert(
        facts.subscription_id.clone(),
        subscription_record_from_facts(
            facts,
            &owner_user_id,
            org_id.as_deref(),
            plan,
            variant_kind,
        ),
    );
    true
}

fn record_payment_event(
    store: &mut AccountStore,
    key: &str,
    event_name: &str,
    now: u64,
    outcome: &str,
) {
    store.payment_events.insert(
        key.to_string(),
        PaymentEventRecord {
            event_key: key.to_string(),
            event_name: event_name.to_string(),
            received_at: now,
            applied_at: (outcome == "applied").then_some(now),
            outcome: outcome.to_string(),
        },
    );
}

/// Records an outcome that is *not* an apply, refusing to overwrite an `applied` row: a concurrent
/// authenticated delivery may have applied the same event between the preflight read and this
/// write, and downgrading it would let a later replay re-run the apply path.
fn record_outcome_unless_applied(
    store: &mut AccountStore,
    key: &str,
    event_name: &str,
    now: u64,
    outcome: &str,
) -> bool {
    if store
        .payment_events
        .get(key)
        .is_some_and(|record| record.outcome == "applied")
    {
        return false;
    }
    record_payment_event(store, key, event_name, now, outcome);
    true
}

fn webhook_event_name(raw: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(raw)
        .ok()?
        .get("meta")?
        .get("event_name")?
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
}

#[allow(clippy::too_many_arguments)]
fn apply_webhook_event(
    store: &mut AccountStore,
    config: &LemonSqueezyConfig,
    key: &str,
    event: &LsEvent,
    facts: &SubscriptionFacts,
    authoritative_updated_at: Option<u64>,
    now: u64,
) -> WebhookAction {
    let decision = match decide_webhook(store, key, event, authoritative_updated_at) {
        Ok(decision) => decision,
        Err(reason) => {
            record_payment_event(store, key, &event.event_name, now, reason);
            return WebhookAction::Ignored(reason);
        }
    };
    let WebhookDecision::Apply(owner) = decision else {
        return WebhookAction::Duplicate;
    };
    let Some((plan, variant_kind)) = variant_to_plan(&facts.variant_id, config) else {
        record_payment_event(
            store,
            key,
            &event.event_name,
            now,
            "ignored:unsupported_variant",
        );
        return WebhookAction::Ignored("ignored:unsupported_variant");
    };
    store.subscriptions.insert(
        facts.subscription_id.clone(),
        subscription_record_from_facts(
            facts,
            &owner.owner_user_id,
            owner.org_id.as_deref(),
            plan,
            variant_kind,
        ),
    );
    record_payment_event(store, key, &event.event_name, now, "applied");
    let scope = owner_scope_for_subscription(store, &owner.owner_user_id, owner.org_id.as_deref());
    evaluate_owner_and_persist(store, &scope, now);
    WebhookAction::Applied
}

pub async fn billing_webhook(
    State(state): State<Arc<AccountState>>,
    Extension(config): Extension<Arc<BillingConfig>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let ls = config.ls.clone();
    require_ls_config(&ls)?;

    let raw = body.to_vec();
    let signature = headers
        .get(WEBHOOK_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let now = billing_now();
    let key = event_key(&raw);
    let event_name = webhook_event_name(&raw);

    if !verify_signature(&ls.webhook_secret, &raw, signature) {
        if let Some(event_name) = event_name {
            let state = state.clone();
            let key = key.clone();
            offload(move || {
                state.mutate(|store| {
                    // An unauthenticated request may record a rejection but never downgrade an
                    // outcome an authenticated delivery already produced: otherwise anyone could
                    // reset the dedupe ledger of a paid webhook by replaying its public body.
                    let authenticated = store
                        .payment_events
                        .get(&key)
                        .is_some_and(|record| !record.outcome.starts_with("rejected:"));
                    if !authenticated {
                        record_payment_event(store, &key, &event_name, now, "rejected:signature");
                    }
                    Ok(())
                })
            })
            .await?;
        }
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "WEBHOOK_SIGNATURE_INVALID",
            "webhook signature verification failed",
        ));
    }

    let event = match parse_event(&raw) {
        Ok(Some(event)) => event,
        Ok(None) => {
            return Ok(Json(WebhookResponse { status: "ignored" }));
        }
        Err(error) => {
            if let Some(event_name) = event_name {
                record_ignored_event(&state, &key, &event_name, now, "ignored:payload_invalid")
                    .await?;
            }
            tracing::warn!(%error, event_key = %key, "billing webhook payload rejected");
            return Ok(Json(WebhookResponse { status: "ignored" }));
        }
    };
    if let Err(error) = event.validate(&ls) {
        record_ignored_event(
            &state,
            &key,
            &event.event_name,
            now,
            "ignored:invalid_event",
        )
        .await?;
        tracing::warn!(%error, event_key = %key, "billing webhook failed validation");
        return Ok(Json(WebhookResponse { status: "ignored" }));
    }

    // Short-circuit authenticated duplicate before external provider fetch (such as invoice
    // authoritative subscription fetch). This prevents provider outages or rate limits from
    // failing replays of already applied valid invoices.
    let pre_dedupe_state = state.clone();
    let pre_dedupe_key = key.clone();
    let is_duplicate = offload(move || {
        let store = load_store(&pre_dedupe_state)?;
        Ok(store
            .payment_events
            .get(&pre_dedupe_key)
            .is_some_and(|record| !record.outcome.starts_with("rejected:")))
    })
    .await?;
    if is_duplicate {
        return Ok(Json(WebhookResponse {
            status: "duplicate",
        }));
    }

    let facts = match event.payload_kind {
        LsPayloadKind::Subscription => SubscriptionFacts::from_event(&event)?,
        LsPayloadKind::Invoice => {
            let client = ls_client(&ls)?;
            let details = client
                .fetch_authoritative_subscription(&event)
                .await
                .map_err(provider_error)?;
            SubscriptionFacts::from_details(&details)
        }
    };
    let authoritative_updated_at = Some(facts.updated_at);

    let preflight_state = state.clone();
    let preflight_event = event.clone();
    let preflight_key = key.clone();
    let preflight = offload(move || {
        let store = load_store(&preflight_state)?;
        Ok(
            match decide_webhook(
                &store,
                &preflight_key,
                &preflight_event,
                authoritative_updated_at,
            ) {
                Ok(WebhookDecision::Duplicate) => Some("duplicate"),
                Ok(WebhookDecision::Apply(_)) => None,
                Err(reason) => Some(reason),
            },
        )
    })
    .await?;
    if let Some(preflight_outcome) = preflight {
        if preflight_outcome == "duplicate" {
            return Ok(Json(WebhookResponse {
                status: "duplicate",
            }));
        }
        record_ignored_event(&state, &key, &event.event_name, now, preflight_outcome).await?;
        return Ok(Json(WebhookResponse { status: "ignored" }));
    }

    let apply_state = state.clone();
    let apply_config = ls.clone();
    let apply_key = key.clone();
    let apply_event = event.clone();
    let apply_facts = facts.clone();
    let action = offload(move || {
        apply_state.mutate(|store| {
            Ok(apply_webhook_event(
                store,
                &apply_config,
                &apply_key,
                &apply_event,
                &apply_facts,
                authoritative_updated_at,
                now,
            ))
        })
    })
    .await?;
    tracing::info!(event_key = %key, event = %event.event_name, ?action, "billing webhook processed");
    Ok(Json(WebhookResponse {
        status: action.status(),
    }))
}

async fn record_ignored_event(
    state: &Arc<AccountState>,
    key: &str,
    event_name: &str,
    now: u64,
    outcome: &'static str,
) -> Result<(), ApiError> {
    let state = state.clone();
    let key = key.to_string();
    let event_name = event_name.to_string();
    offload(move || {
        state.mutate(|store| {
            if !record_outcome_unless_applied(store, &key, &event_name, now, outcome) {
                tracing::warn!(
                    event_key = %key,
                    "keeping an applied outcome: an authenticated delivery already applied this event"
                );
            }
            Ok(())
        })
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMemberView {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub joined_at: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMembersResponse {
    pub org_id: Option<String>,
    pub role: Option<String>,
    pub members: Vec<OrgMemberView>,
}

pub async fn org_members(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<OrgMembersResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let user = session_user(&state, &headers).await?;
    let state_for_read = state.clone();
    let response = offload(move || {
        let store = load_store(&state_for_read)?;
        let scope = owner_scope_for_user(&store, &user.user_id);
        let Some(org_id) = scope.org_id.clone() else {
            return Ok(OrgMembersResponse {
                org_id: None,
                role: None,
                members: Vec::new(),
            });
        };
        let mut members: Vec<OrgMemberView> = store
            .org_members
            .values()
            .filter(|member| member.org_id == org_id)
            .map(|member| OrgMemberView {
                user_id: member.user_id.clone(),
                email: store
                    .users
                    .get(&member.user_id)
                    .map(|record| record.email.clone())
                    .unwrap_or_default(),
                role: member.role.clone(),
                joined_at: member.joined_at,
            })
            .collect();
        members.sort_by(|left, right| {
            left.joined_at
                .cmp(&right.joined_at)
                .then_with(|| left.user_id.cmp(&right.user_id))
        });
        Ok(OrgMembersResponse {
            org_id: Some(org_id),
            role: scope.role.clone(),
            members,
        })
    })
    .await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrgInviteBody {
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgInviteResponse {
    pub email: String,
    pub expires_at: u64,
}

pub async fn org_invite(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<OrgInviteResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let user = session_user(&state, &headers).await?;
    let request: OrgInviteBody = parse_json(body)?;
    let email = normalize_email(&request.email);
    if email.is_empty() || !email.contains('@') {
        return Err(bad_request("email is required"));
    }
    let now = billing_now();
    let token = random_token();
    let expires_at = now.saturating_add(ORG_INVITE_TTL_SECS);

    let write_state = state.clone();
    let inviter_id = user.user_id.clone();
    let invite_email = email.clone();
    let invite_token = token.clone();
    offload(move || {
        write_state.mutate(|store| {
            let scope = owner_scope_for_user(store, &inviter_id);
            let Some(org_id) = scope.org_id.clone() else {
                return Err(org_role_required());
            };
            if !matches!(scope.role.as_deref(), Some(ROLE_OWNER) | Some(ROLE_ADMIN)) {
                return Err(org_role_required());
            }
            let member_ids: Vec<&str> = store
                .org_members
                .values()
                .filter(|member| member.org_id == org_id)
                .map(|member| member.user_id.as_str())
                .collect();
            let already_member = store.users.values().any(|record| {
                record.email == invite_email && member_ids.contains(&record.user_id.as_str())
            });
            if already_member {
                return Err(conflict(
                    "ORG_ALREADY_MEMBER",
                    "that address already belongs to this team",
                ));
            }
            store.org_invites.retain(|_, invite| {
                invite.expires_at > now && (invite.email != invite_email || invite.org_id != org_id)
            });
            store.org_invites.insert(
                token_hash(&invite_token),
                OrgInviteRecord {
                    token_hash: token_hash(&invite_token),
                    org_id,
                    email: invite_email.clone(),
                    expires_at,
                },
            );
            Ok(())
        })
    })
    .await?;

    let mailer = state.mailer.clone();
    let accept_url = format!(
        "{}/org/accept?token={}",
        state.origin.trim_end_matches('/'),
        token
    );
    let mail_to = email.clone();
    offload(move || {
        mailer
            .send_magic_link(&mail_to, &accept_url)
            .map_err(|error| {
                ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "MAIL_FAILED",
                    error.to_string(),
                )
            })
    })
    .await?;

    Ok(Json(OrgInviteResponse { email, expires_at }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrgAcceptBody {
    pub token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgAcceptResponse {
    pub org_id: String,
    pub role: String,
}

pub async fn org_accept(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<OrgAcceptResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let user = session_user(&state, &headers).await?;
    let request: OrgAcceptBody = parse_json(body)?;
    let token = request.token.trim().to_string();
    if token.is_empty() {
        return Err(org_invite_invalid("an invite token is required"));
    }
    let now = billing_now();
    let accept_state = state.clone();
    let accepter_id = user.user_id.clone();
    let accepter_email = normalize_email(&user.email);
    offload(move || {
        accept_state.mutate(|store| {
            let key = token_hash(&token);
            let Some(invite) = store.org_invites.get(&key).cloned() else {
                return Err(org_invite_invalid("this invite is not valid any more"));
            };
            if invite.expires_at <= now {
                store.org_invites.remove(&key);
                return Err(org_invite_invalid("this invite has expired"));
            }
            if invite.email != accepter_email {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "ORG_INVITE_EMAIL_MISMATCH",
                    "this invite was issued to another address",
                ));
            }
            let other_membership = store
                .org_members
                .values()
                .find(|member| member.user_id == accepter_id && member.org_id != invite.org_id);
            if other_membership.is_some() {
                return Err(conflict(
                    "ORG_MEMBERSHIP_CONFLICT",
                    "this account already belongs to another team",
                ));
            }
            let existing_role = store
                .org_members
                .get(&member_key(&invite.org_id, &accepter_id))
                .map(|member| member.role.clone());
            let assigned_role = existing_role.unwrap_or_else(|| ROLE_MEMBER.to_string());

            store.org_members.insert(
                member_key(&invite.org_id, &accepter_id),
                OrgMemberRecord {
                    org_id: invite.org_id.clone(),
                    user_id: accepter_id.clone(),
                    role: assigned_role.clone(),
                    joined_at: now,
                },
            );
            store.org_invites.remove(&key);
            let pool_scope = org_scope(store, &invite.org_id, &accepter_id);
            evaluate_owner_and_persist(store, &pool_scope, now);
            Ok(OrgAcceptResponse {
                org_id: invite.org_id,
                role: assigned_role,
            })
        })
    })
    .await
    .map(Json)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgRemoveResponse {
    pub org_id: String,
    pub removed_user_id: String,
}

pub async fn org_remove_member(
    State(state): State<Arc<AccountState>>,
    AxumPath(target_user_id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<OrgRemoveResponse>, ApiError> {
    require_billing_enabled(&state)?;
    let user = session_user(&state, &headers).await?;
    let now = billing_now();
    let remove_state = state.clone();
    let requester_id = user.user_id.clone();
    offload(move || {
        remove_state.mutate(|store| {
            let scope = owner_scope_for_user(store, &requester_id);
            let Some(org_id) = scope.org_id.clone() else {
                return Err(org_role_required());
            };
            if scope.role.as_deref() != Some(ROLE_OWNER) {
                return Err(org_role_required());
            }
            if target_user_id == requester_id {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "ORG_OWNER_CANNOT_REMOVE_SELF",
                    "the team owner cannot remove themselves",
                ));
            }
            let removed = store
                .org_members
                .remove(&member_key(&org_id, &target_user_id))
                .is_some();
            if !removed {
                return Err(not_found("no such member on this team"));
            }
            // Revoke outstanding invitations for the removed user's normalized email in this org
            // so a previously issued invite token cannot be used to rejoin.
            if let Some(target_user) = store.users.get(&target_user_id) {
                let target_email = normalize_email(&target_user.email);
                store.org_invites.retain(|_, invite| {
                    invite.org_id != org_id || normalize_email(&invite.email) != target_email
                });
            }
            let pool_scope = org_scope(store, &org_id, &requester_id);
            evaluate_owner_and_persist(store, &pool_scope, now);
            let removed_user_scope = owner_scope_for_user(store, &target_user_id);
            evaluate_owner_and_persist(store, &removed_user_scope, now);
            Ok(OrgRemoveResponse {
                org_id,
                removed_user_id: target_user_id,
            })
        })
    })
    .await
    .map(Json)
}

pub async fn org_accept_page(
    State(state): State<Arc<AccountState>>,
) -> Result<Html<&'static str>, ApiError> {
    require_billing_enabled(&state)?;
    Ok(Html(
        "<!DOCTYPE html>\n\
<html lang=\"en\">\n\
<head>\n\
  <meta charset=\"utf-8\">\n\
  <title>Accept Team Invitation - Ferryx</title>\n\
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
  <style>\n\
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; background: #0e0e10; color: #f0f0f5; display: flex; align-items: center; justify-content: center; min-height: 100vh; margin: 0; padding: 20px; }\n\
    .card { background: #18181b; border: 1px solid #27272a; border-radius: 8px; max-width: 440px; width: 100%; padding: 32px; box-sizing: border-box; }\n\
    h1 { font-size: 20px; margin-top: 0; margin-bottom: 12px; }\n\
    p { font-size: 14px; color: #a1a1aa; line-height: 1.5; margin-bottom: 24px; }\n\
    input { background: #27272a; border: 1px solid #3f3f46; color: #fff; padding: 10px 12px; border-radius: 6px; font-size: 14px; width: 100%; box-sizing: border-box; margin-bottom: 12px; }\n\
    button { background: #2563eb; color: #fff; border: none; border-radius: 6px; padding: 10px 16px; font-size: 14px; font-weight: 500; cursor: pointer; width: 100%; }\n\
    button:hover { background: #1d4ed8; }\n\
    button:disabled { background: #3f3f46; cursor: not-allowed; }\n\
    .status { margin-top: 16px; font-size: 13px; line-height: 1.4; }\n\
    .hidden { display: none; }\n\
  </style>\n\
</head>\n\
<body>\n\
  <div class=\"card\">\n\
    <h1>Accept Team Invitation</h1>\n\
    <p>Join your team on Ferryx to access shared relay host machines and resources.</p>\n\
    <div id=\"accept-section\">\n\
      <button id=\"accept-btn\" onclick=\"handleAccept()\">Accept Invitation</button>\n\
    </div>\n\
    <div id=\"login-section\" class=\"hidden\">\n\
      <p style=\"color: #f59e0b; margin-bottom: 12px;\">Please enter your invited email address to sign in and accept this invitation:</p>\n\
      <input type=\"email\" id=\"login-email\" placeholder=\"teammate@example.com\" />\n\
      <button id=\"login-btn\" onclick=\"handleLoginRequest()\">Send Sign-in Link</button>\n\
    </div>\n\
    <div id=\"status\" class=\"status\"></div>\n\
  </div>\n\
  <script>\n\
    const CANONICAL_TOKEN_KEY = 'ferryx_remote_token_account';\n\
    const TOKEN_ORIGIN_KEY = 'ferryx.account.tokenOrigin';\n\
    const ACCOUNT_SESSION_CHANGED_EVENT = 'ferryx:account-session';\n\
\n\
    function cleanOrigin(origin) {\n\
      const trimmed = (origin || '').trim();\n\
      if (!trimmed) {\n\
        return window.location.origin;\n\
      }\n\
      return trimmed.replace(/\\/+$/, '');\n\
    }\n\
\n\
    function getStoredToken() {\n\
      const currentOrigin = cleanOrigin(window.location.origin);\n\
      const issuer = localStorage.getItem(TOKEN_ORIGIN_KEY);\n\
      if (issuer !== currentOrigin) return '';\n\
      return localStorage.getItem(CANONICAL_TOKEN_KEY) || '';\n\
    }\n\
\n\
    function setStoredToken(t) {\n\
      const currentOrigin = cleanOrigin(window.location.origin);\n\
      localStorage.setItem(CANONICAL_TOKEN_KEY, t);\n\
      localStorage.setItem(TOKEN_ORIGIN_KEY, currentOrigin);\n\
      try {\n\
        window.dispatchEvent(new CustomEvent(ACCOUNT_SESSION_CHANGED_EVENT, { detail: { origin: currentOrigin } }));\n\
      } catch (_) {}\n\
    }\n\
\n\
    function clearStoredToken() {\n\
      const currentOrigin = cleanOrigin(window.location.origin);\n\
      localStorage.removeItem(CANONICAL_TOKEN_KEY);\n\
      localStorage.removeItem(TOKEN_ORIGIN_KEY);\n\
      try {\n\
        window.dispatchEvent(new CustomEvent(ACCOUNT_SESSION_CHANGED_EVENT, { detail: { origin: currentOrigin } }));\n\
      } catch (_) {}\n\
    }\n\
\n\
    const urlParams = new URLSearchParams(window.location.search);\n\
    const token = urlParams.get('token') || '';\n\
    const statusDiv = document.getElementById('status');\n\
    const acceptBtn = document.getElementById('accept-btn');\n\
    const acceptSection = document.getElementById('accept-section');\n\
    const loginSection = document.getElementById('login-section');\n\
    const loginBtn = document.getElementById('login-btn');\n\
    const loginEmail = document.getElementById('login-email');\n\
\n\
    async function handleAccept() {\n\
      if (!token) {\n\
        statusDiv.style.color = '#ef4444';\n\
        statusDiv.textContent = 'Error: Missing invite token in link.';\n\
        return;\n\
      }\n\
      const bearer = getStoredToken();\n\
      if (!bearer) {\n\
        acceptSection.classList.add('hidden');\n\
        loginSection.classList.remove('hidden');\n\
        statusDiv.style.color = '#a1a1aa';\n\
        statusDiv.textContent = 'Sign-in required to verify your invited email.';\n\
        return;\n\
      }\n\
      acceptBtn.disabled = true;\n\
      statusDiv.style.color = '#a1a1aa';\n\
      statusDiv.textContent = 'Joining team...';\n\
      try {\n\
        const resp = await fetch('/api/account/v1/org/accept', {\n\
          method: 'POST',\n\
          headers: {\n\
            'Content-Type': 'application/json',\n\
            'Authorization': 'Bearer ' + bearer\n\
          },\n\
          body: JSON.stringify({ token: token })\n\
        });\n\
        const data = await resp.json();\n\
        if (resp.ok) {\n\
          statusDiv.style.color = '#10b981';\n\
          statusDiv.textContent = 'Success! You joined the team as ' + (data.role || 'member') + '. You may close this window.';\n\
          acceptBtn.style.display = 'none';\n\
        } else if (resp.status === 401) {\n\
          clearStoredToken();\n\
          acceptSection.classList.add('hidden');\n\
          loginSection.classList.remove('hidden');\n\
          statusDiv.style.color = '#f59e0b';\n\
          statusDiv.textContent = 'Session expired. Please enter your email to sign in:';\n\
        } else {\n\
          statusDiv.style.color = '#ef4444';\n\
          statusDiv.textContent = data.message || ('Failed to accept (' + (data.code || 'ERROR') + ')');\n\
          acceptBtn.disabled = false;\n\
        }\n\
      } catch (err) {\n\
        statusDiv.style.color = '#ef4444';\n\
        statusDiv.textContent = 'Network error: ' + err.message;\n\
        acceptBtn.disabled = false;\n\
      }\n\
    }\n\
\n\
    async function handleLoginRequest() {\n\
      const email = (loginEmail.value || '').trim();\n\
      if (!email || !email.includes('@')) {\n\
        statusDiv.style.color = '#ef4444';\n\
        statusDiv.textContent = 'Valid email is required.';\n\
        return;\n\
      }\n\
      loginBtn.disabled = true;\n\
      statusDiv.style.color = '#a1a1aa';\n\
      statusDiv.textContent = 'Sending sign-in link...';\n\
      try {\n\
        const resp = await fetch('/api/account/v1/login/request', {\n\
          method: 'POST',\n\
          headers: { 'Content-Type': 'application/json' },\n\
          body: JSON.stringify({ email: email })\n\
        });\n\
        const data = await resp.json();\n\
        if (resp.ok) {\n\
          statusDiv.style.color = '#10b981';\n\
          statusDiv.textContent = 'Check your email for the sign-in code, then return here to complete acceptance.';\n\
          pollLogin(data.loginHandle);\n\
        } else {\n\
          statusDiv.style.color = '#ef4444';\n\
          statusDiv.textContent = data.message || 'Login request failed.';\n\
          loginBtn.disabled = false;\n\
        }\n\
      } catch (err) {\n\
        statusDiv.style.color = '#ef4444';\n\
        statusDiv.textContent = 'Network error: ' + err.message;\n\
        loginBtn.disabled = false;\n\
      }\n\
    }\n\
\n\
    async function pollLogin(handle) {\n\
      if (!handle) return;\n\
      for (let i = 0; i < 60; i++) {\n\
        await new Promise(r => setTimeout(r, 2000));\n\
        try {\n\
          const resp = await fetch('/api/account/v1/login/poll', {\n\
            method: 'POST',\n\
            headers: { 'Content-Type': 'application/json' },\n\
            body: JSON.stringify({ loginHandle: handle })\n\
          });\n\
          if (resp.ok) {\n\
            const data = await resp.json();\n\
            if (data.status === 'approved' && data.token) {\n\
              setStoredToken(data.token);\n\
              loginSection.classList.add('hidden');\n\
              acceptSection.classList.remove('hidden');\n\
              statusDiv.style.color = '#10b981';\n\
              statusDiv.textContent = 'Signed in successfully. Accepting invite...';\n\
              handleAccept();\n\
              return;\n\
            }\n\
          } else {\n\
            const errData = await resp.json().catch(() => ({}));\n\
            loginBtn.disabled = false;\n\
            statusDiv.style.color = '#ef4444';\n\
            statusDiv.textContent = errData.message || ('Polling error: ' + (errData.code || resp.status));\n\
            return;\n\
          }\n\
        } catch (pollErr) {\n\
          loginBtn.disabled = false;\n\
          statusDiv.style.color = '#ef4444';\n\
          statusDiv.textContent = 'Network error during poll: ' + pollErr.message;\n\
          return;\n\
        }\n\
      }\n\
      loginBtn.disabled = false;\n\
      statusDiv.style.color = '#ef4444';\n\
      statusDiv.textContent = 'Sign-in timed out. Please try again.';\n\
    }\n\
\n\
    if (getStoredToken()) {\n\
      handleAccept();\n\
    }\n\
  </script>\n\
</body>\n\
</html>\n\
"
    ))
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod routes_tests;
