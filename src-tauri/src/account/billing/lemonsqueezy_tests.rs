//! Deterministic tests for the Lemon Squeezy adapter.
//!
//! Grounding:
//! - signature header `X-Signature` over the raw request body:
//!   <https://docs.lemonsqueezy.com/guides/developer-guide/webhooks>
//! - subscription object (`first_subscription_item`, signed 24h `urls.customer_portal`):
//!   <https://docs.lemonsqueezy.com/api/subscriptions/the-subscription-object>
//! - quantity updates PATCH the subscription *item*:
//!   <https://docs.lemonsqueezy.com/api/subscription-items/update-subscription-item>
//! - checkout payload (`checkout_data.custom`, `checkout_data.variant_quantities`):
//!   <https://docs.lemonsqueezy.com/api/checkouts/create-checkout>
//! - signed portal URLs expire after 24 hours:
//!   <https://docs.lemonsqueezy.com/guides/developer-guide/customer-portal>
//! - `custom_data` is echoed for Order/Subscription/License-key webhooks only:
//!   <https://docs.lemonsqueezy.com/help/checkout/passing-custom-data>
//!
//! Determinism: no sleep, no polling, no process environment mutation, and no network beyond a
//! loopback stub bound to an ephemeral port before the client is built. Assertions read requests
//! the stub recorded *before* it answered.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{json, Value};

use crate::account::billing::entitlement::{
    PlanKey, SubscriptionStatus, MAX_TRUSTED_HOST_PACKS, MAX_TRUSTED_SEATS,
};

use super::*;

/// `subscription_created` webhook body following the documented example payload. Single line on
/// purpose: the signature and event-key constants are HMAC/SHA-256 over exactly these bytes.
const SUBSCRIPTION_CREATED_BODY: &str = r#"{"meta":{"event_name":"subscription_created","custom_data":{"user_id":"user-6f0d1a2b","org_id":"org-acme-01"}},"data":{"type":"subscriptions","id":"555","attributes":{"store_id":4242,"customer_id":777,"order_id":9,"order_item_id":10,"product_id":11,"variant_id":123456,"product_name":"Ferryx Pro","variant_name":"Pro Monthly","user_email":"buyer@example.com","status":"active","cancelled":false,"trial_ends_at":null,"billing_anchor":12,"first_subscription_item":{"id":888,"subscription_id":555,"price_id":3,"quantity":4,"created_at":"2026-09-01T00:00:00.000000Z","updated_at":"2026-09-01T00:00:00.000000Z"},"urls":{"update_payment_method":"https://ferryx.lemonsqueezy.com/subscription/555/payment-details?expires=1790000000&signature=aaa","customer_portal":"https://ferryx.lemonsqueezy.com/billing?expires=1790000000&signature=82ae290ceac8edd4190c82825dd73a8743346d894a8ddbc4898b97eb96d105a5"},"renews_at":"2026-10-01T00:00:00.000000Z","ends_at":null,"created_at":"2026-09-01T00:00:00.000000Z","updated_at":"2026-09-02T03:04:05.000000Z","test_mode":true}}}"#;

/// `subscription_payment_failed` body: `data` is a subscription *invoice*, so `data.id` (`99001`)
/// is the invoice id and the subscription id exists only in `attributes.subscription_id` (`555`).
/// It carries no `custom_data` because Lemon Squeezy does not echo custom data for invoice
/// webhooks (see the custom-data document above).
const INVOICE_BODY: &str = r#"{"meta":{"event_name":"subscription_payment_failed"},"data":{"type":"subscription-invoices","id":"99001","attributes":{"store_id":4242,"subscription_id":555,"customer_id":777,"status":"pending","billing_reason":"renewal","total":500,"currency":"USD","card_brand":"visa","test_mode":true,"created_at":"2026-09-30T00:00:00.000000Z","updated_at":"2026-09-30T00:00:00.000000Z"}}}"#;

const TEST_WEBHOOK_SECRET: &str = "test_webhook_secret_9f3a";

/// HMAC-SHA256 of the fixture body with [`TEST_WEBHOOK_SECRET`], hex, produced by `node:crypto` so
/// the assertion proves interop with an independent implementation.
const SUBSCRIPTION_CREATED_SIGNATURE: &str =
    "e6df076c215478f06376866d4ee09927f72ca761d8302c3853802451c2fa44fd";

/// SHA-256 of the fixture body, hex, from the same independent implementation; it also pins the
/// embedded fixture byte-for-byte.
const SUBSCRIPTION_CREATED_SHA256: &str =
    "7772275cad69537934e4cc1eb3eda5a5a114a5f6d42ebf38daa8b0e97728f049";

const INVOICE_SHA256: &str = "ab9bb6773ac312aa4e1e16af85d6233d7819761b4cbac12148aee5e4284d4385";

/// HMAC-SHA256 of the fixture body with an *empty* key, from the same independent implementation:
/// an unset signing secret must refuse even a signature that is correct for that empty key.
const EMPTY_SECRET_SIGNATURE: &str =
    "df353aa144748e1c2c282304bb1376d5f6d35935790e7eda46355f482cd1a0dd";

/// Unix seconds of `2026-09-02T03:04:05.000000Z`, the fixture's `attributes.updated_at`.
const SUBSCRIPTION_CREATED_UPDATED_AT: u64 = 1_788_318_245;

const TEST_STORE_ID: &str = "4242";
const VARIANT_PRO_MONTHLY: &str = "123456";
const VARIANT_PRO_ANNUAL: &str = "123457";
const VARIANT_TEAM_MONTHLY: &str = "123458";
const VARIANT_TEAM_ANNUAL: &str = "123459";
const VARIANT_TEAM_HOSTPACK_ANNUAL: &str = "123460";
const TEST_API_KEY: &str = "test_api_key_do_not_log";
const TEST_AUTHORIZATION: &str = "Bearer test_api_key_do_not_log";

const SUBSCRIPTION_ID: &str = "555";
const SUBSCRIPTION_ITEM_ID: &str = "888";
const CHECKOUT_URL: &str = "https://ferryx.lemonsqueezy.com/checkout/buy/6c1d4a11";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Option<Value>,
}

#[derive(Clone)]
struct StubState {
    log: Arc<Mutex<Vec<CapturedRequest>>>,
    responses: Arc<Mutex<VecDeque<(u16, Value)>>>,
    /// When set, every request is answered with a 302 to this location before the script is used.
    redirect_to: Option<String>,
}

/// The loopback endpoint tests may inject: bound to `127.0.0.1:0` before the client exists, so no
/// readiness wait and no sleep is required. Dropping it aborts the serving task.
struct StubServer {
    base_url: String,
    state: StubState,
    handle: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(responses: Vec<(u16, Value)>) -> Self {
        Self::start_with_state(StubState {
            log: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(responses.into())),
            redirect_to: None,
        })
        .await
    }

    /// A stub that answers every request with a 302 to `location`: a client that follows redirects
    /// would hand the request, including its Authorization header, to that target.
    async fn start_redirecting(location: String) -> Self {
        Self::start_with_state(StubState {
            log: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(VecDeque::new())),
            redirect_to: Some(location),
        })
        .await
    }

    async fn start_with_state(state: StubState) -> Self {
        let app = Router::new()
            .fallback(stub_handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback stub");
        let addr = listener.local_addr().expect("stub address");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("loopback stub stopped serving");
        });
        Self {
            base_url: format!("http://{addr}"),
            state,
            handle,
        }
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.state.log.lock().expect("stub log lock").clone()
    }

    fn request(&self, index: usize) -> CapturedRequest {
        let log = self.requests();
        match log.get(index) {
            Some(request) => request.clone(),
            None => panic!("stub captured {} requests, wanted #{index}", log.len()),
        }
    }

    fn request_count(&self) -> usize {
        self.requests().len()
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn stub_handler(
    State(state): State<StubState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let captured = CapturedRequest {
        method: method.to_string(),
        path: uri.path().to_string(),
        authorization: headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string),
        body: if body.is_empty() {
            None
        } else {
            serde_json::from_slice(&body).ok()
        },
    };
    state.log.lock().expect("stub log lock").push(captured);

    if let Some(location) = state.redirect_to.clone() {
        return (
            StatusCode::FOUND,
            [(axum::http::header::LOCATION, location)],
        )
            .into_response();
    }

    match state
        .responses
        .lock()
        .expect("stub script lock")
        .pop_front()
    {
        Some((status, value)) => {
            let status = StatusCode::from_u16(status).expect("scripted status code");
            (status, axum::Json(value)).into_response()
        }
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": "stub script exhausted" })),
        )
            .into_response(),
    }
}

