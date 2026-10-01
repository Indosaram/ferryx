//! Lemon Squeezy adapter: webhook signature verification, event parsing, checkout creation and
//! subscription-quantity updates.
//!
//! Documented sources (fetched 2026-09-30):
//! - signature header `X-Signature` over the raw body:
//!   <https://docs.lemonsqueezy.com/guides/developer-guide/webhooks>
//! - subscription object, `first_subscription_item`, signed 24h `urls.customer_portal`:
//!   <https://docs.lemonsqueezy.com/api/subscriptions/the-subscription-object>
//! - quantity updates target the subscription *item* id:
//!   <https://docs.lemonsqueezy.com/api/subscription-items/update-subscription-item>
//! - checkout creation payload, including the documented `attributes.test_mode` flag (a sandbox-configured
//!   adapter must never open a live checkout) and `attributes.product_options.enabled_variants` (only the
//!   variant this checkout was built for may be purchasable):
//!   <https://docs.lemonsqueezy.com/api/checkouts/create-checkout>
//! - signed customer-portal URLs expire after 24h and are refreshed by re-requesting the object:
//!   <https://docs.lemonsqueezy.com/guides/developer-guide/customer-portal>
//! - `meta.custom_data` is echoed for Order/Subscription/License-key webhooks only, so invoice
//!   webhooks must not be required to carry it:
//!   <https://docs.lemonsqueezy.com/help/checkout/passing-custom-data>

use std::time::Duration;

use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::entitlement::{
    PlanKey, SubscriptionStatus, MAX_TRUSTED_HOST_PACKS, MAX_TRUSTED_SEATS, TEAM_MIN_SEATS,
};

type HmacSha256 = Hmac<Sha256>;

pub const LEMONSQUEEZY_API_BASE: &str = "https://api.lemonsqueezy.com/v1";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Wire-quantity ceiling for anything we send to or accept from Lemon Squeezy, derived from the
/// entitlement trust bounds: the Pro axis encodes packs as `1 + packs` (unit 1 is the base plan),
/// so the ceiling is the pack bound plus one.
pub const MAX_QUANTITY: u32 = MAX_TRUSTED_HOST_PACKS + 1;

const _: () = assert!(MAX_QUANTITY <= MAX_TRUSTED_SEATS + 1);

/// Longest API error detail echoed into an error message.
const MAX_ERROR_DETAIL_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanVariantKind {
    Base,
    Hostpack,
}

/// Wrapper that keeps secrets out of `Debug` output while still showing whether they are set.
struct Redacted<'a>(&'a str);

impl std::fmt::Debug for Redacted<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.trim().is_empty() {
            f.write_str("<empty>")
        } else {
            f.write_str("<redacted>")
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LemonSqueezyConfig {
    pub api_key: String,
    pub store_id: String,
    pub webhook_secret: String,
    pub variant_pro_monthly: String,
    pub variant_pro_annual: String,
    pub variant_team_monthly: String,
    pub variant_team_annual: String,
    pub variant_team_hostpack_annual: String,
    pub api_base_url: Option<String>,
    /// Expected store mode: `false` for a production store, `true` for a Lemon Squeezy test-mode
    /// store. Every webhook event and API read must match it exactly, so sandbox purchases can
    /// never grant production entitlements (or the reverse).
    pub expect_test_mode: bool,
}

/// Credentials are never rendered: `Debug` shows whether the API key and webhook secret are set,
/// never their value.
impl std::fmt::Debug for LemonSqueezyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LemonSqueezyConfig")
            .field("api_key", &Redacted(&self.api_key))
            .field("store_id", &self.store_id)
            .field("webhook_secret", &Redacted(&self.webhook_secret))
            .field("variant_pro_monthly", &self.variant_pro_monthly)
            .field("variant_pro_annual", &self.variant_pro_annual)
            .field("variant_team_monthly", &self.variant_team_monthly)
            .field("variant_team_annual", &self.variant_team_annual)
            .field(
                "variant_team_hostpack_annual",
                &self.variant_team_hostpack_annual,
            )
            .field("api_base_url", &self.api_base_url)
            .field("expect_test_mode", &self.expect_test_mode)
            .finish()
    }
}

impl LemonSqueezyConfig {
    pub fn api_base(&self) -> &str {
        self.api_base_url
            .as_deref()
            .unwrap_or(LEMONSQUEEZY_API_BASE)
    }

    /// Base URL HTTP calls may use. The default is the public HTTPS API; an explicitly injected
    /// base is honored only when it is an HTTPS Lemon Squeezy host or a loopback HTTP test
    /// endpoint. Every other value is rejected with [`LemonSqueezyError::UntrustedUrl`].
    pub fn resolve_api_base(&self) -> Result<&str, LemonSqueezyError> {
        let base = self.api_base().trim();
        if base.is_empty() {
            return Err(LemonSqueezyError::MissingConfiguration(
                "api base URL".to_string(),
            ));
        }
        if is_trusted_ls_url(base) {
            return Ok(base);
        }
        if self.api_base_url.is_some() && is_loopback_http_url(base) {
            return Ok(base);
        }
        Err(LemonSqueezyError::UntrustedUrl(base.to_string()))
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
            && !self.store_id.trim().is_empty()
            && !self.webhook_secret.trim().is_empty()
            && !self.variant_pro_monthly.trim().is_empty()
            && !self.variant_pro_annual.trim().is_empty()
            && !self.variant_team_monthly.trim().is_empty()
            && !self.variant_team_annual.trim().is_empty()
            && !self.variant_team_hostpack_annual.trim().is_empty()
    }

    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("FERRYX_LS_API_KEY").unwrap_or_default(),
            store_id: std::env::var("FERRYX_LS_STORE_ID").unwrap_or_default(),
            webhook_secret: std::env::var("FERRYX_LS_WEBHOOK_SECRET").unwrap_or_default(),
            variant_pro_monthly: std::env::var("FERRYX_LS_VARIANT_PRO_MONTHLY").unwrap_or_default(),
            variant_pro_annual: std::env::var("FERRYX_LS_VARIANT_PRO_ANNUAL").unwrap_or_default(),
            variant_team_monthly: std::env::var("FERRYX_LS_VARIANT_TEAM_MONTHLY")
                .unwrap_or_default(),
            variant_team_annual: std::env::var("FERRYX_LS_VARIANT_TEAM_ANNUAL").unwrap_or_default(),
            variant_team_hostpack_annual: std::env::var("FERRYX_LS_VARIANT_TEAM_HOSTPACK_ANNUAL")
                .unwrap_or_default(),
            api_base_url: std::env::var("FERRYX_LS_API_BASE_URL").ok(),
            expect_test_mode: parse_env_flag(std::env::var("FERRYX_LS_TEST_MODE").ok().as_deref()),
        }
    }
}

