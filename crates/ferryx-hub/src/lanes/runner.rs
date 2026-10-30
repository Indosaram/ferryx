//! Drives a lane by launching a pane and reading it through agent control.
//! A `[CLAIM_DONE]` line claims the lane. A `[REPORT]` line does not.
use std::sync::Arc;

use serde_json::json;
use sqlx::SqlitePool;

use super::guard::{GuardThresholds, HostLoad};
use super::settle::settle;
use super::state::{self, set_claim, set_host, transition, LaneStatus};
use crate::agent_control::{AgentControl, AgentOp, TaskLauncher};
use crate::error::{HubError, Result};
use crate::judge::Judge;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveReport {
    pub session_id: String,
    pub status: LaneStatus,
}

pub struct LaneRunner {
    pub pool: SqlitePool,
    pub control: Arc<dyn AgentControl>,
    pub launcher: Arc<dyn TaskLauncher>,
    pub judge: Judge,
    pub load: HostLoad,
    pub thresholds: GuardThresholds,
}

impl LaneRunner {
    pub async fn drive(&self, key: &str) -> Result<DriveReport> {
        let row: Option<(String, String, String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT status, cwd, title, done_signals, omo_thread_id, brief_path FROM lanes WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, cwd, title, done_signals, pane, brief_path)) = row else {
            return Err(HubError::message(format!("lane {key} not found")));
        };
        // omo's runner skips a paused lane: no launch, no nudge, no claim reading.
        if super::pause::is_paused(&self.pool, key).await? {
            return Err(HubError::message(format!("lane {key} is paused")));
        }
        let mut session_id = pane.unwrap_or_default();
        let mut cwd = cwd;
        let mut host = state::lane_host(&self.pool, key).await?;
        if status == "queued" {
            if self.load.admit(&self.thresholds) {
                // omo's auto worktree: a lane at a repository root gets its own checkout.
                if let Some(tree) = super::worktree::create(&self.pool, key, &cwd).await? {
                    cwd = tree;
                }
                session_id = self.launcher.launch_pane(&cwd, &[]).await?;
            } else {
                // This machine is full: hand the lane to the least busy paired host, in a new
                // worktree there. Signals that read local files or run commands keep it here.
                let target = if super::placement::portable(&done_signals) {
                    let hosts = self.launcher.remote_hosts().await?;
                    super::placement::idle_host(self.control.as_ref(), &hosts, self.thresholds.lanes_max).await
                } else {
                    None
                };
                let Some(target) = target else {
                    return Err(HubError::message(format!(
                        "guard refused to launch lane {key}"
                    )));
                };
                let remote = self.launcher.launch_remote_pane(&target, &cwd, key).await?;
                set_host(&self.pool, key, &target).await?;
                tracing::info!(lane = key, host = %target, "lane placed on a paired host");
                session_id = remote.session_id;
                cwd = remote.cwd;
                host = Some(target);
            }
            // The attach refuses a lane the supervisor adopted while this launch ran, in the
            // same statement, so the legacy path never starts a second worker on adopted work.
            if !crate::supervisor::attach_legacy_pane(&self.pool, key, &session_id).await? {
                // The launcher has no close operation; nothing was sent to the pane.
                tracing::warn!(
                    lane = key,
                    session_id = %session_id,
                    "lane was adopted by the supervisor during its legacy launch; the launched pane is left unused"
                );
                return Err(HubError::message(format!(
                    "{}: lane {key} was adopted by the supervisor during its legacy launch; the hub dispatcher drives it. The launched pane {session_id} was left unused and can be closed",
                    crate::supervisor::SUPERVISOR_OWNED
                )));
            }
            transition(&self.pool, key, LaneStatus::Working, Some("launched")).await?;
            // A lane with a brief file (cron remediation) sends the brief with the title.
            let brief = match brief_path.trim() {
                "" => String::new(),
                path => tokio::fs::read_to_string(path)
                    .await
                    .map(|brief| format!("\n\n{brief}"))
                    .unwrap_or_default(),
            };
            let text = format!(
                "{title}{brief}\n\nDone = {done_signals}\n\nClaim completion with a standalone line: [CLAIM_DONE] {key} | <checkable evidence>. A [REPORT] done line is not a claim."
            );
            self.control
                .execute(
                    host.as_deref(),
                    AgentOp::Send {
                        session_id: session_id.clone(),
                        text,
                        request_id: format!("lane-{key}-brief"),
                    },
                )
                .await?;
        } else if host.is_none() {
            // A reopened lane gets its removed worktree back before the pane is read.
            super::worktree::restore(&self.pool, key).await?;
        }
        let reading = self
            .control
            .execute(
                host.as_deref(),
                AgentOp::Read {
                    session_id: session_id.clone(),
                },
            )
            .await?;
        let screen = reading
            .pointer("/session/screen/text")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let history = reading
            .pointer("/session/history")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let source = if history.is_empty() {
            screen.to_owned()
        } else {
            history
        };
        if let Some(evidence) = claim_from_text(key, &source) {
            let current = state::status_of(&self.pool, key).await?;
            if current == Some(LaneStatus::Working) || current == Some(LaneStatus::Blocked) {
                set_claim(&self.pool, key, &evidence).await?;
                transition(&self.pool, key, LaneStatus::DoneClaimed, Some("claim")).await?;
                settle(&self.pool, key, &cwd, &done_signals, &evidence, &self.judge).await?;
            }
        }
        let status = state::status_of(&self.pool, key)
            .await?
            .ok_or_else(|| HubError::message(format!("lane {key} disappeared")))?;
        let _ = json!({"sessionId": session_id});
        Ok(DriveReport { session_id, status })
    }
}