fn config_with_base(base_url: Option<&str>) -> LemonSqueezyConfig {
    LemonSqueezyConfig {
        api_key: TEST_API_KEY.to_string(),
        store_id: TEST_STORE_ID.to_string(),
        webhook_secret: TEST_WEBHOOK_SECRET.to_string(),
        variant_pro_monthly: VARIANT_PRO_MONTHLY.to_string(),
        variant_pro_annual: VARIANT_PRO_ANNUAL.to_string(),
        variant_team_monthly: VARIANT_TEAM_MONTHLY.to_string(),
        variant_team_annual: VARIANT_TEAM_ANNUAL.to_string(),
        variant_team_hostpack_annual: VARIANT_TEAM_HOSTPACK_ANNUAL.to_string(),
        api_base_url: base_url.map(str::to_string),
        expect_test_mode: true,
    }
}

fn production_config_with_base(base_url: Option<&str>) -> LemonSqueezyConfig {
    LemonSqueezyConfig {
        expect_test_mode: false,
        ..config_with_base(base_url)
    }
}

fn client_for(stub: &StubServer) -> LemonSqueezyClient {
    LemonSqueezyClient::new(config_with_base(Some(&stub.base_url))).expect("loopback client")
}

fn subscription_response(quantity: u32, portal_signature: &str) -> Value {
    json!({
        "data": {
            "type": "subscriptions",
            "id": SUBSCRIPTION_ID,
            "attributes": {
                "store_id": 4242,
                "customer_id": 777,
                "variant_id": 123456,
                "status": "active",
                "ends_at": null,
                "updated_at": "2026-09-30T00:00:00.000000Z",
                "test_mode": true,
                "first_subscription_item": {
                    "id": 888,
                    "subscription_id": 555,
                    "price_id": 3,
                    "quantity": quantity,
                    "created_at": "2026-09-01T00:00:00.000000Z",
                    "updated_at": "2026-09-01T00:00:00.000000Z"
                },
                "urls": {
                    "customer_portal": format!(
                        "https://ferryx.lemonsqueezy.com/billing?expires=1790000100&signature={portal_signature}"
                    )
                }
            }
        }
    })
}

fn item_update_response(quantity: u32) -> Value {
    json!({
        "data": {
            "type": "subscription-items",
            "id": SUBSCRIPTION_ITEM_ID,
            "attributes": { "quantity": quantity }
        }
    })
}

fn checkout_response() -> Value {
    json!({
        "data": {
            "type": "checkouts",
            "id": "6c1d4a11",
            "attributes": { "url": CHECKOUT_URL }
        }
    })
}

fn parsed_subscription_event() -> LsEvent {
    parse_event(SUBSCRIPTION_CREATED_BODY.as_bytes())
        .expect("fixture parses")
        .expect("fixture is a tracked event")
}

fn parsed_invoice_event() -> LsEvent {
    parse_event(INVOICE_BODY.as_bytes())
        .expect("fixture parses")
        .expect("fixture is a tracked event")
}

#[test]
fn signature_verification_accepts_documented_hmac() {
    assert!(verify_signature(
        TEST_WEBHOOK_SECRET,
        SUBSCRIPTION_CREATED_BODY.as_bytes(),
        SUBSCRIPTION_CREATED_SIGNATURE
    ));
    let uppercase = format!("  {}  ", SUBSCRIPTION_CREATED_SIGNATURE.to_uppercase());
    assert!(verify_signature(
        TEST_WEBHOOK_SECRET,
        SUBSCRIPTION_CREATED_BODY.as_bytes(),
        &uppercase
    ));
}

#[test]
fn signature_verification_rejects_single_byte_tamper() {
    let mut tampered = SUBSCRIPTION_CREATED_BODY.as_bytes().to_vec();
    let last = tampered.len() - 1;
    tampered[last] = b']';
    assert_ne!(&tampered[..], SUBSCRIPTION_CREATED_BODY.as_bytes());
    assert!(!verify_signature(
        TEST_WEBHOOK_SECRET,
        &tampered,
        SUBSCRIPTION_CREATED_SIGNATURE
    ));
}

#[test]
fn signature_verification_rejects_other_secret_and_malformed_header() {
    assert!(!verify_signature(
        "another_secret",
        SUBSCRIPTION_CREATED_BODY.as_bytes(),
        SUBSCRIPTION_CREATED_SIGNATURE
    ));
    for header in [
        "",
        "not-hex",
        "abc",
        "e6df076c215478f06376866d4ee09927f72ca761d8302c3853802451c2fa44ffee",
    ] {
        assert!(
            !verify_signature(
                TEST_WEBHOOK_SECRET,
                SUBSCRIPTION_CREATED_BODY.as_bytes(),
                header
            ),
            "header {header:?} must not verify"
        );
    }
}

#[test]
fn empty_or_blank_webhook_secret_never_authenticates() {
    assert!(!verify_signature(
        "",
        SUBSCRIPTION_CREATED_BODY.as_bytes(),
        EMPTY_SECRET_SIGNATURE
    ));
    for blank in [" ", "\t", "\n  "] {
        assert!(!verify_signature(
            blank,
            SUBSCRIPTION_CREATED_BODY.as_bytes(),
            SUBSCRIPTION_CREATED_SIGNATURE
        ));
        assert!(!verify_signature(
            blank,
            SUBSCRIPTION_CREATED_BODY.as_bytes(),
            ""
        ));
    }
}

#[test]
fn refunded_payment_events_do_not_invent_a_subscription_status() {
    let body = INVOICE_BODY.replacen(
        "\"event_name\":\"subscription_payment_failed\"",
        "\"event_name\":\"subscription_payment_refunded\"",
        1,
    );
    let event = parse_event(body.as_bytes())
        .expect("parses")
        .expect("tracked");
    assert_eq!(event.payload_kind, LsPayloadKind::Invoice);
    assert_eq!(event.invoice_id.as_deref(), Some("99001"));
    assert_eq!(event.subscription_id, SUBSCRIPTION_ID);
    assert_eq!(event.status, None);
    assert_eq!(event.quantity, 0);
}

#[tokio::test]
async fn refunded_payment_status_comes_from_the_authoritative_subscription() {
    let body = INVOICE_BODY.replacen(
        "\"event_name\":\"subscription_payment_failed\"",
        "\"event_name\":\"subscription_payment_refunded\"",
        1,
    );
    let event = parse_event(body.as_bytes())
        .expect("parses")
        .expect("tracked");

    let mut response = subscription_response(3, "fresh-signature");
    response["data"]["attributes"]["status"] = json!("past_due");
    let stub = StubServer::start(vec![(200, response)]).await;
    let client = client_for(&stub);

    let details = client
        .fetch_authoritative_subscription(&event)
        .await
        .expect("authoritative subscription");
    assert_eq!(details.status, SubscriptionStatus::PastDue);
}

#[test]
fn event_key_is_sha256_hex_of_the_raw_body() {
    assert_eq!(
        event_key(SUBSCRIPTION_CREATED_BODY.as_bytes()),
        SUBSCRIPTION_CREATED_SHA256
    );
    assert_eq!(event_key(INVOICE_BODY.as_bytes()), INVOICE_SHA256);
}

#[test]
fn parse_subscription_event_matches_documented_payload() {
    let event = parsed_subscription_event();

    assert_eq!(event.event_name, "subscription_created");
    assert_eq!(event.payload_kind, LsPayloadKind::Subscription);
    assert_eq!(event.subscription_id, SUBSCRIPTION_ID);
    assert_eq!(event.invoice_id, None);
    assert_eq!(event.store_id, TEST_STORE_ID);
    assert_eq!(event.customer_id, "777");
    assert_eq!(event.variant_id, VARIANT_PRO_MONTHLY);
    assert_eq!(event.status, Some(SubscriptionStatus::Active));
    assert_eq!(event.quantity, 4);
    assert_eq!(event.ends_at, None);
    assert_eq!(event.updated_at, SUBSCRIPTION_CREATED_UPDATED_AT);
    assert!(event.test_mode);
    assert_eq!(event.custom_user_id.as_deref(), Some("user-6f0d1a2b"));
    assert_eq!(event.custom_org_id.as_deref(), Some("org-acme-01"));
    assert_eq!(event.custom_owner_key().as_deref(), Some("org:org-acme-01"));
    assert_eq!(
        event.manage_url.as_deref(),
        Some("https://ferryx.lemonsqueezy.com/billing?expires=1790000000&signature=82ae290ceac8edd4190c82825dd73a8743346d894a8ddbc4898b97eb96d105a5")
    );
}

