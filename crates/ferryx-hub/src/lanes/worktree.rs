//! omo-gateway's lane auto worktrees and close (lanes/runner.rs create_auto_worktree,
//! close_intent, remove_owned, unmerged_lane_branch, restore_cwd).
//!
//! A lane whose cwd is the root of a git repository (not already a linked worktree, with at
//! least one commit) runs in `<repo>-lanes/<key>` on branch `lane/<key>`, so parallel lanes
//! never share a checkout. `OMON_LANE_AUTO_WORKTREE=0|false|no|off` turns this off.
//! Closing the lane removes the worktree without force (a dirty tree refuses and the lead is
//! told), keeps the branch, and reports a branch that holds unmerged work. Reopening the lane
//! restores the worktree from its branch. Each step re-checks the tree's state, so a close
//! interrupted halfway resumes where it stopped.
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use super::state::{transition, LaneStatus};
use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

pub fn auto_enabled() -> bool {
    crate::hub_env::var("OMON_LANE_AUTO_WORKTREE").map_or(true, |value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    })
}

async fn git_raw(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        crate::child_env::command("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| HubError::message(format!("git {} timed out", args.join(" "))))?
    .map_err(|error| HubError::message(error.to_string()))
}

async fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git_raw(repo, args).await?;
    if !output.status.success() {
        return Err(HubError::message(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn canonical(path: &str) -> Result<PathBuf> {
    tokio::fs::canonicalize(path)
        .await
        .map_err(|error| HubError::message(format!("{path}: {error}")))
}

/// Creates the lane's worktree when its cwd qualifies and points the lane at it.
/// Returns the new cwd, or None when the lane keeps its own cwd.
pub async fn create(pool: &SqlitePool, key: &str, cwd: &str) -> Result<Option<String>> {
    if !auto_enabled() {
        return Ok(None);
    }
    let owned: Option<String> = sqlx::query_scalar(
        "SELECT path FROM lane_owned_worktrees WHERE lane_key = ? AND branch IS NOT NULL LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    if owned.is_some() {
        return Ok(None);
    }
    let dir = Path::new(cwd);
    let probe = git_raw(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
        ],
    )
    .await?;
    if !probe.status.success() {
        let stderr = String::from_utf8_lossy(&probe.stderr);
        if stderr.contains("not a git repository") || stderr.contains("cannot change to") {
            return Ok(None);
        }
        return Err(HubError::message(format!(
            "auto worktree probe {cwd}: {}",
            stderr.trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&probe.stdout).into_owned();
    let mut lines = stdout.lines();
    let (Some(top), Some(git_dir), Some(common)) = (lines.next(), lines.next(), lines.next())
    else {
        return Err(HubError::message("auto worktree probe: unexpected output"));
    };
    let top = canonical(top).await?;
    if canonical(git_dir).await? != canonical(common).await? {
        tracing::info!(
            key,
            cwd,
            "auto worktree skipped: cwd is already a linked worktree"
        );
        return Ok(None);
    }
    if top != canonical(cwd).await? {
        tracing::info!(
            key,
            cwd,
            "auto worktree skipped: cwd is not the repository root"
        );
        return Ok(None);
    }
    let Ok(start) = git(&top, &["rev-parse", "--verify", "--quiet", "HEAD"]).await else {
        tracing::info!(key, cwd, "auto worktree skipped: repository has no commits");
        return Ok(None);
    };
    let (Some(name), Some(parent)) = (top.file_name(), top.parent()) else {
        return Err(HubError::message(
            "auto worktree: repository root has no parent",
        ));
    };
    let lanes_dir = parent.join(format!("{}-lanes", name.to_string_lossy()));
    tokio::fs::create_dir_all(&lanes_dir)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let path = lanes_dir.join(key);
    let branch = format!("lane/{key}");
    let target = path.to_string_lossy().into_owned();
    git(&top, &["worktree", "add", "-b", &branch, &target, "HEAD"]).await?;
    let path = canonical(&target).await?.to_string_lossy().into_owned();
    let repo = top.to_string_lossy().into_owned();
    sqlx::query(
        "INSERT INTO lane_owned_worktrees(lane_key, path, state, repo, branch, start_commit, origin_cwd)
         VALUES (?, ?, 'registered', ?, ?, ?, ?)",
    )
    .bind(key)
    .bind(&path)
    .bind(&repo)
    .bind(&branch)
    .bind(&start)
    .bind(cwd)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE lanes SET cwd = ?, updated_at = ? WHERE key = ?")
        .bind(&path)
        .bind(now_rfc3339())
        .bind(key)
        .execute(pool)
        .await?;
    tracing::info!(key, path = %path, "auto worktree created");
    Ok(Some(path))
}

type OwnedTree = (String, String, String, String, Option<String>);

async fn owned(pool: &SqlitePool, key: &str) -> Result<Vec<OwnedTree>> {
    Ok(sqlx::query_as(
        "SELECT path, state, repo, branch, start_commit FROM lane_owned_worktrees
         WHERE lane_key = ? AND repo IS NOT NULL AND branch IS NOT NULL ORDER BY rowid",
    )
    .bind(key)
    .fetch_all(pool)
    .await?)
}

async fn set_state(pool: &SqlitePool, key: &str, path: &str, state: &str) -> Result<()> {
    sqlx::query("UPDATE lane_owned_worktrees SET state = ? WHERE lane_key = ? AND path = ?")
        .bind(state)
        .bind(key)
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

async fn registered(repo: &Path, path: &str) -> Result<bool> {
    let list = git(repo, &["worktree", "list", "--porcelain"]).await?;
    Ok(list
        .lines()
        .any(|line| line.strip_prefix("worktree ") == Some(path)))
}

/// Re-adds a removed lane worktree from its branch (omo's restore_cwd on reopen).
pub async fn restore(pool: &SqlitePool, key: &str) -> Result<()> {
    for (path, state, repo, branch, _) in owned(pool, key).await? {
        if state == "registered" && tokio::fs::try_exists(&path).await.unwrap_or(false) {
            continue;
        }
        let repo = PathBuf::from(repo);
        if !registered(&repo, &path).await? {
            let _ = git(&repo, &["worktree", "prune"]).await;
            git(&repo, &["worktree", "add", &path, &branch]).await?;
        }
        set_state(pool, key, &path, "registered").await?;
        tracing::info!(key, path = %path, "lane worktree restored");
    }
    Ok(())
}

/// The lane branch holds commits its start commit lacks: the work is not merged anywhere.
async fn unmerged(repo: &Path, branch: &str, start: Option<&str>) -> Result<Option<String>> {
    let base = match start {
        Some(commit)
            if git(repo, &["merge-base", "--is-ancestor", commit, branch])
                .await
                .is_ok() =>
        {
            commit.to_owned()
        }
        _ => "HEAD".to_owned(),
    };
    let ahead: u64 = git(repo, &["rev-list", "--count", &format!("{base}..{branch}")])
        .await?
        .parse()
        .unwrap_or(0);
    Ok((ahead > 0).then(|| format!("{branch} ({ahead} commit(s) ahead)")))
}

async fn lane_event(pool: &SqlitePool, key: &str, kind: &str, body: &str) -> Result<()> {
    let lead: String = sqlx::query_scalar("SELECT lead_session_key FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_one(pool)
        .await?;
    let body: String = body.chars().take(2000).collect();
    let reported: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lane_events WHERE lane_key = ? AND kind = ? AND body = ?)",
    )
    .bind(key)
    .bind(kind)
    .bind(&body)
    .fetch_one(pool)
    .await?;
    if reported && kind == "unmerged_branch" {
        return Ok(());
    }
    let now = now_rfc3339();
    sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at, release_at) VALUES(?,?,?,?,?,?)",
    )
    .bind(key)
    .bind(lead)
    .bind(kind)
    .bind(body)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Closes a lane as omo's close intent does: remove its worktrees, report unmerged work,
/// then move it to closed. `reason` is the transition reason (`forced: …` for a lane that
/// is not verified). A failure leaves the lane in its status and tells the lead.
pub async fn close(pool: &SqlitePool, key: &str, reason: Option<&str>) -> Result<()> {
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM lanes WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    let Some(status) = status else {
        return Err(HubError::message(format!("lane {key} not found")));
    };
    let forced = reason.is_some_and(|reason| reason.starts_with("forced:"));
    let allowed = LaneStatus::Closed
        .allowed_from(reason)
        .iter()
        .any(|from| from.as_str() == status);
    if !allowed || (!forced && status != "verified_done") {
        return Err(HubError::message(format!(
            "IllegalTransition: {status} -> closed"
        )));
    }
    let result: Result<()> = async {
        for (path, state, repo, branch, start) in owned(pool, key).await? {
            let repo = PathBuf::from(repo);
            if state != "removed" {
                set_state(pool, key, &path, "removing").await?;
                let exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
                if exists || registered(&repo, &path).await? {
                    // No --force: a dirty tree refuses, and the owner's work stays put.
                    git(&repo, &["worktree", "remove", &path]).await?;
                }
                set_state(pool, key, &path, "removed").await?;
            }
            if let Some(note) = unmerged(&repo, &branch, start.as_deref()).await? {
                lane_event(
                    pool,
                    key,
                    "unmerged_branch",
                    &format!(
                        "lane work is on unmerged branch {note}; nothing was merged into the repository HEAD"
                    ),
                )
                .await?;
            }
        }
        transition(pool, key, LaneStatus::Closed, reason)
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        tracing::warn!(key, %error, "lane close failed");
        lane_event(pool, key, "close_failed", &format!("close {key}: {error}")).await?;
    }
    result
}

#[cfg(test)]
mod tests;
