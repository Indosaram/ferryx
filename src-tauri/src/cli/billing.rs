//! Host-side client for the account billing contract that `ferryx-cli account plan` reads.
//!
//! Wire contract (`.omo/plans/relay-monetization.md`, "API contract"):
//!
//! ```text
//! GET /api/account/v1/billing/entitlement
//! 200 {"plan":"free|pro_monthly|pro_annual|team_monthly|team_annual",
//!      "status":"ok|over_limit|past_due|stopped","machineLimit":n,"machinesUsed":n,
//!      "seats":n|null,"hostPacks":n,"graceEndsAt":unix|null,"orgId":str|null,
//!      "role":"owner|admin|member"|null,"manageUrl":str|null}
//! 402 PLAN_LIMIT_REACHED details{plan,limit,used}
//! 402 REMOTE_SUSPENDED   details{plan,status,graceEndsAt,stoppedAt}
//! 503 BILLING_UNCONFIGURED
//! ```
//!
//! The read is account-session authenticated: `Authorization: Bearer <session>`, the same credential
//! the desktop and web clients hold (`account/service.rs` resolves it with `user_for_session`). The
//! machine's enrolled Ed25519 identity is deliberately not sent here — that route does not accept it,
//! and the CLI does not invent an auth scheme.
//!
//! Sibling billing routes stay off the CLI surface: `POST /api/account/v1/billing/checkout` answers
//! `200 {"url":"https://...lemonsqueezy.com/..."}` and `POST /api/account/v1/billing/quantity`
//! answers the same entitlement document as the GET above. Both are session-authenticated user actions
//! owned by the desktop and remote surfaces (todos 11/12); the CLI reads state and explains refusals.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::account::billing::entitlement::{EntitlementStatus, PlanKey};
use crate::account::enroll_client::EnrollError;

pub const ENTITLEMENT_PATH: &str = "/api/account/v1/billing/entitlement";

/// Public pricing page every upgrade hint points at. Local, LAN and self-hosted use stay free.
pub const PRICING_URL: &str = "https://ferryx.dev/docs/pricing/";

pub const PLAN_LIMIT_REACHED: &str = "PLAN_LIMIT_REACHED";
pub const REMOTE_SUSPENDED: &str = "REMOTE_SUSPENDED";
pub const BILLING_UNCONFIGURED: &str = "BILLING_UNCONFIGURED";
pub const ACCOUNT_SESSION_REQUIRED: &str = "ACCOUNT_SESSION_REQUIRED";

/// Environment input for an existing account session: the bearer and the origin that issued it.
pub const SESSION_TOKEN_ENV: &str = "FERRYX_ACCOUNT_SESSION_TOKEN";
pub const SESSION_ORIGIN_ENV: &str = "FERRYX_ACCOUNT_SESSION_ORIGIN";

pub const EXIT_PLAN_BLOCKED: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    Owner,
    Admin,
    Member,
}

/// The canonical entitlement document. `plan` and `status` reuse the billing domain enums, so the
/// CLI can never drift from the server-side spelling of the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingEntitlement {
    pub plan: PlanKey,
    pub status: EntitlementStatus,
    pub machine_limit: u32,
    pub machines_used: u32,
    pub seats: Option<u32>,
    pub host_packs: u32,
    pub grace_ends_at: Option<u64>,
    pub org_id: Option<String>,
    pub role: Option<OrgRole>,
    pub manage_url: Option<String>,
}

/// `details` of a `402 PLAN_LIMIT_REACHED` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLimitDetails {
    pub plan: String,
    pub limit: u64,
    pub used: u64,
}

impl PlanLimitDetails {
    pub fn from_details(details: Option<&serde_json::Value>) -> Option<Self> {
        Some(Self {
            plan: details?.get("plan")?.as_str()?.to_string(),
            limit: details?.get("limit")?.as_u64()?,
            used: details?.get("used")?.as_u64()?,
        })
    }
}

/// `details` of a `402 REMOTE_SUSPENDED` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspensionDetails {
    pub plan: String,
    pub status: String,
    pub grace_ends_at: Option<u64>,
    pub stopped_at: Option<u64>,
}

