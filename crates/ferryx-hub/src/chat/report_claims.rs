//! omo-gateway's report claims (report/claims.rs): the lead may append
//! `[[measure ["argv",...] expect~"regex"]]` after a factual sentence. The marker never reaches
//! Slack; 30 seconds after the reply, the hub re-runs the read-only command (a strict allowlist
//! on top of the `read` effect, secrets stripped from its environment, 120 s ceiling) and posts
//! "정정: <sentence> -> 실제 측정값: <output>" into the thread when the output no longer matches.
//! `OMON_REPORT_CLAIMS_ENABLED=0|false|no|off` turns it off. Pending claims left by a restart
//! become `unmeasurable`.
use std::path::PathBuf;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::effect::{classify_argv, Effect};

pub const DEFAULT_REMEASURE_DELAY: Duration = Duration::from_secs(30);
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const STDOUT_DISPLAY_CHARS: usize = 300;

tokio::task_local! {
    /// Test hook: re-measure after this delay instead of 30 seconds.
    pub static DELAY_OVERRIDE: Duration;
}

pub fn delay() -> Duration {
    DELAY_OVERRIDE
        .try_with(|delay| *delay)
        .unwrap_or(DEFAULT_REMEASURE_DELAY)
}

pub fn enabled() -> bool {
    !matches!(
        crate::hub_env::var("OMON_REPORT_CLAIMS_ENABLED")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0" | "false" | "no" | "off")
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedClaim {
    pub sentence: String,
    pub argv: Vec<String>,
    pub expect_regex: String,
}

/// An interim message's safe text: an unclosed trailing `[[...` is held back.
pub fn strip_interim(text: &str) -> (String, Vec<ExtractedClaim>) {
    strip_and_extract_claims(hold_incomplete_marker_suffix(text))
}

/// Strips markers from a reply when claims are on; otherwise the reply is untouched.
pub fn strip_reply(text: &str) -> (String, Vec<ExtractedClaim>) {
    if enabled() {
        strip_and_extract_claims(text)
    } else {
        (text.to_owned(), Vec::new())
    }
}

/// Where a correction goes.
#[derive(Clone, Debug)]
pub struct Target {
    pub session_key: String,
    pub channel: String,
    pub thread: Option<String>,
    pub cwd: PathBuf,
}

/// Records the claims and re-measures each allowed one after `delay`.
pub async fn record_and_schedule(
    pool: &SqlitePool,
    target: &Target,
    claims: Vec<ExtractedClaim>,
    delay: Duration,
) -> Vec<i64> {
    if !enabled() || claims.is_empty() {
        return Vec::new();
    }
    let cwd = target.cwd.to_string_lossy().to_string();
    let mut ids = Vec::new();
    for claim in claims {
        let allowed = classify_argv(&claim.argv, Some(&cwd)).effect == Effect::Read
            && remeasure_allowed(&claim.argv);
        let argv_json = serde_json::to_string(&claim.argv).unwrap_or_else(|_| "[]".into());
        let inserted = sqlx::query(
            "INSERT INTO report_claims
                (session_key, platform_message_id, sentence, argv_json, expect_regex, state, checked_at)
             VALUES (?, ?, ?, ?, ?, ?, CASE WHEN ? THEN NULL ELSE CURRENT_TIMESTAMP END)",
        )
        .bind(&target.session_key)
        .bind(target.thread.clone().unwrap_or_default())
        .bind(&claim.sentence)
        .bind(&argv_json)
        .bind(&claim.expect_regex)
        .bind(if allowed { "pending" } else { "refused" })
        .bind(allowed)
        .execute(pool)
        .await;
        let id = match inserted {
            Ok(done) => done.last_insert_rowid(),
            Err(error) => {
                tracing::warn!(%error, "report claim was not recorded");
                continue;
            }
        };
        ids.push(id);
        if allowed {
            let pool = pool.clone();
            let target = target.clone();
            super::slack::spawn_carrying_test_api(async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                if let Err(error) = execute_pending_claim(&pool, &target, id).await {
                    tracing::warn!(%error, "report claim was not re-measured");
                }
            });
        }
    }
    ids
}

pub async fn execute_pending_claim(
    pool: &SqlitePool,
    target: &Target,
    claim_id: i64,
) -> sqlx::Result<&'static str> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT sentence, argv_json, expect_regex FROM report_claims WHERE id = ? AND state = 'pending'",
    )
    .bind(claim_id)
    .fetch_optional(pool)
    .await?;
    let Some((sentence, argv_json, expect_regex)) = row else {
        return Ok("skipped");
    };
    let argv: Vec<String> = match serde_json::from_str::<Vec<String>>(&argv_json) {
        Ok(argv) if !argv.is_empty() => argv,
        _ => return mark(pool, claim_id, "unmeasurable", None).await,
    };
    if classify_argv(&argv, target.cwd.to_str()).effect != Effect::Read || !remeasure_allowed(&argv)
    {
        return mark(pool, claim_id, "refused", None).await;
    }
    let Ok(regex) = Regex::new(&expect_regex) else {
        return mark(pool, claim_id, "unmeasurable", None).await;
    };
    let mut command = tokio::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if target.cwd.exists() {
        command.current_dir(&target.cwd);
    }
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if ["TOKEN", "SECRET", "KEY", "PASSWORD"]
            .iter()
            .any(|needle| upper.contains(needle))
        {
            command.env_remove(&name);
        }
    }
    let Ok(child) = command.spawn() else {
        return mark(pool, claim_id, "unmeasurable", None).await;
    };
    let output = match tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) if output.status.success() => output,
        _ => return mark(pool, claim_id, "unmeasurable", None).await,
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let shown: String = stdout.chars().take(STDOUT_DISPLAY_CHARS).collect();
    if regex.is_match(&stdout) {
        return mark(pool, claim_id, "confirmed", Some(&shown)).await;
    }
    let correction = format!("정정: {sentence} -> 실제 측정값: {shown}");
    if let Err(error) =
        super::slack::deliver(&target.channel, target.thread.as_deref(), &correction).await
    {
        tracing::warn!(%error, "report claim correction was not posted");
    }
    mark(pool, claim_id, "corrected", Some(&shown)).await
}