/// `FERRYX_LS_TEST_MODE` accepts `1`/`true`/`yes`/`on` (case-insensitive) for a test-mode store and
/// `0`/`false`/`no`/`off` or an absent value for a production store. An unrecognized value stays
/// production, so a typo can never silently accept test-mode traffic in production.
pub fn parse_env_flag(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

#[derive(Debug, thiserror::Error)]
pub enum LemonSqueezyError {
    #[error("Missing or invalid signature header: {0}")]
    InvalidSignatureHeader(String),
    #[error("Signature verification failed")]
    SignatureVerificationFailed,
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Untrusted API URL: {0}")]
    UntrustedUrl(String),
    #[error("API request failed with status {status}: {message}")]
    ApiFailure { status: u16, message: String },
    #[error("Invalid event structure: {0}")]
    InvalidEvent(String),
    #[error("{entity} mismatch: requested {requested}, response carries {received}")]
    ResourceMismatch {
        entity: &'static str,
        requested: String,
        received: String,
    },
    #[error("Subscription item not found for subscription {0}")]
    SubscriptionItemNotFound(String),
    #[error("Invalid quantity: {0}")]
    InvalidQuantity(String),
    #[error("Unsupported plan variant: {0}")]
    UnsupportedVariant(String),
    #[error("Event does not belong to the configured store: {0}")]
    UnsupportedStore(String),
    #[error("Invalid identifier: {0}")]
    InvalidIdentifier(String),
    #[error("Webhook event carries no owner (meta.custom_data.user_id/org_id): {0}")]
    MissingOwnerData(String),
    #[error("Customer portal URL unavailable for subscription {0}")]
    PortalUrlUnavailable(String),
    #[error("Lemon Squeezy configuration is incomplete: {0}")]
    MissingConfiguration(String),
    #[error("Test mode mismatch: configured {expected}, received {actual} ({context})")]
    TestModeMismatch {
        expected: bool,
        actual: bool,
        context: String,
    },
}

/// Which Lemon Squeezy object the webhook `data` member carried.
///
/// An invoice payload describes a payment, not the subscription: it carries no variant, quantity
/// or status. Callers must resolve those through
/// [`LemonSqueezyClient::fetch_authoritative_subscription`] instead of trusting the invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LsPayloadKind {
    Subscription,
    Invoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LsEvent {
    pub event_name: String,
    pub payload_kind: LsPayloadKind,
    pub subscription_id: String,
    /// Set only for [`LsPayloadKind::Invoice`]; the invoice id is never a subscription id.
    pub invoice_id: Option<String>,
    pub store_id: String,
    pub customer_id: String,
    /// Empty for invoice payloads; read the variant back from the authoritative subscription.
    pub variant_id: String,
    /// `None` for invoice payloads, which do not carry the subscription status.
    pub status: Option<SubscriptionStatus>,
    /// `0` means "not stated by this payload" (invoice payloads).
    pub quantity: u32,
    pub ends_at: Option<u64>,
    pub updated_at: u64,
    pub custom_user_id: Option<String>,
    pub custom_org_id: Option<String>,
    pub manage_url: Option<String>,
    pub test_mode: bool,
}

impl LsEvent {
    /// Owner key the checkout `custom_data` points at: `org:<id>` when an org is present, else the
    /// user id. `None` for events that carry no custom data (invoice webhooks).
    pub fn custom_owner_key(&self) -> Option<String> {
        match (
            self.custom_org_id.as_deref(),
            self.custom_user_id.as_deref(),
        ) {
            (Some(org), _) if !org.trim().is_empty() => Some(format!("org:{}", org.trim())),
            (_, Some(user)) if !user.trim().is_empty() => Some(user.trim().to_string()),
            (Some(_), None) | (None, None) | (None, Some(_)) | (Some(_), Some(_)) => None,
        }
    }

    /// Validates a parsed event against the store configuration: identifiers are numeric, the
    /// event belongs to this store, the variant is one we sell, and inventory quantities stay
    /// inside [`MAX_QUANTITY`].
    ///
    /// Invoice payloads skip the variant/quantity rules by design - they carry neither, so the
    /// authoritative subscription fetched through
    /// [`LemonSqueezyClient::fetch_authoritative_subscription`] is what gets validated instead.
    pub fn validate(&self, config: &LemonSqueezyConfig) -> Result<(), LemonSqueezyError> {
        require_numeric_id("subscription_id", &self.subscription_id)?;
        require_numeric_id("store_id", &self.store_id)?;

        let configured_store = config.store_id.trim();
        if !configured_store.is_empty() && configured_store != self.store_id.trim() {
            return Err(LemonSqueezyError::UnsupportedStore(self.store_id.clone()));
        }

        if self.test_mode != config.expect_test_mode {
            return Err(LemonSqueezyError::TestModeMismatch {
                expected: config.expect_test_mode,
                actual: self.test_mode,
                context: format!("event {}", self.event_name),
            });
        }

        match self.payload_kind {
            LsPayloadKind::Subscription => {
                require_numeric_id("variant_id", &self.variant_id)?;
                if variant_to_plan(&self.variant_id, config).is_none() {
                    return Err(LemonSqueezyError::UnsupportedVariant(
                        self.variant_id.clone(),
                    ));
                }
                check_quantity(self.quantity)?;
            }
            LsPayloadKind::Invoice => {
                let invoice_id = self.invoice_id.as_deref().ok_or_else(|| {
                    LemonSqueezyError::InvalidEvent("missing invoice id".to_string())
                })?;
                require_numeric_id("invoice_id", invoice_id)?;
            }
        }
        Ok(())
    }
}

pub fn verify_signature(secret: &str, raw_body: &[u8], signature_header: &str) -> bool {
    // An empty or blank signing secret can never authenticate a request.
    if secret.trim().is_empty() {
        return false;
    }

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(raw_body);

    let hex_sig = signature_header.trim();
    let expected_bytes = match decode_hex(hex_sig) {
        Some(b) => b,
        None => return false,
    };

    mac.verify_slice(&expected_bytes).is_ok()
}

fn decode_hex(hex_str: &str) -> Option<Vec<u8>> {
    if hex_str.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    for chunk in hex_str.as_bytes().chunks_exact(2) {
        let high = char_to_nibble(chunk[0])?;
        let low = char_to_nibble(chunk[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX_CHARS[(b >> 4) as usize] as char);
        s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    s
}

fn char_to_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn event_key(raw_body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_body);
    let result = hasher.finalize();
    encode_hex(&result)
}

pub fn variant_to_plan(
    variant_id: &str,
    config: &LemonSqueezyConfig,
) -> Option<(PlanKey, PlanVariantKind)> {
    let vid = variant_id.trim();
    if vid.is_empty() {
        return None;
    }

    if vid == config.variant_pro_monthly.trim() {
        Some((PlanKey::ProMonthly, PlanVariantKind::Base))
    } else if vid == config.variant_pro_annual.trim() {
        Some((PlanKey::ProAnnual, PlanVariantKind::Base))
    } else if vid == config.variant_team_monthly.trim() {
        Some((PlanKey::TeamMonthly, PlanVariantKind::Base))
    } else if vid == config.variant_team_annual.trim() {
        Some((PlanKey::TeamAnnual, PlanVariantKind::Base))
    } else if vid == config.variant_team_hostpack_annual.trim() {
        Some((PlanKey::TeamAnnual, PlanVariantKind::Hostpack))
    } else {
        None
    }
}

/// Parses the RFC 3339 timestamps Lemon Squeezy documents (`2026-09-02T03:04:05.000000Z`) into
/// whole unix seconds, or `None` when the value is not a timestamp we accept.
///
/// The instant math comes from `time`'s RFC 3339 parser; `time`'s `parsing` feature is already
/// enabled transitively (`cookie` requires `time` with `parsing`), so no dependency change is
/// involved. A short lexical gate in front of it keeps the accept/reject boundary exactly as Lemon
/// Squeezy documents it, because RFC 3339 parsing is deliberately looser than the documented shape:
/// it accepts any separator byte (including a space), hour `24`, and leap seconds.
/// <https://docs.lemonsqueezy.com/api/subscriptions/the-subscription-object>
pub fn parse_timestamp(iso_str: &str) -> Option<u64> {
    if !has_documented_timestamp_shape(iso_str) {
        return None;
    }
    let parsed =
        time::OffsetDateTime::parse(iso_str, &time::format_description::well_known::Rfc3339)
            .ok()?;
    u64::try_from(parsed.unix_timestamp()).ok()
}

/// `YYYY-MM-DDThh:mm:ss` with hour <= 23 and second <= 59, then an optional `.fraction`
/// (1-9 digits) and either `Z`/`z` or a `+hh:mm`/`-hh:mm` offset. Month, day and offset ranges are
/// left to `time`'s parser.
fn has_documented_timestamp_shape(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }

    let prefix_ok = bytes[..19]
        .iter()
        .enumerate()
        .all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => matches!(*byte, b'T' | b't'),
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        });
    if !prefix_ok || two_digit_number(bytes, 11) > 23 || two_digit_number(bytes, 17) > 59 {
        return false;
    }

    let mut tail = &value[19..];
    if let Some(rest) = tail.strip_prefix('.') {
        let digits = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 || digits > 9 {
            return false;
        }
        tail = &rest[digits..];
    }

    match tail {
        "Z" | "z" => true,
        offset => {
            let offset = offset.as_bytes();
            offset.len() == 6
                && matches!(offset[0], b'+' | b'-')
                && offset[1].is_ascii_digit()
                && offset[2].is_ascii_digit()
                && offset[3] == b':'
                && offset[4].is_ascii_digit()
                && offset[5].is_ascii_digit()
        }
    }
}