impl SuspensionDetails {
    pub fn from_details(details: Option<&serde_json::Value>) -> Option<Self> {
        Some(Self {
            plan: details?.get("plan")?.as_str()?.to_string(),
            status: details?.get("status")?.as_str()?.to_string(),
            grace_ends_at: details?.get("graceEndsAt").and_then(|value| value.as_u64()),
            stopped_at: details?.get("stoppedAt").and_then(|value| value.as_u64()),
        })
    }
}

/// CLI failure carrying the process exit code the account commands must use.
///
/// `3` is reserved for responses that mean "the plan blocks this": `PLAN_LIMIT_REACHED` (too many
/// enrolled computers) and `REMOTE_SUSPENDED` (remote access stopped after the grace period).
/// Everything else keeps the historical exit code `1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCliError {
    pub message: String,
    pub exit_code: i32,
}

impl AccountCliError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    pub fn local(code: &str, message: impl Into<String>) -> Self {
        Self::usage(format!("{code}: {}", message.into()))
    }

    pub fn plan_blocked(&self) -> bool {
        self.exit_code == EXIT_PLAN_BLOCKED
    }
}

impl std::fmt::Display for AccountCliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

/// Help shown whenever the account-session input is missing, mismatched, or unusable.
pub const SESSION_HELP: &str =
    "the entitlement read reuses an existing account session: set FERRYX_ACCOUNT_SESSION_TOKEN to the \
     session bearer and FERRYX_ACCOUNT_SESSION_ORIGIN to the account origin that issued it (it must be \
     the requested --origin); this command never writes the session to disk";

/// Resolves the account session bearer from the environment.
pub fn resolve_session_token(requested_origin: &str) -> Result<String, AccountCliError> {
    resolve_session_token_with(requested_origin, |key| std::env::var(key).ok())
}

/// [`resolve_session_token`] over an injected environment lookup, so the origin guard is testable
/// without mutating process-wide state. The bearer is only returned when the configured issuer origin
/// equals the requested origin, so a session minted for one account service is never sent to another.
pub fn resolve_session_token_with<F>(
    requested_origin: &str,
    lookup: F,
) -> Result<String, AccountCliError>
where
    F: Fn(&str) -> Option<String>,
{
    let token = lookup(SESSION_TOKEN_ENV)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AccountCliError::local(ACCOUNT_SESSION_REQUIRED, SESSION_HELP))?;
    let issuer = lookup(SESSION_ORIGIN_ENV)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AccountCliError::local(ACCOUNT_SESSION_REQUIRED, SESSION_HELP))?;
    let issuer = crate::account::origin::normalize_account_origin(&issuer)
        .map_err(|_| AccountCliError::local(ACCOUNT_SESSION_REQUIRED, SESSION_HELP))?;
    let requested = crate::account::origin::normalize_account_origin(requested_origin)
        .map_err(|_| AccountCliError::local(ACCOUNT_SESSION_REQUIRED, SESSION_HELP))?;
    if issuer != requested {
        return Err(AccountCliError::local(
            ACCOUNT_SESSION_REQUIRED,
            format!(
                "the account session belongs to {issuer}, not {requested}; {}",
                SESSION_HELP
            ),
        ));
    }
    Ok(token)
}

pub fn parse_entitlement(body: &str) -> Result<BillingEntitlement, AccountCliError> {
    serde_json::from_str::<BillingEntitlement>(body).map_err(|error| {
        AccountCliError::local(
            "ACCOUNT_BAD_RESPONSE",
            format!("unparseable entitlement document: {error}"),
        )
    })
}

pub fn entitlement_json(entitlement: &BillingEntitlement) -> Result<String, AccountCliError> {
    serde_json::to_string_pretty(entitlement).map_err(|error| {
        AccountCliError::local(
            "ACCOUNT_BAD_RESPONSE",
            format!("cannot encode the entitlement document: {error}"),
        )
    })
}

