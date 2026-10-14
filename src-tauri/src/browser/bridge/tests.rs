use super::*;
use std::time::{Duration, Instant};

const KINDS: [TrustedBridgeKind; 2] = [TrustedBridgeKind::Isolated, TrustedBridgeKind::Unavailable];
const ATTESTATIONS: [Attestation; 3] = [
    Attestation::IsolatedWorld,
    Attestation::NativeUserInitiated,
    Attestation::None,
];
const URL: &str = "https://example.com/file.pdf";

fn ctx<'a>(
    is_popup: bool,
    token: Option<&'a str>,
    native: &'a [(String, Instant)],
    now: Instant,
) -> AdmitCtx<'a> {
    AdmitCtx {
        is_popup,
        armed_pick_token: token,
        recent_native_user_initiated: native,
        now,
    }
}

fn trusted(kind: TrustedBridgeKind, attestation: Attestation) -> bool {
    kind == TrustedBridgeKind::Isolated && attestation == Attestation::IsolatedWorld
}

#[test]
fn parse_envelope_accepts_well_formed_v1_envelopes() {
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"download","url":"https://example.com/a.zip"}"#),
        Ok(TrustedAction::Download { url: "https://example.com/a.zip".into() })
    );
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"link","url":"http://example.com/","modifier":true}"#),
        Ok(TrustedAction::Link { url: "http://example.com/".into(), modifier: true })
    );
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"link","url":"https://example.com/"}"#),
        Ok(TrustedAction::Link { url: "https://example.com/".into(), modifier: false })
    );
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"popupOpen","url":"https://auth.example/","handle":"d1-p1","activationAgeMs":12.5}"#),
        Ok(TrustedAction::PopupOpen { url: "https://auth.example/".into(), handle: "d1-p1".into() })
    );
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"pickReady","token":"tok-1"}"#),
        Ok(TrustedAction::PickReady { token: "tok-1".into() })
    );
    assert_eq!(
        parse_envelope(r#"{"v":1,"kind":"popupSelfClose"}"#),
        Ok(TrustedAction::PopupSelfClose)
    );
}

