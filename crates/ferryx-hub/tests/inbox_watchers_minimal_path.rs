//! The daemon hosting the hub runs with launchd's PATH (`/usr/bin:/bin`). Inbox watchers must
//! still find `imsg` (found only through the login shell here) and `zele` (only in
//! `~/.bun/bin`, a well-known fallback directory), start, report `running`, and ingest items.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watchers_start_and_ingest_under_a_launchd_minimal_path() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("ferryx-minimal-path-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let login_bin = root.join("login-bin");
    let calls = root.join("calls.log");
    let now = chrono::Utc::now().to_rfc3339();
    // imsg: reachable only via the login shell's PATH.
    script(
        &login_bin.join("imsg"),
        &format!(
            r#"echo "imsg $*" >> '{calls}'
case "$1" in
  watch) echo '{{"rowid": 7}}'; exec sleep 600 ;;
  chats) echo '[{{"id":"c1","last_message_at":"{now}","name":"Stub chat"}}]' ;;
  history) echo '[{{"id":"m1","created_at":"{now}","text":"stub imessage","sender":"+100"}}]' ;;
esac"#,
            calls = calls.display()
        ),
    );
    // zele: reachable only through the ~/.bun/bin fallback.
    script(
        &home.join(".bun/bin/zele"),
        &format!(
            r#"echo "zele $*" >> '{calls}'
case "$2" in
  watch) exec sleep 600 ;;
  list) echo '{{"items":[{{"thread_id":"t1","snippet":"stub mail","internal_date":"{now}","from":"a@b.c","subject":"Stub"}}]}}' ;;
esac"#,
            calls = calls.display()
        ),
    );
    // typesafe-judge and bun: only in ~/.cargo/bin and ~/.bun/bin.
    script(
        &home.join(".cargo/bin/typesafe-judge"),
        &format!("echo \"judge $1\" >> '{}'\necho fyi", calls.display()),
    );
    script(&home.join(".bun/bin/bun"), "echo \"bun ran $*\"");
    let shell = root.join("login-shell");
    script(
        &shell,
        &format!(
            "PATH='{}':/usr/bin:/bin\nexport PATH\nshift\nexec /bin/sh -c \"$1\"",
            login_bin.display()
        ),
    );
    // Set before any hub code runs; this test binary holds only this test.
    {
        std::env::set_var("PATH", "/usr/bin:/bin");
        std::env::set_var("HOME", &home);
        std::env::set_var("SHELL", &shell);
        std::env::remove_var("FERRYX_HUB_EXTRA_PATH");
        std::env::remove_var("OMON_EXTRA_PATH");
        std::env::remove_var("EXTRA_PATH");
        // The hub env file and the developer's shell must not configure this run.
        std::env::remove_var("FERRYX_HUB_ENV_FILE");
        std::env::remove_var("OMON_JEV_ENABLED");
        std::env::remove_var("OMON_INBOX_ENABLED");
        // The mail source is off unless an account is configured.
        std::env::set_var("OMON_MAIL_ACCOUNT", "owner@example.test");
        std::env::set_var("OMON_PIRATETALK_URL", "http://127.0.0.1:9");
    }
    let path = tokio::task::spawn_blocking(ferryx_hub::child_env::init)
        .await
        .unwrap();
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    assert_eq!(dirs.first(), Some(&login_bin), "{dirs:?}");
    assert!(dirs.contains(&home.join(".bun/bin")), "{dirs:?}");
    assert_eq!(
        ferryx_hub::child_env::which("imsg"),
        Some(login_bin.join("imsg"))
    );
    assert_eq!(
        ferryx_hub::child_env::which("zele"),
        Some(home.join(".bun/bin/zele"))
    );

    let pool = ferryx_hub::storage::memory().await.unwrap();
    let watch_pool = pool.clone();
    let watchers = tokio::spawn(async move { ferryx_hub::inbox_watch::spawn(watch_pool).await });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let (health, items) = loop {
        let health: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT source, status, detail FROM inbox_watcher_health ORDER BY source",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let items: Vec<(String,)> =
            sqlx::query_as("SELECT source FROM inbox_items ORDER BY source")
                .fetch_all(&pool)
                .await
                .unwrap();
        let running = health
            .iter()
            .filter(|(_, status, _)| status == "running")
            .count();
        let sources: Vec<&str> = items.iter().map(|(s,)| s.as_str()).collect();
        if running == 2 && sources.contains(&"imessage") && sources.contains(&"mail") {
            break (health, items);
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "health={health:?} items={items:?} calls={:?}",
                std::fs::read_to_string(&calls).unwrap_or_default()
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert_eq!(
        health,
        [
            ("imessage".to_owned(), "running".to_owned(), None),
            ("mail".to_owned(), "running".to_owned(), None)
        ]
    );
    assert!(items.len() >= 2, "{items:?}");
    let log = std::fs::read_to_string(&calls).unwrap();
    for call in [
        "imsg watch --json",
        "imsg chats",
        "zele mail watch",
        "zele mail list",
    ] {
        assert!(log.contains(call), "{call} not run: {log}");
    }
    let decided: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbox_decisions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        decided >= 2,
        "judge not reached through the child PATH: {log}"
    );

    // A cron job whose script calls bare `bun` (the 09:36 failure: "bun: command not found").
    let executed = ferryx_hub::cron_exec::run_payload(
        &pool,
        "minimal-path-cron",
        &serde_json::json!({"script": "bun run job.ts", "no_agent": true}),
        &NoModel,
        None,
    )
    .await
    .unwrap();
    assert!(
        executed.output.contains("bun ran run job.ts"),
        "{}",
        executed.output
    );
    let tool = ferryx_hub::tools::run_command("command -v imsg")
        .await
        .unwrap();
    assert!(tool.contains("login-bin/imsg"), "{tool}");
    watchers.abort();
    let _ = std::fs::remove_dir_all(&root);
}

struct NoModel;

#[async_trait::async_trait]
impl ferryx_hub::lanes::kibitzer::ModelTurn for NoModel {
    async fn complete(&self, _prompt: &str) -> ferryx_hub::error::Result<String> {
        panic!("no_agent cron jobs never reach the model")
    }
}