pub fn render_plan_text(entitlement: &BillingEntitlement) -> String {
    let mut out = String::from("Ferryx account plan\n");
    out.push_str(&row("Plan", plan_line(entitlement.plan)));
    out.push_str(&row(
        "Machines",
        &format!("{} / {}", entitlement.machines_used, entitlement.machine_limit),
    ));
    out.push_str(&row("Status", status_line(entitlement.status)));
    out.push_str(&row(
        "Grace ends",
        &entitlement
            .grace_ends_at
            .map(render_unix)
            .unwrap_or_else(|| "-".to_string()),
    ));
    if let Some(seats) = entitlement.seats {
        out.push_str(&row("Seats", &seats.to_string()));
    }
    out.push_str(&row("Host packs", &entitlement.host_packs.to_string()));
    if let Some(org_id) = entitlement.org_id.as_deref() {
        let role = entitlement.role.map(role_label).unwrap_or("member");
        out.push_str(&row("Organization", &format!("{org_id} ({role})")));
    }
    if let Some(url) = entitlement.manage_url.as_deref() {
        out.push_str(&row("Manage", url));
    }
    out
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<14}{value}\n")
}

fn plan_line(plan: PlanKey) -> &'static str {
    match plan {
        PlanKey::Free => "free",
        PlanKey::ProMonthly => "pro_monthly (Pro, billed monthly)",
        PlanKey::ProAnnual => "pro_annual (Pro, billed yearly)",
        PlanKey::TeamMonthly => "team_monthly (Team, billed monthly)",
        PlanKey::TeamAnnual => "team_annual (Team, billed yearly)",
    }
}

fn status_line(status: EntitlementStatus) -> &'static str {
    match status {
        EntitlementStatus::Ok => "ok",
        EntitlementStatus::OverLimit => {
            "over_limit (more computers than the plan allows; remote stops when the grace period ends)"
        }
        EntitlementStatus::PastDue => {
            "past_due (the last payment failed; remote stops when the grace period ends)"
        }
        EntitlementStatus::Stopped => {
            "stopped (remote access is suspended until billing or the computer list is fixed)"
        }
    }
}

fn role_label(role: OrgRole) -> &'static str {
    match role {
        OrgRole::Owner => "owner",
        OrgRole::Admin => "admin",
        OrgRole::Member => "member",
    }
}

/// Unix seconds render as RFC 3339 UTC, the format the rest of the account surface prints.
fn render_unix(seconds: u64) -> String {
    let Ok(timestamp) = i64::try_from(seconds) else {
        return seconds.to_string();
    };
    match time::OffsetDateTime::from_unix_timestamp(timestamp) {
        Ok(value) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            value.year(),
            value.month() as u8,
            value.day(),
            value.hour(),
            value.minute(),
            value.second()
        ),
        Err(_) => seconds.to_string(),
    }
}

pub fn billing_error(origin: &str, status: u16, body: &str) -> AccountCliError {
    let envelope: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let code = envelope
        .get("code")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("ACCOUNT_HTTP_{status}"));
    let message = envelope
        .get("message")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| body.trim().to_string());
    let details = envelope.get("details").filter(|value| !value.is_null());
    explained_error(origin, status, &code, &message, details)
}

/// The structured `{code, message, details}` envelope is preserved end to end and branched on by
/// `code`; account IPC errors are never matched with regular expressions (AGENTS.md).
pub fn from_enroll_error(origin: &str, error: &EnrollError) -> AccountCliError {
    explained_error(
        origin,
        error.status,
        &error.code,
        &error.message,
        error.details.as_ref(),
    )
}