/// A `[CLAIM_DONE] {key} | evidence` line is a claim. A `[REPORT]` line is not.
pub fn claim_from_text(key: &str, text: &str) -> Option<String> {
    let prefix = format!("[CLAIM_DONE] {key} | ");
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("[REPORT]") {
            continue;
        }
        if let Some(evidence) = line.strip_prefix(&prefix) {
            let evidence = evidence.trim();
            if !evidence.is_empty() {
                return Some(evidence.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::{FixedLauncher, RecordingControl};
    use crate::judge::Judge;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn a_report_line_is_not_a_claim() {
        let text = "[REPORT] done\n[CLAIM_DONE] lane-g4 | marker exists";
        assert_eq!(
            claim_from_text("lane-g4", text).as_deref(),
            Some("marker exists")
        );
        assert!(claim_from_text("lane-g4", "[REPORT] [CLAIM_DONE] lane-g4 | nope").is_none());
    }

    #[tokio::test]
    async fn a_lane_reaches_verified_done_on_a_pane() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-lane-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker"), "ok").unwrap();
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(
            &pool,
            "lane-g4",
            "prove the marker",
            "lead",
            dir.to_str().unwrap(),
            "file marker",
        )
        .await
        .unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({
            "session": {"screen": {"text": "[CLAIM_DONE] lane-g4 | marker exists\n", "revision": "7"}}
        });
        let runner = LaneRunner {
            pool: pool.clone(),
            control: control.clone(),
            launcher: Arc::new(FixedLauncher::new("pane-g4")),
            judge: Judge::fallback_yes(),
            load: HostLoad {
                load: 0.1,
                mem_free_pct: 50,
                disk_free_gb: 100,
                active_lanes: 0,
            },
            thresholds: GuardThresholds {
                load_alert: 10.0,
                mem_free_min_pct: 10,
                disk_free_min_gb: 1,
                lanes_max: 4,
            },
        };
        // A paused lane is not driven: no launch, no read.
        crate::lanes::pause::pause(&pool, "lane-g4", "session:lead")
            .await
            .unwrap();
        assert!(runner.drive("lane-g4").await.is_err());
        assert!(control.ops().is_empty());
        crate::lanes::pause::resume(&pool, "lane-g4").await.unwrap();
        let report = runner.drive("lane-g4").await.unwrap();
        assert_eq!(report.session_id, "pane-g4");
        assert_eq!(report.status, LaneStatus::VerifiedDone);
        let ops = control.ops();
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Send { .. })));
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Read { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Opens remote panes on whichever host it is asked for and records the requests.
    #[derive(Default)]
    struct RemoteLauncher {
        remote: std::sync::Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl TaskLauncher for RemoteLauncher {
        async fn launch_pane(&self, _cwd: &str, _command: &[String]) -> Result<String> {
            Err(HubError::message("this machine is full"))
        }
        async fn remote_hosts(&self) -> Result<Vec<String>> {
            Ok(vec!["mac".into(), "linux".into()])
        }
        async fn launch_remote_pane(
            &self,
            host_id: &str,
            _cwd: &str,
            lane_key: &str,
        ) -> Result<crate::agent_control::RemotePane> {
            self.remote.lock().unwrap().push((host_id.into(), lane_key.into()));
            Ok(crate::agent_control::RemotePane {
                session_id: "remote-pane".into(),
                cwd: "/remote/lane".into(),
            })
        }
    }

    fn full_runner(pool: SqlitePool, control: Arc<RecordingControl>, launcher: Arc<RemoteLauncher>) -> LaneRunner {
        LaneRunner {
            pool,
            control,
            launcher,
            judge: Judge::fallback_yes(),
            load: HostLoad { load: 0.1, mem_free_pct: 50, disk_free_gb: 100, active_lanes: 4 },
            thresholds: GuardThresholds { load_alert: 10.0, mem_free_min_pct: 10, disk_free_min_gb: 1, lanes_max: 4 },
        }
    }

    #[tokio::test]
    async fn a_refused_lane_runs_on_a_paired_host_and_is_driven_there() {
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "lane-r", "ship it", "lead", "/repo", "merged acme/app#12")
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"sessions": []});
        control.fail_hosts.lock().unwrap().insert("mac".into());
        let launcher = Arc::new(RemoteLauncher::default());
        let runner = full_runner(pool.clone(), control.clone(), launcher.clone());
        let report = runner.drive("lane-r").await.unwrap();
        assert_eq!(report.session_id, "remote-pane");
        assert_eq!(report.status, LaneStatus::Working);
        assert_eq!(*launcher.remote.lock().unwrap(), vec![("linux".to_owned(), "lane-r".to_owned())]);
        assert_eq!(state::lane_host(&pool, "lane-r").await.unwrap().as_deref(), Some("linux"));
        // The brief and the read both go to the host the lane runs on.
        let ops = control.ops();
        for (host, op) in &ops {
            if matches!(op, AgentOp::Send { .. } | AgentOp::Read { .. }) {
                assert_eq!(host.as_deref(), Some("linux"), "{op:?}");
            }
        }
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Send { .. })));
        assert!(ops.iter().any(|(_, op)| matches!(op, AgentOp::Read { .. })));
        assert_eq!(state::local_active_lanes(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_lane_with_local_signals_is_refused_instead_of_moved() {
        let pool = crate::storage::memory().await.unwrap();
        state::insert_lane(&pool, "lane-f", "write it", "lead", "/repo", "file marker")
            .await
            .unwrap();
        let control = Arc::new(RecordingControl::default());
        let launcher = Arc::new(RemoteLauncher::default());
        let runner = full_runner(pool.clone(), control.clone(), launcher.clone());
        let error = runner.drive("lane-f").await.unwrap_err();
        assert!(error.to_string().contains("guard refused"), "{error}");
        assert!(launcher.remote.lock().unwrap().is_empty());
        assert!(control.ops().is_empty());
        assert_eq!(state::status_of(&pool, "lane-f").await.unwrap(), Some(LaneStatus::Queued));
    }
}
