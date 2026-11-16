//! Supervisor setup status: what this installation still needs before the unified supervisor
//! runs on its own, in the order the owner (or the app's setup driver) has to do it.
//!
//! [`setup_status`] reads only; it is the shared `SetupStatus` contract of the desktop app,
//! the daemon, the phone and the CLI. Its `nextAction` is the first unmet step: turn the
//! engine on, restart the hub generation so it is installed, wait for it, fix it, create the
//! owner, map a project, cut over, seed owner memory (when opted in), adopt old lanes.
//!
//! The engine state also reads the calls the engine actually made
//! ([`recorded_call_fault`]): an engine that reports itself ready while its turns keep failing
//! is `degraded`, and `supervisorHealthy` says so.
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::cutover::{self, LegacyIntake, Origin, AUTO_CUTOVER_KEY};
use super::{identities, SupervisorError, ADOPT_MESSAGE_PREFIX, LANE_KEY_PREFIX};
use crate::meta::mhc_profile::REQUIRED_CAPABILITIES;
use crate::meta::mhc_supervisor::{HealthState, SupervisorHealth};

/// `hub_meta` key: `1` when the owner opted in to seeding owner memory from earlier sessions.
pub const MEMORY_IMPORT_KEY: &str = "supervisor_memory_import";

/// Longest owner principal ([`super::provision_identity`] refuses longer ones).
const MAX_PRINCIPAL_CHARS: usize = 256;

/// How far back the recorded engine calls are read, and how many of the newest count.
const CALL_WINDOW_MINUTES: i64 = 30;
const CALL_WINDOW_CALLS: i64 = 20;
/// This many newest calls ending in [`EXITED`] make the engine degraded on their own.
const EXITED_STREAK: usize = 3;

/// `supervisor_model_calls.outcome` of a turn the engine answered.
const CALL_OK: &str = "ok";
/// The owner cancelled the turn (or its caller went away): not an engine failure.
const CALL_CANCELLED: &str = "MHC_CANCELLED";
const EXITED: &str = "MHC_ENGINE_EXITED";

/// The `engineState` of an engine whose recorded calls are failing.
const DEGRADED: &str = "degraded";

/// What the caller knows about the engine of this process.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineView {
    /// The engine is on for this process: the hub env file turns it on
    /// (`FERRYX_HUB_MHC_SUPERVISOR`) and this process's environment does not turn it off
    /// ([`Self::enabled_for`]).
    pub enabled: bool,
    /// An engine is installed in the running hub generation.
    pub installed: bool,
    /// Its health, when installed.
    pub health: Option<SupervisorHealth>,
}

impl EngineView {
    /// Whether the engine is on as [`crate::meta::mhc_profile::MhcProfileConfig::from_env_in`]
    /// decides it: `shared` (the hub env file entries) turns it on and `process` (this
    /// process's `FERRYX_HUB_MHC_SUPERVISOR`) does not turn it off. A process value never
    /// turns it on.
    pub fn enabled_for(
        shared: &std::collections::BTreeMap<String, String>,
        process: Option<&str>,
    ) -> bool {
        let process_off = process.is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        });
        crate::meta::mhc_profile::MhcProfileConfig::enabled_in(shared) && !process_off
    }

    /// The engine as this process sees it now: the hub env file read fresh, and the engine
    /// installed in this process, if any. Reads a file: call it off the async runtime threads.
    pub fn current() -> Self {
        let installed = crate::meta::mhc_supervisor::installed();
        let process = std::env::var(crate::meta::mhc_profile::ENABLE_KEY).ok();
        Self {
            enabled: Self::enabled_for(&crate::hub_env::read_file_env(), process.as_deref()),
            installed: installed.is_some(),
            health: installed.map(|engine| engine.health()),
        }
    }

    /// [`Self::current`] for async callers: the hub env file is read on the blocking pool so
    /// a slow disk never stalls a runtime worker. When that read cannot finish (its task
    /// panicked or was cancelled) the engine counts as off, and the installed engine is still
    /// reported as it is.
    pub async fn read() -> Self {
        Self::read_with(Self::current).await
    }

    /// [`Self::read`] with the blocking reader given (tests pass one that never touches the
    /// developer's hub env file).
    pub(crate) async fn read_with(reader: impl FnOnce() -> Self + Send + 'static) -> Self {
        match tokio::task::spawn_blocking(reader).await {
            Ok(view) => view,
            Err(error) => {
                tracing::warn!(%error, "the hub env file was not read; the supervisor engine counts as off");
                let installed = crate::meta::mhc_supervisor::installed();
                Self {
                    enabled: false,
                    installed: installed.is_some(),
                    health: installed.map(|engine| engine.health()),
                }
            }
        }
    }
}

