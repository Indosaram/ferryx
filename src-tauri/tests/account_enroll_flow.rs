use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::account::mailer::FileMailer;
use ferryx_lib::account::service::AccountState;
use ferryx_lib::account::store::{now_secs, AccountStore};
use ferryx_lib::remote::auth::{enrollment_code_hash, sign_account_enrollment, MachineIdentity};
use serde_json::{json, Value};

const ORIGIN: &str = "https://account.example";

struct Server {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn spawn(state: AccountState) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        let _ = ferryx_lib::account::service::serve(listener, Arc::new(state)).await;
    });
    Server {
        base: format!("http://{addr}"),
        handle,
    }
}

async fn post(base: &str, path: &str, body: Value, bearer: Option<&str>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{base}{path}")).json(&body);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.expect("request");
    let status = response.status().as_u16();
    let text = response.text().await.expect("body");
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    };
    (status, value)
}

fn read_magic_link(mail_dir: &std::path::Path) -> String {
    let entry = std::fs::read_dir(mail_dir)
        .expect("mail dir")
        .filter_map(Result::ok)
        .find(|entry| entry.path().is_file())
        .expect("a mail file");
    std::fs::read_to_string(entry.path()).expect("mail body")
}

fn code_from(text: &str) -> String {
    text.split("code=")
        .nth(1)
        .expect("code param")
        .trim()
        .to_string()
}

