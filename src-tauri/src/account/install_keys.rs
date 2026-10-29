//! Account install keys: short-lived, single-use, owner-bound enrollment secrets for the
//! one-line installer (`fxk_<64 hex>`). Only the sha256 is stored, in `enrollment_codes` with
//! `kind = 'install_key'`; a successful enroll marks the row consumed and keeps it for audit.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::service::{
    authenticate_user, machines_used_for_user, parse_json, plan_key_str,
    run_account_blocking, AccountState, ApiError,
};
use super::store::{now_secs, random_token, EnrollmentCodeRecord, ENROLLMENT_KIND_INSTALL_KEY};

pub const INSTALL_KEY_PREFIX: &str = "fxk_";
pub const INSTALL_KEY_DEFAULT_TTL_SECS: u64 = 3600;
pub const INSTALL_KEY_MIN_TTL_SECS: u64 = 600;
pub const INSTALL_KEY_MAX_TTL_SECS: u64 = 86_400;
/// Maximum unconsumed, unexpired install keys one user may hold at a time.
pub const INSTALL_KEY_OUTSTANDING_LIMIT: usize = 10;
const INSTALL_KEY_LABEL_MAX_CHARS: usize = 120;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssueInstallKeyBody {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallCommand {
    pub sh: String,
    pub ps1: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IssueInstallKeyResponse {
    pub key_id: String,
    pub key: String,
    pub expires_at: u64,
    pub install_command: InstallCommand,
}

/// Install key metadata for listing. Never carries the key or its hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallKeyView {
    pub key_id: String,
    pub label: Option<String>,
    pub created_at: u64,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumed_machine_id: Option<String>,
}

/// Requested TTL clamped to `[INSTALL_KEY_MIN_TTL_SECS, INSTALL_KEY_MAX_TTL_SECS]`.
pub fn clamp_install_key_ttl(requested: Option<u64>) -> u64 {
    requested
        .unwrap_or(INSTALL_KEY_DEFAULT_TTL_SECS)
        .clamp(INSTALL_KEY_MIN_TTL_SECS, INSTALL_KEY_MAX_TTL_SECS)
}

pub fn new_install_key() -> String {
    format!("{INSTALL_KEY_PREFIX}{}", random_token())
}

/// One-liners for the origin that issued `key`. The downloaded script defaults to the hosted
/// relay, so any other origin is passed to it as `FERRYX_ORIGIN` (quoted for each shell);
/// the hosted origin keeps the short commands.
pub fn install_command(origin: &str, key: &str) -> InstallCommand {
    let origin = origin.trim_end_matches('/');
    if origin == crate::remote::state::DEFAULT_RELAY_URL {
        return InstallCommand {
            sh: format!("curl -fsSL {origin}/install.sh | sh -s -- --key {key}"),
            ps1: format!("& ([scriptblock]::Create((irm {origin}/install.ps1))) -Key {key}"),
        };
    }
    let sh_url = crate::ssh::direct::quote_posix(&format!("{origin}/install.sh"));
    let sh_origin = crate::ssh::direct::quote_posix(origin);
    let ps1_url = quote_powershell(&format!("{origin}/install.ps1"));
    let ps1_origin = quote_powershell(origin);
    InstallCommand {
        sh: format!("curl -fsSL {sh_url} | FERRYX_ORIGIN={sh_origin} sh -s -- --key {key}"),
        ps1: format!(
            "$env:FERRYX_ORIGIN={ps1_origin}; & ([scriptblock]::Create((irm {ps1_url}))) -Key {key}"
        ),
    }
}

/// PowerShell single-quoted string literal: `'` is escaped by doubling.
fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `POST /api/account/v1/install-keys`
pub async fn issue_install_key(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<IssueInstallKeyResponse>, ApiError> {
    let request: IssueInstallKeyBody = if body.iter().all(u8::is_ascii_whitespace) {
        IssueInstallKeyBody::default()
    } else {
        parse_json(body).await?
    };
    let label = request
        .label
        .map(|label| label.trim().chars().take(INSTALL_KEY_LABEL_MAX_CHARS).collect::<String>())
        .filter(|label| !label.is_empty());
    let ttl = clamp_install_key_ttl(request.ttl_secs);
    let key = new_install_key();
    let key_hash = crate::remote::auth::enrollment_code_hash(&key);
    let key_id = format!("ik_{}", &random_token()[..20]);

    let blocking_state = Arc::clone(&state);
    let blocking_key_id = key_id.clone();
    let expires_at = run_account_blocking(move || {
        let user = authenticate_user(&blocking_state, &headers)?;
        let now = now_secs();
        let billing_now = crate::account::billing::routes::billing_now();
        blocking_state.mutate(|store| {
            let outstanding = store
                .enrollment_codes
                .values()
                .filter(|code| {
                    code.is_install_key()
                        && code.user_id == user.user_id
                        && code.consumed_at.is_none()
                        && code.expires_at > now
                })
                .count();
            if outstanding >= INSTALL_KEY_OUTSTANDING_LIMIT {
                return Ok(Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "INSTALL_KEY_LIMIT",
                    "too many outstanding install keys; revoke or use one first",
                )
                .with_details(serde_json::json!({
                    "limit": INSTALL_KEY_OUTSTANDING_LIMIT,
                    "outstanding": outstanding,
                }))));
            }
            if blocking_state.deployment_mode.is_billing_enabled() {
                let entitlement = crate::account::billing::routes::entitlement_for_user_in_store(
                    store,
                    &user.user_id,
                    billing_now,
                );
                if !entitlement.may_enroll_new {
                    let used = machines_used_for_user(store, &user.user_id);
                    // Ok(Err(..)) commits any persisted billing transition without issuing a key.
                    return Ok(Err(ApiError::new(
                        StatusCode::PAYMENT_REQUIRED,
                        "PLAN_LIMIT_REACHED",
                        "plan machine limit reached",
                    )
                    .with_details(serde_json::json!({
                        "plan": plan_key_str(entitlement.effective_plan),
                        "limit": entitlement.machine_limit,
                        "used": used,
                    }))));
                }
            }
            let expires_at = now + ttl;
            store.enrollment_codes.insert(
                key_hash.clone(),
                EnrollmentCodeRecord {
                    user_id: user.user_id.clone(),
                    account_origin: blocking_state.origin.clone(),
                    expires_at,
                    kind: ENROLLMENT_KIND_INSTALL_KEY.to_string(),
                    key_id: Some(blocking_key_id.clone()),
                    label: label.clone(),
                    created_at: now,
                    ..Default::default()
                },
            );
            Ok(Ok(expires_at))
        })?
    })
    .await?;

    let install_command = install_command(&state.origin, &key);
    Ok(Json(IssueInstallKeyResponse {
        key_id,
        key,
        expires_at,
        install_command,
    }))
}

