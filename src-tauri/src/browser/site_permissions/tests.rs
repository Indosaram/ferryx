use super::*;
use std::sync::mpsc;

fn ask(browser: &str, profile: &str, origin: &str, kinds: &[PermissionKind]) -> PermissionAsk {
    PermissionAsk {
        browser_id: browser.to_string(),
        profile: profile.to_string(),
        origin: Some(origin.to_string()),
        kinds: kinds.to_vec(),
        external: None,
    }
}

/// An answer that reports the decision it delivered to the "engine".
fn answer() -> (Answer, mpsc::Receiver<bool>) {
    let (sender, receiver) = mpsc::channel();
    (Answer::new(Box::new(move |allow| { let _ = sender.send(allow); })), receiver)
}

/// Sends a decided outcome like the app does and returns the value it decided.
fn decided(outcome: Outcome) -> Option<bool> {
    match outcome {
        Outcome::Decided(answer, allow) => {
            answer.send(allow);
            Some(allow)
        }
        Outcome::Prompt(_) => None,
    }
}

fn prompt(outcome: Outcome) -> PendingPrompt {
    match outcome {
        Outcome::Prompt(prompt) => prompt,
        Outcome::Decided(_, allow) => panic!("expected a prompt, decided {allow}"),
    }
}

const CAM_MIC: [PermissionKind; 2] = [PermissionKind::Camera, PermissionKind::Microphone];

#[test]
fn origins_are_normalized_and_non_web_origins_have_none() {
    assert_eq!(site_origin("https://Meet.Example.com/room?x=1").as_deref(), Some("https://meet.example.com"));
    assert_eq!(site_origin("https://a.test:8443/").as_deref(), Some("https://a.test:8443"));
    assert_eq!(site_origin("http://localhost:3000").as_deref(), Some("http://localhost:3000"));
    assert_eq!(site_origin("file:///etc/passwd"), None);
    assert_eq!(site_origin("data:text/html,hi"), None);
    assert_eq!(site_origin("about:blank"), None);
    assert_eq!(site_origin("null"), None);
}

#[test]
fn an_unknown_site_is_asked_and_nothing_is_granted_yet() {
    let store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, engine) = answer();
    let prompt = prompt(broker.request(&store, ask("b1", "default", "https://meet.test/x", &CAM_MIC), answer));
    assert_eq!(prompt.origin, "https://meet.test");
    assert_eq!(prompt.kinds, CAM_MIC.to_vec());
    assert!(prompt.can_remember);
    assert!(engine.try_recv().is_err(), "no decision before the user answers");
    assert_eq!(broker.pending_for("b1"), vec![prompt]);
}

#[test]
fn an_unanswered_request_is_denied_when_its_tab_closes() {
    let store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, engine) = answer();
    prompt(broker.request(&store, ask("b1", "default", "https://meet.test", &CAM_MIC), answer));
    let forgotten = broker.forget_browser("b1");
    assert_eq!(forgotten.len(), 1);
    drop(forgotten);
    assert_eq!(engine.try_recv(), Ok(false));
    assert!(broker.pending_for("b1").is_empty());
}

#[test]
fn a_dropped_answer_denies_and_a_sent_answer_is_delivered_once() {
    let (dropped, dropped_engine) = answer();
    drop(dropped);
    assert_eq!(dropped_engine.try_recv(), Ok(false));
    let (sent, sent_engine) = answer();
    sent.send(true);
    assert_eq!(sent_engine.try_recv(), Ok(true));
    assert!(sent_engine.try_recv().is_err());
}