/// Two ASCII digits already validated by [`has_documented_timestamp_shape`].
fn two_digit_number(bytes: &[u8], start: usize) -> u32 {
    u32::from(bytes[start] - b'0') * 10 + u32::from(bytes[start + 1] - b'0')
}

/// Reads a timestamp attribute that Lemon Squeezy may send as `null`. An absent or `null` value
/// means "not stated"; a present value must be a valid RFC 3339 string, anything else is a typed
/// error instead of a silent `None`.
fn optional_timestamp_field(
    attributes: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<u64>, LemonSqueezyError> {
    match attributes.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(raw)) => parse_timestamp(raw).map(Some).ok_or_else(|| {
            LemonSqueezyError::InvalidEvent(format!(
                "{key} is not a valid RFC 3339 timestamp: {raw:?}"
            ))
        }),
        Some(other) => Err(LemonSqueezyError::InvalidEvent(format!(
            "{key} must be a timestamp string or null, got {other}"
        ))),
    }
}

/// Reads a timestamp attribute Lemon Squeezy always sends (`updated_at`); missing or invalid is a
/// typed error, never a silent `0`.
fn required_timestamp_field(
    attributes: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<u64, LemonSqueezyError> {
    optional_timestamp_field(attributes, key)?
        .ok_or_else(|| LemonSqueezyError::InvalidEvent(format!("{key} is missing")))
}

pub fn parse_subscription_status(status_str: &str) -> Option<SubscriptionStatus> {
    match status_str.to_ascii_lowercase().as_str() {
        "active" => Some(SubscriptionStatus::Active),
        "on_trial" => Some(SubscriptionStatus::OnTrial),
        "past_due" => Some(SubscriptionStatus::PastDue),
        "unpaid" => Some(SubscriptionStatus::Unpaid),
        "cancelled" => Some(SubscriptionStatus::Cancelled),
        "expired" => Some(SubscriptionStatus::Expired),
        "paused" => Some(SubscriptionStatus::Paused),
        _ => None,
    }
}

/// `true` for HTTPS URLs on `lemonsqueezy.com` or any of its subdomains. This is the only URL
/// shape production calls are allowed to use.
pub fn is_trusted_ls_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    match parsed.host_str() {
        Some(host) => host == "lemonsqueezy.com" || host.ends_with(".lemonsqueezy.com"),
        None => false,
    }
}

