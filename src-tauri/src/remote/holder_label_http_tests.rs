//! Wire contract for the holder label on a typed `CONTROL_CONFLICT` refusal.
//!
//! Occupancy returns `409`, with legacy code/name lines or a negotiated JSON envelope.
//! The holder name comes from `AuthManager::live_device`, so it is
//! display text only: it never proves identity and never feeds the admission decision.

use crate::daemon::session_service::DaemonSessionService;
use crate::remote::server::machine_input_fixture::Fixture;
use crate::remote::{DeviceAccessScope, DevicePermission};
use futures_util::FutureExt;
use std::time::Duration;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error};

const DEADLINE: Duration = Duration::from_secs(15);
const CODE_LINE: &str = "CONTROL_CONFLICT";
const UNNAMED_DEVICE: &str = "Remote Device";

async fn refusal_body_for_holder_named(requested_name: &str) -> (String, String) {
    refusal_body_for_holder_named_with(requested_name, false, false).await
}

async fn refusal_body_for_holder_named_with(
    requested_name: &str,
    revoke_holder: bool,
    json_refusal: bool,
) -> (String, String) {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap().to_string();
        let epoch = session["target"]["daemonEpoch"].as_str().unwrap().to_string();
        let code = fixture
            .state
            .auth_manager
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (_, holder) = fixture
            .state
            .auth_manager
            .exchange_pairing_code(&code, requested_name)
            .unwrap();
        let registered = fixture
            .state
            .auth_manager
            .live_device(&holder.id)
            .expect("a freshly paired holder is live")
            .name;
        // Reserving without a socket keeps the occupancy deterministic: a real holder socket
        // releases its controller reservation on a teardown schedule no assertion can observe.
        let services = fixture.state.machine_services.as_ref().unwrap();
        let lease = DaemonSessionService::acquire_machine_controller(
            &mut *services.sessions.machine_controllers.lock().await,
            &id,
            &holder.id,
        )
        .unwrap();
        if revoke_holder {
            fixture
                .state
                .auth_manager
                .revoke_device(&holder.id)
                .unwrap();
        }
        let mut request = format!(
            "{}/api/v1/terminal/{id}?daemonEpoch={epoch}",
            fixture.base.replace("http:", "ws:")
        )
        .into_client_request()
        .unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", fixture.token).parse().unwrap(),
        );
        if json_refusal {
            request.headers_mut().insert("x-ferryx-refusal-format", "json".parse().unwrap());
        }
        let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
            .await
            .expect("bounded occupancy handshake");
        let Err(Error::Http(response)) = result else {
            panic!("a peer control device must be refused, not attached");
        };
        assert_eq!(response.status().as_u16(), 409);
        if json_refusal {
            assert_eq!(response.headers()["content-type"], "application/json");
        }
        assert!(
            !*lease.cancelled.borrow(),
            "a refusal must not displace the holder"
        );
        let body =
            String::from_utf8_lossy(response.body().as_deref().unwrap_or_default()).into_owned();
        if !revoke_holder {
            fixture
                .state
                .auth_manager
                .revoke_device(&holder.id)
                .unwrap();
        }
        (registered, body)
    })
    .catch_unwind()
    .await;
    fixture.cleanup().await;
    match outcome {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[tokio::test]
async fn negotiated_conflict_returns_json_with_a_display_only_holder() {
    let requested = "Controller laptop";
    let (_, body) = refusal_body_for_holder_named_with(requested, false, true).await;
    let envelope: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(envelope["code"], CODE_LINE);
    assert_eq!(envelope["details"]["holder"], requested);
    assert_eq!(envelope["details"]["takeover"], true);
}

#[tokio::test]
async fn control_conflict_names_the_pairing_registered_holder() {
    let requested = "Continuity holder laptop";
    let (registered, body) = refusal_body_for_holder_named(requested).await;
    assert_eq!(registered, requested);
    let mut lines = body.lines();
    assert_eq!(
        lines.next(),
        Some(CODE_LINE),
        "the typed code must stay the first line: {body:?}"
    );
    assert_eq!(
        lines.next(),
        Some(requested),
        "the detail line is the holder's registered name: {body:?}"
    );
    assert_eq!(lines.next(), None, "nothing follows the holder name: {body:?}");
}

#[tokio::test]
async fn control_conflict_names_remote_device_when_pairing_carried_no_name() {
    for requested in ["", "   "] {
        let (registered, body) = refusal_body_for_holder_named(requested).await;
        assert_eq!(registered, UNNAMED_DEVICE, "requested {requested:?}");
        assert_eq!(
            body,
            format!("CONTROL_CONFLICT\n{UNNAMED_DEVICE}"),
            "requested {requested:?}"
        );
    }
}

#[tokio::test]
async fn control_conflict_names_at_most_the_registration_name_limit() {
    let requested = "Holder ".repeat(50);
    let (registered, body) = refusal_body_for_holder_named(&requested).await;
    assert_eq!(registered.chars().count(), 100, "registered: {registered:?}");
    assert_eq!(body, format!("CONTROL_CONFLICT\n{registered}"));
    assert_eq!(body.lines().count(), 2, "one code line plus one name line");
}

#[tokio::test]
async fn a_holder_name_cannot_forge_the_typed_code_line() {
    let (registered, body) = refusal_body_for_holder_named("Evil\nFAKE_CODE").await;
    // Registration only trims and caps a name, so the newline survives registration: the
    // refusal folds it out, and the code line stays the one the gateway wrote.
    assert_eq!(registered, "Evil\nFAKE_CODE");
    assert_eq!(
        body,
        "CONTROL_CONFLICT\nEvilFAKE_CODE",
        "an injected line break must not create a second code line: {body:?}"
    );
    assert_eq!(body.lines().count(), 2, "one code line plus one name line");
}

#[tokio::test]
async fn a_holder_name_arrives_without_control_or_invisible_characters() {
    let (registered, body) =
        refusal_body_for_holder_named("Phone\u{1b}[31mred\u{0}\u{202e}\u{2066}").await;
    assert_eq!(registered, "Phone\u{1b}[31mred\u{0}\u{202e}\u{2066}");
    assert_eq!(
        body, "CONTROL_CONFLICT\nPhone[31mred",
        "display text carries no escape, NUL or bidi character: {body:?}"
    );
    // The code/name separator is a newline, so the body itself always carries one control character.
    let name_line = body.lines().nth(1).unwrap_or_default();
    assert!(
        !name_line.chars().any(|ch| ch.is_control()),
        "the holder name line must carry no control characters: {name_line:?}"
    );
}

#[tokio::test]
async fn a_holder_name_of_only_invisible_characters_leaves_a_code_only_refusal() {
    let (registered, body) = refusal_body_for_holder_named("\u{0}\u{202e}\u{200b}").await;
    assert_eq!(registered, "\u{0}\u{202e}\u{200b}");
    assert_eq!(
        body, CODE_LINE,
        "nothing printable means no name line at all: {body:?}"
    );
}

#[tokio::test]
async fn control_conflict_falls_back_when_the_holder_device_is_gone() {
    let (registered, body) =
        refusal_body_for_holder_named_with("Continuity holder laptop", true, false).await;
    assert_eq!(registered, "Continuity holder laptop");
    assert_eq!(
        body, "CONTROL_CONFLICT\nanother device",
        "a holder that is no longer live leaves no name to show"
    );
}

#[tokio::test]
async fn owner_hop_preserves_negotiated_conflict() {
    use crate::daemon::protocol::DAEMON_PROTOCOL_VERSION;
    use crate::daemon::proxy::LegacyPeer;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const ID: &str = "held-by-predecessor";
    const PLAIN: &str = "CONTROL_CONFLICT\nOther phone";
    const JSON: &str = r#"{"code":"CONTROL_CONFLICT","details":{"holder":"Other phone","takeover":false}}"#;
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let path = fixture.root.path().join("owner.sock");
        #[cfg(unix)]
        let owner = {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(fixture.root.path(), std::fs::Permissions::from_mode(0o700)).await.unwrap();
            tokio::net::UnixListener::bind(&path).unwrap()
        };
        #[cfg(not(unix))]
        let owner = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tokio::fs::write(&path, listener.local_addr().unwrap().port().to_string()).await.unwrap();
            listener
        };
        fixture.state.machine_services.as_ref().unwrap().sessions.router()
            .add_legacy_peer(Arc::new(LegacyPeer::new(path, vec![ID.into()])));

        let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().unwrap();
        for (opt_in, owner_knows_json) in [(false, true), (true, false), (true, true)] {
            let (mime, body) = if opt_in && owner_knows_json { ("application/json", JSON) } else { ("text/plain", PLAIN) };
            let serve = async {
                let (stream, _) = owner.accept().await.unwrap();
                let mut io = BufReader::new(stream);
                let hello = serde_json::json!({"type":"handshakeOk", "version":DAEMON_PROTOCOL_VERSION, "pid":1, "epoch":1});
                for (kind, reply) in [("handshake", hello), ("machineGateway", serde_json::json!({"type":"machineGatewayOk"}))] {
                    let mut line = String::new();
                    io.read_line(&mut line).await.unwrap();
                    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(request["type"], kind);
                    io.get_mut().write_all(format!("{reply}\n").as_bytes()).await.unwrap();
                }
                let mut line = String::new();
                io.read_line(&mut line).await.unwrap();
                assert_eq!(line.trim_end(), format!("GET /api/v1/terminal/{ID}?daemonEpoch=1&render=grid HTTP/1.1"));
                let (mut authorized, mut requested_json) = (false, false);
                loop {
                    line.clear();
                    assert!(io.read_line(&mut line).await.unwrap() > 0, "owner request ended before headers");
                    if line == "\r\n" { break; }
                    let (name, value) = line.split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("authorization") {
                        authorized = value.trim().strip_prefix("Bearer ") == Some(fixture.token.as_str());
                    } else if name.eq_ignore_ascii_case("x-ferryx-refusal-format") {
                        requested_json = value.trim() == "json";
                    }
                }
                assert!(authorized, "owner must receive the authenticated request");
                assert_eq!(requested_json, opt_in);
                let reply = format!("HTTP/1.1 409 Conflict\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                io.get_mut().write_all(reply.as_bytes()).await.unwrap();
                let _ = io.get_mut().shutdown().await;
            };
            let dial = async {
                let url = format!("{}/api/v1/terminal/{ID}?daemonEpoch=1&render=grid", fixture.base);
                let mut request = url.replacen("http:", "ws:", 1).into_client_request().unwrap();
                request.headers_mut().insert("authorization", format!("Bearer {}", fixture.token).parse().unwrap());
                if opt_in { request.headers_mut().insert("x-ferryx-refusal-format", "json".parse().unwrap()); }
                let reply = client.get(url).headers(request.into_parts().0.headers).send().await.unwrap();
                assert_eq!(reply.status().as_u16(), 409);
                assert_eq!(reply.headers()["content-type"].to_str().unwrap().split(';').next(), Some(mime));
                assert_eq!(reply.bytes().await.unwrap().as_ref(), body.as_bytes());
            };
            tokio::time::timeout(DEADLINE, async { tokio::join!(serve, dial) }).await.expect("bounded owner refusal");
        }
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