/// The first unmet setup step. After a rollback the cutover is not named again: the owner
/// chose the legacy intake, and cutting over once more is an explicit `ferryx hub cutover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    EnableEngine,
    RestartRequired,
    WaitEngine,
    FixEngine,
    CreateOwner,
    MapProjects,
    Cutover,
    ImportMemory,
    Adopt,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineFault {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupOwner {
    pub supervisor_id: String,
    pub owner_principal: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupIntake {
    pub unified_open: bool,
    /// `open`, `stopped` or `idle`.
    pub legacy: String,
    /// `fresh`, `upgrade`, `cutover` or `rollback`.
    pub origin: String,
    pub legacy_history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryImportStatus {
    pub enabled: bool,
    pub sessions: i64,
    pub turns: i64,
    pub last_at: Option<String>,
}

/// The shared setup status (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub engine_enabled: bool,
    pub engine_installed: bool,
    /// `absent`, `starting`, `ready`, `waiting`, `degraded`, `stopped` or `exited`.
    pub engine_state: String,
    pub engine_fault: Option<EngineFault>,
    /// False when the engine's own health, or its recorded calls, say it is failing. Unknown
    /// call history never makes it false.
    pub supervisor_healthy: bool,
    pub missing_capabilities: Vec<String>,
    pub restart_required: bool,
    pub owner: Option<SetupOwner>,
    pub projects: i64,
    pub intake: SetupIntake,
    pub auto_cutover: bool,
    pub memory_import: MemoryImportStatus,
    pub adoptable: i64,
    pub next_action: NextAction,
}

fn legacy_text(legacy: LegacyIntake) -> &'static str {
    match legacy {
        LegacyIntake::Open => "open",
        LegacyIntake::Stopped => "stopped",
        LegacyIntake::Idle => "idle",
    }
}

fn origin_text(origin: Origin) -> &'static str {
    match origin {
        Origin::Fresh => "fresh",
        Origin::Upgrade => "upgrade",
        Origin::Cutover => "cutover",
        Origin::Rollback => "rollback",
    }
}

/// How the engine stands for setup.
enum EngineStep {
    Absent,
    Wait,
    Fix,
    Ready,
}

/// The newest recorded engine calls inside the window, newest first. Outcomes are `ok` or the
/// typed fault code the turn ended in ([`CALL_CANCELLED`] among them).
async fn recorded_call_outcomes(pool: &SqlitePool) -> Result<Vec<String>, SupervisorError> {
    let since = (Utc::now() - Duration::minutes(CALL_WINDOW_MINUTES)).to_rfc3339();
    let outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT outcome FROM supervisor_model_calls
          WHERE julianday(created_at) >= julianday(?)
          ORDER BY id DESC LIMIT ?",
    )
    .bind(since)
    .bind(CALL_WINDOW_CALLS)
    .fetch_all(pool)
    .await?;
    Ok(outcomes)
}

/// Whether a recorded outcome is an engine failure: a refusal, a crash, a timeout. A turn the
/// owner cancelled is not.
fn call_failed(outcome: &str) -> bool {
    outcome != CALL_OK && outcome != CALL_CANCELLED
}