fn explained_error(
    origin: &str,
    status: u16,
    code: &str,
    message: &str,
    details: Option<&serde_json::Value>,
) -> AccountCliError {
    let exit_code = if status == 402 { EXIT_PLAN_BLOCKED } else { 1 };
    let mut text = format!("{code}: {message}");
    match code {
        PLAN_LIMIT_REACHED => {
            if let Some(limit) = PlanLimitDetails::from_details(details) {
                text.push_str(&format!(
                    "\n  Plan {}: limit {}, used {}.",
                    limit.plan, limit.limit, limit.used
                ));
            }
            text.push_str(&format!("\n  Remove a computer or upgrade: {PRICING_URL}"));
        }
        REMOTE_SUSPENDED => {
            if let Some(suspension) = SuspensionDetails::from_details(details) {
                text.push_str(&format!(
                    "\n  Remote access is stopped for the {} plan (status {}).",
                    suspension.plan, suspension.status
                ));
                if let Some(grace_ends_at) = suspension.grace_ends_at {
                    text.push_str(&format!(
                        "\n  The 7-day grace period ended {}.",
                        render_unix(grace_ends_at)
                    ));
                }
                if let Some(stopped_at) = suspension.stopped_at {
                    text.push_str(&format!(" Remote stopped {}.", render_unix(stopped_at)));
                }
            }
            text.push_str(&format!("\n  Restore billing or upgrade: {PRICING_URL}"));
        }
        BILLING_UNCONFIGURED => {
            text.push_str(
                "\n  This account service runs in commercial mode without Lemon Squeezy \
                 configuration; the operator must set the FERRYX_LS_* variables \
                 (see docs/billing-operations.md).",
            );
        }
        _ => {
            if status == 404 {
                text.push_str(&format!(
                    "\n  {origin} does not serve billing; self-hosted relays have no plans."
                ));
            } else if status == 401 {
                text.push_str(
                    "\n  The account session was rejected; sign in again to mint a fresh session.",
                );
            }
        }
    }
    AccountCliError {
        message: text,
        exit_code,
    }
}

fn http_client() -> Result<reqwest::Client, AccountCliError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AccountCliError::local("ACCOUNT_CLIENT_FAILED", error.to_string()))
}