async fn mark(
    pool: &SqlitePool,
    claim_id: i64,
    state: &'static str,
    measured_output: Option<&str>,
) -> sqlx::Result<&'static str> {
    sqlx::query(
        "UPDATE report_claims SET state = ?, measured_output = ?, checked_at = CURRENT_TIMESTAMP
         WHERE id = ?",
    )
    .bind(state)
    .bind(measured_output)
    .bind(claim_id)
    .execute(pool)
    .await?;
    Ok(state)
}

/// omo's startup sweep: a claim left pending by a restart is never re-run.
pub async fn recover_stale(pool: &SqlitePool) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE report_claims SET state = 'unmeasurable', checked_at = CURRENT_TIMESTAMP
         WHERE state = 'pending'",
    )
    .execute(pool)
    .await?
    .rows_affected())
}

fn hold_incomplete_marker_suffix(text: &str) -> &str {
    if let Some(open_idx) = text.rfind("[[") {
        let tail = &text[open_idx..];
        if !tail.contains("]]") {
            return &text[..open_idx];
        }
    }
    if let Some(stripped) = text.strip_suffix('[') {
        return stripped;
    }
    text
}

pub fn strip_and_extract_claims(text: &str) -> (String, Vec<ExtractedClaim>) {
    let mut cleaned = String::with_capacity(text.len());
    let mut claims = Vec::new();
    let mut cursor = 0;

    while let Some(rel_start) = text[cursor..].find("[[measure") {
        let start = cursor + rel_start;
        let before = &text[cursor..start];
        cleaned.push_str(before);

        let after_open = &text[start..];
        let Some(end_rel) = find_marker_end(after_open) else {
            cleaned.push_str(&text[start..start + 2]);
            cursor = start + 2;
            continue;
        };
        let marker_body = &after_open[2..end_rel - 2];
        match parse_measure_marker(marker_body) {
            Some((argv, expect_regex)) => claims.push(ExtractedClaim {
                sentence: extract_preceding_sentence(&cleaned),
                argv,
                expect_regex,
            }),
            None => cleaned.push_str(&text[start..start + end_rel]),
        }
        cursor = start + end_rel;
    }

    if cursor < text.len() {
        cleaned.push_str(&text[cursor..]);
    }

    let normalized = collapse_double_spaces(&cleaned);
    (normalized, claims)
}