/// The most frequent failure code, the most recent one on a tie (`failures` is newest first).
fn dominant_code<'a>(failures: &[&'a str]) -> Option<&'a str> {
    let mut counts: Vec<(&'a str, usize)> = Vec::new();
    for failure in failures.iter().copied() {
        match counts.iter_mut().find(|(code, _)| *code == failure) {
            Some((_, count)) => *count += 1,
            None => counts.push((failure, 1)),
        }
    }
    let mut best: Option<(&'a str, usize)> = None;
    for (code, count) in counts {
        if best.is_none_or(|(_, best_count)| count > best_count) {
            best = Some((code, count));
        }
    }
    best.map(|(code, _)| code)
}

/// The fault the recorded calls imply, `None` while they do not say the engine is failing.
///
/// No call in the window is an unknown engine, not a broken one: it stays as its own health
/// reports it. More than half the newest calls failing makes the engine degraded, as does a
/// newest run of [`EXITED_STREAK`] exits.
fn call_fault(outcomes: &[String]) -> Option<EngineFault> {
    let failures: Vec<&str> = outcomes
        .iter()
        .map(String::as_str)
        .filter(|outcome| call_failed(outcome))
        .collect();
    let exited_run = outcomes.len() >= EXITED_STREAK
        && outcomes[..EXITED_STREAK].iter().all(|outcome| outcome.as_str() == EXITED);
    let mostly_failing = failures.len() * 2 > outcomes.len();
    if !exited_run && !mostly_failing {
        return None;
    }
    let code = if exited_run {
        EXITED
    } else {
        dominant_code(&failures).unwrap_or(EXITED)
    };
    let message = if exited_run {
        format!("the supervisor engine exited on the last {EXITED_STREAK} calls")
    } else {
        format!(
            "{} of the last {} supervisor engine calls failed",
            failures.len(),
            outcomes.len()
        )
    };
    Some(EngineFault {
        code: code.to_owned(),
        message: format!("{message} ({code})"),
    })
}

/// [`call_fault`] over what this installation recorded.
async fn recorded_call_fault(pool: &SqlitePool) -> Result<Option<EngineFault>, SupervisorError> {
    Ok(call_fault(&recorded_call_outcomes(pool).await?))
}

/// Todo 2's `supervisorHealthy` for a caller that only has the pool: false when this
/// installation's newest recorded engine calls say the engine is failing. No recorded call is
/// an unknown engine, not a broken one, and reads healthy.
pub async fn supervisor_degraded(pool: &SqlitePool) -> bool {
    match recorded_call_fault(pool).await {
        Ok(fault) => fault.is_some(),
        // An unreadable log is not proof the engine works; the supervisor escalates instead.
        Err(error) => {
            tracing::warn!(%error, "the recorded engine calls could not be read");
            true
        }
    }
}

/// (state text, fault, missing capabilities, step) of the engine view.
fn engine_reading(engine: &EngineView) -> (String, Option<EngineFault>, Vec<String>, EngineStep) {
    if !engine.installed {
        return ("absent".into(), None, Vec::new(), EngineStep::Absent);
    }
    let Some(health) = &engine.health else {
        return ("starting".into(), None, Vec::new(), EngineStep::Wait);
    };
    let fault = health.fault.as_ref().map(|fault| EngineFault {
        code: fault.code().to_owned(),
        message: fault.to_string(),
    });
    let missing: Vec<String> = REQUIRED_CAPABILITIES
        .iter()
        .filter(|capability| !health.capabilities.iter().any(|have| have == *capability))
        .map(|capability| (*capability).to_owned())
        .collect();
    match health.state {
        HealthState::Ready if missing.is_empty() => ("ready".into(), fault, missing, EngineStep::Ready),
        HealthState::Ready => {
            let fault = fault.or_else(|| {
                Some(EngineFault {
                    code: "MHC_CAPABILITY_MISSING".into(),
                    message: format!(
                        "the mhc supervisor engine does not advertise {}",
                        missing.join(", ")
                    ),
                })
            });
            ("ready".into(), fault, missing, EngineStep::Fix)
        }
        HealthState::Degraded => ("degraded".into(), fault, Vec::new(), EngineStep::Fix),
        // Never launched yet: the engine starts with the hub generation or its first use.
        HealthState::Stopped if health.generation == 0 && fault.is_none() => {
            ("starting".into(), None, Vec::new(), EngineStep::Wait)
        }
        HealthState::Stopped | HealthState::Waiting | HealthState::Exited => {
            let state = match health.state {
                HealthState::Stopped => "stopped",
                HealthState::Waiting => "waiting",
                _ => "exited",
            };
            let step = if fault.is_some() { EngineStep::Fix } else { EngineStep::Wait };
            (state.into(), fault, Vec::new(), step)
        }
    }
}

async fn meta_flag(pool: &SqlitePool, key: &str) -> Result<bool, SupervisorError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value.as_deref() == Some("1"))
}

