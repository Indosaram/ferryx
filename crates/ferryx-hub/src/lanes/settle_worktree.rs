//! The git half of the settlement gate: what a lane's worktrees still owe before it may close.
//!
//! Hardenings this module owns (todo 6):
//! * every `GIT_*` override in the daemon's environment is cleared, so a hub started from inside
//!   a git hook or with `GIT_DIR` set cannot read a different repository than the lane's;
//! * `LC_ALL=C`, so the "not a git repository" probe does not depend on the host's language;
//! * an unreadable git state is reported as unsent work, never as a clean tree: a check that
//!   cannot answer is not a pass.
use sqlx::SqlitePool;

use crate::error::{HubError, Result};

async fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<std::process::Output> {
    let mut command = crate::child_env::command("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE")
        .kill_on_drop(true);
    // Every inherited or hub-file `GIT_*` variable goes: git reads dozens of them to relocate
    // the repository, index, objects or config, and a lane's checks must read the lane's own.
    let inherited = std::env::vars().map(|(name, _)| name);
    let from_hub_file = crate::hub_env::child_env().into_iter().map(|(name, _)| name);
    for name in inherited.chain(from_hub_file) {
        if name.starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
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

/// Uncommitted tracked files and unpushed commits on this lane's worktrees. A directory that is
/// not a git repository has nothing to settle; one whose git state cannot be read is unsent work.
pub(crate) async fn worktree_reasons(
    pool: &SqlitePool,
    key: &str,
    cwd: &str,
) -> Result<Vec<String>> {
    let cwd = std::path::Path::new(cwd);
    let probe = match git_output(cwd, &["rev-parse", "--is-inside-work-tree"]).await {
        Ok(output) => output,
        Err(error) => return Ok(vec![format!("git state unreadable: {error}")]),
    };
    if !probe.status.success() {
        let stderr = String::from_utf8_lossy(&probe.stderr);
        if stderr.contains("not a git repository") || stderr.contains("No such file") {
            return Ok(Vec::new());
        }
        return Ok(vec![format!(
            "git state unreadable: {}",
            stderr.trim().to_owned()
        )]);
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
    let listed = match git_text(&canonical, &["worktree", "list", "--porcelain"]).await {
        Ok(listed) => listed,
        Err(error) => {
            return Ok(vec![format!(
                "git worktree list unreadable for {}: {error}",
                canonical.display()
            )]);
        }
    };
    {
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
            let created = tokio::fs::metadata(&full)
                .await
                .ok()
                .and_then(|meta| meta.created().ok())
                .map(chrono::DateTime::<chrono::Utc>::from);
            // An unknown creation time is included: fail closed rather than skip the worktree.
            let include = full.starts_with(&canonical)
                || started.is_some_and(|start| created.is_none_or(|created| created >= start));
            if include && !paths.contains(&full) {
                paths.push(full);
            }
        }
    }
    let mut reasons = Vec::new();
    for path in paths {
        let status = match git_text(&path, &["status", "--porcelain", "--untracked-files=no"]).await
        {
            Ok(status) => status,
            Err(error) => {
                reasons.push(format!("git state unreadable for {}: {error}", path.display()));
                continue;
            }
        };
        if !status.trim().is_empty() {
            reasons.push(format!("uncommitted {}", path.display()));
        }
        let head = match git_output(&path, &["rev-parse", "--verify", "-q", "HEAD"]).await {
            Ok(head) => head,
            Err(error) => {
                reasons.push(format!("git state unreadable for {}: {error}", path.display()));
                continue;
            }
        };
        if head.status.code() == Some(1) {
            continue;
        }
        if !head.status.success() {
            reasons.push(format!(
                "git state unreadable for {}: {}",
                path.display(),
                String::from_utf8_lossy(&head.stderr).trim()
            ));
            continue;
        }
        let count_text =
            match git_text(&path, &["rev-list", "--count", "HEAD", "--not", "--remotes"]).await {
                Ok(count) => count,
                Err(error) => {
                    reasons.push(format!(
                        "git state unreadable for {}: {error}",
                        path.display()
                    ));
                    continue;
                }
            };
        let Ok(count) = count_text.trim().parse::<u64>() else {
            reasons.push(format!(
                "git state unreadable for {}: rev-list count {count_text:?}",
                path.display()
            ));
            continue;
        };
        if count > 0 {
            let remote = git_text(&path, &["remote"]).await.unwrap_or_default();
            if remote.trim().is_empty() {
                reasons.push(format!("unpushed {} (no remote)", path.display()));
            } else {
                reasons.push(format!("unpushed {} ({} commit(s))", path.display(), count));
            }
        }
    }
    Ok(reasons)
}