#[test]
fn a_request_without_a_web_origin_or_kinds_is_denied_without_a_prompt() {
    let store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (file, file_engine) = answer();
    assert_eq!(decided(broker.request(&store, ask("b1", "default", "file:///tmp/a.html", &CAM_MIC), file)), Some(false));
    assert_eq!(file_engine.try_recv(), Ok(false));
    let (unknown, unknown_engine) = answer();
    let mut no_origin = ask("b1", "default", "", &CAM_MIC);
    no_origin.origin = None;
    assert_eq!(decided(broker.request(&store, no_origin, unknown)), Some(false));
    assert_eq!(unknown_engine.try_recv(), Ok(false));
    let (empty, empty_engine) = answer();
    assert_eq!(decided(broker.request(&store, ask("b1", "default", "https://a.test", &[]), empty)), Some(false));
    assert_eq!(empty_engine.try_recv(), Ok(false));
    assert!(broker.pending_for("b1").is_empty());
}

#[test]
fn allow_once_grants_without_remembering() {
    let mut store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, engine) = answer();
    let prompt = prompt(broker.request(&store, ask("b1", "default", "https://meet.test", &CAM_MIC), answer));
    let applied = broker.decide(&mut store, prompt.request_id, true, false).expect("pending");
    assert_eq!(applied.prompt, prompt);
    assert!(!applied.store_changed);
    applied.answer.send(true);
    assert_eq!(engine.try_recv(), Ok(true));
    assert!(store.entries().is_empty());
    assert!(broker.decide(&mut store, prompt.request_id, true, false).is_none(), "answered only once");
}

#[test]
fn a_remembered_allow_answers_the_next_request_and_revoke_asks_again() {
    let mut store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (first, _first_engine) = answer();
    let first = prompt(broker.request(&store, ask("b1", "default", "https://meet.test", &CAM_MIC), first));
    assert!(broker.decide(&mut store, first.request_id, true, true).expect("pending").store_changed);

    let (second, second_engine) = answer();
    let camera = [PermissionKind::Camera];
    assert_eq!(decided(broker.request(&store, ask("b2", "default", "https://meet.test/other", &camera), second)), Some(true));
    assert_eq!(second_engine.try_recv(), Ok(true));

    // Another profile and another origin do not share the decision.
    let (third, _) = answer();
    prompt(broker.request(&store, ask("b3", "work", "https://meet.test", &camera), third));
    let (fourth, _) = answer();
    prompt(broker.request(&store, ask("b4", "default", "https://evil.test", &camera), fourth));

    assert!(store.revoke("default", "https://meet.test", Some(PermissionKind::Camera)));
    assert_eq!(store.get("default", "https://meet.test", PermissionKind::Microphone), Some(Decision::Allow));
    let (fifth, _) = answer();
    prompt(broker.request(&store, ask("b5", "default", "https://meet.test", &camera), fifth));
    assert!(store.revoke("default", "https://meet.test", None));
    assert!(store.entries().is_empty());
    assert!(!store.revoke("default", "https://meet.test", None));
}

#[test]
fn a_remembered_block_of_one_kind_blocks_the_whole_request() {
    let mut store = SitePermissionStore::default();
    store.set("default", "https://a.test", PermissionKind::Camera, Decision::Allow);
    store.set("default", "https://a.test", PermissionKind::Microphone, Decision::Block);
    let mut broker = Broker::default();
    let (both, both_engine) = answer();
    assert_eq!(decided(broker.request(&store, ask("b1", "default", "https://a.test", &CAM_MIC), both)), Some(false));
    assert_eq!(both_engine.try_recv(), Ok(false));
    let (camera, camera_engine) = answer();
    let only_camera = [PermissionKind::Camera];
    assert_eq!(decided(broker.request(&store, ask("b1", "default", "https://a.test", &only_camera), camera)), Some(true));
    assert_eq!(camera_engine.try_recv(), Ok(true));
}

#[test]
fn a_remembered_block_from_the_prompt_blocks_later_requests() {
    let mut store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let kinds = [PermissionKind::Geolocation];
    let (first, first_engine) = answer();
    let first = prompt(broker.request(&store, ask("b1", "default", "https://maps.test", &kinds), first));
    let applied = broker.decide(&mut store, first.request_id, false, true).expect("pending");
    applied.answer.send(false);
    assert_eq!(first_engine.try_recv(), Ok(false));
    let (second, _) = answer();
    assert_eq!(decided(broker.request(&store, ask("b2", "default", "https://maps.test", &kinds), second)), Some(false));
}