/// Legacy lanes the owner could adopt: queued or blocked, not admitted by the supervisor and
/// not already adopted.
const ADOPTABLE_WHERE: &str = "FROM lanes l
          WHERE substr(l.key, 1, length(?)) <> ?
            AND l.status IN ('queued','blocked')
            AND NOT EXISTS(SELECT 1 FROM supervisor_goals g WHERE g.lane_key = l.key)
            AND NOT EXISTS(SELECT 1 FROM supervisor_requests r
                            WHERE r.source_message_id = ? || l.key)";

/// How many legacy lanes the owner could adopt ([`ADOPTABLE_WHERE`]).
pub async fn adoptable_lanes(pool: &SqlitePool) -> Result<i64, SupervisorError> {
    Ok(sqlx::query_scalar(&format!("SELECT COUNT(*) {ADOPTABLE_WHERE}"))
    .bind(LANE_KEY_PREFIX)
    .bind(LANE_KEY_PREFIX)
    .bind(ADOPT_MESSAGE_PREFIX)
    .fetch_one(pool)
    .await?)
}

/// The keys of the lanes [`adoptable_lanes`] counts, in key order.
pub async fn adoptable_lane_keys(pool: &SqlitePool) -> Result<Vec<String>, SupervisorError> {
    Ok(sqlx::query_scalar(&format!("SELECT l.key {ADOPTABLE_WHERE} ORDER BY l.key"))
        .bind(LANE_KEY_PREFIX)
        .bind(LANE_KEY_PREFIX)
        .bind(ADOPT_MESSAGE_PREFIX)
        .fetch_all(pool)
        .await?)
}

/// This installation's setup status. Reads only. A ready engine whose unified intake is
/// closed asks for the cutover, except after a rollback: the owner deliberately went back to
/// the legacy intake, so `nextAction` is `none` instead of pushing the cutover again (memory
/// import and adoption are unified work and wait for that explicit cutover).
pub async fn setup_status(
    pool: &SqlitePool,
    engine: EngineView,
) -> Result<SetupStatus, SupervisorError> {
    let (mut engine_state, mut engine_fault, missing_capabilities, step) = engine_reading(&engine);
    // An installed engine that keeps failing its turns is degraded whatever it says about
    // itself. Its own fault, when it named one, stays the more specific diagnosis.
    if engine.installed {
        if let Some(fault) = recorded_call_fault(pool).await? {
            engine_state = DEGRADED.to_owned();
            engine_fault.get_or_insert(fault);
        }
    }
    let supervisor_healthy = engine_state != DEGRADED;
    let restart_required = engine.enabled && !engine.installed;
    let owner = identities(pool).await?.into_iter().next().map(|identity| SetupOwner {
        supervisor_id: identity.supervisor_id,
        owner_principal: identity.owner_principal,
    });
    let projects: i64 = match &owner {
        Some(owner) => sqlx::query_scalar("SELECT COUNT(*) FROM supervisor_projects WHERE supervisor_id = ?")
            .bind(&owner.supervisor_id)
            .fetch_one(pool)
            .await?,
        None => 0,
    };
    let intake = cutover::installation_intake(pool).await?;
    let auto_cutover = meta_flag(pool, AUTO_CUTOVER_KEY).await?;
    let totals = super::memory_import::import_totals(pool).await?;
    let memory_import = MemoryImportStatus {
        enabled: meta_flag(pool, MEMORY_IMPORT_KEY).await?,
        sessions: totals.sessions,
        turns: totals.turns,
        last_at: totals.last_at,
    };
    let adoptable = adoptable_lanes(pool).await?;
    let next_action = if !engine.enabled {
        NextAction::EnableEngine
    } else if restart_required {
        NextAction::RestartRequired
    } else {
        match step {
            EngineStep::Absent | EngineStep::Wait => NextAction::WaitEngine,
            EngineStep::Fix => NextAction::FixEngine,
            EngineStep::Ready if owner.is_none() => NextAction::CreateOwner,
            EngineStep::Ready if projects == 0 => NextAction::MapProjects,
            EngineStep::Ready if !intake.unified_open && intake.origin == Origin::Rollback => {
                NextAction::None
            }
            EngineStep::Ready if !intake.unified_open => NextAction::Cutover,
            EngineStep::Ready if memory_import.enabled && memory_import.last_at.is_none() => {
                NextAction::ImportMemory
            }
            EngineStep::Ready if adoptable > 0 => NextAction::Adopt,
            EngineStep::Ready => NextAction::None,
        }
    };
    Ok(SetupStatus {
        engine_enabled: engine.enabled,
        engine_installed: engine.installed,
        engine_state,
        engine_fault,
        supervisor_healthy,
        missing_capabilities,
        restart_required,
        owner,
        projects,
        intake: SetupIntake {
            unified_open: intake.unified_open,
            legacy: legacy_text(intake.legacy).to_owned(),
            origin: origin_text(intake.origin).to_owned(),
            legacy_history: intake.legacy_history,
        },
        auto_cutover,
        memory_import,
        adoptable,
        next_action,
    })
}