fn find_marker_end(s: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    let bytes = s.as_bytes();
    let mut i = 2;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_quotes {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if !in_quotes && b == b']' && bytes[i + 1] == b']' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn parse_measure_marker(body: &str) -> Option<(Vec<String>, String)> {
    let rest = body.trim().strip_prefix("measure")?.trim();
    let bracket_start = rest.find('[')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut bracket_end = None;
    for (idx, ch) in rest[bracket_start..].char_indices() {
        if esc {
            esc = false;
            continue;
        }
        if ch == '\\' && in_str {
            esc = true;
            continue;
        }
        if ch == '"' {
            in_str = !in_str;
            continue;
        }
        if !in_str {
            if ch == '[' {
                depth += 1;
            } else if ch == ']' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    bracket_end = Some(bracket_start + idx + 1);
                    break;
                }
            }
        }
    }
    let end = bracket_end?;
    let argv_slice = &rest[bracket_start..end];
    let argv: Vec<String> = serde_json::from_str(argv_slice).ok()?;
    if argv.is_empty() {
        return None;
    }
    let after_argv = rest[end..].trim();
    let after_expect = after_argv.strip_prefix("expect~")?.trim();
    let expect_regex: String = if after_expect.starts_with('"') {
        quoted_expect_value(after_expect)?
    } else {
        after_expect.to_string()
    };
    Some((argv, expect_regex))
}

/// A quoted `expect~` value: strict JSON when the model escaped it, otherwise a single properly
/// closed quoted span (`expect~"^\d+$"` is a valid regex but not valid JSON). Anything with text
/// after the closing quote is malformed and yields no claim, so a broken marker is never turned
/// into a garbage regex.
fn quoted_expect_value(raw: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<String>(raw) {
        return Some(value);
    }
    let inner = raw.strip_prefix('"')?;
    let mut chars = inner.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next()?;
            }
            '"' => return (index + 1 == inner.len()).then(|| inner[..index].to_string()),
            _ => {}
        }
    }
    None
}

fn extract_preceding_sentence(cleaned_so_far: &str) -> String {
    let trimmed = cleaned_so_far.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    let last_line = trimmed.lines().last().unwrap_or(trimmed).trim();
    for sep in [". ", "! ", "? ", "다. ", "요. "] {
        if let Some((_, tail)) = last_line.rsplit_once(sep) {
            if !tail.trim().is_empty() {
                return tail.trim().to_string();
            }
        }
    }
    last_line.to_string()
}

fn collapse_double_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