/// `true` for plain-HTTP loopback URLs. Only ever accepted as the explicitly injected test base
/// (`FERRYX_LS_API_BASE_URL`), never as a value derived from a webhook or an API response.
pub fn is_loopback_http_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "http" {
        return false;
    }
    matches!(
        parsed.host_str(),
        Some("127.0.0.1") | Some("localhost") | Some("::1") | Some("[::1]")
    )
}

/// `true` for plain decimal identifiers; Lemon Squeezy resource ids are integers. Rejecting
/// anything else keeps a webhook-supplied id from reshaping a request path.
pub fn is_numeric_id(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.len() <= 20 && trimmed.bytes().all(|b| b.is_ascii_digit())
}

fn require_numeric_id(label: &str, value: &str) -> Result<(), LemonSqueezyError> {
    if is_numeric_id(value) {
        Ok(())
    } else {
        Err(LemonSqueezyError::InvalidIdentifier(format!(
            "{label}={value:?}"
        )))
    }
}

fn check_quantity(quantity: u32) -> Result<(), LemonSqueezyError> {
    if quantity == 0 || quantity > MAX_QUANTITY {
        return Err(LemonSqueezyError::InvalidQuantity(format!(
            "quantity must be within 1..={MAX_QUANTITY}, got {quantity}"
        )));
    }
    Ok(())
}

/// Reads a JSON quantity as a checked `u32` inside [`MAX_QUANTITY`]. A wider value is a typed error,
/// never a truncated number.
fn parse_quantity_amount(value: &serde_json::Value, label: &str) -> Result<u32, LemonSqueezyError> {
    let raw = value.as_u64().ok_or_else(|| {
        LemonSqueezyError::InvalidQuantity(format!(
            "{label} is not a non-negative integer: {value}"
        ))
    })?;
    let quantity = u32::try_from(raw).map_err(|_| {
        LemonSqueezyError::InvalidQuantity(format!("{label} does not fit in 32 bits: {raw}"))
    })?;
    check_quantity(quantity)?;
    Ok(quantity)
}

fn check_axis_amount(label: &str, amount: u32, ceiling: u32) -> Result<(), LemonSqueezyError> {
    if amount > ceiling {
        return Err(LemonSqueezyError::InvalidQuantity(format!(
            "{label} exceeds the trusted maximum {ceiling}: {amount}"
        )));
    }
    Ok(())
}

/// Pro variants use graduated pricing: unit 1 is the base plan and each extra unit is a 5-host
/// pack, so the subscription quantity is `1 + packs`.
/// <https://docs.lemonsqueezy.com/help/products/pricing-models>
pub fn pro_subscription_quantity(packs: u32) -> Result<u32, LemonSqueezyError> {
    check_axis_amount("hostPacks", packs, MAX_TRUSTED_HOST_PACKS)?;
    let quantity = packs + 1;
    check_quantity(quantity)?;
    Ok(quantity)
}

/// Team variants use standard pricing: the quantity *is* the seat count, floored at
/// [`super::entitlement::TEAM_MIN_SEATS`] and capped at [`MAX_TRUSTED_SEATS`].
pub fn team_subscription_quantity(seats: u32) -> Result<u32, LemonSqueezyError> {
    let seats = seats.max(TEAM_MIN_SEATS);
    check_axis_amount("seats", seats, MAX_TRUSTED_SEATS)?;
    check_quantity(seats)?;
    Ok(seats)
}

/// The Team host-pack annual variant is a quantity-only product: quantity is the pack count.
pub fn team_hostpack_quantity(packs: u32) -> Result<u32, LemonSqueezyError> {
    let packs = packs.max(1);
    check_axis_amount("hostPacks", packs, MAX_TRUSTED_HOST_PACKS)?;
    check_quantity(packs)?;
    Ok(packs)
}