fn clear_mail_dir(mail_dir: &std::path::Path) {
    for entry in std::fs::read_dir(mail_dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}

async fn sign_in(server: &Server, mail_dir: &std::path::Path, email: &str) -> String {
    clear_mail_dir(mail_dir);
    let (status, _) = post(
        &server.base,
        "/api/account/v1/login/request",
        json!({ "email": email }),
        None,
    )
    .await;
    assert_eq!(status, 202);
    let code = code_from(&read_magic_link(mail_dir));
    let (status, body) = post(
        &server.base,
        "/api/account/v1/login/consume",
        json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body["token"].as_str().expect("token").to_string()
}

async fn issue_enrollment_code(server: &Server, token: &str) -> String {
    let (status, body) = post(
        &server.base,
        "/api/account/v1/enrollment-codes",
        json!({}),
        Some(token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body["code"].as_str().expect("code").to_string()
}

fn machine_identity(machine_id: &str) -> MachineIdentity {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    MachineIdentity {
        machine_id: machine_id.to_string(),
        display_name: "build box".into(),
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    }
}

fn attach_public_key() -> String {
    STANDARD.encode([7u8; 32])
}

async fn challenge_for(server: &Server, machine_id: &str) -> (String, u64) {
    let (status, challenge) = post(
        &server.base,
        "/api/account/v1/machines/enroll/challenge",
        json!({ "machineId": machine_id }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{challenge}");
    (
        challenge["nonce"].as_str().expect("nonce").to_string(),
        now_secs(),
    )
}

async fn enroll(
    server: &Server,
    identity: &MachineIdentity,
    code: &str,
    origin: &str,
) -> (u16, Value) {
    enroll_with_attach(server, identity, code, origin, &attach_public_key()).await
}

async fn enroll_with_attach(
    server: &Server,
    identity: &MachineIdentity,
    code: &str,
    origin: &str,
    attach_public_key: &str,
) -> (u16, Value) {
    let (nonce, timestamp) = challenge_for(server, &identity.machine_id).await;
    let signature = sign_account_enrollment(
        identity,
        origin,
        &enrollment_code_hash(code),
        &nonce,
        timestamp,
    )
    .expect("sign");
    post(
        &server.base,
        "/api/account/v1/machines/enroll",
        json!({
            "apiVersion": 1,
            "enrollmentCode": code,
            "machineId": identity.machine_id,
            "displayName": identity.display_name,
            "publicKey": identity.public_key,
            "attachPublicKey": attach_public_key,
            "platform": "linux",
            "appVersion": "test",
            "nonce": nonce,
            "timestamp": timestamp,
            "signature": signature,
        }),
        None,
    )
    .await
}

#[tokio::test]
async fn account_enroll_flow() {
    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let state = AccountState::new(
        data_dir.path(),
        ORIGIN,
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;

    let token = sign_in(&server, mail_dir.path(), "owner@b.co").await;
    let other_token = sign_in(&server, mail_dir.path(), "other@b.co").await;

    let identity = machine_identity("machine-one");
    let code = issue_enrollment_code(&server, &token).await;
    let (status, body) = enroll(&server, &identity, &code, ORIGIN).await;
    assert_eq!(status, 200, "{body}");
    let machine_record_id = body["machineRecordId"].as_str().expect("record").to_string();
    assert_eq!(body["enrollmentEpoch"], "1");
    assert_eq!(body["relayOrigin"], "https://relay.checka.cc");

    let (status, body) = enroll(&server, &identity, &code, ORIGIN).await;
    assert_eq!(status, 401, "a spent enrollment code cannot be reused: {body}");
    assert_eq!(body["code"], "ENROLL_CODE_INVALID");

    let code = issue_enrollment_code(&server, &token).await;
    let (status, body) = enroll(&server, &identity, &code, ORIGIN).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["enrollmentEpoch"], "2",
        "the same owner re-enrolling bumps the epoch"
    );
    assert_eq!(body["machineRecordId"], machine_record_id.as_str());

    let code = issue_enrollment_code(&server, &other_token).await;
    let (status, body) = enroll(&server, &identity, &code, ORIGIN).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "ACCOUNT_MACHINE_CLAIMED");

    let store = AccountStore::load(data_dir.path()).expect("store");
    assert_eq!(store.machines.len(), 1);
    let raw = std::fs::read_to_string(data_dir.path().join("account-store.json")).expect("raw");
    assert!(
        !raw.contains(&identity.private_key),
        "machine private keys never reach the account store"
    );
}

#[tokio::test]
async fn account_grant_is_pairing_capability() {
    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let state = AccountState::new(
        data_dir.path(),
        ORIGIN,
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;

    let token = sign_in(&server, mail_dir.path(), "owner@b.co").await;
    let other_token = sign_in(&server, mail_dir.path(), "other@b.co").await;

    let identity = machine_identity("machine-two");
    let code = issue_enrollment_code(&server, &token).await;
    let (status, body) = enroll(&server, &identity, &code, ORIGIN).await;
    assert_eq!(status, 200, "{body}");
    let machine_record_id = body["machineRecordId"].as_str().expect("record").to_string();
    let epoch = body["enrollmentEpoch"].as_str().expect("epoch").to_string();

    let grants_path = format!("/api/account/v1/machines/{machine_record_id}/grants");
    let request = json!({
        "machineRecordId": machine_record_id,
        "enrollmentEpoch": epoch,
        "deviceLabel": "MacBook",
        "installationId": "install-1",
        "grantScope": "machine",
        "attachPublicKey": attach_public_key(),
    });

    let (status, body) = post(&server.base, &grants_path, request.clone(), None).await;
    assert_eq!(status, 401, "{body}");

    let (status, body) = post(&server.base, &grants_path, request.clone(), Some(&other_token)).await;
    assert_eq!(status, 404, "another account cannot see this machine: {body}");

    let (status, body) = post(&server.base, &grants_path, request.clone(), Some(&token)).await;
    assert_eq!(status, 200, "{body}");
    let pairing_token = body["pairingToken"].as_str().expect("pairing token");
    assert_eq!(pairing_token.len(), 64);
    assert!(pairing_token
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    assert!(body.get("deviceToken").is_none());
    assert!(body.get("token").is_none());
    assert_eq!(body["machineAttachPublicKey"], attach_public_key());
    assert_eq!(body["grantScope"], "machine");
    assert!(body["expiresAt"].as_u64().expect("expiry") <= now_secs() + 600);

    let mut stale = request.clone();
    stale["enrollmentEpoch"] = json!("99");
    let (status, body) = post(&server.base, &grants_path, stale, Some(&token)).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "ACCOUNT_ENROLLMENT_EPOCH_MISMATCH");

    let store = AccountStore::load(data_dir.path()).expect("store");
    assert_eq!(store.grants.len(), 1, "a rejected grant writes nothing");
    let raw = std::fs::read_to_string(data_dir.path().join("account-store.json")).expect("raw");
    assert!(
        !raw.contains(pairing_token),
        "the pairing capability is stored hashed, never in the clear"
    );
}

#[tokio::test]
async fn account_enroll_replay_and_bad_signature_are_rejected() {
    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let state = AccountState::new(
        data_dir.path(),
        ORIGIN,
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;
    let token = sign_in(&server, mail_dir.path(), "owner@b.co").await;

    let identity = machine_identity("machine-three");
    let code = issue_enrollment_code(&server, &token).await;
    let (nonce, timestamp) = challenge_for(&server, &identity.machine_id).await;
    let signature = sign_account_enrollment(
        &identity,
        ORIGIN,
        &enrollment_code_hash(&code),
        &nonce,
        timestamp,
    )
    .expect("sign");
    let body = json!({
        "apiVersion": 1,
        "enrollmentCode": code,
        "machineId": identity.machine_id,
        "displayName": identity.display_name,
        "publicKey": identity.public_key,
        "attachPublicKey": attach_public_key(),
        "platform": "windows",
        "appVersion": "test",
        "nonce": nonce,
        "timestamp": timestamp,
        "signature": signature,
    });

    let (status, response) = post(&server.base, "/api/account/v1/machines/enroll", body.clone(), None).await;
    assert_eq!(status, 200, "{response}");

    let (status, _) = post(&server.base, "/api/account/v1/machines/enroll", body.clone(), None).await;
    assert_eq!(status, 401, "the same nonce cannot be spent twice");

    let other = machine_identity("machine-four");
    let code = issue_enrollment_code(&server, &token).await;
    let (nonce, timestamp) = challenge_for(&server, &other.machine_id).await;
    let forged = sign_account_enrollment(
        &identity,
        ORIGIN,
        &enrollment_code_hash(&code),
        &nonce,
        timestamp,
    )
    .expect("sign with the wrong key");
    let (status, body) = post(
        &server.base,
        "/api/account/v1/machines/enroll",
        json!({
            "apiVersion": 1,
            "enrollmentCode": code,
            "machineId": other.machine_id,
            "displayName": other.display_name,
            "publicKey": other.public_key,
            "attachPublicKey": attach_public_key(),
            "platform": "macos",
            "appVersion": "test",
            "nonce": nonce,
            "timestamp": timestamp,
            "signature": forged,
        }),
        None,
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["code"], "ENROLL_SIGNATURE_INVALID");

    let store = AccountStore::load(data_dir.path()).expect("store");
    assert_eq!(store.machines.len(), 1);
    assert_eq!(
        store.machines.values().next().expect("machine").platform,
        "windows"
    );
}

#[tokio::test]
async fn account_grant_seals_an_offer_the_daemon_can_open() {
    use ferryx_lib::account::offer_sink::{apply_grant_offer, open_grant_offer};
    use ferryx_lib::remote::account_protocol::AccountGrantOfferEnvelope;
    use ferryx_lib::remote::attach_identity::AttachIdentity;
    use ferryx_lib::remote::auth::{AuthManager, DeviceAccessScope};

    let data_dir = tempfile::tempdir().expect("data");
    let mail_dir = tempfile::tempdir().expect("mail");
    let manager_dir = tempfile::tempdir().expect("manager");
    let state = AccountState::new(
        data_dir.path(),
        ORIGIN,
        Arc::new(FileMailer::with_dir(mail_dir.path())),
    );
    let server = spawn(state).await;
    let token = sign_in(&server, mail_dir.path(), "owner@b.co").await;

    let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
    let attach_public = STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes());
    let attach = AttachIdentity {
        public_key: attach_public.clone(),
        private_key: STANDARD.encode(secret.to_bytes()),
    };

    let identity = machine_identity("machine-sealed");
    let code = issue_enrollment_code(&server, &token).await;
    let (status, body) = enroll_with_attach(&server, &identity, &code, ORIGIN, &attach_public).await;
    assert_eq!(status, 200, "{body}");
    let machine_record_id = body["machineRecordId"].as_str().expect("record").to_string();
    let epoch = body["enrollmentEpoch"].as_str().expect("epoch").to_string();

    let grants_path = format!("/api/account/v1/machines/{machine_record_id}/grants");
    let (status, body) = post(
        &server.base,
        &grants_path,
        json!({
            "machineRecordId": machine_record_id,
            "enrollmentEpoch": epoch,
            "deviceLabel": "MacBook",
            "installationId": "install-1",
            "grantScope": "machine",
            "attachPublicKey": attach_public,
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let envelope: AccountGrantOfferEnvelope =
        serde_json::from_value(body["sealedOffer"].clone()).expect("sealed offer carried");

    let now = now_secs();
    let offer = open_grant_offer(&attach, &envelope, &identity.machine_id, &epoch, now)
        .expect("the machine opens the offer the account sealed");
    assert_eq!(offer.device_label, "MacBook");
    assert!(
        !envelope.sealed.contains(&offer.pairing_token),
        "the relay payload never carries the token in the clear"
    );

    let auth = AuthManager::with_persistence(Some(manager_dir.path().join("remote-auth.json")));
    let ack = apply_grant_offer(&auth, &attach, &identity.machine_id, &epoch, &envelope, now)
        .expect("the machine registers the capability");
    assert_eq!(ack.status, "ready");

    let (_bearer, device) = auth
        .exchange_pairing_code_with_installation(&offer.pairing_token, "MacBook", Some("install-1"))
        .expect("the sealed grant redeems into a device");
    assert_eq!(device.access_scope, DeviceAccessScope::Machine);
}