#[test]
fn parse_invoice_event_uses_attribute_subscription_id_not_invoice_id() {
    let event = parsed_invoice_event();

    assert_eq!(event.event_name, "subscription_payment_failed");
    assert_eq!(event.payload_kind, LsPayloadKind::Invoice);
    assert_eq!(event.invoice_id.as_deref(), Some("99001"));
    assert_eq!(event.subscription_id, SUBSCRIPTION_ID);
    assert_eq!(event.status, None);
    assert_eq!(event.quantity, 0);
    assert!(event.variant_id.is_empty());
    assert_eq!(event.custom_owner_key(), None);
    assert!(event.custom_user_id.is_none());
    assert_eq!(event.store_id, TEST_STORE_ID);
    assert!(event.test_mode);
    event
        .validate(&config_with_base(None))
        .expect("invoice event validates against our store");
}

#[test]
fn parse_accepts_the_singular_invoice_type_spelling() {
    let body = INVOICE_BODY.replacen(
        "\"type\":\"subscription-invoices\"",
        "\"type\":\"subscription-invoice\"",
        1,
    );
    let event = parse_event(body.as_bytes())
        .expect("parses")
        .expect("tracked");
    assert_eq!(event.payload_kind, LsPayloadKind::Invoice);
    assert_eq!(event.invoice_id.as_deref(), Some("99001"));
}

#[test]
fn missing_custom_data_is_a_typed_error_for_subscription_payloads() {
    // Surgical JSON mutation: the previous string replacement was silently a no-op because the fixture's
    // `custom_data` object is followed by `}}` (closing `meta`), not by a comma - so the test never
    // removed anything and proved nothing. Remove the key from the parsed document instead, and assert
    // the removal actually happened before asserting the parse error.
    let mut body: serde_json::Value =
        serde_json::from_str(SUBSCRIPTION_CREATED_BODY).expect("fixture parses as JSON");
    let custom_data = body["meta"]
        .as_object_mut()
        .expect("meta object")
        .remove("custom_data")
        .expect("the fixture carries meta.custom_data");
    assert!(
        custom_data.is_object(),
        "the removed custom_data must be the documented object, got {custom_data}"
    );

    let tampered = serde_json::to_vec(&body).expect("re-serialize the fixture");
    let error = parse_event(&tampered).expect_err("owner-less subscription event");
    assert!(
        matches!(error, LemonSqueezyError::MissingOwnerData(_)),
        "expected MissingOwnerData, got {error:?}"
    );
}

#[test]
fn custom_data_with_only_an_org_still_resolves_an_owner() {
    let body = SUBSCRIPTION_CREATED_BODY.replacen(
        "\"custom_data\":{\"user_id\":\"user-6f0d1a2b\",\"org_id\":\"org-acme-01\"}",
        "\"custom_data\":{\"org_id\":\"org-acme-01\"}",
        1,
    );
    let event = parse_event(body.as_bytes())
        .expect("parses")
        .expect("tracked");
    assert_eq!(event.custom_user_id, None);
    assert_eq!(event.custom_owner_key().as_deref(), Some("org:org-acme-01"));
}

#[test]
fn unknown_event_name_and_unknown_payload_type_are_ignored() {
    let unknown_event = SUBSCRIPTION_CREATED_BODY.replacen(
        "\"event_name\":\"subscription_created\"",
        "\"event_name\":\"order_created\"",
        1,
    );
    assert_eq!(parse_event(unknown_event.as_bytes()).expect("parses"), None);

    let unknown_payload =
        SUBSCRIPTION_CREATED_BODY.replacen("\"type\":\"subscriptions\"", "\"type\":\"orders\"", 1);
    assert_eq!(
        parse_event(unknown_payload.as_bytes()).expect("parses"),
        None
    );
}

#[test]
fn malformed_events_report_typed_errors() {
    let missing_store = SUBSCRIPTION_CREATED_BODY.replacen("\"store_id\":4242,", "", 1);
    let error = parse_event(missing_store.as_bytes()).expect_err("missing store id");
    assert!(
        matches!(error, LemonSqueezyError::InvalidEvent(_)),
        "expected InvalidEvent, got {error:?}"
    );

    let non_boolean_test_mode =
        SUBSCRIPTION_CREATED_BODY.replacen("\"test_mode\":true", "\"test_mode\":\"true\"", 1);
    let error = parse_event(non_boolean_test_mode.as_bytes()).expect_err("test_mode must be bool");
    assert!(
        matches!(error, LemonSqueezyError::InvalidEvent(_)),
        "expected InvalidEvent, got {error:?}"
    );

    let unknown_status =
        SUBSCRIPTION_CREATED_BODY.replacen("\"status\":\"active\"", "\"status\":\"weird\"", 1);
    let error = parse_event(unknown_status.as_bytes()).expect_err("unknown status");
    assert!(matches!(error, LemonSqueezyError::InvalidEvent(_)));

    let invoice_without_subscription_id = INVOICE_BODY.replacen("\"subscription_id\":555,", "", 1);
    let error = parse_event(invoice_without_subscription_id.as_bytes())
        .expect_err("invoice must name its subscription");
    assert!(
        matches!(error, LemonSqueezyError::InvalidEvent(_)),
        "expected InvalidEvent, got {error:?}"
    );

    assert!(matches!(
        parse_event(b"not json").expect_err("not json"),
        LemonSqueezyError::Json(_)
    ));
}

#[test]
fn missing_test_mode_attribute_defaults_to_false() {
    let body = SUBSCRIPTION_CREATED_BODY.replacen(",\"test_mode\":true", "", 1);
    let event = parse_event(body.as_bytes())
        .expect("parses")
        .expect("tracked");
    assert!(!event.test_mode);
}

#[test]
fn out_of_range_event_quantity_is_a_typed_error() {
    let huge = SUBSCRIPTION_CREATED_BODY.replacen("\"quantity\":4", "\"quantity\":100001", 1);
    let error = parse_event(huge.as_bytes()).expect_err("quantity above the ceiling");
    assert!(
        matches!(error, LemonSqueezyError::InvalidQuantity(_)),
        "expected InvalidQuantity, got {error:?}"
    );

    let zero = SUBSCRIPTION_CREATED_BODY.replacen("\"quantity\":4", "\"quantity\":0", 1);
    // The parse boundary is strict: `parse_quantity_amount` -> `check_quantity` rejects 0, so the error
    // must surface from `parse_event` itself rather than from a later `validate` call.
    let error = parse_event(zero.as_bytes()).expect_err("zero quantity is rejected at parse");
    assert!(
        matches!(error, LemonSqueezyError::InvalidQuantity(_)),
        "expected InvalidQuantity, got {error:?}"
    );
}

#[test]
fn validate_event_rejects_foreign_store_unknown_variant_and_non_numeric_id() {
    let event = parsed_subscription_event();
    let config = config_with_base(None);

    let mut foreign = event.clone();
    foreign.store_id = "9999".to_string();
    assert!(matches!(
        foreign.validate(&config).expect_err("foreign store"),
        LemonSqueezyError::UnsupportedStore(_)
    ));

    let mut unknown_variant = event.clone();
    unknown_variant.variant_id = "777777".to_string();
    assert!(matches!(
        unknown_variant
            .validate(&config)
            .expect_err("unknown variant"),
        LemonSqueezyError::UnsupportedVariant(_)
    ));

    let mut non_numeric = event.clone();
    non_numeric.subscription_id = "../subscriptions".to_string();
    assert!(matches!(
        non_numeric.validate(&config).expect_err("non numeric id"),
        LemonSqueezyError::InvalidIdentifier(_)
    ));

    event.validate(&config).expect("the fixture is valid");
}

#[test]
fn configured_test_mode_must_match_the_event() {
    let event = parsed_subscription_event();
    event
        .validate(&config_with_base(None))
        .expect("a sandbox config accepts a test-mode event");

    let error = event
        .validate(&production_config_with_base(None))
        .expect_err("production must reject a test-mode event");
    assert!(
        matches!(
            error,
            LemonSqueezyError::TestModeMismatch {
                expected: false,
                actual: true,
                ..
            }
        ),
        "got {error:?}"
    );

    let mut live_event = event.clone();
    live_event.test_mode = false;
    let error = live_event
        .validate(&config_with_base(None))
        .expect_err("a sandbox store must reject a live-mode event");
    assert!(
        matches!(
            error,
            LemonSqueezyError::TestModeMismatch {
                expected: true,
                actual: false,
                ..
            }
        ),
        "got {error:?}"
    );

    let invoice = parsed_invoice_event();
    assert!(
        matches!(
            invoice
                .validate(&production_config_with_base(None))
                .expect_err("invoice events carry test_mode too"),
            LemonSqueezyError::TestModeMismatch { .. }
        ),
        "invoice events must be mode-checked as well"
    );
}