/// Reads the entitlement with the account session bearer the canonical route requires.
pub async fn fetch_entitlement(
    origin: &str,
    session_token: &str,
) -> Result<BillingEntitlement, AccountCliError> {
    let session_token = session_token.trim();
    if session_token.is_empty() {
        return Err(AccountCliError::local(
            ACCOUNT_SESSION_REQUIRED,
            "no account session bearer was supplied for the entitlement read",
        ));
    }
    let origin = origin.trim_end_matches('/');
    let url = format!("{origin}{ENTITLEMENT_PATH}");
    let response = http_client()?
        .get(&url)
        .bearer_auth(session_token)
        .send()
        .await
        .map_err(|error| AccountCliError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|error| AccountCliError::local("ACCOUNT_UNREACHABLE", error.to_string()))?;
    if status != 200 {
        return Err(billing_error(origin, status, &body));
    }
    parse_entitlement(&body)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::Router;

    use super::*;

    const SESSION: &str = "session-bearer-1";

    const CANONICAL_BODY: &str = r#"{
        "plan": "pro_monthly",
        "status": "over_limit",
        "machineLimit": 10,
        "machinesUsed": 12,
        "seats": null,
        "hostPacks": 0,
        "graceEndsAt": 1793491200,
        "orgId": null,
        "role": null,
        "manageUrl": "https://ferryx.lemonsqueezy.com/billing"
    }"#;

    const PLAN_LIMIT_BODY: &str = r#"{"code":"PLAN_LIMIT_REACHED","message":"the free plan allows one computer","details":{"plan":"free","limit":1,"used":2}}"#;

    const DOWNSTREAM_PATH: &str = "/api/account/v1/billing/entitlement/redirected";

    fn lookup(entries: Vec<(&'static str, &str)>) -> impl Fn(&str) -> Option<String> {
        let entries: Vec<(&'static str, String)> = entries
            .into_iter()
            .map(|(key, value)| (key, value.to_string()))
            .collect();
        move |key: &str| {
            entries
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn row_value(text: &str, label: &str) -> Option<String> {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(label)
                .map(|rest| rest.trim().to_string())
        })
    }

    fn json_response(status: u16, body: &str) -> Response {
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("stub response")
    }

    #[derive(Default)]
    struct StubLog {
        headers: Option<HeaderMap>,
        hits: Vec<String>,
    }

    fn hit(log: &Arc<Mutex<StubLog>>, path: &str, headers: Option<HeaderMap>) {
        let mut log = log.lock().expect("stub lock");
        log.hits.push(path.to_string());
        if let Some(headers) = headers {
            log.headers = Some(headers);
        }
    }

    fn recorded_paths(log: &Arc<Mutex<StubLog>>) -> Vec<String> {
        log.lock().expect("stub lock").hits.clone()
    }

    async fn stub_entitlement(
        State(log): State<Arc<Mutex<StubLog>>>,
        uri: axum::http::Uri,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        hit(&log, uri.path(), Some(headers));
        json_response(200, CANONICAL_BODY)
    }

    async fn stub_plan_limit(
        State(log): State<Arc<Mutex<StubLog>>>,
        uri: axum::http::Uri,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        hit(&log, uri.path(), Some(headers));
        json_response(402, PLAN_LIMIT_BODY)
    }

    async fn stub_redirect(
        State(log): State<Arc<Mutex<StubLog>>>,
        uri: axum::http::Uri,
    ) -> impl IntoResponse {
        hit(&log, uri.path(), None);
        Response::builder()
            .status(302)
            .header("location", DOWNSTREAM_PATH)
            .body(Body::empty())
            .expect("stub redirect")
    }

    async fn stub_downstream(
        State(log): State<Arc<Mutex<StubLog>>>,
        uri: axum::http::Uri,
    ) -> impl IntoResponse {
        hit(&log, uri.path(), None);
        json_response(200, CANONICAL_BODY)
    }

    fn entitlement_router(log: &Arc<Mutex<StubLog>>) -> Router {
        Router::new()
            .route(ENTITLEMENT_PATH, get(stub_entitlement))
            .with_state(Arc::clone(log))
    }

    fn plan_limit_router(log: &Arc<Mutex<StubLog>>) -> Router {
        Router::new()
            .route(ENTITLEMENT_PATH, get(stub_plan_limit))
            .with_state(Arc::clone(log))
    }

    fn redirect_router(log: &Arc<Mutex<StubLog>>) -> Router {
        Router::new()
            .route(ENTITLEMENT_PATH, get(stub_redirect))
            .route(DOWNSTREAM_PATH, get(stub_downstream))
            .with_state(Arc::clone(log))
    }

    /// Binds before returning, so the client never races the stub's readiness. `axum::serve` returns
    /// `Serve`, which implements `IntoFuture` rather than `Future`, so the task awaits it explicitly;
    /// a stub failure surfaces through `expect` instead of being dropped.
    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind stub");
        let origin = format!("http://{}", listener.local_addr().expect("stub address"));
        (
            origin,
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .await
                    .expect("test stub server failed");
            }),
        )
    }

    #[test]
    fn entitlement_parses_the_canonical_contract_shape() {
        let entitlement = parse_entitlement(CANONICAL_BODY).expect("canonical body");
        assert_eq!(entitlement.plan, PlanKey::ProMonthly);
        assert_eq!(entitlement.status, EntitlementStatus::OverLimit);
        assert_eq!(entitlement.machine_limit, 10);
        assert_eq!(entitlement.machines_used, 12);
        assert_eq!(entitlement.seats, None);
        assert_eq!(entitlement.host_packs, 0);
        assert_eq!(entitlement.grace_ends_at, Some(1_793_491_200));
        assert_eq!(entitlement.org_id, None);
        assert_eq!(entitlement.role, None);
        assert_eq!(
            entitlement.manage_url.as_deref(),
            Some("https://ferryx.lemonsqueezy.com/billing")
        );
    }

    #[test]
    fn plan_text_carries_the_contract_values() {
        let entitlement = parse_entitlement(CANONICAL_BODY).expect("canonical body");
        let text = render_plan_text(&entitlement);
        assert!(text.contains("pro_monthly"), "{text}");
        assert!(text.contains("over_limit"), "{text}");
        assert!(text.contains("2026-11-01T00:00:00Z"), "{text}");
        assert!(text.contains("https://ferryx.lemonsqueezy.com/billing"), "{text}");
        assert_eq!(row_value(&text, "Machines").as_deref(), Some("12 / 10"));
    }

    #[test]
    fn unix_timestamps_render_as_rfc3339_utc() {
        assert_eq!(render_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(render_unix(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(render_unix(1_793_491_200), "2026-11-01T00:00:00Z");
    }

    #[test]
    fn plan_text_omits_rows_whose_fields_are_null() {
        let body = r#"{"plan":"free","status":"ok","machineLimit":1,"machinesUsed":1,"seats":null,"hostPacks":0,"graceEndsAt":null,"orgId":null,"role":null,"manageUrl":null}"#;
        let entitlement = parse_entitlement(body).expect("free body");
        let text = render_plan_text(&entitlement);
        assert_eq!(row_value(&text, "Grace ends").as_deref(), Some("-"));
        assert_eq!(row_value(&text, "Machines").as_deref(), Some("1 / 1"));
        assert!(!text.contains("Organization"), "{text}");
        assert!(!text.contains("Seats"), "{text}");
    }

    #[test]
    fn team_org_fields_and_json_round_trip() {
        let body = r#"{"plan":"team_annual","status":"ok","machineLimit":40,"machinesUsed":3,"seats":3,"hostPacks":2,"graceEndsAt":null,"orgId":"org-1","role":"owner","manageUrl":null}"#;
        let entitlement = parse_entitlement(body).expect("team body");
        assert_eq!(entitlement.seats, Some(3));
        assert_eq!(entitlement.role, Some(OrgRole::Owner));
        assert_eq!(entitlement.host_packs, 2);
        assert_eq!(entitlement.org_id.as_deref(), Some("org-1"));

        let json = entitlement_json(&entitlement).expect("json");
        assert_eq!(parse_entitlement(&json).expect("reparse"), entitlement);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["plan"].as_str(), Some("team_annual"));
        assert_eq!(value["status"].as_str(), Some("ok"));
        assert_eq!(value["machineLimit"].as_u64(), Some(40));
        assert_eq!(value["machinesUsed"].as_u64(), Some(3));
        assert_eq!(value["hostPacks"].as_u64(), Some(2));
        assert!(value["graceEndsAt"].is_null());
        assert_eq!(value["orgId"].as_str(), Some("org-1"));
        assert_eq!(value["role"].as_str(), Some("owner"));
    }

    #[test]
    fn plan_limit_details_parse_from_the_402_envelope() {
        let body: serde_json::Value = serde_json::from_str(PLAN_LIMIT_BODY).expect("envelope");
        let details = PlanLimitDetails::from_details(body.get("details")).expect("details");
        assert_eq!(
            details,
            PlanLimitDetails {
                plan: "free".to_string(),
                limit: 1,
                used: 2,
            }
        );
    }

    #[test]
    fn suspension_details_parse_from_the_402_envelope() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"details":{"plan":"pro_monthly","status":"stopped","graceEndsAt":1793491200,"stoppedAt":1793491260}}"#,
        )
        .expect("envelope");
        let details = SuspensionDetails::from_details(body.get("details")).expect("details");
        assert_eq!(
            details,
            SuspensionDetails {
                plan: "pro_monthly".to_string(),
                status: "stopped".to_string(),
                grace_ends_at: Some(1_793_491_200),
                stopped_at: Some(1_793_491_260),
            }
        );
    }

    #[test]
    fn suspension_details_tolerate_missing_timestamps() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"details":{"plan":"pro_annual","status":"stopped","graceEndsAt":null,"stoppedAt":null}}"#,
        )
        .expect("envelope");
        let details = SuspensionDetails::from_details(body.get("details")).expect("details");
        assert_eq!(details.grace_ends_at, None);
        assert_eq!(details.stopped_at, None);
    }

    #[test]
    fn plan_blocked_responses_exit_three() {
        let limit = billing_error("https://account.example", 402, PLAN_LIMIT_BODY);
        assert_eq!(limit.exit_code, EXIT_PLAN_BLOCKED);
        assert!(limit.plan_blocked());
        assert!(limit.message.contains(PLAN_LIMIT_REACHED), "{}", limit.message);
        assert!(limit.message.contains(PRICING_URL), "{}", limit.message);

        let suspended = billing_error(
            "https://account.example",
            402,
            r#"{"code":"REMOTE_SUSPENDED","message":"remote access is stopped","details":{"plan":"pro_monthly","status":"stopped","graceEndsAt":1793491200,"stoppedAt":1793491260}}"#,
        );
        assert_eq!(suspended.exit_code, EXIT_PLAN_BLOCKED);
        assert!(suspended.message.contains(REMOTE_SUSPENDED), "{}", suspended.message);
        assert!(suspended.message.contains("2026-11-01T00:00:00Z"), "{}", suspended.message);
        assert!(suspended.message.contains(PRICING_URL), "{}", suspended.message);

        let bare = billing_error("https://account.example", 402, "");
        assert_eq!(bare.exit_code, EXIT_PLAN_BLOCKED);
        assert!(bare.message.contains("ACCOUNT_HTTP_402"), "{}", bare.message);
    }

    #[test]
    fn non_blocking_responses_exit_one_and_keep_the_server_code() {
        for (status, body, code) in [
            (500u16, r#"{"code":"INTERNAL_ERROR","message":"boom"}"#, "INTERNAL_ERROR"),
            (401, r#"{"code":"UNAUTHORIZED","message":"unknown or expired session"}"#, "UNAUTHORIZED"),
            (404, r#"{"code":"NOT_FOUND","message":"not found"}"#, "NOT_FOUND"),
            (503, r#"{"code":"BILLING_UNCONFIGURED","message":"billing is not configured"}"#, BILLING_UNCONFIGURED),
        ] {
            let error = billing_error("https://account.example", status, body);
            assert_eq!(error.exit_code, 1, "{code}");
            assert!(error.message.contains(code), "{}", error.message);
            assert!(!error.plan_blocked());
        }
    }

    #[test]
    fn enroll_errors_keep_status_and_details() {
        let limit = EnrollError {
            code: PLAN_LIMIT_REACHED.to_string(),
            message: "the free plan allows one computer".to_string(),
            status: 402,
            details: Some(serde_json::json!({"plan": "free", "limit": 1, "used": 2})),
        };
        let mapped = from_enroll_error("https://account.example", &limit);
        assert_eq!(mapped.exit_code, EXIT_PLAN_BLOCKED);
        assert_eq!(
            PlanLimitDetails::from_details(limit.details.as_ref()),
            Some(PlanLimitDetails {
                plan: "free".to_string(),
                limit: 1,
                used: 2,
            })
        );

        let rejected = EnrollError {
            code: "ACCOUNT_ENROLLMENT_INVALID".to_string(),
            message: "unknown enrollment code".to_string(),
            status: 400,
            details: None,
        };
        let mapped = from_enroll_error("https://account.example", &rejected);
        assert_eq!(mapped.exit_code, 1);
        assert!(mapped.message.contains("ACCOUNT_ENROLLMENT_INVALID"), "{}", mapped.message);
    }

    #[test]
    fn unparseable_entitlement_is_a_bad_response_failure() {
        let error = parse_entitlement("{\"plan\":\"platinum\"}").expect_err("unknown plan");
        assert_eq!(error.exit_code, 1);
        assert!(error.message.contains("ACCOUNT_BAD_RESPONSE"), "{}", error.message);
    }

    #[test]
    fn session_token_resolves_when_the_issuer_origin_matches() {
        let token = resolve_session_token_with(
            "https://account.example",
            lookup(vec![
                (SESSION_TOKEN_ENV, "session-bearer-1"),
                (SESSION_ORIGIN_ENV, "https://account.example/"),
            ]),
        )
        .expect("matching issuer");
        assert_eq!(token, "session-bearer-1");
    }

    #[test]
    fn session_token_requires_both_environment_inputs() {
        for entries in [
            vec![(SESSION_ORIGIN_ENV, "https://account.example")],
            vec![(SESSION_TOKEN_ENV, "session-bearer-1")],
            vec![
                (SESSION_TOKEN_ENV, "   "),
                (SESSION_ORIGIN_ENV, "https://account.example"),
            ],
            vec![
                (SESSION_TOKEN_ENV, "session-bearer-1"),
                (SESSION_ORIGIN_ENV, "not-an-origin"),
            ],
        ] {
            let error = resolve_session_token_with("https://account.example", lookup(entries))
                .expect_err("blocked");
            assert_eq!(error.exit_code, 1);
            assert!(
                error.message.contains(ACCOUNT_SESSION_REQUIRED),
                "{}",
                error.message
            );
            assert!(!error.message.contains("session-bearer-1"), "{}", error.message);
        }
    }

    #[test]
    fn session_token_is_never_sent_to_another_origin() {
        for (issuer, requested) in [
            ("https://account.example", "https://other.example"),
            ("http://127.0.0.1:43822", "http://127.0.0.1:43823"),
        ] {
            let error = resolve_session_token_with(
                requested,
                lookup(vec![
                    (SESSION_TOKEN_ENV, "session-bearer-1"),
                    (SESSION_ORIGIN_ENV, issuer),
                ]),
            )
            .expect_err("origin mismatch");
            assert!(
                error.message.contains(ACCOUNT_SESSION_REQUIRED),
                "{}",
                error.message
            );
            assert!(error.message.contains(issuer), "{}", error.message);
            assert!(error.message.contains(requested), "{}", error.message);
            assert!(!error.message.contains("session-bearer-1"), "{}", error.message);
        }
    }

    #[test]
    fn empty_session_token_is_a_typed_blocker() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let error = runtime
            .block_on(fetch_entitlement("https://account.example", "   "))
            .expect_err("no session");
        assert_eq!(error.exit_code, 1);
        assert!(
            error.message.contains(ACCOUNT_SESSION_REQUIRED),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn plan_request_sends_the_resolved_account_session_bearer_to_the_canonical_route() {
        let log = Arc::new(Mutex::new(StubLog::default()));
        let (origin, server) = serve(entitlement_router(&log)).await;
        let token = resolve_session_token_with(
            &origin,
            lookup(vec![
                (SESSION_TOKEN_ENV, SESSION),
                (SESSION_ORIGIN_ENV, origin.as_str()),
            ]),
        )
        .expect("session for the requested origin");
        let entitlement = fetch_entitlement(&origin, &token)
            .await
            .expect("entitlement response");
        assert_eq!(entitlement.plan, PlanKey::ProMonthly);
        assert_eq!(entitlement.machine_limit, 10);

        assert_eq!(recorded_paths(&log), vec![ENTITLEMENT_PATH.to_string()]);
        let authorization = log
            .lock()
            .expect("stub lock")
            .headers
            .clone()
            .expect("request seen")
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        assert_eq!(authorization, Some(format!("Bearer {}", SESSION)));
        server.abort();
    }

    #[tokio::test]
    async fn plan_request_maps_a_402_response_to_the_plan_blocked_exit() {
        let log = Arc::new(Mutex::new(StubLog::default()));
        let (origin, server) = serve(plan_limit_router(&log)).await;
        let error = fetch_entitlement(&origin, SESSION)
            .await
            .expect_err("402 response");
        assert_eq!(error.exit_code, EXIT_PLAN_BLOCKED);
        assert!(error.message.contains(PLAN_LIMIT_REACHED), "{}", error.message);
        assert_eq!(recorded_paths(&log), vec![ENTITLEMENT_PATH.to_string()]);
        server.abort();
    }

    #[tokio::test]
    async fn session_origin_mismatch_never_reaches_the_account_service() {
        let log = Arc::new(Mutex::new(StubLog::default()));
        let (origin, server) = serve(entitlement_router(&log)).await;
        let error = resolve_session_token_with(
            &origin,
            lookup(vec![
                (SESSION_TOKEN_ENV, SESSION),
                (SESSION_ORIGIN_ENV, "https://other.example"),
            ]),
        )
        .expect_err("origin mismatch");
        assert!(
            error.message.contains(ACCOUNT_SESSION_REQUIRED),
            "{}",
            error.message
        );
        assert!(!error.message.contains(SESSION), "{}", error.message);
        assert!(
            recorded_paths(&log).is_empty(),
            "a mismatched session must never reach the account service"
        );
        server.abort();
    }

    #[tokio::test]
    async fn plan_request_does_not_follow_redirects_with_the_session_bearer() {
        let log = Arc::new(Mutex::new(StubLog::default()));
        let (origin, server) = serve(redirect_router(&log)).await;
        let error = fetch_entitlement(&origin, SESSION)
            .await
            .expect_err("a redirect is not followed");
        assert_eq!(error.exit_code, 1);
        assert!(error.message.contains("ACCOUNT_HTTP_302"), "{}", error.message);
        assert!(!error.message.contains(SESSION), "{}", error.message);
        assert_eq!(recorded_paths(&log), vec![ENTITLEMENT_PATH.to_string()]);
        server.abort();
    }
}