#[test]
fn the_private_profile_never_remembers_and_is_always_asked() {
    let mut store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, _) = answer();
    let prompt = prompt(broker.request(&store, ask("p1", "private", "https://meet.test", &CAM_MIC), answer));
    assert!(!prompt.can_remember);
    assert!(!broker.decide(&mut store, prompt.request_id, true, true).expect("pending").store_changed);
    assert!(store.entries().is_empty());
    assert!(!store.set("private", "https://meet.test", PermissionKind::Camera, Decision::Allow));
}

#[test]
fn an_engine_dismissal_denies_and_removes_the_prompt() {
    let store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, engine) = answer();
    let mut request = ask("b1", "default", "https://meet.test", &[PermissionKind::Notifications]);
    request.external = Some("default:7".into());
    let prompt = prompt(broker.request(&store, request, answer));
    assert!(broker.dismiss_external("default:8").is_none());
    let (dismissed, answer) = broker.dismiss_external("default:7").expect("pending");
    assert_eq!(dismissed, prompt);
    drop(answer);
    assert_eq!(engine.try_recv(), Ok(false));
    assert!(broker.pending_for("b1").is_empty());
}

#[test]
fn duplicate_kinds_collapse_into_one() {
    let store = SitePermissionStore::default();
    let mut broker = Broker::default();
    let (answer, _) = answer();
    let kinds = [PermissionKind::Microphone, PermissionKind::Camera, PermissionKind::Microphone];
    assert_eq!(prompt(broker.request(&store, ask("b1", "default", "https://a.test", &kinds), answer)).kinds, CAM_MIC.to_vec());
}

#[test]
fn the_store_survives_a_save_and_load_and_rejects_unknown_kinds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested").join(STORE_FILE);
    assert_eq!(load_store(&path).expect("missing file is empty"), SitePermissionStore::default());
    let mut store = SitePermissionStore::default();
    store.set("default", "https://meet.test", PermissionKind::Camera, Decision::Allow);
    store.set("work", "https://maps.test", PermissionKind::Geolocation, Decision::Block);
    save_store(&path, &store).expect("save");
    assert_eq!(load_store(&path).expect("load"), store);
    std::fs::write(
        &path,
        r#"{"entries":[{"profile":"default","origin":"https://a.test","kind":"teleport","decision":"allow"}]}"#,
    )
    .expect("write");
    assert!(load_store(&path).is_err(), "an unknown kind is not read as something else");
}

#[test]
fn prompts_serialize_kind_names_for_the_ui() {
    let prompt = PendingPrompt {
        request_id: 3,
        browser_id: "b1".into(),
        profile: "default".into(),
        origin: "https://a.test".into(),
        kinds: vec![PermissionKind::Screen, PermissionKind::Clipboard],
        can_remember: true,
    };
    let json = serde_json::to_value(&prompt).expect("json");
    assert_eq!(json["requestId"], 3);
    assert_eq!(json["browserId"], "b1");
    assert_eq!(json["kinds"], serde_json::json!(["screen", "clipboard"]));
    assert_eq!(json["canRemember"], true);
}

#[test]
fn media_capture_reflects_the_capture_kinds_only() {
    let capture = MediaCapture::of(&[PermissionKind::Camera, PermissionKind::Notifications]);
    assert_eq!(capture, MediaCapture { camera: true, microphone: false, screen: false });
    assert!(capture.any());
    assert!(!MediaCapture::of(&[PermissionKind::Geolocation]).any());
    assert_eq!(
        capture.union(MediaCapture::of(&[PermissionKind::Screen])),
        MediaCapture { camera: true, microphone: false, screen: true }
    );
}
