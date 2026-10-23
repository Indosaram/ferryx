//! Settlement moves `done_claimed` to `verified_done` when every signal passes and the
//! judge accepts the claim. It runs when a claim arrives, not on a poll.
use sqlx::SqlitePool;

use super::signals::{check_signals, parse_signals};
use super::state::{transition, LaneStatus};
use crate::error::{HubError, Result};
use crate::judge::{Choice, Judge, Purpose};

pub struct Settlement {
    pub passed: bool,
    pub detail: String,
}

pub async fn settle(
    pool: &SqlitePool,
    key: &str,
    cwd: &str,
    done_signals: &str,
    claim_evidence: &str,
    judge: &Judge,
) -> Result<Settlement> {
    // A cron remediation lane settles on the hub's own verified rerun (omo `settle_lane`).
    let checks = match Box::pin(crate::cron_remediate::settle_lane(pool, key)).await? {
        Some(checks) => checks,
        None => {
            let signals = parse_signals(done_signals)
                .map_err(|error| HubError::message(error.to_string()))?;
            check_signals(&signals, std::path::Path::new(cwd)).await
        }
    };
    if checks.iter().any(|check| !check.passed) {
        transition(pool, key, LaneStatus::NotDone, Some("signals failed")).await?;
        return Ok(Settlement {
            passed: false,
            detail: checks
                .into_iter()
                .map(|check| check.detail)
                .collect::<Vec<_>>()
                .join("; "),
        });
    }
    let git = worktree_reasons(pool, key, cwd).await?;
    if !git.is_empty() {
        transition(pool, key, LaneStatus::NotDone, Some("git not settled")).await?;
        return Ok(Settlement {
            passed: false,
            detail: git.join("; "),
        });
    }
    let choice = judge
        .choice(
            Purpose::DoneDiff,
            claim_evidence,
            "Does the claim evidence show the lane is done? Reply yes or no.",
            &["yes", "no"],
        )
        .await?;
    if choice.as_str() == "yes" {
        transition(
            pool,
            key,
            LaneStatus::VerifiedDone,
            Some("settlement_started"),
        )
        .await?;
        Ok(Settlement {
            passed: true,
            detail: choice.as_str().to_owned(),
        })
    } else {
        transition(pool, key, LaneStatus::NotDone, Some("judge declined")).await?;
        Ok(Settlement {
            passed: false,
            detail: choice.as_str().to_owned(),
        })
    }
}

pub fn accepting_fallback(selected: &str) -> Choice {
    Choice::fallback(selected)
}

async fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<std::process::Output> {
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::child_env::command("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output(),
    )
    .await
    .map_err(|_| HubError::message(format!("git check timed out: {}", cwd.display())))?
    .map_err(|error| HubError::message(error.to_string()))
}

async fn git_text(cwd: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = git_output(cwd, args).await?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "git check {}: {}",
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Uncommitted tracked files and unpushed commits on this lane's worktrees.
/// A directory that is not a git repository has nothing to settle.
async fn worktree_reasons(pool: &SqlitePool, key: &str, cwd: &str) -> Result<Vec<String>> {
    let cwd = std::path::Path::new(cwd);
    let probe = match git_output(cwd, &["rev-parse", "--is-inside-work-tree"]).await {
        Ok(output) => output,
        Err(_) => return Ok(Vec::new()),
    };
    if !probe.status.success() {
        let stderr = String::from_utf8_lossy(&probe.stderr);
        if stderr.contains("not a git repository") || stderr.contains("No such file") {
            return Ok(Vec::new());
        }
        return Err(HubError::message(stderr.trim().to_owned()));
    }
    let canonical = tokio::fs::canonicalize(cwd)
        .await
        .unwrap_or_else(|_| cwd.to_path_buf());
    let started: Option<String> = sqlx::query_scalar("SELECT created_at FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    let started = started.and_then(|text| {
        chrono::DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|value| value.with_timezone(&chrono::Utc))
    });
    let others: Vec<String> = sqlx::query_scalar(
        "SELECT path FROM lane_owned_worktrees WHERE lane_key <> ? AND state = 'registered'",
    )
    .bind(key)
    .fetch_all(pool)
    .await?;
    let mut paths = vec![canonical.clone()];
    if let Ok(listed) = git_text(&canonical, &["worktree", "list", "--porcelain"]).await {
        for line in listed.lines() {
            let Some(path) = line.strip_prefix("worktree ") else {
                continue;
            };
            let path = std::path::PathBuf::from(path);
            if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
                continue;
            }
            let full = tokio::fs::canonicalize(&path)
                .await
                .unwrap_or_else(|_| path.clone());
            if !full.starts_with(&canonical)
                && others
                    .iter()
                    .any(|owned| std::path::Path::new(owned) == full)
            {
                continue;
            }
            let include = full.starts_with(&canonical)
                || started.is_some_and(|start| {
                    std::fs::metadata(&full)
                        .and_then(|meta| meta.created())
                        .ok()
                        .map(chrono::DateTime::<chrono::Utc>::from)
                        .is_some_and(|created| created >= start)
                });
            if include && !paths.contains(&full) {
                paths.push(full);
            }
        }
    }
    let mut reasons = Vec::new();
    for path in paths {
        let status = git_text(&path, &["status", "--porcelain", "--untracked-files=no"]).await?;
        if !status.trim().is_empty() {
            reasons.push(format!("uncommitted {}", path.display()));
        }
        let head = git_output(&path, &["rev-parse", "--verify", "-q", "HEAD"]).await?;
        if head.status.code() == Some(1) {
            continue;
        }
        if !head.status.success() {
            return Err(HubError::message(format!(
                "git check {}: {}",
                path.display(),
                String::from_utf8_lossy(&head.stderr).trim()
            )));
        }
        let count_text = git_text(
            &path,
            &["rev-list", "--count", "HEAD", "--not", "--remotes"],
        )
        .await?;
        let count: u64 = count_text.trim().parse().map_err(|error| {
            HubError::message(format!("git rev-list --count {}: {error}", path.display()))
        })?;
        if count > 0 {
            let remote = git_text(&path, &["remote"]).await?;
            if remote.trim().is_empty() {
                reasons.push(format!("unpushed {} (no remote)", path.display()));
            } else {
                reasons.push(format!("unpushed {}", path.display()));
            }
        }
    }
    Ok(reasons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_uncommitted_tracked_file_blocks_settlement() {
        let root = std::env::temp_dir().join(format!("ferryx-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = crate::child_env::std_command("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        };
        git(&["init"]);
        git(&["config", "user.email", "settle@example.com"]);
        git(&["config", "user.name", "settle"]);
        std::fs::write(root.join("ok.txt"), "ok").unwrap();
        git(&["add", "ok.txt"]);
        git(&["commit", "-m", "ok"]);
        std::fs::write(root.join("ok.txt"), "dirty").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        let cwd = root.to_string_lossy().to_string();
        sqlx::query(
            "INSERT INTO lanes(key,title,lead_session_key,cwd,session_key,status,done_signals,last_activity_at) VALUES('lane-a','A','lead',?,'lead','done_claimed','file ok.txt','2020-01-01T00:00:00Z')",
        )
        .bind(&cwd)
        .execute(&pool)
        .await
        .unwrap();
        let judge = crate::judge::Judge::fallback_yes();
        let settled = settle(&pool, "lane-a", &cwd, "file ok.txt", "done", &judge)
            .await
            .unwrap();
        assert!(!settled.passed);
        assert!(settled.detail.contains("uncommitted"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