/// Builds an `ApiFailure` without ever echoing the raw response body: only the documented JSON:API
/// `errors[].detail` / `errors[].title` strings are surfaced, control characters stripped and the
/// result length-capped. Any other shape yields a status-only message, so store data, signed URLs
/// or credentials inside an unexpected body can never reach logs.
fn api_failure(status: u16, body: &str) -> LemonSqueezyError {
    LemonSqueezyError::ApiFailure {
        status,
        message: safe_error_detail(body).unwrap_or_else(|| "<no detail>".to_string()),
    }
}

fn safe_error_detail(body: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(body).ok()?;
    for error in root.get("errors")?.as_array()? {
        for key in ["detail", "title"] {
            let Some(text) = error.get(key).and_then(|value| value.as_str()) else {
                continue;
            };
            let cleaned: String = text
                .chars()
                .filter(|character| !character.is_control())
                .take(MAX_ERROR_DETAIL_CHARS)
                .collect();
            let cleaned = cleaned.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// Events that carry state we track. Order, customer and license-key events are not part of the
/// billing contract, so they are reported as `Ok(None)` instead of an error.
const SUPPORTED_EVENTS: [&str; 11] = [
    "subscription_created",
    "subscription_updated",
    "subscription_cancelled",
    "subscription_resumed",
    "subscription_expired",
    "subscription_paused",
    "subscription_unpaused",
    "subscription_payment_success",
    "subscription_payment_failed",
    "subscription_payment_recovered",
    "subscription_payment_refunded",
];

/// JSON:API `id` values arrive as strings in webhook payloads and as numbers in API responses.
fn json_id(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        return if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    value.as_u64().map(|n| n.to_string())
}

fn required_id(
    label: &str,
    value: Option<&serde_json::Value>,
) -> Result<String, LemonSqueezyError> {
    json_id(value).ok_or_else(|| LemonSqueezyError::InvalidEvent(format!("missing {label}")))
}

fn custom_data_id(meta: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    meta.get("custom_data")
        .and_then(|value| value.as_object())
        .and_then(|custom| json_id(custom.get(key)))
}

/// `data.type` of a webhook body to the object it carries. Both documented invoice spellings are
/// accepted; anything else is not a payload this adapter understands.
fn payload_kind_for(data_type: &str) -> Option<LsPayloadKind> {
    match data_type {
        "subscriptions" => Some(LsPayloadKind::Subscription),
        "subscription-invoices" | "subscription-invoice" => Some(LsPayloadKind::Invoice),
        _ => None,
    }
}

/// `test_mode` is documented as a boolean and defaults to `false` when absent. A non-boolean
/// value means the payload is not shaped the way Lemon Squeezy documents it.
fn parse_test_mode(
    attributes: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool, LemonSqueezyError> {
    match attributes.get("test_mode") {
        None => Ok(false),
        Some(serde_json::Value::Bool(flag)) => Ok(*flag),
        Some(other) => Err(LemonSqueezyError::InvalidEvent(format!(
            "test_mode must be a boolean, got {other}"
        ))),
    }
}

/// Parses a signature-verified Lemon Squeezy webhook body.
///
/// * `Ok(Some(event))` - a tracked subscription or subscription-invoice event.
/// * `Ok(None)` - an event type or payload type this adapter does not track.
/// * `Err(_)` - a typed error; a subscription payload without checkout `custom_data` cannot be
///   attributed to an owner and yields [`LemonSqueezyError::MissingOwnerData`]. Invoice payloads
///   are exempt: Lemon Squeezy only echoes `custom_data` for Order/Subscription/License-key
///   webhooks, so their owner is resolved from the stored subscription record instead.
pub fn parse_event(raw: &[u8]) -> Result<Option<LsEvent>, LemonSqueezyError> {
    let root: serde_json::Value = serde_json::from_slice(raw)?;

    let meta = root
        .get("meta")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing meta object".to_string()))?;

    let event_name = meta
        .get("event_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing event_name in meta".to_string()))?;

    if !SUPPORTED_EVENTS.contains(&event_name) {
        return Ok(None);
    }

    let data = root
        .get("data")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing data object".to_string()))?;

    let data_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let Some(payload_kind) = payload_kind_for(data_type) else {
        return Ok(None);
    };

    let attributes = data
        .get("attributes")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing attributes in data".to_string()))?;

    let store_id = required_id("store_id", attributes.get("store_id"))?;
    let customer_id = json_id(attributes.get("customer_id")).unwrap_or_default();
    let custom_user_id = custom_data_id(meta, "user_id");
    let custom_org_id = custom_data_id(meta, "org_id");
    let test_mode = parse_test_mode(attributes)?;

    let updated_at = required_timestamp_field(attributes, "updated_at")?;

    match payload_kind {
        LsPayloadKind::Subscription => {
            let subscription_id = required_id("subscription id", data.get("id"))?;

            let variant_id = required_id("variant_id", attributes.get("variant_id"))?;

            let status_raw = attributes
                .get("status")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing status".to_string()))?;
            let status = parse_subscription_status(status_raw).ok_or_else(|| {
                LemonSqueezyError::InvalidEvent(format!(
                    "unknown subscription status: {status_raw}"
                ))
            })?;

            let quantity = match attributes
                .get("first_subscription_item")
                .and_then(|v| v.as_object())
                .and_then(|item| item.get("quantity"))
            {
                None => 1,
                Some(value) => parse_quantity_amount(value, "first_subscription_item.quantity")?,
            };

            if custom_user_id.is_none() && custom_org_id.is_none() {
                return Err(LemonSqueezyError::MissingOwnerData(event_name.to_string()));
            }

            let ends_at = optional_timestamp_field(attributes, "ends_at")?;

            let manage_url = attributes
                .get("urls")
                .and_then(|v| v.as_object())
                .and_then(|urls| urls.get("customer_portal"))
                .and_then(|v| v.as_str())
                .map(str::to_string);

            Ok(Some(LsEvent {
                event_name: event_name.to_string(),
                payload_kind,
                subscription_id,
                invoice_id: None,
                store_id,
                customer_id,
                variant_id,
                status: Some(status),
                quantity,
                ends_at,
                updated_at,
                custom_user_id,
                custom_org_id,
                manage_url,
                test_mode,
            }))
        }
        LsPayloadKind::Invoice => {
            // The invoice's own `data.id` is the invoice id. The subscription id only ever comes
            // from `attributes.subscription_id`; variant, quantity and status stay unset here and
            // are read back from the authoritative subscription object.
            let invoice_id = required_id("invoice id", data.get("id"))?;
            let subscription_id = required_id(
                "subscription_id in subscription-invoice",
                attributes.get("subscription_id"),
            )?;

            Ok(Some(LsEvent {
                event_name: event_name.to_string(),
                payload_kind,
                subscription_id,
                invoice_id: Some(invoice_id),
                store_id,
                customer_id,
                variant_id: String::new(),
                status: None,
                quantity: 0,
                ends_at: None,
                updated_at,
                custom_user_id,
                custom_org_id,
                manage_url: None,
                test_mode,
            }))
        }
    }
}

fn required_item_id(
    first_item: &serde_json::Map<String, serde_json::Value>,
    subscription_id: &str,
) -> Result<String, LemonSqueezyError> {
    json_id(first_item.get("id"))
        .ok_or_else(|| LemonSqueezyError::SubscriptionItemNotFound(subscription_id.to_string()))
}

pub struct LemonSqueezyClient {
    client: reqwest::Client,
    config: LemonSqueezyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionDetails {
    pub subscription_id: String,
    /// `first_subscription_item.id` - the id a quantity update must PATCH.
    pub item_id: String,
    pub store_id: String,
    pub customer_id: String,
    pub variant_id: String,
    pub status: SubscriptionStatus,
    pub quantity: u32,
    pub ends_at: Option<u64>,
    pub updated_at: u64,
    pub customer_portal_url: Option<String>,
    pub test_mode: bool,
}

impl SubscriptionDetails {
    /// Validates the authoritative subscription against the store configuration, exactly like
    /// [`LsEvent::validate`] does for webhook payloads.
    pub fn validate(&self, config: &LemonSqueezyConfig) -> Result<(), LemonSqueezyError> {
        require_numeric_id("subscription_id", &self.subscription_id)?;
        require_numeric_id("item_id", &self.item_id)?;
        require_numeric_id("store_id", &self.store_id)?;

        let configured_store = config.store_id.trim();
        if !configured_store.is_empty() && configured_store != self.store_id.trim() {
            return Err(LemonSqueezyError::UnsupportedStore(self.store_id.clone()));
        }

        if self.test_mode != config.expect_test_mode {
            return Err(LemonSqueezyError::TestModeMismatch {
                expected: config.expect_test_mode,
                actual: self.test_mode,
                context: format!("subscription {}", self.subscription_id),
            });
        }

        require_numeric_id("variant_id", &self.variant_id)?;
        if variant_to_plan(&self.variant_id, config).is_none() {
            return Err(LemonSqueezyError::UnsupportedVariant(
                self.variant_id.clone(),
            ));
        }
        check_quantity(self.quantity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutPlanChoice {
    ProMonthly { packs: u32 },
    ProAnnual { packs: u32 },
    TeamMonthly { seats: u32 },
    TeamAnnual { seats: u32 },
    TeamHostpackAnnual { packs: u32 },
}

impl LemonSqueezyClient {
    pub fn new(config: LemonSqueezyConfig) -> Result<Self, LemonSqueezyError> {
        config.resolve_api_base()?;

        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.api+json"));
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/vnd.api+json"),
        );

        let client = reqwest::Client::builder()
            // The trusted-URL check only covers the URL we build; a 3xx must never carry our
            // authorization header to another host, so redirects are not followed at all and the
            // redirect status surfaces as a typed API failure.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .timeout(DEFAULT_TIMEOUT)
            .default_headers(headers)
            .build()?;

        Ok(Self { client, config })
    }

    pub fn config(&self) -> &LemonSqueezyConfig {
        &self.config
    }

    /// Fetches one subscription. This is the authoritative read: an invoice webhook never carries
    /// the variant, quantity or status, so those are always taken from here.
    pub async fn get_subscription(
        &self,
        subscription_id: &str,
    ) -> Result<SubscriptionDetails, LemonSqueezyError> {
        require_numeric_id("subscription_id", subscription_id)?;
        let sub_id = subscription_id.trim();

        let url = format!(
            "{}/subscriptions/{}",
            self.config.resolve_api_base()?,
            sub_id
        );
        if !is_trusted_ls_url(&url) && !is_loopback_http_url(&url) {
            return Err(LemonSqueezyError::UntrustedUrl(url));
        }

        let resp = self
            .client
            .get(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.config.api_key.trim()),
            )
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(api_failure(status.as_u16(), &body));
        }

        let root: serde_json::Value = serde_json::from_str(&body)?;
        let data = root
            .get("data")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                LemonSqueezyError::InvalidEvent("missing data in subscription response".to_string())
            })?;

        let id = required_id("subscription id", data.get("id"))?;
        require_numeric_id("subscription_id", &id)?;
        if id != sub_id {
            return Err(LemonSqueezyError::ResourceMismatch {
                entity: "subscription",
                requested: sub_id.to_string(),
                received: id,
            });
        }

        let attributes = data
            .get("attributes")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                LemonSqueezyError::InvalidEvent(
                    "missing attributes in subscription response".to_string(),
                )
            })?;

        let first_item = attributes
            .get("first_subscription_item")
            .and_then(|v| v.as_object())
            .ok_or_else(|| LemonSqueezyError::SubscriptionItemNotFound(id.clone()))?;

        // Only the item id can be PATCHed; a subscription id would hit the wrong endpoint.
        let item_id = required_item_id(first_item, &id)?;

        let quantity = match first_item.get("quantity") {
            None => 1,
            Some(value) => parse_quantity_amount(value, "first_subscription_item.quantity")?,
        };

        let store_id = json_id(attributes.get("store_id")).unwrap_or_default();
        let customer_id = json_id(attributes.get("customer_id")).unwrap_or_default();
        let variant_id = json_id(attributes.get("variant_id")).unwrap_or_default();

        let status_raw = attributes
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LemonSqueezyError::InvalidEvent("missing status".to_string()))?;
        let sub_status = parse_subscription_status(status_raw).ok_or_else(|| {
            LemonSqueezyError::InvalidEvent(format!("unknown subscription status: {status_raw}"))
        })?;

        let ends_at = optional_timestamp_field(attributes, "ends_at")?;
        let updated_at = required_timestamp_field(attributes, "updated_at")?;

        let customer_portal_url = attributes
            .get("urls")
            .and_then(|v| v.as_object())
            .and_then(|urls| urls.get("customer_portal"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let test_mode = parse_test_mode(attributes)?;

        Ok(SubscriptionDetails {
            subscription_id: id,
            item_id,
            store_id,
            customer_id,
            variant_id,
            status: sub_status,
            quantity,
            ends_at,
            updated_at,
            customer_portal_url,
            test_mode,
        })
    }

    /// Signed customer-portal URLs expire 24 hours after they are issued, so a billing button asks
    /// for a fresh one by re-requesting the subscription object.
    ///
    /// The refreshed URL is accepted only when it is an HTTPS Lemon Squeezy URL. A store that serves
    /// its portal from a custom domain is refused with [`LemonSqueezyError::UntrustedUrl`] until an
    /// explicit host allowlist exists, so a foreign or downgraded URL can never reach a client.
    /// <https://docs.lemonsqueezy.com/guides/developer-guide/customer-portal>
    pub async fn refresh_portal_url(
        &self,
        subscription_id: &str,
    ) -> Result<String, LemonSqueezyError> {
        let details = self.get_subscription(subscription_id).await?;
        let portal_url = details.customer_portal_url.clone();
        let portal_url = portal_url.ok_or_else(|| {
            LemonSqueezyError::PortalUrlUnavailable(details.subscription_id.clone())
        })?;
        if !is_trusted_ls_url(&portal_url) {
            return Err(LemonSqueezyError::UntrustedUrl(portal_url));
        }
        Ok(portal_url)
    }

    /// Resolves an event to the authoritative subscription object.
    ///
    /// For invoice events this is the only way to learn variant, quantity and status; for
    /// subscription events it re-reads the same record and is validated against the configured
    /// store and variants before being handed back.
    pub async fn fetch_authoritative_subscription(
        &self,
        event: &LsEvent,
    ) -> Result<SubscriptionDetails, LemonSqueezyError> {
        let details = self.get_subscription(&event.subscription_id).await?;
        details.validate(&self.config)?;
        Ok(details)
    }

    /// PATCHes a subscription **item**, never the subscription: the item id comes from
    /// `first_subscription_item.id`. Prorations are disabled so the customer is charged at the
    /// next renewal.
    /// <https://docs.lemonsqueezy.com/api/subscription-items/update-subscription-item>
    pub async fn update_subscription_item_quantity(
        &self,
        item_id: &str,
        new_quantity: u32,
    ) -> Result<u32, LemonSqueezyError> {
        require_numeric_id("item_id", item_id)?;
        check_quantity(new_quantity)?;
        let i_id = item_id.trim();

        let url = format!(
            "{}/subscription-items/{}",
            self.config.resolve_api_base()?,
            i_id
        );
        if !is_trusted_ls_url(&url) && !is_loopback_http_url(&url) {
            return Err(LemonSqueezyError::UntrustedUrl(url));
        }

        let payload = serde_json::json!({
            "data": {
                "type": "subscription-items",
                "id": i_id,
                "attributes": {
                    "quantity": new_quantity,
                    "disable_prorations": true,
                    "invoice_immediately": false
                }
            }
        });

        let resp = self
            .client
            .patch(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.config.api_key.trim()),
            )
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(api_failure(status.as_u16(), &body));
        }

        let root: serde_json::Value = serde_json::from_str(&body)?;
        let data = root
            .get("data")
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                LemonSqueezyError::InvalidEvent(
                    "missing data in subscription item response".to_string(),
                )
            })?;

        let returned_type = data
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if returned_type != "subscription-items" {
            return Err(LemonSqueezyError::InvalidEvent(format!(
                "unexpected type in subscription item response: {returned_type:?}"
            )));
        }

        let returned_item_id = required_id("subscription item id", data.get("id"))?;
        if returned_item_id != i_id {
            return Err(LemonSqueezyError::ResourceMismatch {
                entity: "subscription item",
                requested: i_id.to_string(),
                received: returned_item_id,
            });
        }

        let attributes = data
            .get("attributes")
            .and_then(|value| value.as_object())
            .ok_or_else(|| {
                LemonSqueezyError::InvalidEvent(
                    "missing attributes in subscription item response".to_string(),
                )
            })?;
        let returned_quantity = attributes.get("quantity").ok_or_else(|| {
            LemonSqueezyError::InvalidQuantity(
                "subscription_item.quantity is missing from the response".to_string(),
            )
        })?;
        let res_quantity = parse_quantity_amount(returned_quantity, "subscription_item.quantity")?;

        Ok(res_quantity)
    }

    /// Reads the current subscription (validating it against our store and variants), then updates
    /// the quantity on its first subscription item.
    pub async fn update_subscription_quantity(
        &self,
        subscription_id: &str,
        new_quantity: u32,
    ) -> Result<u32, LemonSqueezyError> {
        let sub = self.get_subscription(subscription_id).await?;
        sub.validate(&self.config)?;
        self.update_subscription_item_quantity(&sub.item_id, new_quantity)
            .await
    }

    pub async fn create_checkout(
        &self,
        choice: CheckoutPlanChoice,
        user_id: &str,
        org_id: Option<&str>,
        email: Option<&str>,
    ) -> Result<String, LemonSqueezyError> {
        let uid = user_id.trim();
        if uid.is_empty() {
            return Err(LemonSqueezyError::InvalidEvent(
                "user_id cannot be empty for checkout".to_string(),
            ));
        }

        let (variant_id, quantity) = match choice {
            CheckoutPlanChoice::ProMonthly { packs } => (
                self.config.variant_pro_monthly.clone(),
                pro_subscription_quantity(packs)?,
            ),
            CheckoutPlanChoice::ProAnnual { packs } => (
                self.config.variant_pro_annual.clone(),
                pro_subscription_quantity(packs)?,
            ),
            CheckoutPlanChoice::TeamMonthly { seats } => (
                self.config.variant_team_monthly.clone(),
                team_subscription_quantity(seats)?,
            ),
            CheckoutPlanChoice::TeamAnnual { seats } => (
                self.config.variant_team_annual.clone(),
                team_subscription_quantity(seats)?,
            ),
            CheckoutPlanChoice::TeamHostpackAnnual { packs } => (
                self.config.variant_team_hostpack_annual.clone(),
                team_hostpack_quantity(packs)?,
            ),
        };

        if variant_id.trim().is_empty() {
            return Err(LemonSqueezyError::MissingConfiguration(
                "variant id for the selected plan".to_string(),
            ));
        }
        require_numeric_id("variant_id", &variant_id)?;

        let store_id = self.config.store_id.trim();
        if store_id.is_empty() {
            return Err(LemonSqueezyError::MissingConfiguration(
                "store_id".to_string(),
            ));
        }
        require_numeric_id("store_id", store_id)?;

        let url = format!("{}/checkouts", self.config.resolve_api_base()?);

        let mut custom = serde_json::Map::new();
        custom.insert(
            "user_id".to_string(),
            serde_json::Value::String(uid.to_string()),
        );
        if let Some(oid) = org_id {
            if !oid.trim().is_empty() {
                custom.insert(
                    "org_id".to_string(),
                    serde_json::Value::String(oid.trim().to_string()),
                );
            }
        }

        let mut checkout_data = serde_json::Map::new();
        checkout_data.insert("custom".to_string(), serde_json::Value::Object(custom));
        if let Some(em) = email {
            if !em.trim().is_empty() {
                checkout_data.insert(
                    "email".to_string(),
                    serde_json::Value::String(em.trim().to_string()),
                );
            }
        }

        let var_id_parsed = variant_id.trim().parse::<u64>().map_err(|_| {
            LemonSqueezyError::InvalidIdentifier(format!("variant_id={variant_id:?}"))
        })?;

        checkout_data.insert(
            "variant_quantities".to_string(),
            serde_json::json!([
                {
                    "variant_id": var_id_parsed,
                    "quantity": quantity
                }
            ]),
        );

        let payload = serde_json::json!({
            "data": {
                "type": "checkouts",
                "attributes": {
                    // Documented `attributes.test_mode`: sandbox credentials must request a sandbox
                    // checkout, so the flag mirrors the configured store mode instead of relying on the
                    // provider default (`false`).
                    "test_mode": self.config.expect_test_mode,
                    // Documented `product_options.enabled_variants`: without it the checkout displays every
                    // variant of the product, so the buyer could switch monthly/annual or seat/host-pack and
                    // the entitlement the webhook later applies (plan kind, quantity axis, `custom_data`
                    // owner) would no longer match what this request selected. Exactly one variant is
                    // enabled, and no other `product_options` field is set.
                    "product_options": {
                        "enabled_variants": [var_id_parsed]
                    },
                    "checkout_data": checkout_data
                },
                "relationships": {
                    "store": {
                        "data": {
                            "type": "stores",
                            "id": store_id
                        }
                    },
                    "variant": {
                        "data": {
                            "type": "variants",
                            "id": variant_id.trim()
                        }
                    }
                }
            }
        });

        let resp = self
            .client
            .post(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.config.api_key.trim()),
            )
            .json(&payload)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            return Err(api_failure(status.as_u16(), &body));
        }

        let root: serde_json::Value = serde_json::from_str(&body)?;
        let checkout_url = root
            .get("data")
            .and_then(|v| v.as_object())
            .and_then(|d| d.get("attributes"))
            .and_then(|v| v.as_object())
            .and_then(|a| a.get("url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                LemonSqueezyError::InvalidEvent(
                    "missing attributes.url in checkout response".to_string(),
                )
            })?;

        if !is_trusted_ls_url(checkout_url) {
            return Err(LemonSqueezyError::UntrustedUrl(checkout_url.to_string()));
        }

        Ok(checkout_url.to_string())
    }
}

#[cfg(test)]
#[path = "lemonsqueezy_tests.rs"]
mod lemonsqueezy_tests;