#[tokio::test]
async fn authoritative_subscription_must_match_the_configured_test_mode() {
    let stub = StubServer::start(vec![(200, subscription_response(3, "fresh-signature"))]).await;
    let client =
        LemonSqueezyClient::new(production_config_with_base(Some(&stub.base_url))).expect("client");

    let error = client
        .fetch_authoritative_subscription(&parsed_invoice_event())
        .await
        .expect_err("production must reject a test-mode subscription");
    assert!(
        matches!(error, LemonSqueezyError::TestModeMismatch { .. }),
        "got {error:?}"
    );
}

#[test]
fn hostpack_variant_is_a_separate_plan_kind_not_a_seat_subscription() {
    let config = config_with_base(None);
    assert_eq!(
        variant_to_plan(VARIANT_TEAM_HOSTPACK_ANNUAL, &config),
        Some((PlanKey::TeamAnnual, PlanVariantKind::Hostpack))
    );
    assert_eq!(
        variant_to_plan(VARIANT_TEAM_ANNUAL, &config),
        Some((PlanKey::TeamAnnual, PlanVariantKind::Base))
    );
    assert_ne!(VARIANT_TEAM_HOSTPACK_ANNUAL, VARIANT_TEAM_ANNUAL);

    let mut event = parsed_subscription_event();
    event.variant_id = VARIANT_TEAM_HOSTPACK_ANNUAL.to_string();
    event.quantity = 3;
    event
        .validate(&config)
        .expect("a host-pack event carries a pack count");
}

#[test]
fn variant_to_plan_maps_every_configured_variant() {
    let config = config_with_base(None);
    let table = [
        (
            VARIANT_PRO_MONTHLY,
            (PlanKey::ProMonthly, PlanVariantKind::Base),
        ),
        (
            VARIANT_PRO_ANNUAL,
            (PlanKey::ProAnnual, PlanVariantKind::Base),
        ),
        (
            VARIANT_TEAM_MONTHLY,
            (PlanKey::TeamMonthly, PlanVariantKind::Base),
        ),
        (
            VARIANT_TEAM_ANNUAL,
            (PlanKey::TeamAnnual, PlanVariantKind::Base),
        ),
        (
            VARIANT_TEAM_HOSTPACK_ANNUAL,
            (PlanKey::TeamAnnual, PlanVariantKind::Hostpack),
        ),
    ];
    for (variant, expected) in table {
        assert_eq!(
            variant_to_plan(variant, &config),
            // `PlanVariantKind` is `Clone` but not `Copy` (`PlanKey` is both), so the tuple is moved by
            // the first `Some(..)` and cannot be reused by the second comparison without a clone.
            Some(expected.clone()),
            "variant {variant}"
        );
        assert_eq!(
            variant_to_plan(&format!(" {variant} "), &config),
            Some(expected)
        );
    }
    assert_eq!(variant_to_plan("", &config), None);
    assert_eq!(variant_to_plan("424242", &config), None);
}

#[test]
fn subscription_status_and_timestamp_parsing_match_the_documented_vocabulary() {
    let table = [
        ("active", SubscriptionStatus::Active),
        ("on_trial", SubscriptionStatus::OnTrial),
        ("past_due", SubscriptionStatus::PastDue),
        ("unpaid", SubscriptionStatus::Unpaid),
        ("cancelled", SubscriptionStatus::Cancelled),
        ("expired", SubscriptionStatus::Expired),
        ("paused", SubscriptionStatus::Paused),
        ("ACTIVE", SubscriptionStatus::Active),
    ];
    for (raw, expected) in table {
        assert_eq!(
            parse_subscription_status(raw),
            Some(expected),
            "status {raw}"
        );
    }
    assert_eq!(parse_subscription_status("trialing"), None);

    assert_eq!(
        parse_timestamp("2026-09-02T03:04:05.000000Z"),
        Some(SUBSCRIPTION_CREATED_UPDATED_AT)
    );
    assert_eq!(parse_timestamp("1969-12-31T23:59:59Z"), None);
    assert_eq!(parse_timestamp("2026-09-02 03:04:05"), None);
}

#[test]
fn parse_timestamp_accepts_documented_shapes_and_rejects_lenient_ones() {
    assert_eq!(
        parse_timestamp("2026-09-02T03:04:05Z"),
        Some(SUBSCRIPTION_CREATED_UPDATED_AT)
    );
    assert_eq!(
        parse_timestamp("2021-08-11T13:47:28.000000Z"),
        Some(1_628_689_648)
    );
    assert_eq!(parse_timestamp("2024-02-29T00:00:00Z"), Some(1_709_164_800));
    assert_eq!(
        parse_timestamp("2026-09-02T03:04:05+02:00"),
        Some(1_788_311_045)
    );
    assert_eq!(
        parse_timestamp("2026-09-02T03:04:05-00:00"),
        Some(SUBSCRIPTION_CREATED_UPDATED_AT)
    );

    for invalid in [
        "",
        "not-a-timestamp",
        "2026-09-02 03:04:05Z",
        "2026-09-02T03:04:05",
        "2026-09-02T03:04:05.Z",
        "2026-09-02T03:04:60Z",
        "2026-09-02T24:00:00Z",
        "2026-13-02T03:04:05Z",
        "2026-02-30T03:04:05Z",
        "2025-02-29T00:00:00Z",
        "1969-12-31T23:59:59Z",
        "2026-09-02T03:04:05+0200",
    ] {
        assert_eq!(parse_timestamp(invalid), None, "{invalid:?} must not parse");
    }
}