#[test]
fn parse_envelope_rejects_everything_else() {
    let oversize = format!(
        r#"{{"v":1,"kind":"download","url":"https://example.com/{}"}}"#,
        "a".repeat(MAX_ENVELOPE_BYTES)
    );
    let long_handle = format!(
        r#"{{"v":1,"kind":"popupOpen","url":"https://a.example/","handle":"{}","activationAgeMs":1}}"#,
        "h".repeat(200)
    );
    let cases: [(&str, &str); 16] = [
        ("unknown field", r#"{"v":1,"kind":"download","url":"https://e.com/","extra":1}"#),
        ("version 2", r#"{"v":2,"kind":"download","url":"https://e.com/"}"#),
        ("missing version", r#"{"kind":"download","url":"https://e.com/"}"#),
        ("unknown kind", r#"{"v":1,"kind":"shortcut","url":"https://e.com/"}"#),
        ("javascript url", r#"{"v":1,"kind":"link","url":"javascript:alert(1)"}"#),
        ("data url", r#"{"v":1,"kind":"download","url":"data:text/html,x"}"#),
        ("file url", r#"{"v":1,"kind":"link","url":"file:///etc/passwd"}"#),
        ("relative url", r#"{"v":1,"kind":"link","url":"/x"}"#),
        ("missing url", r#"{"v":1,"kind":"download"}"#),
        ("popup without handle", r#"{"v":1,"kind":"popupOpen","url":"https://a.example/","activationAgeMs":1}"#),
        ("popup without activation", r#"{"v":1,"kind":"popupOpen","url":"https://a.example/","handle":"p1"}"#),
        ("stale activation", r#"{"v":1,"kind":"popupOpen","url":"https://a.example/","handle":"p1","activationAgeMs":1000.5}"#),
        ("negative activation", r#"{"v":1,"kind":"popupOpen","url":"https://a.example/","handle":"p1","activationAgeMs":-1}"#),
        ("pick without token", r#"{"v":1,"kind":"pickReady"}"#),
        ("empty token", r#"{"v":1,"kind":"pickReady","token":"  "}"#),
        ("not json", "nope"),
    ];
    for (label, raw) in cases {
        assert!(parse_envelope(raw).is_err(), "{label} must be rejected");
    }
    assert_eq!(parse_envelope(&oversize), Err(EnvelopeError::TooLarge));
    assert_eq!(parse_envelope(&long_handle), Err(EnvelopeError::TooLarge));
}

#[test]
fn admit_download_row() {
    let now = Instant::now();
    for kind in KINDS {
        for attestation in ATTESTATIONS {
            let decision = admit(
                TrustedAction::Download { url: URL.into() },
                attestation,
                kind,
                &ctx(false, None, &[], now),
            );
            let emit = BridgeEmit::Download { url: URL.into() };
            if trusted(kind, attestation) {
                assert_eq!(decision, Decision::Emit(emit), "{kind:?}/{attestation:?}");
            } else {
                assert_eq!(decision, Decision::NeedsConfirmation(emit), "{kind:?}/{attestation:?}");
            }
        }
    }
    // A fresh native record for the same URL attests it even without a world.
    let native = [(URL.to_string(), now)];
    assert_eq!(
        admit(
            TrustedAction::Download { url: URL.into() },
            Attestation::NativeUserInitiated,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &native, now + Duration::from_millis(500)),
        ),
        Decision::Emit(BridgeEmit::Download { url: URL.into() })
    );
}

#[test]
fn admit_link_row_downgrades_without_attestation() {
    let now = Instant::now();
    let url = "https://example.com/docs";
    for kind in KINDS {
        for attestation in ATTESTATIONS {
            for modifier in [false, true] {
                let decision = admit(
                    TrustedAction::Link { url: url.into(), modifier },
                    attestation,
                    kind,
                    &ctx(false, None, &[], now),
                );
                if trusted(kind, attestation) {
                    assert_eq!(decision, Decision::Emit(BridgeEmit::Link { url: url.into(), modifier }));
                } else {
                    assert_eq!(decision, Decision::Downgrade(BridgeEmit::NavigateInTab { url: url.into() }));
                }
            }
        }
    }
    let native = [(url.to_string(), now)];
    assert_eq!(
        admit(
            TrustedAction::Link { url: url.into(), modifier: true },
            Attestation::NativeUserInitiated,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &native, now + Duration::from_millis(1000)),
        ),
        Decision::Emit(BridgeEmit::Link { url: url.into(), modifier: true })
    );
    // External link with a native record older than 1000 ms is downgraded.
    assert_eq!(
        admit(
            TrustedAction::Link { url: url.into(), modifier: true },
            Attestation::NativeUserInitiated,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &native, now + Duration::from_millis(1001)),
        ),
        Decision::Downgrade(BridgeEmit::NavigateInTab { url: url.into() })
    );
    // A record for a different URL does not attest this one.
    let other = [("https://example.com/other".to_string(), now)];
    assert_eq!(
        admit(
            TrustedAction::Link { url: url.into(), modifier: true },
            Attestation::NativeUserInitiated,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &other, now),
        ),
        Decision::Downgrade(BridgeEmit::NavigateInTab { url: url.into() })
    );
    // A page-originated request cannot borrow a native record.
    assert_eq!(
        admit(
            TrustedAction::Link { url: url.into(), modifier: true },
            Attestation::None,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &native, now),
        ),
        Decision::Downgrade(BridgeEmit::NavigateInTab { url: url.into() })
    );
}

#[test]
fn admit_popup_open_row_denies_without_activation() {
    let now = Instant::now();
    let url = "https://auth.example/authorize";
    let action = || TrustedAction::PopupOpen { url: url.into(), handle: "d-p1".into() };
    for kind in KINDS {
        for attestation in ATTESTATIONS {
            let decision = admit(action(), attestation, kind, &ctx(false, None, &[], now));
            if trusted(kind, attestation) {
                assert_eq!(decision, Decision::Emit(BridgeEmit::PopupOpen { url: url.into(), handle: "d-p1".into() }));
            } else {
                assert_eq!(decision, Decision::Deny(DENY_POPUP_ACTIVATION), "{kind:?}/{attestation:?}");
            }
        }
    }
    let native = [(url.to_string(), now)];
    assert_eq!(
        admit(action(), Attestation::NativeUserInitiated, TrustedBridgeKind::Unavailable, &ctx(false, None, &native, now)),
        Decision::Emit(BridgeEmit::PopupOpen { url: url.into(), handle: "d-p1".into() })
    );
    assert_eq!(
        admit(
            action(),
            Attestation::NativeUserInitiated,
            TrustedBridgeKind::Unavailable,
            &ctx(false, None, &native, now + Duration::from_millis(1500)),
        ),
        Decision::Deny(DENY_POPUP_ACTIVATION)
    );
}

#[test]
fn admit_pick_ready_row_requires_world_and_armed_token() {
    let now = Instant::now();
    let pick = |token: &str| TrustedAction::PickReady { token: token.into() };
    // Accepted only with Isolated kind, world attestation and the armed token.
    assert_eq!(
        admit(pick("tok"), Attestation::IsolatedWorld, TrustedBridgeKind::Isolated, &ctx(false, Some("tok"), &[], now)),
        Decision::Emit(BridgeEmit::ElementPicked { token: "tok".into() })
    );
    // Wrong token.
    assert_eq!(
        admit(pick("guess"), Attestation::IsolatedWorld, TrustedBridgeKind::Isolated, &ctx(false, Some("tok"), &[], now)),
        Decision::Deny(DENY_PICK_TOKEN)
    );
    // Absent token, and consumed token (the host clears it on accept, so the ctx is None).
    assert_eq!(
        admit(pick("tok"), Attestation::IsolatedWorld, TrustedBridgeKind::Isolated, &ctx(false, None, &[], now)),
        Decision::Deny(DENY_PICK_TOKEN)
    );
    for kind in KINDS {
        for attestation in ATTESTATIONS {
            if trusted(kind, attestation) {
                continue;
            }
            assert_eq!(
                admit(pick("tok"), attestation, kind, &ctx(false, Some("tok"), &[], now)),
                Decision::Deny(DENY_PICK_UNAVAILABLE),
                "{kind:?}/{attestation:?}"
            );
        }
    }
}

#[test]
fn admit_popup_self_close_row_requires_a_popup_tab() {
    let now = Instant::now();
    for kind in KINDS {
        for attestation in ATTESTATIONS {
            assert_eq!(
                admit(TrustedAction::PopupSelfClose, attestation, kind, &ctx(true, None, &[], now)),
                Decision::Emit(BridgeEmit::CloseSelf)
            );
            assert_eq!(
                admit(TrustedAction::PopupSelfClose, attestation, kind, &ctx(false, None, &[], now)),
                Decision::Deny(DENY_SELF_CLOSE_NOT_POPUP),
                "{kind:?}/{attestation:?}"
            );
        }
    }
}

#[test]
fn trusted_bridge_kind_defaults_to_unavailable_and_is_camel_case() {
    assert_eq!(TrustedBridgeKind::default(), TrustedBridgeKind::Unavailable);
    assert_eq!(serde_json::to_string(&TrustedBridgeKind::Isolated).unwrap(), "\"isolated\"");
    assert_eq!(serde_json::to_string(&TrustedBridgeKind::Unavailable).unwrap(), "\"unavailable\"");
    // Browser state persisted before the field existed still loads, as Unavailable.
    let legacy = serde_json::json!({
        "browserId": "b1", "webviewLabel": "browser-x", "workspaceId": null,
        "worktreePath": null, "profileId": "default", "generation": 1,
        "url": "https://example.com/", "title": null, "loading": false,
        "canGoBack": false, "canGoForward": false, "zoomFactor": 1.0,
        "loadError": null, "visible": true
    });
    let state: crate::browser::BrowserState = serde_json::from_value(legacy).unwrap();
    assert_eq!(state.trusted_bridge, TrustedBridgeKind::Unavailable);
    let round = serde_json::to_value(&state).unwrap();
    assert_eq!(round["trustedBridge"], "unavailable");
}

/// Every `window.<name> =` / `window[...] =` / defineProperty(window, ...) write.
fn writes_page_global(script: &str) -> bool {
    script.contains("window.__")
        || script.contains("window[")
        || script.contains("defineProperty(window")
        || script.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("window.") && trimmed.contains(" = ")
        })
}

#[test]
fn picker_scripts_write_no_page_globals_and_require_trusted_clicks() {
    let armed = picker_arm_script("tok-\"quoted</script>");
    for script in [PICKER_ARM_SCRIPT, armed.as_str(), PICKER_READ_SCRIPT, PICKER_DISARM_SCRIPT] {
        assert!(!writes_page_global(script), "picker must keep state world-local:\n{script}");
        assert!(!script.contains("__ferryxElementPick"));
        assert!(!script.contains("__ferryxPickReady"));
        assert!(!script.contains("__ferryxPicker"));
        assert!(script.contains("globalThis.__ferryxWorld"));
    }
    assert!(PICKER_ARM_SCRIPT.contains("if (event.isTrusted !== true) return;"));
    assert!(PICKER_ARM_SCRIPT.contains("kind: 'pickReady', token: state.token"));
    // The token is embedded as an escaped JSON string, never raw.
    assert!(!armed.contains("__FERRYX_PICK_TOKEN__"));
    assert!(armed.contains(r#"token: "tok-\"quoted<\/script>""#));
    assert!(PICKER_READ_SCRIPT.contains("picker.pick"));
}

#[test]
fn broker_script_is_world_local_and_gates_on_trusted_events() {
    assert!(!writes_page_global(TRUSTED_BROKER_SCRIPT));
    assert!(TRUSTED_BROKER_SCRIPT.contains("event.isTrusted === true"));
    assert!(TRUSTED_BROKER_SCRIPT.contains("if (!isTrusted(event)) return;"));
    // The jsdom test hook stays inert in the shipped script.
    assert!(TRUSTED_BROKER_SCRIPT.contains("const hooks = null;"));
    assert!(TRUSTED_BROKER_SCRIPT.contains("const ACTIVATION_WINDOW_MS = 1000;"));
    assert!(TRUSTED_BROKER_SCRIPT.contains("ferryx-intent"));
    assert!(TRUSTED_BROKER_SCRIPT.contains("webkit.messageHandlers.ferryxTrusted"));
    assert!(TRUSTED_BROKER_SCRIPT.contains("__ferryxTrusted(body)"));
    assert!(PICKER_ARM_SCRIPT.contains("detach();\n      state.armed = false;"));
    assert!(!TRUSTED_BROKER_SCRIPT.contains(".ferryx.invalid"));
    assert!(!TRUSTED_BROKER_SCRIPT.contains("nonce"));
    for kind in ["'download'", "'link'", "'popupOpen'", "'popupSelfClose'"] {
        assert!(TRUSTED_BROKER_SCRIPT.contains(kind), "{kind}");
    }
}

#[test]
fn new_windows_are_never_opened_natively() {
    // Isolated: the broker owns window.open, so the native request is denied silently.
    assert_eq!(
        new_window_policy(TrustedBridgeKind::Isolated, "https://accounts.example.com/o/oauth2"),
        NewWindowDecision::Deny
    );
    // Unavailable + http(s): denied, with a visible warning, never auto-opened as a tab.
    for url in ["https://accounts.example.com/o/oauth2", "http://example.com/"] {
        assert_eq!(
            new_window_policy(TrustedBridgeKind::Unavailable, url),
            NewWindowDecision::DenyWithWarning(DENY_NEW_WINDOW_UNATTESTED)
        );
    }
    // Unavailable + anything else: denied.
    for url in ["about:blank", "file:///tmp/x.html", "javascript:alert(1)", "not a url"] {
        assert_eq!(new_window_policy(TrustedBridgeKind::Unavailable, url), NewWindowDecision::Deny);
    }
}

#[test]
fn fallback_reports_unavailable_without_platform_api() {
    let app = tauri::test::mock_app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "bridge-fallback", Default::default())
        .build()
        .expect("mock webview");
    let ctx = BridgeInstallCtx {
        browser_id: "b1".into(),
        manager: Arc::new(BrowserManager::new()),
        sink: Arc::new(|_: &str, _: &str, _: Attestation| {}),
    };
    assert_eq!(
        fallback::install(webview.as_ref(), &ctx),
        TrustedBridgeKind::Unavailable
    );
    let error = tauri::async_runtime::block_on(fallback::eval_in_world(
        webview.as_ref().clone(),
        "1".into(),
    ))
    .expect_err("no world");
    assert!(matches!(error, BrowserError::PlatformUnsupported(_)));
}
