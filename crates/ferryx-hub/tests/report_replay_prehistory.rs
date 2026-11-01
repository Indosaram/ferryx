use serde_json::{json, Value};
use std::time::Duration;

#[tokio::test]
async fn attachments_require_a_suppressed_content_parent_in_the_same_thread() {
    let root = std::env::temp_dir().join(format!("ferryx-attachment-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let rows = root.join("rows.jsonl");
    let provenance = root.join("provenance.jsonl");
    let session = ferryx_hub::chat::session::SessionKey::new("slack", None::<String>, "UTEST", None::<String>, "UTEST");
    for (salience, parent_ts, channel, source, valid) in [
        ("internal", "100.1", "C1", "content-intel-publish-ready", true),
        ("decision", "100.1", "C1", "content-intel-publish-ready", false),
        ("internal", "99.1", "C1", "content-intel-publish-ready", false),
        ("internal", "100.1", "C2", "content-intel-publish-ready", false),
        ("internal", "100.1", "C1", "unrelated-producer", false),
    ] {
        let parent = json!({"i":0,"ts":"100.1","channel":"C1","sender":"wawabot","text":"fixture"});
        let child = json!({"i":1,"ts":"101.1","channel":channel,"thread_ts":parent_ts,"sender":"wawabot","text":"[files:1]"});
        std::fs::write(&rows, format!("{parent}\n{child}\n")).unwrap();
        let parent_input = json!({"ts":"100.1","evidence":"isolated parent","producer":{
            "kind":"external","session":session,"request":{"source":source,"topic":"fixture",
            "state":"1","salience":salience,"body":"Approval requested"}}});
        let child_input = json!({"ts":"101.1","evidence":"attachment receipt dependency fixture",
            "producer":{"kind":"content_attachment","parent_ts":parent_ts}});
        std::fs::write(&provenance, format!("{parent_input}\n{child_input}\n")).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_report_replay"));
        command.args([&rows, &provenance]).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await.unwrap().unwrap();
        assert_eq!(output.status.success(), valid, "{}", String::from_utf8_lossy(&output.stderr));
        let reports: Vec<Value> = String::from_utf8(output.stdout).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0]["delivery_policy"], if salience == "decision" { "ready" } else { "not_queued" });
        assert_eq!(reports[0]["transport_executed"], false);
        assert_eq!(reports[1]["outcome"], if valid { "parent_suppressed" } else { "unclassified" });
        if valid { assert_eq!(reports[1]["parent_decision"], "internal"); }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn prehistory_deduplicates_without_becoming_an_audit_row() {
    let root = std::env::temp_dir().join(format!("ferryx-replay-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let session = ferryx_hub::chat::session::SessionKey::new(
        "slack", None::<String>, "UTEST", None::<String>, "UTEST",
    );
    let entry = json!({
        "ts":"100.1", "evidence":"isolated producer fixture",
        "producer":{"kind":"external","session":session,"request":{
            "source":"fixture","topic":"fixture:topic","state":"snapshot:1",
            "salience":"decision","body":"승인을 선택해 주세요."
        }}
    });
    let rows = root.join("rows.jsonl");
    let provenance = root.join("provenance.jsonl");
    let prehistory = root.join("prehistory.jsonl");
    std::fs::write(&rows, json!({"i":0,"ts":"100.1","sender":"wawabot","text":"fixture"}).to_string()).unwrap();
    std::fs::write(&provenance, entry.to_string()).unwrap();
    for (seed_ts, evidence, succeeds) in [("99.1", "prior receipt", true), ("101.1", "future receipt", false), ("99.1", "", false)] {
        let mut prior = entry.clone();
        prior["ts"] = json!(seed_ts);
        prior["evidence"] = json!(evidence);
        std::fs::write(&prehistory, json!({"provenance":prior,"text":"fixture"}).to_string()).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_report_replay"));
        command.args([&rows, &provenance, &prehistory]).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await.unwrap().unwrap();
        assert_eq!(output.status.success(), succeeds, "{}", String::from_utf8_lossy(&output.stderr));
        if succeeds {
            let stdout = String::from_utf8(output.stdout).unwrap();
            let reports: Vec<Value> = stdout.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0]["i"], 0);
            assert_eq!(reports[0]["decision"], "duplicate");
            assert_eq!(reports[0]["stage"], "delivery_policy");
            assert_eq!(reports[0]["delivery_checked"], true);
            assert_eq!(reports[0]["delivery_policy"], "ready");
            assert_eq!(reports[0]["transport_executed"], false);
        } else {
            assert!(output.stdout.is_empty());
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "hub-slack")]
#[tokio::test]
async fn slack_turn_replay_suppresses_intermediate_and_keeps_final() {
    let root = std::env::temp_dir().join(format!("ferryx-replay-turn-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let rows = root.join("rows.jsonl");
    let provenance = root.join("provenance.jsonl");
    let session = ferryx_hub::chat::session::SessionKey::new("slack", None::<String>, "D1", None::<String>, "U1");
    for (final_answer, decision) in [(false, "internal"), (true, "sent")] {
        std::fs::write(&rows, json!({"i":0,"ts":"100.1","sender":"wawabot",
            "text":"The build failed because the runner was offline."}).to_string()).unwrap();
        std::fs::write(&provenance, json!({"ts":"100.1","evidence":"isolated turn fixture","producer":{
            "kind":"slack_turn","session":session,"trigger":"Why did the build fail?",
            "status_ts":"99.1","background":false,"final_answer":final_answer}}).to_string()).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_report_replay"));
        command.args([&rows, &provenance]).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await.unwrap().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(row["decision"], decision);
        assert_eq!(row["transport_executed"], false);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn only_named_external_bots_are_excluded_from_coverage() {
    let root = std::env::temp_dir().join(format!("ferryx-replay-scope-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let rows = root.join("rows.jsonl");
    for (sender, excluded) in [("GeekNews", true), ("Instinct", true), ("wawabot", false), ("unknown", false)] {
        std::fs::write(&rows, json!({"i":0,"ts":"100.1","sender":sender,"text":"fixture"}).to_string()).unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_report_replay"));
        command.arg(&rows).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output()).await.unwrap().unwrap();
        assert_eq!(output.status.success(), excluded, "{}", String::from_utf8_lossy(&output.stderr));
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(row["outcome"], if excluded { "external_bot" } else { "unclassified" });
        if excluded { assert_eq!(row["sender"], sender); }
    }
    std::fs::remove_dir_all(root).unwrap();
}