/// Strict allowlist for claim re-measurement on top of the `read` classification: only a few
/// read-only programs, invoked by bare name, and never on secret-looking paths. Output of a
/// mismatch is posted back to the chat, so anything outside this list is refused.
fn remeasure_allowed(argv: &[String]) -> bool {
    let Some(program) = argv.first() else {
        return false;
    };
    let args = &argv[1..];
    let sensitive = |arg: &str| {
        let lower = arg.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or(&lower);
        [".ssh", ".aws", ".gnupg", "key", "secret", "token"]
            .iter()
            .any(|needle| lower.contains(needle))
            || base.starts_with(".env")
    };
    match program.as_str() {
        "echo" | "wc" | "ls" | "stat" | "test" => !args.iter().any(|a| sensitive(a)),
        "date" => !args
            .iter()
            .any(|a| a == "-s" || a.starts_with("--set") || a.starts_with("-f")),
        "cat" | "head" | "tail" => !args.iter().any(|a| sensitive(a)),
        "git" => {
            // The subcommand must come first: no global options such as -c/--config-env/-C.
            let Some(sub) = args.first() else {
                return false;
            };
            let rest = &args[1..];
            let no_writes = !rest.iter().any(|a| {
                a.starts_with("--output")
                    || a.starts_with("--ext-diff")
                    || a.starts_with("--config-env")
                    || a == "-c"
            });
            match sub.as_str() {
                "status" | "log" | "rev-parse" | "show" | "diff" => no_writes,
                "branch" => rest.iter().all(|a| {
                    matches!(
                        a.as_str(),
                        "-a" | "-r"
                            | "-v"
                            | "-vv"
                            | "--all"
                            | "--remotes"
                            | "--list"
                            | "--show-current"
                            | "--merged"
                            | "--no-merged"
                    )
                }),
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclosed_measure_marker_keeps_rest_of_reply() {
        let text = "Result is ready [[measure [\"echo\", \"test\"] expect~\"test\" but forgot closing. And more text.";
        let (cleaned, claims) = strip_and_extract_claims(text);
        assert!(cleaned.contains("And more text."));
        assert!(cleaned.contains("[[measure"));
        assert!(claims.is_empty());

        let unbalanced = "Notice [[measure [\"echo\"] expect~\"say \"hi\"\" extra]] final words.";
        let (cleaned_unbalanced, claims_unbalanced) = strip_and_extract_claims(unbalanced);
        assert!(cleaned_unbalanced.contains("final words."));
        assert!(claims_unbalanced.is_empty());
    }

    #[test]
    fn markers_are_stripped_and_only_read_only_programs_may_rerun() {
        let (clean, claims) = strip_and_extract_claims(
            "빌드 완료. 파일은 3줄이에요. [[measure [\"wc\",\"-l\",\"a.txt\"] expect~\"\\\\b3\\\\b\"]] 끝.",
        );
        assert_eq!(clean, "빌드 완료. 파일은 3줄이에요.  끝.");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].sentence, "파일은 3줄이에요.");
        assert_eq!(claims[0].argv, vec!["wc", "-l", "a.txt"]);
        assert_eq!(claims[0].expect_regex, "\\b3\\b");
        // An interim message holds back a marker that has not closed yet.
        assert_eq!(strip_interim("done [[measure [\"ls\"").0, "done");
        let argv = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(remeasure_allowed(&argv(&["git", "rev-parse", "HEAD"])));
        assert!(!remeasure_allowed(&argv(&[
            "cat",
            "/home/u/.ssh/id_ed25519"
        ])));
        assert!(!remeasure_allowed(&argv(&["git", "-c", "x=y", "status"])));
        assert!(!remeasure_allowed(&argv(&["rm", "-rf", "/"])));
    }

    #[tokio::test]
    async fn a_wrong_claim_is_corrected_in_its_thread_and_a_true_one_is_confirmed() {
        let pool = crate::storage::memory().await.unwrap();
        let (base, calls) = super::super::slack_web::mock::serve_scripted("UBOT", Vec::new()).await;
        let dir = std::env::temp_dir().join(format!("fx-claims-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n").unwrap();
        let target = Target {
            session_key: "slack:T1:C1:9.9:".into(),
            channel: "C1".into(),
            thread: Some("9.9".into()),
            cwd: dir.clone(),
        };
        let (_, claims) = strip_and_extract_claims(
            "파일은 3줄이에요. [[measure [\"cat\",\"a.txt\"] expect~\"^1\"]] \
             줄 수는 3이에요. [[measure [\"wc\",\"-l\",\"a.txt\"] expect~\"^\\s*3\\b\"]] \
             지웠어요. [[measure [\"rm\",\"a.txt\"] expect~\"x\"]]",
        );
        assert_eq!(claims.len(), 3);
        let ids = super::super::slack::with_test_api(base, "xoxb-test".into(), async {
            let ids = record_and_schedule(&pool, &target, claims, Duration::ZERO).await;
            for _ in 0..200 {
                let pending: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM report_claims WHERE state = 'pending'",
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                if pending == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            ids
        })
        .await;
        let states: Vec<String> = {
            let mut out = Vec::new();
            for id in &ids {
                out.push(
                    sqlx::query_scalar("SELECT state FROM report_claims WHERE id = ?")
                        .bind(id)
                        .fetch_one(&pool)
                        .await
                        .unwrap(),
                );
            }
            out
        };
        assert_eq!(states, vec!["confirmed", "corrected", "refused"]);
        let posts: Vec<serde_json::Value> = calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "chat.postMessage")
            .map(|(_, body)| body.clone())
            .collect();
        assert_eq!(posts.len(), 1, "{posts:?}");
        assert_eq!(posts[0]["thread_ts"], "9.9");
        let text = posts[0]["text"].as_str().unwrap();
        assert!(
            text.starts_with("정정: 줄 수는 3이에요. -> 실제 측정값: 4"),
            "{text}"
        );
        assert!(dir.join("a.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn claims_left_pending_by_a_restart_become_unmeasurable() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query(
            "INSERT INTO report_claims (session_key, platform_message_id, sentence, argv_json, expect_regex, state)
             VALUES ('s1', 'm1', 'sentence', '[\"echo\"]', 'regex', 'pending')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(recover_stale(&pool).await.unwrap(), 1);
        let state: String = sqlx::query_scalar("SELECT state FROM report_claims")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(state, "unmeasurable");
    }
}