#[test]
fn timestamp_gate_is_stricter_than_the_crate_parser() {
    // RFC 3339 lets the date and time be separated by any single byte; the documented Lemon Squeezy
    // shape is `T`, so the gate is what rejects a space. This also proves at compile time that
    // `time`'s `parsing` feature is enabled in this build.
    let rfc3339_lenient = "2026-09-02 03:04:05Z";
    assert!(
        time::OffsetDateTime::parse(
            rfc3339_lenient,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok(),
        "the crate parses the space separator"
    );
    assert_eq!(parse_timestamp(rfc3339_lenient), None);

    let documented = "2026-09-02T03:04:05Z";
    assert!(time::OffsetDateTime::parse(
        documented,
        &time::format_description::well_known::Rfc3339
    )
    .is_ok());
    assert_eq!(
        parse_timestamp(documented),
        Some(SUBSCRIPTION_CREATED_UPDATED_AT)
    );
}

#[test]
fn env_flag_accepts_only_documented_spellings() {
    for yes in ["1", "true", "TRUE", " yes ", "On"] {
        assert!(parse_env_flag(Some(yes)), "{yes:?}");
    }
    for no in [
        None,
        Some(""),
        Some("  "),
        Some("0"),
        Some("false"),
        Some("off"),
        Some("sandbox"),
    ] {
        assert!(!parse_env_flag(no), "{no:?}");
    }
}

#[test]
fn invalid_timestamps_are_rejected_instead_of_defaulted() {
    let bad_updated = SUBSCRIPTION_CREATED_BODY.replacen(
        "\"updated_at\":\"2026-09-02T03:04:05.000000Z\"",
        "\"updated_at\":\"2026-09-02 03:04:05\"",
        1,
    );
    assert!(matches!(
        parse_event(bad_updated.as_bytes()).expect_err("bad updated_at"),
        LemonSqueezyError::InvalidEvent(_)
    ));

    let missing_updated = SUBSCRIPTION_CREATED_BODY.replacen(
        "\"updated_at\":\"2026-09-02T03:04:05.000000Z\",",
        "",
        1,
    );
    assert!(matches!(
        parse_event(missing_updated.as_bytes()).expect_err("missing updated_at"),
        LemonSqueezyError::InvalidEvent(_)
    ));

    let bad_ends =
        SUBSCRIPTION_CREATED_BODY.replacen("\"ends_at\":null", "\"ends_at\":\"soon\"", 1);
    assert!(matches!(
        parse_event(bad_ends.as_bytes()).expect_err("bad ends_at"),
        LemonSqueezyError::InvalidEvent(_)
    ));

    let numeric_ends =
        SUBSCRIPTION_CREATED_BODY.replacen("\"ends_at\":null", "\"ends_at\":1700000000", 1);
    assert!(matches!(
        parse_event(numeric_ends.as_bytes()).expect_err("numeric ends_at"),
        LemonSqueezyError::InvalidEvent(_)
    ));

    assert_eq!(parsed_subscription_event().ends_at, None);
}

#[test]
fn trusted_urls_are_https_lemonsqueezy_hosts_only() {
    assert!(is_trusted_ls_url(LEMONSQUEEZY_API_BASE));
    assert!(is_trusted_ls_url("https://ferryx.lemonsqueezy.com/billing"));
    assert!(is_trusted_ls_url("https://lemonsqueezy.com"));
    assert!(!is_trusted_ls_url("http://api.lemonsqueezy.com/v1"));
    assert!(!is_trusted_ls_url("https://evil.example.com"));
    assert!(!is_trusted_ls_url(
        "https://lemonsqueezy.com.evil.example.com"
    ));
    assert!(!is_trusted_ls_url("https://notlemonsqueezy.com"));
    assert!(!is_trusted_ls_url(""));

    assert!(is_loopback_http_url("http://127.0.0.1:8080"));
    assert!(is_loopback_http_url("http://localhost:1/v1"));
    assert!(is_loopback_http_url("http://[::1]:9"));
    assert!(!is_loopback_http_url("http://127.0.0.1.evil.example.com"));
    assert!(!is_loopback_http_url("https://127.0.0.1"));
}

#[test]
fn numeric_id_predicate_rejects_path_shaped_input() {
    assert!(is_numeric_id("555"));
    assert!(is_numeric_id(" 555 "));
    assert!(!is_numeric_id("../subscriptions/1"));
    assert!(!is_numeric_id("555/../1"));
    assert!(!is_numeric_id("https://evil.example.com"));
    assert!(!is_numeric_id(""));
    assert!(!is_numeric_id("５５５"));
    assert!(!is_numeric_id(&"9".repeat(21)));
}

#[test]
fn default_base_is_the_public_https_api() {
    let config = config_with_base(None);
    assert_eq!(
        config.resolve_api_base().expect("default base"),
        LEMONSQUEEZY_API_BASE
    );
    assert!(LemonSqueezyClient::new(config).is_ok());
}

#[test]
fn client_rejects_untrusted_bases() {
    // `LemonSqueezyClient` deliberately implements no `Debug` (nothing consumes it, and the struct
    // carries the credential-bearing config), so `expect_err`/`unwrap_err` cannot be used on the
    // constructor - both require the *`Ok`* type to be `Debug`. Pattern-match the result instead of
    // adding a `Debug` impl that only a test would want.
    // A plain-HTTP *loopback* base is deliberately accepted when it is explicitly injected
    // (`resolve_api_base`), so the negative case here must be a non-loopback HTTP host. TEST-NET-1 is
    // used so the URL is unmistakably unroutable: the constructor only validates the URL string and
    // never opens a connection, so no traffic is attempted.
    for base in [
        "https://evil.example.com/v1",
        "http://api.lemonsqueezy.com/v1",
        "https://lemonsqueezy.com.evil.example.com/v1",
        "http://192.0.2.1/v1",
    ] {
        assert!(
            matches!(
                LemonSqueezyClient::new(config_with_base(Some(base))),
                Err(LemonSqueezyError::UntrustedUrl(_))
            ),
            "base {base} must be rejected as UntrustedUrl"
        );
    }

    // The explicitly injected loopback test endpoint is the only base outside the HTTPS Lemon Squeezy
    // policy that must still be accepted, so the constructor's `Ok` arm is exercised, not assumed.
    assert!(matches!(
        LemonSqueezyClient::new(config_with_base(Some("http://127.0.0.1:9"))),
        Ok(_)
    ));
}

#[test]
fn config_debug_redacts_credentials() {
    let config = config_with_base(Some("http://127.0.0.1:9"));
    let debug = format!("{config:?}");
    assert!(!debug.contains(TEST_API_KEY), "api key leaked: {debug}");
    assert!(
        !debug.contains(TEST_WEBHOOK_SECRET),
        "webhook secret leaked: {debug}"
    );
    assert!(debug.contains("api_key"));
    assert!(debug.contains("webhook_secret"));
    assert!(debug.contains(TEST_STORE_ID));
    assert!(debug.contains(VARIANT_TEAM_HOSTPACK_ANNUAL));

    let empty = LemonSqueezyConfig {
        api_key: String::new(),
        webhook_secret: String::new(),
        ..config_with_base(None)
    };
    let empty_debug = format!("{empty:?}");
    assert!(empty_debug.contains("api_key"));
    assert!(!empty_debug.contains("\"\""));
}

#[test]
fn quantity_helpers_encode_the_documented_variant_axes() {
    assert_eq!(MAX_QUANTITY, 10_001);
    assert_eq!(MAX_TRUSTED_HOST_PACKS, 10_000);
    assert_eq!(MAX_TRUSTED_SEATS, 10_000);
    // `LemonSqueezyError` cannot implement `PartialEq` (`reqwest::Error` and `serde_json::Error` do
    // not), so these compare the unwrapped quantity instead of `Result` values.
    assert_eq!(pro_subscription_quantity(0).expect("0 packs"), 1);
    assert_eq!(pro_subscription_quantity(3).expect("3 packs"), 4);
    assert_eq!(team_subscription_quantity(0).expect("0 seats"), 2);
    assert_eq!(team_subscription_quantity(1).expect("1 seat"), 2);
    assert_eq!(team_subscription_quantity(5).expect("5 seats"), 5);
    assert_eq!(team_hostpack_quantity(0).expect("0 packs"), 1);
    assert_eq!(team_hostpack_quantity(4).expect("4 packs"), 4);

    assert!(matches!(
        pro_subscription_quantity(MAX_QUANTITY),
        Err(LemonSqueezyError::InvalidQuantity(_))
    ));
    assert!(matches!(
        team_subscription_quantity(MAX_QUANTITY + 1),
        Err(LemonSqueezyError::InvalidQuantity(_))
    ));

    assert_eq!(
        pro_subscription_quantity(MAX_TRUSTED_HOST_PACKS).expect("pack ceiling"),
        MAX_QUANTITY
    );
    assert_eq!(
        team_subscription_quantity(MAX_TRUSTED_SEATS).expect("seat ceiling"),
        MAX_TRUSTED_SEATS
    );
    assert!(matches!(
        pro_subscription_quantity(MAX_TRUSTED_HOST_PACKS + 1),
        Err(LemonSqueezyError::InvalidQuantity(_))
    ));
    assert!(matches!(
        team_subscription_quantity(MAX_TRUSTED_SEATS + 1),
        Err(LemonSqueezyError::InvalidQuantity(_))
    ));
}

#[test]
fn oversized_quantities_are_rejected_not_truncated() {
    let wide = SUBSCRIPTION_CREATED_BODY.replacen("\"quantity\":4", "\"quantity\":4294967297", 1);
    assert!(
        matches!(
            parse_event(wide.as_bytes()).expect_err("2^32 + 1"),
            LemonSqueezyError::InvalidQuantity(_)
        ),
        "a 33-bit quantity must be rejected, not truncated"
    );

    let negative = SUBSCRIPTION_CREATED_BODY.replacen("\"quantity\":4", "\"quantity\":-1", 1);
    assert!(matches!(
        parse_event(negative.as_bytes()).expect_err("negative quantity"),
        LemonSqueezyError::InvalidQuantity(_)
    ));

    let above_ceiling =
        SUBSCRIPTION_CREATED_BODY.replacen("\"quantity\":4", "\"quantity\":10002", 1);
    assert!(matches!(
        parse_event(above_ceiling.as_bytes()).expect_err("above the trust bound"),
        LemonSqueezyError::InvalidQuantity(_)
    ));
}

#[tokio::test]
async fn checkout_quantity_mapping_covers_all_five_choices() {
    let cases: [(CheckoutPlanChoice, &str, u32); 8] = [
        (
            CheckoutPlanChoice::ProMonthly { packs: 0 },
            VARIANT_PRO_MONTHLY,
            1,
        ),
        (
            CheckoutPlanChoice::ProMonthly { packs: 3 },
            VARIANT_PRO_MONTHLY,
            4,
        ),
        (
            CheckoutPlanChoice::ProAnnual { packs: 2 },
            VARIANT_PRO_ANNUAL,
            3,
        ),
        (
            CheckoutPlanChoice::TeamMonthly { seats: 1 },
            VARIANT_TEAM_MONTHLY,
            2,
        ),
        (
            CheckoutPlanChoice::TeamMonthly { seats: 5 },
            VARIANT_TEAM_MONTHLY,
            5,
        ),
        (
            CheckoutPlanChoice::TeamAnnual { seats: 0 },
            VARIANT_TEAM_ANNUAL,
            2,
        ),
        (
            CheckoutPlanChoice::TeamHostpackAnnual { packs: 4 },
            VARIANT_TEAM_HOSTPACK_ANNUAL,
            4,
        ),
        (
            CheckoutPlanChoice::TeamHostpackAnnual { packs: 0 },
            VARIANT_TEAM_HOSTPACK_ANNUAL,
            1,
        ),
    ];

    let stub = StubServer::start(
        (0..cases.len())
            .map(|_| (200u16, checkout_response()))
            .collect(),
    )
    .await;
    let client = client_for(&stub);

    for (index, (choice, _, _)) in cases.iter().enumerate() {
        let url = client
            .create_checkout(
                *choice,
                "user-6f0d1a2b",
                Some("org-acme-01"),
                Some("buyer@example.com"),
            )
            .await
            .unwrap_or_else(|error| panic!("checkout {index}: {error:?}"));
        assert_eq!(url, CHECKOUT_URL);
    }

    assert_eq!(stub.request_count(), cases.len());
    for (index, (_, expected_variant, expected_quantity)) in cases.iter().enumerate() {
        let request = stub.request(index);
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/checkouts");
        assert_eq!(request.authorization.as_deref(), Some(TEST_AUTHORIZATION));

        let body = request.body.expect("checkout body");
        assert_eq!(body["data"]["type"], "checkouts");
        assert_eq!(
            body["data"]["attributes"]["test_mode"],
            json!(true),
            "case {index}: sandbox credentials must request a test-mode checkout"
        );
        assert_eq!(
            body["data"]["relationships"]["store"]["data"]["id"],
            TEST_STORE_ID
        );
        assert_eq!(
            body["data"]["relationships"]["variant"]["data"]["id"],
            json!(*expected_variant)
        );
        assert_eq!(
            body["data"]["attributes"]["checkout_data"]["variant_quantities"][0]["variant_id"],
            json!(expected_variant.parse::<u64>().expect("numeric variant id"))
        );
        assert_eq!(
            body["data"]["attributes"]["checkout_data"]["variant_quantities"][0]["quantity"],
            json!(*expected_quantity),
            "case {index} quantity"
        );
        assert_eq!(
            body["data"]["attributes"]["checkout_data"]["custom"]["user_id"],
            "user-6f0d1a2b"
        );
        assert_eq!(
            body["data"]["attributes"]["checkout_data"]["custom"]["org_id"],
            "org-acme-01"
        );
        assert_eq!(
            body["data"]["attributes"]["checkout_data"]["email"],
            "buyer@example.com"
        );
    }
}

#[tokio::test]
async fn checkout_requests_the_configured_ls_test_mode() {
    // Documented `attributes.test_mode` on create-checkout: sandbox credentials must open a sandbox
    // checkout and live credentials a live one, so the flag is driven by the configured store mode.
    // No provider call is made - both cases run against the loopback stub and the response body is the
    // same fixture the response-shape tests already use.
    for expect_test_mode in [true, false] {
        let stub = StubServer::start(vec![(200, checkout_response())]).await;
        let mut config = config_with_base(Some(&stub.base_url));
        config.expect_test_mode = expect_test_mode;
        let client = LemonSqueezyClient::new(config).expect("client");

        let url = client
            .create_checkout(
                CheckoutPlanChoice::ProMonthly { packs: 1 },
                "user-1",
                None,
                None,
            )
            .await
            .expect("checkout");
        assert_eq!(url, CHECKOUT_URL);

        assert_eq!(stub.request_count(), 1);
        let body = stub.request(0).body.expect("checkout body");
        assert_eq!(
            body["data"]["attributes"]["test_mode"],
            json!(expect_test_mode),
            "expect_test_mode={expect_test_mode} must reach the checkout payload"
        );
    }
}

#[tokio::test]
async fn checkout_enables_only_the_selected_variant() {
    // Documented `product_options.enabled_variants`: "An array of variant IDs to enable for this checkout.
    // If this is empty, all variants will be enabled." Because the plan kind, the quantity axis and the
    // `custom_data` owner are all derived from the variant this request selected, the payload must pin
    // exactly that variant - otherwise the buyer can jump to another monthly/annual or seat/host-pack
    // variant and the webhook would apply metadata that no longer matches the purchase. The
    // `product_options` object carries that one key and nothing else (no extra defaults are injected).
    let cases: [(CheckoutPlanChoice, &str); 5] = [
        (
            CheckoutPlanChoice::ProMonthly { packs: 1 },
            VARIANT_PRO_MONTHLY,
        ),
        (
            CheckoutPlanChoice::ProAnnual { packs: 0 },
            VARIANT_PRO_ANNUAL,
        ),
        (
            CheckoutPlanChoice::TeamMonthly { seats: 3 },
            VARIANT_TEAM_MONTHLY,
        ),
        (
            CheckoutPlanChoice::TeamAnnual { seats: 2 },
            VARIANT_TEAM_ANNUAL,
        ),
        (
            CheckoutPlanChoice::TeamHostpackAnnual { packs: 2 },
            VARIANT_TEAM_HOSTPACK_ANNUAL,
        ),
    ];

    let stub = StubServer::start(
        (0..cases.len())
            .map(|_| (200u16, checkout_response()))
            .collect(),
    )
    .await;
    let client = client_for(&stub);

    for (index, (choice, expected_variant)) in cases.iter().enumerate() {
        client
            .create_checkout(*choice, "user-1", None, None)
            .await
            .unwrap_or_else(|error| panic!("checkout {index}: {error:?}"));

        let body = stub.request(index).body.expect("checkout body");
        let attributes = &body["data"]["attributes"];
        let product_options = attributes["product_options"]
            .as_object()
            .expect("product_options object");
        assert_eq!(
            product_options
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["enabled_variants"],
            "case {index}: nothing but the selected variant may be configured"
        );

        let selected = expected_variant.parse::<u64>().expect("numeric variant id");
        assert_eq!(
            product_options
                .get("enabled_variants")
                .expect("enabled_variants"),
            &json!([selected]),
            "case {index}: exactly the selected variant may be purchasable"
        );
        assert_eq!(
            attributes["checkout_data"]["variant_quantities"][0]["variant_id"],
            json!(selected),
            "case {index}: the quantity axis belongs to the selected variant"
        );
        assert_eq!(
            body["data"]["relationships"]["variant"]["data"]["id"],
            json!(*expected_variant),
            "case {index}: the checkout relationship is the selected variant"
        );
    }
    assert_eq!(stub.request_count(), cases.len());
}

#[tokio::test]
async fn checkout_omits_org_and_email_when_absent() {
    let stub = StubServer::start(vec![(200, checkout_response())]).await;
    let client = client_for(&stub);

    client
        .create_checkout(
            CheckoutPlanChoice::ProMonthly { packs: 1 },
            "user-1",
            None,
            Some(" "),
        )
        .await
        .expect("checkout");

    let body = stub.request(0).body.expect("checkout body");
    let custom = body["data"]["attributes"]["checkout_data"]["custom"]
        .as_object()
        .expect("custom object");
    assert_eq!(custom.get("user_id"), Some(&json!("user-1")));
    assert!(!custom.contains_key("org_id"));
    assert!(body["data"]["attributes"]["checkout_data"]
        .get("email")
        .is_none());
}

#[tokio::test]
async fn checkout_rejects_non_numeric_variant_without_a_request() {
    let stub = StubServer::start(Vec::new()).await;
    let mut config = config_with_base(Some(&stub.base_url));
    config.variant_pro_monthly = "not-a-number".to_string();
    let client = LemonSqueezyClient::new(config).expect("client");

    let error = client
        .create_checkout(
            CheckoutPlanChoice::ProMonthly { packs: 0 },
            "user-1",
            None,
            None,
        )
        .await
        .expect_err("variant id must be numeric");
    assert!(
        matches!(error, LemonSqueezyError::InvalidIdentifier(_)),
        "expected InvalidIdentifier, got {error:?}"
    );
    assert_eq!(stub.request_count(), 0);
}

#[tokio::test]
async fn checkout_requires_configured_store_and_variant() {
    let stub = StubServer::start(Vec::new()).await;

    let mut missing_variant = config_with_base(Some(&stub.base_url));
    missing_variant.variant_team_annual = "  ".to_string();
    let client = LemonSqueezyClient::new(missing_variant).expect("client");
    let error = client
        .create_checkout(
            CheckoutPlanChoice::TeamAnnual { seats: 2 },
            "user-1",
            None,
            None,
        )
        .await
        .expect_err("variant must be configured");
    assert!(
        matches!(error, LemonSqueezyError::MissingConfiguration(_)),
        "expected MissingConfiguration, got {error:?}"
    );

    let mut missing_store = config_with_base(Some(&stub.base_url));
    missing_store.store_id = String::new();
    let client = LemonSqueezyClient::new(missing_store).expect("client");
    let error = client
        .create_checkout(
            CheckoutPlanChoice::TeamAnnual { seats: 2 },
            "user-1",
            None,
            None,
        )
        .await
        .expect_err("store must be configured");
    assert!(
        matches!(error, LemonSqueezyError::MissingConfiguration(_)),
        "expected MissingConfiguration, got {error:?}"
    );

    assert_eq!(stub.request_count(), 0);
}

#[tokio::test]
async fn checkout_rejects_urls_outside_lemonsqueezy_hosts() {
    for url in [
        "http://ferryx.lemonsqueezy.com/checkout/buy/6c1d4a11",
        "https://evil.example.com/checkout/buy/6c1d4a11",
        "https://lemonsqueezy.com.evil.example.com/checkout/buy/6c1d4a11",
        "https://notlemonsqueezy.com/checkout/buy/6c1d4a11",
    ] {
        let mut response = checkout_response();
        response["data"]["attributes"]["url"] = json!(url);
        let stub = StubServer::start(vec![(200, response)]).await;
        let client = client_for(&stub);

        let error = client
            .create_checkout(
                CheckoutPlanChoice::ProMonthly { packs: 0 },
                "user-1",
                None,
                None,
            )
            .await
            .expect_err("checkout url must be an HTTPS Lemon Squeezy host");
        assert!(
            matches!(error, LemonSqueezyError::UntrustedUrl(_)),
            "url {url}: expected UntrustedUrl, got {error:?}"
        );
    }
}

#[tokio::test]
async fn quantity_update_reads_the_subscription_then_patches_the_item() {
    let stub = StubServer::start(vec![
        (200, subscription_response(3, "fresh-signature")),
        (200, item_update_response(4)),
    ])
    .await;
    let client = client_for(&stub);

    let quantity = client
        .update_subscription_quantity(SUBSCRIPTION_ID, 4)
        .await
        .expect("quantity update");
    assert_eq!(quantity, 4);

    assert_eq!(stub.request_count(), 2);
    let read = stub.request(0);
    assert_eq!(read.method, "GET");
    assert_eq!(read.path, format!("/subscriptions/{SUBSCRIPTION_ID}"));
    assert_eq!(read.authorization.as_deref(), Some(TEST_AUTHORIZATION));

    let patch = stub.request(1);
    assert_eq!(patch.method, "PATCH");
    assert_eq!(
        patch.path,
        format!("/subscription-items/{SUBSCRIPTION_ITEM_ID}")
    );
    assert_ne!(patch.path, format!("/subscriptions/{SUBSCRIPTION_ID}"));
    assert_eq!(patch.authorization.as_deref(), Some(TEST_AUTHORIZATION));

    let body = patch.body.expect("patch body");
    assert_eq!(body["data"]["type"], "subscription-items");
    assert_eq!(body["data"]["id"], SUBSCRIPTION_ITEM_ID);
    assert_eq!(body["data"]["attributes"]["quantity"], 4);
    assert_eq!(body["data"]["attributes"]["disable_prorations"], true);
    assert_eq!(body["data"]["attributes"]["invoice_immediately"], false);
}

#[tokio::test]
async fn quantity_update_sends_the_pro_pack_axis_quantity() {
    let stub = StubServer::start(vec![
        (200, subscription_response(1, "fresh-signature")),
        (200, item_update_response(4)),
    ])
    .await;
    let client = client_for(&stub);

    let packs = 3;
    let quantity = pro_subscription_quantity(packs).expect("quantity");
    client
        .update_subscription_quantity(SUBSCRIPTION_ID, quantity)
        .await
        .expect("quantity update");

    let body = stub.request(1).body.expect("patch body");
    assert_eq!(body["data"]["attributes"]["quantity"], 4);
}

#[tokio::test]
async fn quantity_update_rejects_out_of_bounds_before_any_request() {
    let stub = StubServer::start(Vec::new()).await;
    let client = client_for(&stub);

    for invalid in [0, MAX_QUANTITY + 1] {
        let error = client
            .update_subscription_item_quantity(SUBSCRIPTION_ITEM_ID, invalid)
            .await
            .expect_err("quantity must be bounded");
        assert!(
            matches!(error, LemonSqueezyError::InvalidQuantity(_)),
            "quantity {invalid}: expected InvalidQuantity, got {error:?}"
        );
    }
    assert!(matches!(
        client
            .update_subscription_item_quantity("../subscriptions", 2)
            .await
            .expect_err("item id must be numeric"),
        LemonSqueezyError::InvalidIdentifier(_)
    ));
    assert_eq!(stub.request_count(), 0);
}

#[tokio::test]
async fn quantity_update_rejects_a_foreign_store_without_patching() {
    let mut response = subscription_response(3, "fresh-signature");
    response["data"]["attributes"]["store_id"] = json!(9999);
    let stub = StubServer::start(vec![(200, response)]).await;
    let client = client_for(&stub);

    let error = client
        .update_subscription_quantity(SUBSCRIPTION_ID, 4)
        .await
        .expect_err("foreign store");
    assert!(
        matches!(error, LemonSqueezyError::UnsupportedStore(_)),
        "expected UnsupportedStore, got {error:?}"
    );
    assert_eq!(stub.request_count(), 1);
}

#[tokio::test]
async fn quantity_update_reports_a_missing_subscription_item() {
    let mut response = subscription_response(3, "fresh-signature");
    response["data"]["attributes"]
        .as_object_mut()
        .expect("attributes")
        .remove("first_subscription_item");
    let stub = StubServer::start(vec![(200, response)]).await;
    let client = client_for(&stub);

    let error = client
        .update_subscription_quantity(SUBSCRIPTION_ID, 4)
        .await
        .expect_err("no subscription item");
    assert!(
        matches!(error, LemonSqueezyError::SubscriptionItemNotFound(_)),
        "expected SubscriptionItemNotFound, got {error:?}"
    );
    assert_eq!(stub.request_count(), 1);
}

#[tokio::test]
async fn reads_reject_non_numeric_ids_without_a_request() {
    let stub = StubServer::start(Vec::new()).await;
    let client = client_for(&stub);

    for id in ["", " ", "../../subscriptions/1", "https://evil.example.com"] {
        let error = client
            .get_subscription(id)
            .await
            .expect_err("id must be numeric");
        assert!(
            matches!(error, LemonSqueezyError::InvalidIdentifier(_)),
            "id {id:?}: expected InvalidIdentifier, got {error:?}"
        );
    }
    assert_eq!(stub.request_count(), 0);
}

#[tokio::test]
async fn api_failure_carries_status_and_a_sanitized_message() {
    let stub = StubServer::start(vec![(
        400,
        json!({ "errors": [{ "detail": "subscription not found" }] }),
    )])
    .await;
    let client = client_for(&stub);

    let error = client
        .get_subscription(SUBSCRIPTION_ID)
        .await
        .expect_err("400 is a failure");
    match error {
        LemonSqueezyError::ApiFailure { status, message } => {
            assert_eq!(status, 400);
            assert!(
                message.contains("subscription not found"),
                "message: {message}"
            );
        }
        other => panic!("expected ApiFailure, got {other:?}"),
    }

    let unexpected = json!({
        "card": { "number": "4242 4242 4242 4242" },
        "url": "https://ferryx.lemonsqueezy.com/billing?signature=leaky",
        "blob": "x".repeat(4_000)
    });
    let stub = StubServer::start(vec![(500, unexpected)]).await;
    let client = client_for(&stub);
    let error = client
        .get_subscription(SUBSCRIPTION_ID)
        .await
        .expect_err("500 is a failure");
    let message = error.to_string();
    for leaked in ["4242", "leaky", "xxx"] {
        assert!(!message.contains(leaked), "body leaked into {message}");
    }
    assert!(message.len() < 200, "message length {}", message.len());
}

#[tokio::test]
async fn api_failure_surfaces_only_the_documented_error_detail() {
    let body = json!({
        "errors": [{ "title": "Unprocessable Entity", "detail": "quantity is out of range\u{7}" }]
    });
    let stub = StubServer::start(vec![(422, body)]).await;
    let client = client_for(&stub);

    let error = client
        .update_subscription_item_quantity(SUBSCRIPTION_ITEM_ID, 2)
        .await
        .expect_err("422 is a failure");
    match error {
        LemonSqueezyError::ApiFailure { status, message } => {
            assert_eq!(status, 422);
            assert_eq!(message, "quantity is out of range");
        }
        other => panic!("expected ApiFailure, got {other:?}"),
    }
}

#[tokio::test]
async fn api_subscription_with_an_invalid_timestamp_is_rejected() {
    let mut response = subscription_response(3, "fresh-signature");
    response["data"]["attributes"]["updated_at"] = json!("2026-09-30 00:00:00");
    let stub = StubServer::start(vec![(200, response)]).await;
    let client = client_for(&stub);

    let error = client
        .get_subscription(SUBSCRIPTION_ID)
        .await
        .expect_err("invalid timestamp");
    assert!(
        matches!(error, LemonSqueezyError::InvalidEvent(_)),
        "got {error:?}"
    );
}

#[tokio::test]
async fn portal_url_refresh_returns_the_fresh_signed_url() {
    let stub =
        StubServer::start(vec![(200, subscription_response(3, "refreshed-signature"))]).await;
    let client = client_for(&stub);

    let url = client
        .refresh_portal_url(SUBSCRIPTION_ID)
        .await
        .expect("portal url");
    assert!(url.contains("refreshed-signature"));
    assert!(!url.contains("82ae290ceac8edd4190c82825dd73a8743346d894a8ddbc4898b97eb96d105a5"));

    let read = stub.request(0);
    assert_eq!(read.method, "GET");
    assert_eq!(read.path, format!("/subscriptions/{SUBSCRIPTION_ID}"));
    assert_eq!(stub.request_count(), 1);
}

#[tokio::test]
async fn portal_url_refresh_fails_when_the_stored_url_is_absent() {
    let mut response = subscription_response(3, "unused");
    response["data"]["attributes"]["urls"]
        .as_object_mut()
        .expect("urls")
        .remove("customer_portal");
    let stub = StubServer::start(vec![(200, response)]).await;
    let client = client_for(&stub);

    let error = client
        .refresh_portal_url(SUBSCRIPTION_ID)
        .await
        .expect_err("no portal url");
    assert!(
        matches!(error, LemonSqueezyError::PortalUrlUnavailable(_)),
        "expected PortalUrlUnavailable, got {error:?}"
    );
}

#[tokio::test]
async fn portal_url_refresh_rejects_urls_outside_lemonsqueezy_hosts() {
    for url in [
        "http://ferryx.lemonsqueezy.com/billing?expires=1790000100&signature=fresh-signature",
        "https://evil.example.com/billing?expires=1790000100&signature=fresh-signature",
        "https://lemonsqueezy.com.evil.example.com/billing?signature=fresh-signature",
    ] {
        let mut response = subscription_response(3, "fresh-signature");
        response["data"]["attributes"]["urls"]["customer_portal"] = json!(url);
        let stub = StubServer::start(vec![(200, response)]).await;
        let client = client_for(&stub);

        let error = client
            .refresh_portal_url(SUBSCRIPTION_ID)
            .await
            .expect_err("portal url must be an HTTPS Lemon Squeezy host");
        assert!(
            matches!(error, LemonSqueezyError::UntrustedUrl(_)),
            "url {url}: expected UntrustedUrl, got {error:?}"
        );
    }
}

#[tokio::test]
async fn invoice_events_resolve_authority_through_the_subscription_id_attribute() {
    let event = parsed_invoice_event();
    assert_eq!(event.invoice_id.as_deref(), Some("99001"));

    let stub = StubServer::start(vec![(200, subscription_response(3, "fresh-signature"))]).await;
    let client = client_for(&stub);

    let details = client
        .fetch_authoritative_subscription(&event)
        .await
        .expect("authoritative subscription");

    assert_eq!(stub.request_count(), 1);
    let read = stub.request(0);
    assert_eq!(read.method, "GET");
    assert_eq!(read.path, format!("/subscriptions/{SUBSCRIPTION_ID}"));
    assert_ne!(read.path, "/subscriptions/99001");

    assert_eq!(details.subscription_id, SUBSCRIPTION_ID);
    assert_eq!(details.item_id, SUBSCRIPTION_ITEM_ID);
    assert_eq!(details.variant_id, VARIANT_PRO_MONTHLY);
    assert_eq!(details.quantity, 3);
    assert_eq!(details.status, SubscriptionStatus::Active);
    assert_eq!(details.store_id, TEST_STORE_ID);
    assert!(details.test_mode);
}

#[tokio::test]
async fn authoritative_subscription_is_validated_against_the_store() {
    let event = parsed_subscription_event();

    let mut foreign_store = subscription_response(3, "fresh-signature");
    foreign_store["data"]["attributes"]["store_id"] = json!(9999);
    let stub = StubServer::start(vec![(200, foreign_store)]).await;
    let client = client_for(&stub);
    let error = client
        .fetch_authoritative_subscription(&event)
        .await
        .expect_err("foreign store");
    assert!(
        matches!(error, LemonSqueezyError::UnsupportedStore(_)),
        "expected UnsupportedStore, got {error:?}"
    );

    let mut unknown_variant = subscription_response(3, "fresh-signature");
    unknown_variant["data"]["attributes"]["variant_id"] = json!(777777);
    let stub = StubServer::start(vec![(200, unknown_variant)]).await;
    let client = client_for(&stub);
    let error = client
        .fetch_authoritative_subscription(&event)
        .await
        .expect_err("unknown variant");
    assert!(
        matches!(error, LemonSqueezyError::UnsupportedVariant(_)),
        "expected UnsupportedVariant, got {error:?}"
    );
}

#[tokio::test]
async fn client_does_not_follow_redirects() {
    let target = StubServer::start(vec![(200, subscription_response(3, "sig"))]).await;
    let redirecting = StubServer::start_redirecting(format!(
        "{}/subscriptions/{SUBSCRIPTION_ID}",
        target.base_url
    ))
    .await;
    let client = client_for(&redirecting);

    let error = client
        .get_subscription(SUBSCRIPTION_ID)
        .await
        .expect_err("a 302 must not be followed");
    match error {
        LemonSqueezyError::ApiFailure { status, .. } => assert_eq!(status, 302),
        other => panic!("expected ApiFailure with status 302, got {other:?}"),
    }
    assert_eq!(redirecting.request_count(), 1);
    assert_eq!(
        target.request_count(),
        0,
        "the redirect target must never receive the request or its Authorization header"
    );
}

#[tokio::test]
async fn subscription_response_must_carry_the_requested_id() {
    let mut missing_id = subscription_response(3, "sig");
    missing_id["data"]
        .as_object_mut()
        .expect("data")
        .remove("id");
    let stub = StubServer::start(vec![(200, missing_id)]).await;
    let client = client_for(&stub);
    assert!(matches!(
        client
            .get_subscription(SUBSCRIPTION_ID)
            .await
            .expect_err("missing id"),
        LemonSqueezyError::InvalidEvent(_)
    ));

    let mut non_numeric_id = subscription_response(3, "sig");
    non_numeric_id["data"]["id"] = json!("abc");
    let stub = StubServer::start(vec![(200, non_numeric_id)]).await;
    let client = client_for(&stub);
    assert!(matches!(
        client
            .get_subscription(SUBSCRIPTION_ID)
            .await
            .expect_err("non numeric id"),
        LemonSqueezyError::InvalidIdentifier(_)
    ));

    let mut wrong_id = subscription_response(3, "sig");
    wrong_id["data"]["id"] = json!("999");
    let stub = StubServer::start(vec![(200, wrong_id)]).await;
    let client = client_for(&stub);
    assert!(matches!(
        client
            .get_subscription(SUBSCRIPTION_ID)
            .await
            .expect_err("wrong subscription"),
        LemonSqueezyError::ResourceMismatch { .. }
    ));
}

#[tokio::test]
async fn quantity_update_rejects_malformed_success_bodies() {
    let mut missing_data = item_update_response(4);
    missing_data.as_object_mut().expect("root").remove("data");

    let mut wrong_type = item_update_response(4);
    wrong_type["data"]["type"] = json!("subscriptions");

    let mut wrong_id = item_update_response(4);
    wrong_id["data"]["id"] = json!("999");

    let mut missing_quantity = item_update_response(4);
    missing_quantity["data"]["attributes"]
        .as_object_mut()
        .expect("attributes")
        .remove("quantity");

    let mut non_numeric_quantity = item_update_response(4);
    non_numeric_quantity["data"]["attributes"]["quantity"] = json!("four");

    let cases: Vec<(Value, fn(&LemonSqueezyError) -> bool)> = vec![
        (missing_data, |error| {
            matches!(error, LemonSqueezyError::InvalidEvent(_))
        }),
        (wrong_type, |error| {
            matches!(error, LemonSqueezyError::InvalidEvent(_))
        }),
        (wrong_id, |error| {
            matches!(error, LemonSqueezyError::ResourceMismatch { .. })
        }),
        (missing_quantity, |error| {
            matches!(error, LemonSqueezyError::InvalidQuantity(_))
        }),
        (non_numeric_quantity, |error| {
            matches!(error, LemonSqueezyError::InvalidQuantity(_))
        }),
    ];

    for (index, (body, matches_expected)) in cases.into_iter().enumerate() {
        let stub = StubServer::start(vec![(200, body)]).await;
        let client = client_for(&stub);

        let error = client
            .update_subscription_item_quantity(SUBSCRIPTION_ITEM_ID, 4)
            .await
            .expect_err("a malformed success body must not report success");
        assert!(matches_expected(&error), "case {index}: got {error:?}");
    }
}
