//! `POST /api/account/v1/account/delete` (account session): in-app account deletion
//! (App Store 5.1.1(v), Google Play account deletion policy).
//!
//! Body `{"confirmEmail": "..."}` must match the session account email case-insensitively,
//! else `400 CONFIRMATION_MISMATCH`. On success (`200 {"deleted": true}`) one store transaction
//! removes the user row and everything keyed to it: every session, enrollment code / install
//! key, enrolled machine (which is what the peer-record route and relay admission read), grant,
//! owned org with its members and invites, org memberships, billing state, expired subscription
//! records, and the pending login / device / machine-request / invite rows that carry the email.
//! The address can sign up again as a brand-new account.
//!
//! Billing: the Lemon Squeezy adapter has no cancel call, so an account whose subscriptions
//! (personal or of an org it owns) are still active is refused with `409 SUBSCRIPTION_ACTIVE`
//! instead of silently dropping a paid plan. The user cancels in the billing portal first.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::billing::routes::billing_now;
use super::service::{bearer, parse_json, ApiError, AccountState};
use super::store::{normalize_email, now_secs, AccountStore, SubscriptionRecord};
use crate::account::billing::entitlement::is_subscription_active;
use crate::account::billing::lemonsqueezy::parse_subscription_status;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteAccountBody {
    pub confirm_email: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteAccountResponse {
    pub deleted: bool,
}

fn unauthorized(message: &str) -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
}

fn is_active(record: &SubscriptionRecord, now: u64) -> bool {
    parse_subscription_status(&record.status)
        .is_some_and(|status| is_subscription_active(status, record.ends_at, now))
}

/// Removes the user and every record keyed to its id or email. Callers checked billing first.
fn purge_user(store: &mut AccountStore, user_id: &str, email: &str) {
    let owned_orgs: Vec<String> = store
        .orgs
        .values()
        .filter(|org| org.owner_user_id == user_id)
        .map(|org| org.org_id.clone())
        .collect();
    let owned_machines: Vec<String> = store
        .machines
        .values()
        .filter(|machine| machine.owner_user_id == user_id)
        .map(|machine| machine.machine_record_id.clone())
        .collect();

    store.users.remove(user_id);
    store.sessions.retain(|_, s| s.user_id != user_id);
    store.enrollment_codes.retain(|_, c| c.user_id != user_id);
    store.machines.retain(|_, m| m.owner_user_id != user_id);
    store.grants.retain(|_, g| {
        g.owner_user_id != user_id && !owned_machines.contains(&g.machine_record_id)
    });
    store.subscriptions.retain(|_, s| {
        s.owner_user_id != user_id
            && !s.org_id.as_ref().is_some_and(|org| owned_orgs.contains(org))
    });
    store.billing_states.remove(user_id);
    for org_id in &owned_orgs {
        store.billing_states.remove(&format!("org:{org_id}"));
    }
    store.orgs.retain(|_, o| o.owner_user_id != user_id);
    store
        .org_members
        .retain(|_, m| m.user_id != user_id && !owned_orgs.contains(&m.org_id));
    store
        .org_invites
        .retain(|_, i| normalize_email(&i.email) != email && !owned_orgs.contains(&i.org_id));
    store.login_codes.retain(|_, c| normalize_email(&c.email) != email);
    store.device_auths.retain(|_, a| normalize_email(&a.email) != email);
    store.machine_requests.retain(|_, r| {
        r.decided_by.as_deref() != Some(user_id)
            && r.email_hint.as_deref().map(normalize_email).as_deref() != Some(email)
    });
}

pub async fn delete_account(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<DeleteAccountResponse>, ApiError> {
    let token = bearer(&headers).ok_or_else(|| unauthorized("missing bearer token"))?;
    let request: DeleteAccountBody = parse_json(body).await?;
    let now = now_secs();
    let billing_clock = billing_now();
    state.mutate(|store| {
        let user = store
            .user_for_session(&token, now)
            .cloned()
            .ok_or_else(|| unauthorized("unknown or expired session"))?;
        let email = normalize_email(&user.email);
        if normalize_email(&request.confirm_email) != email {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "CONFIRMATION_MISMATCH",
                "confirmEmail does not match the signed-in account",
            ));
        }
        let owned_orgs: Vec<&str> = store
            .orgs
            .values()
            .filter(|org| org.owner_user_id == user.user_id)
            .map(|org| org.org_id.as_str())
            .collect();
        let has_active_subscription = store.subscriptions.values().any(|s| {
            (s.owner_user_id == user.user_id
                || s.org_id.as_deref().is_some_and(|org| owned_orgs.contains(&org)))
                && is_active(s, billing_clock)
        });
        if has_active_subscription {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "SUBSCRIPTION_ACTIVE",
                "cancel the active subscription in the billing portal before deleting the account",
            ));
        }
        purge_user(store, &user.user_id, &email);
        Ok(())
    })?;
    Ok(Json(DeleteAccountResponse { deleted: true }))
}

#[cfg(test)]
#[path = "deletion_tests.rs"]
mod tests;