/// `GET /api/account/v1/install-keys`
pub async fn list_install_keys(
    State(state): State<Arc<AccountState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<InstallKeyView>>, ApiError> {
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        let store = state.load().map_err(ApiError::internal)?;
        let mut keys: Vec<InstallKeyView> = store
            .enrollment_codes
            .values()
            .filter(|code| code.is_install_key() && code.user_id == user.user_id)
            .filter_map(|code| {
                Some(InstallKeyView {
                    key_id: code.key_id.clone()?,
                    label: code.label.clone(),
                    created_at: code.created_at,
                    expires_at: code.expires_at,
                    consumed_at: code.consumed_at,
                    consumed_machine_id: code.consumed_machine_id.clone(),
                })
            })
            .collect();
        keys.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.key_id.cmp(&right.key_id))
        });
        Ok(keys)
    })
    .await
    .map(Json)
}

/// `POST /api/account/v1/install-keys/{keyId}/revoke`
///
/// Removes an unconsumed key so it can no longer enroll. A consumed key is already unusable and
/// stays as the audit record; revoking it is a no-op.
pub async fn revoke_install_key(
    State(state): State<Arc<AccountState>>,
    Path(key_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    run_account_blocking(move || {
        let user = authenticate_user(&state, &headers)?;
        state.mutate(|store| {
            let found = store
                .enrollment_codes
                .iter()
                .find(|(_, code)| {
                    code.is_install_key()
                        && code.user_id == user.user_id
                        && code.key_id.as_deref() == Some(key_id.as_str())
                })
                .map(|(hash, code)| (hash.clone(), code.consumed_at.is_some()));
            let Some((hash, consumed)) = found else {
                return Err(ApiError::new(
                    StatusCode::NOT_FOUND,
                    "INSTALL_KEY_NOT_FOUND",
                    "no such install key on this account",
                ));
            };
            if !consumed {
                store.enrollment_codes.remove(&hash);
            }
            Ok(())
        })
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_is_clamped_to_the_allowed_range() {
        assert_eq!(clamp_install_key_ttl(None), 3600);
        assert_eq!(clamp_install_key_ttl(Some(1)), 600);
        assert_eq!(clamp_install_key_ttl(Some(7200)), 7200);
        assert_eq!(clamp_install_key_ttl(Some(10_000_000)), 86_400);
    }

    #[test]
    fn key_format_and_install_commands() {
        let key = new_install_key();
        assert!(key.starts_with("fxk_"));
        assert_eq!(key.len(), 4 + 64);
        assert!(key[4..].chars().all(|c| c.is_ascii_hexdigit()));
        let command = install_command("https://relay.test/", "fxk_abc");
        assert_eq!(
            command.sh,
            "curl -fsSL 'https://relay.test/install.sh' | FERRYX_ORIGIN='https://relay.test' sh -s -- --key fxk_abc"
        );
        assert_eq!(
            command.ps1,
            "$env:FERRYX_ORIGIN='https://relay.test'; & ([scriptblock]::Create((irm 'https://relay.test/install.ps1'))) -Key fxk_abc"
        );
    }

    #[test]
    fn hosted_origin_commands_are_unchanged() {
        for origin in ["https://relay.ferryx.dev", "https://relay.ferryx.dev/"] {
            let command = install_command(origin, "fxk_abc");
            assert_eq!(
                command.sh,
                "curl -fsSL https://relay.ferryx.dev/install.sh | sh -s -- --key fxk_abc"
            );
            assert_eq!(
                command.ps1,
                "& ([scriptblock]::Create((irm https://relay.ferryx.dev/install.ps1))) -Key fxk_abc"
            );
        }
    }

    #[test]
    fn self_hosted_origin_is_carried_into_both_commands() {
        let command = install_command("http://127.0.0.1:8787", "fxk_abc");
        assert_eq!(
            command.sh,
            "curl -fsSL 'http://127.0.0.1:8787/install.sh' | FERRYX_ORIGIN='http://127.0.0.1:8787' sh -s -- --key fxk_abc"
        );
        assert_eq!(
            command.ps1,
            "$env:FERRYX_ORIGIN='http://127.0.0.1:8787'; & ([scriptblock]::Create((irm 'http://127.0.0.1:8787/install.ps1'))) -Key fxk_abc"
        );
    }

    #[test]
    fn origin_needing_quoting_is_escaped_per_shell() {
        let command = install_command("https://it's.example/a b;$(x)", "fxk_abc");
        assert_eq!(
            command.sh,
            "curl -fsSL 'https://it'\\''s.example/a b;$(x)/install.sh' | FERRYX_ORIGIN='https://it'\\''s.example/a b;$(x)' sh -s -- --key fxk_abc"
        );
        assert_eq!(
            command.ps1,
            "$env:FERRYX_ORIGIN='https://it''s.example/a b;$(x)'; & ([scriptblock]::Create((irm 'https://it''s.example/a b;$(x)/install.ps1'))) -Key fxk_abc"
        );
    }
}