async fn set_flag(pool: &SqlitePool, key: &str, on: bool) -> Result<(), SupervisorError> {
    sqlx::query(
        "INSERT INTO hub_meta(key, value) VALUES(?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(if on { "1" } else { "0" })
    .execute(pool)
    .await?;
    Ok(())
}

/// Records the owner's setup choices; `None` leaves a choice unchanged.
pub async fn set_setup_flags(
    pool: &SqlitePool,
    auto_cutover: Option<bool>,
    memory_import: Option<bool>,
) -> Result<(), SupervisorError> {
    if let Some(on) = auto_cutover {
        set_flag(pool, AUTO_CUTOVER_KEY, on).await?;
    }
    if let Some(on) = memory_import {
        set_flag(pool, MEMORY_IMPORT_KEY, on).await?;
    }
    Ok(())
}

/// The Slack team (workspace) id this hub has stored a Slack conversation for, when exactly
/// one is known; automatic setup uses it for the owner principal `slack:<team>:<user>`.
pub async fn known_slack_team(pool: &SqlitePool) -> Result<Option<String>, SupervisorError> {
    let teams: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT guild_id FROM sessions
          WHERE platform = 'slack' AND guild_id IS NOT NULL AND guild_id <> '' LIMIT 2",
    )
    .fetch_all(pool)
    .await?;
    Ok(match teams.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
}

/// `text` without surrounding whitespace or control characters; `None` when nothing is left.
fn principal_part(text: Option<&str>) -> Option<String> {
    let cleaned: String = text?.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_owned())
}

/// The owner principal setup creates: the Slack owner `slack:<team>:<user>` when both are
/// known (the principal the Slack adapter authenticates), else `local:<account>`. The result
/// always passes the identity rules: no surrounding whitespace or control characters, at most
/// 256 characters, never empty.
pub fn default_owner_principal(
    slack_team: Option<&str>,
    slack_user: Option<&str>,
    local_account: &str,
) -> String {
    let principal = match (principal_part(slack_team), principal_part(slack_user)) {
        (Some(team), Some(user)) => format!("slack:{team}:{user}"),
        _ => format!(
            "local:{}",
            principal_part(Some(local_account)).unwrap_or_else(|| "owner".to_owned())
        ),
    };
    principal.chars().take(MAX_PRINCIPAL_CHARS).collect::<String>().trim_end().to_owned()
}
