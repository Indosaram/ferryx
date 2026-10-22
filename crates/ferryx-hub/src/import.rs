use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

use crate::error::{HubError, Result};
use crate::storage;

/// One-time copy of the hub tables from an existing omo-gateway database.
///
/// A second invocation is refused. Each table is copied only when the source has it.
/// A column mismatch or a dropped row is an error. Nothing here starts by itself:
/// the daemon calls it only when import was invoked.
pub async fn import_once(pool: &SqlitePool, source: &Path) -> Result<ImportReport> {
    let already: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'imported_from'")
            .fetch_optional(pool)
            .await?;
    if already.is_some() {
        return Err(HubError::config(
            "omo-gateway import already ran for this hub database",
        ));
    }
    if !source.is_file() {
        return Err(HubError::config(format!(
            "omo-gateway database does not exist: {}",
            source.display()
        )));
    }
    let from = storage::connect_readonly(source).await?;
    let slack_home_channel = slack_home_from_omo_config(source);
    if let Some(channel) = &slack_home_channel {
        sqlx::query("INSERT OR IGNORE INTO hub_meta(key, value) VALUES('slack_home_channel', ?)")
            .bind(channel)
            .execute(pool)
            .await?;
    }
    let copied = copy_state(&from, pool).await?;
    let report = ImportReport {
        lanes: copied.lanes,
        watches: copied.watches,
        notices: copied.notices,
        issues: copied.issues,
        teams: copied.teams,
        states: copied.states,
        labels: copied.labels,
        activity: copied.activity,
        cron_jobs: copied.cron_jobs,
        inbox_items: copied.inbox_items,
        proposals: copied.proposals,
        allowlist: copied.allowlist,
        bot_profiles: copied.bot_profiles,
        lane_threads: copied.lane_threads,
        cron_jobs_enabled: copied.cron_jobs_enabled,
        slack_home_channel,
        tables: copied.tables,
    };
    let path = source.to_string_lossy().into_owned();
    sqlx::query("INSERT INTO hub_meta(key, value) VALUES('imported_from', ?)")
        .bind(&path)
        .execute(pool)
        .await?;
    from.close().await;
    Ok(report)
}

async fn table_exists(pool: &SqlitePool, name: &str) -> Result<bool> {
    let found: Option<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(found.is_some())
}

type LaneRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
);

async fn copy_lanes(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "lanes").await? {
        return Ok(0);
    }
    let rows: Vec<LaneRow> = sqlx::query_as(
        "SELECT key, title, lead_session_key, cwd, COALESCE(brief_path, ''), session_key, omo_thread_id, status, done_signals, claim_evidence, claim_at, COALESCE(turns_run, 0), last_activity_at FROM lanes",
    )
    .fetch_all(from)
    .await?;
    let mut copied = 0u64;
    for (
        key,
        title,
        lead,
        cwd,
        brief,
        session,
        thread,
        status,
        signals,
        evidence,
        claim_at,
        turns,
        activity,
    ) in rows
    {
        let inserted = sqlx::query(
            "INSERT INTO lanes(key,title,lead_session_key,cwd,brief_path,session_key,omo_thread_id,status,done_signals,claim_evidence,claim_at,turns_run,last_activity_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(key) DO UPDATE SET title = excluded.title, lead_session_key = excluded.lead_session_key, cwd = excluded.cwd, brief_path = excluded.brief_path, session_key = excluded.session_key, omo_thread_id = excluded.omo_thread_id, status = excluded.status, done_signals = excluded.done_signals, claim_evidence = excluded.claim_evidence, claim_at = excluded.claim_at, turns_run = excluded.turns_run, last_activity_at = excluded.last_activity_at \
             WHERE lanes.title IS NOT excluded.title OR lanes.lead_session_key IS NOT excluded.lead_session_key OR lanes.cwd IS NOT excluded.cwd OR lanes.brief_path IS NOT excluded.brief_path OR lanes.session_key IS NOT excluded.session_key OR lanes.omo_thread_id IS NOT excluded.omo_thread_id OR lanes.status IS NOT excluded.status OR lanes.done_signals IS NOT excluded.done_signals OR lanes.claim_evidence IS NOT excluded.claim_evidence OR lanes.claim_at IS NOT excluded.claim_at OR lanes.turns_run IS NOT excluded.turns_run OR lanes.last_activity_at IS NOT excluded.last_activity_at",
        )
        .bind(key)
        .bind(title)
        .bind(lead)
        .bind(cwd)
        .bind(brief)
        .bind(session)
        .bind(thread)
        .bind(status)
        .bind(signals)
        .bind(evidence)
        .bind(claim_at)
        .bind(turns)
        .bind(activity)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

async fn copy_watches(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "session_watches").await? {
        return Ok(0);
    }
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT target, added_at, COALESCE(reason, 'manual') FROM session_watches")
            .fetch_all(from)
            .await?;
    let mut copied = 0u64;
    for (target, added_at, reason) in rows {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO session_watches(target, added_at, reason) VALUES(?,?,?)",
        )
        .bind(target)
        .bind(added_at)
        .bind(reason)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

async fn copy_notices(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "session_watch_notices").await? {
        return Ok(0);
    }
    let rows: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT dedupe_key, target, kind, sent_at FROM session_watch_notices")
            .fetch_all(from)
            .await?;
    let mut copied = 0u64;
    for (key, target, kind, sent_at) in rows {
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO session_watch_notices(dedupe_key, target, kind, sent_at) VALUES(?,?,?,?)",
        )
        .bind(key)
        .bind(target)
        .bind(kind)
        .bind(sent_at)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub lanes: u64,
    pub watches: u64,
    pub notices: u64,
    pub issues: u64,
    pub teams: u64,
    pub states: u64,
    pub labels: u64,
    pub activity: u64,
    pub cron_jobs: u64,
    pub inbox_items: u64,
    pub proposals: u64,
    pub allowlist: u64,
    pub bot_profiles: u64,
    pub lane_threads: u64,
    pub cron_jobs_enabled: u64,
    pub slack_home_channel: Option<String>,
    pub tables: BTreeMap<String, u64>,
}

struct Copied {
    lanes: u64,
    watches: u64,
    notices: u64,
    issues: u64,
    teams: u64,
    states: u64,
    labels: u64,
    activity: u64,
    cron_jobs: u64,
    inbox_items: u64,
    proposals: u64,
    allowlist: u64,
    bot_profiles: u64,
    lane_threads: u64,
    cron_jobs_enabled: u64,
    tables: BTreeMap<String, u64>,
}

/// Counts source rows. Does not open or create a destination database.
pub async fn preview_from_omo(source: &Path) -> Result<ImportReport> {
    let from = storage::connect_readonly(source).await?;
    let mut tables = BTreeMap::new();
    for name in PARITY_TABLES {
        tables.insert((*name).to_owned(), count_rows(&from, name).await?);
    }
    tables.insert("lanes".into(), count_rows(&from, "lanes").await?);
    tables.insert("cron_jobs".into(), count_rows(&from, "cron_jobs").await?);
    let cron_jobs_enabled = count_enabled_cron(&from).await?;
    from.close().await;
    Ok(ImportReport {
        lanes: tables["lanes"],
        issues: *tables.get("tracker_issues").unwrap_or(&0),
        teams: *tables.get("tracker_teams").unwrap_or(&0),
        states: *tables.get("tracker_states").unwrap_or(&0),
        labels: *tables.get("tracker_labels").unwrap_or(&0),
        activity: *tables.get("tracker_activity").unwrap_or(&0),
        cron_jobs: *tables.get("cron_jobs").unwrap_or(&0),
        inbox_items: *tables.get("inbox_items").unwrap_or(&0),
        proposals: *tables.get("action_proposals").unwrap_or(&0),
        allowlist: *tables.get("approval_allowlist").unwrap_or(&0),
        bot_profiles: *tables.get("bot_profiles").unwrap_or(&0),
        lane_threads: *tables.get("lane_thread_bindings").unwrap_or(&0),
        cron_jobs_enabled,
        tables,
        ..ImportReport::default()
    })
}

/// Opens `dest` and imports, or counts the source when `dry_run` is set.
/// A dry run does not create `dest` or its parent directory.
/// `cwd` overrides the working directory recorded for job scripts.
/// `record_as` is the path stored in `imported_from` when the bytes came from a snapshot.
pub async fn open_and_migrate(
    dest: &Path,
    source: &Path,
    dry_run: bool,
    cwd: Option<&Path>,
    record_as: Option<&Path>,
) -> Result<ImportReport> {
    if dry_run {
        return preview_from_omo(source).await;
    }
    if let Some(parent) = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| HubError::message(format!("database directory: {error}")))?;
    }
    let pool = storage::connect(dest).await?;
    let report = migrate_from_omo(&pool, source, false).await?;
    if let Some(cwd) = cwd.filter(|cwd| !cwd.as_os_str().is_empty()) {
        remember_working_directory(&pool, cwd).await?;
    }
    if let Some(record_as) = record_as.filter(|path| !path.as_os_str().is_empty()) {
        let text = record_as.to_string_lossy();
        sqlx::query("INSERT OR REPLACE INTO hub_meta(key, value) VALUES('imported_from', ?)")
            .bind(text.as_ref())
            .execute(&pool)
            .await?;
    }
    pool.close().await;
    Ok(report)
}

/// Idempotent copy. A second run inserts nothing new. Cron rows, issue rows, and
/// inbox rows whose source values changed are updated. SQL errors are returned.
/// `dry_run` counts source rows and writes nothing, including the destination file.
pub async fn migrate_from_omo(
    pool: &SqlitePool,
    source: &Path,
    dry_run: bool,
) -> Result<ImportReport> {
    if dry_run {
        return preview_from_omo(source).await;
    }
    let from = storage::connect_readonly(source).await?;
    let slack_home_channel = slack_home_from_omo_config(source);
    if let Some(channel) = &slack_home_channel {
        sqlx::query("INSERT OR IGNORE INTO hub_meta(key, value) VALUES('slack_home_channel', ?)")
            .bind(channel)
            .execute(pool)
            .await?;
    }
    let copied = copy_state(&from, pool).await?;
    sqlx::query("INSERT OR REPLACE INTO hub_meta(key, value) VALUES('imported_from', ?)")
        .bind(source.to_string_lossy().as_ref())
        .execute(pool)
        .await?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    remember_working_directory(pool, &omo_working_directory(source, home.as_deref())).await?;
    from.close().await;
    Ok(ImportReport {
        lanes: copied.lanes,
        watches: copied.watches,
        notices: copied.notices,
        issues: copied.issues,
        teams: copied.teams,
        states: copied.states,
        labels: copied.labels,
        activity: copied.activity,
        cron_jobs: copied.cron_jobs,
        inbox_items: copied.inbox_items,
        proposals: copied.proposals,
        allowlist: copied.allowlist,
        bot_profiles: copied.bot_profiles,
        lane_threads: copied.lane_threads,
        cron_jobs_enabled: copied.cron_jobs_enabled,
        slack_home_channel,
        tables: copied.tables,
    })
}

/// Live gateway database. The launchd plist `WorkingDirectory` wins, then the
/// checkout at `~/code/project/omon-gateway/omon_gateway.db`, then `~/.omon/gateway.db`.
/// The plist is read only for that one string. Nothing from it is logged.
pub fn discover_omo_database(home: &Path) -> Option<PathBuf> {
    let plist = home.join("Library/LaunchAgents/ai.omon.gateway.plist");
    if let Some(dir) = working_directory(&plist) {
        let db = PathBuf::from(dir).join("omon_gateway.db");
        if db.is_file() {
            return Some(db);
        }
    }
    let project = home.join("code/project/omon-gateway/omon_gateway.db");
    if project.is_file() {
        return Some(project);
    }
    let legacy = home.join(".omon/gateway.db");
    legacy.is_file().then_some(legacy)
}

fn working_directory(plist: &Path) -> Option<String> {
    let text = std::fs::read_to_string(plist).ok()?;
    let key = "<key>WorkingDirectory</key>";
    let rest = text.get(text.find(key)? + key.len()..)?;
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    let value = rest[start..start + end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Directory job scripts run in. The gateway plist's `WorkingDirectory` wins when
/// that directory exists, including when `source` is a sqlite `.backup` snapshot.
/// Otherwise the database's parent directory is used.
pub fn omo_working_directory(source: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(home) = home {
        let plist = home.join("Library/LaunchAgents/ai.omon.gateway.plist");
        if let Some(dir) = working_directory(&plist) {
            let dir = PathBuf::from(dir);
            if dir.is_dir() || source == dir.join("omon_gateway.db") {
                return dir;
            }
        }
    }
    source
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn remember_working_directory(pool: &SqlitePool, dir: &Path) -> Result<()> {
    let text = dir.to_string_lossy();
    sqlx::query("INSERT OR REPLACE INTO hub_meta(key, value) VALUES('omo_working_directory', ?)")
        .bind(text.as_ref())
        .execute(pool)
        .await?;
    Ok(())
}

const PARITY_TABLES: &[&str] = &[
    "tracker_users",
    "tracker_teams",
    "tracker_states",
    "tracker_labels",
    "tracker_projects",
    "tracker_project_teams",
    "tracker_project_members",
    "tracker_milestones",
    "tracker_cycles",
    "tracker_issues",
    "tracker_issue_labels",
    "tracker_subscribers",
    "tracker_comments",
    "tracker_reactions",
    "tracker_relations",
    "tracker_links",
    "tracker_activity",
    "tracker_views",
    "tracker_favorites",
    "tracker_notifications",
    "tracker_meta",
    "inbox_items",
    "inbox_decisions",
    "inbox_cursors",
    "approval_allowlist",
    "action_precedents",
    "action_proposals",
    "bot_profiles",
    "sessions",
    "messages",
    "memories",
    "lane_thread_bindings",
    "cron_problems",
];

async fn copy_state(from: &SqlitePool, into: &SqlitePool) -> Result<Copied> {
    let lanes = copy_lanes(from, into).await?;
    let _ = copy_thread_refs(from, into).await?;
    let watches = copy_watches(from, into).await?;
    let notices = copy_notices(from, into).await?;
    let mut tables = BTreeMap::new();
    for name in PARITY_TABLES {
        let (copied, skipped) = copy_named(from, into, name).await?;
        tables.insert((*name).to_owned(), copied);
        if skipped > 0 {
            tables.insert(format!("orphans_skipped.{name}"), skipped);
        }
    }
    let cron_jobs = copy_cron_jobs(from, into).await?;
    tables.insert("cron_jobs".into(), cron_jobs);
    Ok(Copied {
        lanes,
        watches,
        notices,
        issues: tables["tracker_issues"],
        teams: tables["tracker_teams"],
        states: tables["tracker_states"],
        labels: tables["tracker_labels"],
        activity: tables["tracker_activity"],
        cron_jobs,
        inbox_items: tables["inbox_items"],
        proposals: tables["action_proposals"],
        allowlist: tables["approval_allowlist"],
        bot_profiles: tables["bot_profiles"],
        lane_threads: *tables.get("lane_thread_bindings").unwrap_or(&0),
        cron_jobs_enabled: count_enabled_cron(from).await?,
        tables,
    })
}

async fn count_enabled_cron(pool: &SqlitePool) -> Result<u64> {
    if !table_exists(pool, "cron_jobs").await? {
        return Ok(0);
    }
    let names = columns(pool, "cron_jobs").await?;
    if names.iter().any(|name| name == "enabled") {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_jobs WHERE enabled = 1")
            .fetch_one(pool)
            .await?;
        return Ok(count as u64);
    }
    count_rows(pool, "cron_jobs").await
}

async fn copy_thread_refs(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "lanes").await? {
        return Ok(0);
    }
    let names = columns(from, "lanes").await?;
    if !names.iter().any(|name| name == "thread_channel_id") {
        return Ok(0);
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, thread_channel_id FROM lanes WHERE thread_channel_id IS NOT NULL AND thread_channel_id != ''",
    )
    .fetch_all(from)
    .await?;
    let mut copied = 0u64;
    for (key, thread) in rows {
        let updated = sqlx::query("UPDATE lanes SET thread_channel_id = ? WHERE key = ?")
            .bind(&thread)
            .bind(&key)
            .execute(into)
            .await?;
        copied += updated.rows_affected();
    }
    Ok(copied)
}

async fn count_rows(pool: &SqlitePool, table: &str) -> Result<u64> {
    if !table_exists(pool, table).await? {
        return Ok(0);
    }
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count: i64 = sqlx::query_scalar(&sql).fetch_one(pool).await?;
    Ok(count as u64)
}

async fn columns(pool: &SqlitePool, table: &str) -> Result<Vec<String>> {
    let sql = format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid");
    let names: Vec<String> = sqlx::query_scalar(&sql).fetch_all(pool).await?;
    Ok(names)
}

/// Copies the columns that exist on both sides. A missing source table copies nothing.
/// A source table that is the live tracker shape but cannot satisfy the destination
/// returns the sqlite error instead of an empty success.
async fn copy_named(from: &SqlitePool, into: &SqlitePool, table: &str) -> Result<(u64, u64)> {
    if !table_exists(from, table).await? || !table_exists(into, table).await? {
        return Ok((0, 0));
    }
    if table == "tracker_issues" {
        let source_cols = columns(from, table).await?;
        if source_cols.iter().any(|name| name == "description") {
            for required in [
                "id",
                "title",
                "description",
                "state_id",
                "team_id",
                "creator_id",
            ] {
                if !source_cols.iter().any(|name| name == required) {
                    return Err(HubError::message(format!(
                        "tracker_issues import refused: source is missing column {required}"
                    )));
                }
            }
        }
    }
    let source_cols = columns(from, table).await?;
    let dest_cols = columns(into, table).await?;
    let shared: Vec<&str> = dest_cols
        .iter()
        .filter(|name| source_cols.iter().any(|source| source == *name))
        .map(String::as_str)
        .collect();
    if shared.is_empty() {
        return Err(HubError::message(format!("no shared columns for {table}")));
    }
    let list = shared.join(", ");
    // Rows whose ON DELETE CASCADE parent is gone in the source are rows sqlite would
    // already have deleted with foreign keys on. They are skipped and counted, not copied,
    // because the destination enforces foreign keys and would fail the whole import.
    // An ON DELETE SET NULL reference to a missing parent is copied as NULL.
    let (projection, filter) = orphan_guards(from, into, table, &shared).await?;
    let selected = sqlx::query(&format!("SELECT {projection} FROM {table}{filter}"))
        .fetch_all(from)
        .await?;
    let skipped = count_rows(from, table)
        .await?
        .saturating_sub(selected.len() as u64);
    let placeholders = vec!["?"; shared.len()].join(", ");
    let primary = primary_key(into, table).await?;
    let insert_sql = if refreshes_on_conflict(table) {
        refresh_sql(table, &shared, &primary).unwrap_or_else(|| {
            format!("INSERT OR IGNORE INTO {table} ({list}) VALUES ({placeholders})")
        })
    } else {
        format!("INSERT OR IGNORE INTO {table} ({list}) VALUES ({placeholders})")
    };
    let mut copied = 0u64;
    for row in selected {
        let mut values = Vec::with_capacity(shared.len());
        for index in 0..shared.len() {
            values.push(cell_text(&row, index)?);
        }
        let mut statement = sqlx::query(&insert_sql);
        for value in &values {
            statement = statement.bind(value.clone());
        }
        let affected = statement.execute(into).await?.rows_affected();
        if affected == 0
            && !primary.is_empty()
            && !row_exists(into, table, &primary, &shared, &values).await?
        {
            return Err(HubError::message(format!(
                "{table} import skipped a row that is not already present"
            )));
        }
        copied += affected;
    }
    Ok((copied, skipped))
}

/// Single-column destination foreign keys, checked against the source copy of the parent.
async fn orphan_guards(
    from: &SqlitePool,
    into: &SqlitePool,
    table: &str,
    shared: &[&str],
) -> Result<(String, String)> {
    let sql = format!(
        "SELECT \"table\", \"from\", \"to\", on_delete FROM pragma_foreign_key_list('{table}') \
         WHERE id IN (SELECT id FROM pragma_foreign_key_list('{table}') GROUP BY id HAVING COUNT(*) = 1)"
    );
    let keys: Vec<(String, String, Option<String>, String)> =
        sqlx::query_as(&sql).fetch_all(into).await?;
    let mut projection: Vec<String> = shared.iter().map(|name| (*name).to_owned()).collect();
    let mut filters = Vec::new();
    for (parent, column, to, on_delete) in keys {
        let Some(index) = shared.iter().position(|name| *name == column) else {
            continue;
        };
        if !table_exists(from, &parent).await? {
            continue;
        }
        let to = match to {
            Some(to) => to,
            None => match primary_key(from, &parent).await?.as_slice() {
                [only] => only.clone(),
                _ => continue,
            },
        };
        if !columns(from, &parent).await?.contains(&to) {
            continue;
        }
        let present = format!("{column} IN (SELECT {to} FROM {parent})");
        match on_delete.to_ascii_uppercase().as_str() {
            "CASCADE" => filters.push(format!("({column} IS NULL OR {present})")),
            "SET NULL" => {
                projection[index] = format!("CASE WHEN {present} THEN {column} END AS {column}")
            }
            _ => {}
        }
    }
    let filter = if filters.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", filters.join(" AND "))
    };
    Ok((projection.join(", "), filter))
}

/// Tables whose rows can change in omo between the cutover snapshot and bootout.
fn refreshes_on_conflict(table: &str) -> bool {
    matches!(
        table,
        "tracker_issues" | "inbox_items" | "inbox_decisions" | "inbox_cursors" | "action_proposals"
    )
}

fn refresh_sql(table: &str, columns: &[&str], primary: &[String]) -> Option<String> {
    if primary.is_empty() || primary.iter().any(|key| !columns.contains(&key.as_str())) {
        return None;
    }
    let updates: Vec<&str> = columns
        .iter()
        .copied()
        .filter(|column| !primary.iter().any(|key| key.as_str() == *column))
        .collect();
    if updates.is_empty() {
        return None;
    }
    let list = columns.join(", ");
    let placeholders = vec!["?"; columns.len()].join(", ");
    let set = updates
        .iter()
        .map(|column| format!("{column} = excluded.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    let changed = updates
        .iter()
        .map(|column| format!("{table}.{column} IS NOT excluded.{column}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let conflict = primary.join(", ");
    Some(format!(
        "INSERT INTO {table} ({list}) VALUES ({placeholders}) ON CONFLICT({conflict}) DO UPDATE SET {set} WHERE {changed}"
    ))
}

async fn primary_key(pool: &SqlitePool, table: &str) -> Result<Vec<String>> {
    let sql = format!("SELECT name FROM pragma_table_info('{table}') WHERE pk > 0 ORDER BY pk");
    Ok(sqlx::query_scalar(&sql).fetch_all(pool).await?)
}

async fn row_exists(
    pool: &SqlitePool,
    table: &str,
    primary: &[String],
    columns: &[&str],
    values: &[Option<String>],
) -> Result<bool> {
    let mut filters = Vec::new();
    let mut bound = Vec::new();
    for key in primary {
        let Some(index) = columns.iter().position(|name| name == key) else {
            return Ok(false);
        };
        filters.push(format!("{key} IS ?"));
        bound.push(values.get(index).cloned().unwrap_or(None));
    }
    let sql = format!(
        "SELECT 1 FROM {table} WHERE {} LIMIT 1",
        filters.join(" AND ")
    );
    let mut query = sqlx::query_scalar::<_, i64>(&sql);
    for value in bound {
        query = query.bind(value);
    }
    Ok(query.fetch_optional(pool).await?.is_some())
}

fn cell_text(row: &sqlx::sqlite::SqliteRow, index: usize) -> Result<Option<String>> {
    if let Ok(value) = row.try_get::<Option<String>, _>(index) {
        return Ok(value);
    }
    if let Ok(value) = row.try_get::<Option<i64>, _>(index) {
        return Ok(value.map(|number| number.to_string()));
    }
    if let Ok(value) = row.try_get::<Option<f64>, _>(index) {
        return Ok(value.map(|number| number.to_string()));
    }
    Err(HubError::message(format!(
        "import column {index} is not text or a number"
    )))
}

async fn copy_cron_jobs(from: &SqlitePool, into: &SqlitePool) -> Result<u64> {
    if !table_exists(from, "cron_jobs").await? {
        return Ok(0);
    }
    let cols = columns(from, "cron_jobs").await?;
    if !cols.iter().any(|name| name == "payload_json") {
        return Ok(0);
    }
    type CronRow = (String, Option<String>, String, String, i64, Option<String>);
    let rows: Vec<CronRow> = sqlx::query_as(
        "SELECT id, session_key, expression, payload_json, enabled, next_run_at FROM cron_jobs",
    )
    .fetch_all(from)
    .await?;
    let mut copied = 0u64;
    for (id, session_key, expression, payload_json, enabled, next_run_at) in rows {
        let (_, delivery) = crate::schedule::map_payload(&payload_json)?;
        let inserted = sqlx::query(
            "INSERT INTO scheduled_jobs(id, session_key, expression, payload_json, enabled, next_run_at, delivery_json) VALUES(?,?,?,?,?,?,?) \
             ON CONFLICT(id) DO UPDATE SET session_key = excluded.session_key, expression = excluded.expression, payload_json = excluded.payload_json, enabled = excluded.enabled, next_run_at = excluded.next_run_at, delivery_json = excluded.delivery_json \
             WHERE scheduled_jobs.session_key IS NOT excluded.session_key OR scheduled_jobs.expression IS NOT excluded.expression OR scheduled_jobs.payload_json IS NOT excluded.payload_json OR scheduled_jobs.enabled IS NOT excluded.enabled OR scheduled_jobs.next_run_at IS NOT excluded.next_run_at OR scheduled_jobs.delivery_json IS NOT excluded.delivery_json",
        )
        .bind(&id)
        .bind(&session_key)
        .bind(&expression)
        .bind(&payload_json)
        .bind(enabled)
        .bind(&next_run_at)
        .bind(&delivery)
        .execute(into)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

/// Home channel copied by the last import, if the omo-gateway config named one.
pub async fn imported_slack_home(pool: &SqlitePool) -> Result<Option<String>> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'slack_home_channel'")
            .fetch_optional(pool)
            .await?;
    Ok(value.and_then(|value| crate::config::normalize_slack_home(&value)))
}

/// `SLACK_HOME_CHANNEL` from the omo-gateway config next to the database.
///
/// Looks in the database directory, then its parent, at `config.yaml`, `config.yml`,
/// `config.json`, and `.env`. A Discord `home_channel` is not a Slack channel.
pub fn slack_home_from_omo_config(database: &Path) -> Option<String> {
    let mut dirs = Vec::new();
    if let Some(dir) = database.parent() {
        dirs.push(dir.to_path_buf());
        if let Some(parent) = dir.parent() {
            if parent != dir {
                dirs.push(parent.to_path_buf());
            }
        }
    }
    for dir in dirs {
        for name in ["config.yaml", "config.yml", "config.json", ".env"] {
            let path = dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let text = text.chars().take(65_536).collect::<String>();
            if let Some(channel) = slack_home_in_config_text(&text, name) {
                return Some(channel);
            }
        }
    }
    None
}

fn slack_home_in_config_text(text: &str, name: &str) -> Option<String> {
    if name.ends_with(".json") {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let raw = value
            .get("slackHomeChannel")
            .or_else(|| value.pointer("/slack/home_channel"))
            .or_else(|| value.get("SLACK_HOME_CHANNEL"))
            .and_then(|item| item.as_str())?;
        return crate::config::normalize_slack_home(raw);
    }
    if name == ".env" {
        return slack_home_from_env_file(text);
    }
    slack_home_from_yaml(text)
}

fn slack_home_from_env_file(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        if key.trim() != "SLACK_HOME_CHANNEL" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        return crate::config::normalize_slack_home(value);
    }
    None
}

fn slack_home_from_yaml(text: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        if indent == 0 {
            section = trimmed.strip_suffix(':').unwrap_or("");
            continue;
        }
        if section != "slack" {
            continue;
        }
        let Some(value) = trimmed.strip_prefix("home_channel:") else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        return crate::config::normalize_slack_home(value);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn import_copies_lanes_once_and_refuses_a_second_run() {
        let dir = std::env::temp_dir().join(format!("ferryx-hub-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("omo.sqlite");
        let source_pool = storage::connect(&source).await.unwrap();
        sqlx::query("INSERT INTO lanes(key,title,lead_session_key,cwd,session_key,status,done_signals,last_activity_at) VALUES('lane-a','A','lead','/tmp','lead','queued','file /tmp/x','2020-01-01T00:00:00Z')")
            .execute(&source_pool)
            .await
            .unwrap();
        source_pool.close().await;

        let dest = storage::memory().await.unwrap();
        let report = import_once(&dest, &source).await.unwrap();
        assert_eq!(report.lanes, 1);
        let again = import_once(&dest, &source).await;
        assert!(again.is_err(), "a second import must be refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_omo_config_home_channel_is_slack_and_not_the_discord_one() {
        let yaml = "discord:\n  home_channel: \"999\"\nslack:\n  home_channel: \"#C0C9XG5FP8W\"\n";
        assert_eq!(slack_home_from_yaml(yaml).as_deref(), Some("C0C9XG5FP8W"));
        assert_eq!(
            slack_home_from_env_file("export SLACK_HOME_CHANNEL=\"CHOME,COTHER\"\n").as_deref(),
            Some("CHOME")
        );
        assert!(slack_home_from_yaml("discord:\n  home_channel: \"999\"\n").is_none());
    }

    #[tokio::test]
    async fn import_carries_the_slack_home_channel_and_the_hub_uses_it() {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            "discord:\n  home_channel: \"999\"\nslack:\n  home_channel: \"C0C9XG5FP8W\"\n",
        )
        .unwrap();
        let source = dir.join("omo.sqlite");
        let source_pool = storage::connect(&source).await.unwrap();
        source_pool.close().await;

        let dest_path = dir.join("hub.sqlite");
        let dest = storage::connect(&dest_path).await.unwrap();
        let report = import_once(&dest, &source).await.unwrap();
        assert_eq!(report.slack_home_channel.as_deref(), Some("C0C9XG5FP8W"));
        assert_eq!(
            imported_slack_home(&dest).await.unwrap().as_deref(),
            Some("C0C9XG5FP8W")
        );
        dest.close().await;

        let hub = crate::hub::Hub::open(
            crate::config::HubConfig {
                database_path: Some(dest_path),
                owner_slack_id: Some("UOWNER".into()),
                ..crate::config::HubConfig::default()
            },
            std::sync::Arc::new(crate::agent_control::RecordingControl::default()),
        )
        .await
        .unwrap();
        let session = hub.owner_session();
        assert_eq!(session.channel_id, "C0C9XG5FP8W");
        assert_eq!(session.user_id, "UOWNER");
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn raw_db(path: &std::path::Path, sql: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let uri = format!("sqlite://{}", path.display());
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(&uri)
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        for statement in sql.split(';') {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool.close().await;
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ferryx-hub-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn linear_tracker_cron_and_allowlist_import_and_a_missing_column_is_an_error() {
        let dir = scratch("parity-import");
        let source = dir.join("omo.sqlite");
        raw_db(
            &source,
            "
            CREATE TABLE tracker_users (id TEXT PRIMARY KEY, name TEXT, display_name TEXT, email TEXT, avatar_color TEXT, kind TEXT, model TEXT, is_me INTEGER, created_at TEXT);
            INSERT INTO tracker_users VALUES ('u1','ada','Ada',NULL,'#000','human',NULL,1,'2020-01-01T00:00:00Z');
            CREATE TABLE tracker_teams (id TEXT PRIMARY KEY, key TEXT, name TEXT, icon TEXT, color TEXT, issue_seq INTEGER, system INTEGER, cycles_enabled INTEGER, cycle_weeks INTEGER, triage_enabled INTEGER, created_at TEXT, sort_order REAL);
            INSERT INTO tracker_teams VALUES ('t1','AGT','Agents','bot','#fff',1,0,0,2,0,'2020-01-01T00:00:00Z',0);
            CREATE TABLE tracker_states (id TEXT PRIMARY KEY, team_id TEXT, name TEXT, type TEXT, color TEXT, position REAL, lane_status TEXT, description TEXT);
            INSERT INTO tracker_states VALUES ('s1','t1','Todo','unstarted','#fff',0,NULL,NULL);
            CREATE TABLE tracker_labels (id TEXT PRIMARY KEY, team_id TEXT, name TEXT, color TEXT, parent_id TEXT, description TEXT, created_at TEXT);
            INSERT INTO tracker_labels VALUES ('l1','t1','bug','#f00',NULL,NULL,'2020-01-01T00:00:00Z');
            CREATE TABLE tracker_issues (id TEXT PRIMARY KEY, team_id TEXT, number INTEGER, identifier TEXT, title TEXT, description TEXT, state_id TEXT, priority INTEGER, creator_id TEXT, created_at TEXT, updated_at TEXT);
            INSERT INTO tracker_issues VALUES ('i1','t1',1,'AGT-1','Gate','the body','s1',0,'u1','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z');
            CREATE TABLE tracker_activity (id INTEGER PRIMARY KEY, issue_id TEXT, actor_id TEXT, kind TEXT, from_value TEXT, to_value TEXT, created_at TEXT);
            INSERT INTO tracker_activity (issue_id, actor_id, kind, created_at) VALUES ('i1','u1','created','2020-01-01T00:00:00Z');
            CREATE TABLE cron_jobs (id TEXT, session_key TEXT, expression TEXT, payload_json TEXT, enabled INTEGER, next_run_at TEXT, created_at TEXT);
            INSERT INTO cron_jobs VALUES ('c1',NULL,'every 15m','{\"deliver\":\"slack:C0WORK:1700.0001\"}',1,NULL,'2020-01-01T00:00:00Z');
            CREATE TABLE approval_allowlist (pattern_key TEXT PRIMARY KEY, created_at TEXT);
            INSERT INTO approval_allowlist VALUES ('git status','2020-01-01T00:00:00Z');
            INSERT INTO approval_allowlist VALUES ('cargo test','2020-01-01T00:00:00Z');
            INSERT INTO approval_allowlist VALUES ('ls','2020-01-01T00:00:00Z');
            INSERT INTO approval_allowlist VALUES ('pwd','2020-01-01T00:00:00Z');
            INSERT INTO approval_allowlist VALUES ('true','2020-01-01T00:00:00Z');
            CREATE TABLE inbox_items (id TEXT PRIMARY KEY, source TEXT, source_ref TEXT, thread_ref TEXT, author TEXT, title TEXT, snippet TEXT, received_at TEXT, link TEXT, raw_hash TEXT, collected_at TEXT);
            INSERT INTO inbox_items VALUES ('in1','github','ref-1',NULL,'octo','ci','[CI failed] red','2020-01-01T00:00:00Z',NULL,'hash','2020-01-01T00:00:00Z');
            CREATE TABLE action_proposals (id INTEGER PRIMARY KEY, origin TEXT, session_key TEXT, signature TEXT, effect TEXT, state TEXT);
            INSERT INTO action_proposals (origin, session_key, signature, effect, state) VALUES ('inbox','s','sig','read','pending');
            CREATE TABLE bot_profiles (bot_id TEXT PRIMARY KEY, name TEXT);
            INSERT INTO bot_profiles (bot_id, name) VALUES ('b1','one');
            INSERT INTO bot_profiles (bot_id, name) VALUES ('b2','two');
            INSERT INTO bot_profiles (bot_id, name) VALUES ('b3','three');
            INSERT INTO bot_profiles (bot_id, name) VALUES ('b4','four');
            ",
        )
        .await;

        let dest = storage::memory().await.unwrap();
        let dry = migrate_from_omo(&dest, &source, true).await.unwrap();
        assert_eq!(dry.issues, 1);
        assert_eq!(dry.allowlist, 5);
        assert_eq!(dry.bot_profiles, 4);
        assert_eq!(dry.cron_jobs, 1);
        assert_eq!(dry.activity, 1);
        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
            .fetch_one(&dest)
            .await
            .unwrap();
        assert_eq!(stored, 0, "dry-run writes nothing");

        let report = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(report.issues, 1);
        assert_eq!(report.teams, 1);
        assert_eq!(report.states, 1);
        assert_eq!(report.labels, 1);
        assert_eq!(report.activity, 1);
        assert_eq!(report.cron_jobs, 1);
        assert_eq!(report.inbox_items, 1);
        assert_eq!(report.proposals, 1);
        assert_eq!(report.allowlist, 5);
        assert_eq!(report.bot_profiles, 4);
        let description: String =
            sqlx::query_scalar("SELECT description FROM tracker_issues WHERE id = 'i1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(description, "the body");
        let state_id: String =
            sqlx::query_scalar("SELECT state_id FROM tracker_issues WHERE id = 'i1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(state_id, "s1");
        let delivery: String =
            sqlx::query_scalar("SELECT delivery_json FROM scheduled_jobs WHERE id = 'c1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert!(delivery.contains("1700.0001"), "{delivery}");

        let again = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(again.issues, 0, "a second migrate inserts nothing new");
        let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
            .fetch_one(&dest)
            .await
            .unwrap();
        assert_eq!(still, 1);

        let broken = dir.join("broken.sqlite");
        raw_db(
            &broken,
            "
            CREATE TABLE tracker_issues (id TEXT, title TEXT, description TEXT, state_id TEXT, creator_id TEXT);
            INSERT INTO tracker_issues VALUES ('i1','t','body','s1','u1');
            ",
        )
        .await;
        let refused = migrate_from_omo(&dest, &broken, false).await;
        assert!(
            refused.is_err(),
            "a missing team_id must not import as zero issues"
        );

        let legacy = dir.join("legacy.sqlite");
        raw_db(
            &legacy,
            "
            CREATE TABLE tracker_issues (id INTEGER, title TEXT, body TEXT, status TEXT, created_at TEXT);
            INSERT INTO tracker_issues VALUES (1,'old','body text','open','2020-01-01T00:00:00Z');
            ",
        )
        .await;
        let legacy_result = migrate_from_omo(&dest, &legacy, false).await;
        assert!(
            legacy_result.is_err(),
            "the body/status shape must surface an error, not an empty success"
        );

        let bad_cron = dir.join("bad-cron.sqlite");
        raw_db(
            &bad_cron,
            "
            CREATE TABLE cron_jobs (id TEXT, session_key TEXT, expression TEXT, payload_json TEXT, enabled INTEGER, next_run_at TEXT);
            INSERT INTO cron_jobs VALUES ('c','s','every 1m','{',1,NULL);
            ",
        )
        .await;
        let bad = migrate_from_omo(&dest, &bad_cron, false).await;
        let message = bad.expect_err("invalid cron payload").to_string();
        assert!(message.contains("invalid cron payload"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn rows_of_a_deleted_issue_are_skipped_not_a_foreign_key_failure() {
        let dir = scratch("orphans");
        let source = dir.join("omo.sqlite");
        raw_db(
            &source,
            "
            CREATE TABLE tracker_users (id TEXT PRIMARY KEY, name TEXT, display_name TEXT, email TEXT, avatar_color TEXT, kind TEXT, model TEXT, is_me INTEGER, created_at TEXT);
            INSERT INTO tracker_users VALUES ('u1','ada','Ada',NULL,'#000','human',NULL,1,'2020-01-01T00:00:00Z');
            CREATE TABLE tracker_teams (id TEXT PRIMARY KEY, key TEXT, name TEXT, icon TEXT, color TEXT, issue_seq INTEGER, system INTEGER, cycles_enabled INTEGER, cycle_weeks INTEGER, triage_enabled INTEGER, created_at TEXT, sort_order REAL);
            INSERT INTO tracker_teams VALUES ('t1','AGT','Agents','bot','#fff',1,0,0,2,0,'2020-01-01T00:00:00Z',0);
            CREATE TABLE tracker_states (id TEXT PRIMARY KEY, team_id TEXT, name TEXT, type TEXT, color TEXT, position REAL, lane_status TEXT, description TEXT);
            INSERT INTO tracker_states VALUES ('s1','t1','Todo','unstarted','#fff',0,NULL,NULL);
            CREATE TABLE tracker_labels (id TEXT PRIMARY KEY, team_id TEXT, name TEXT, color TEXT, parent_id TEXT, description TEXT, created_at TEXT);
            INSERT INTO tracker_labels VALUES ('l1','t1','bug','#f00',NULL,NULL,'2020-01-01T00:00:00Z');
            CREATE TABLE tracker_issues (id TEXT PRIMARY KEY, team_id TEXT, number INTEGER, identifier TEXT, title TEXT, description TEXT, state_id TEXT, priority INTEGER, creator_id TEXT, created_at TEXT, updated_at TEXT);
            INSERT INTO tracker_issues VALUES ('i1','t1',1,'AGT-1','Kept','body','s1',0,'u1','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z');
            CREATE TABLE tracker_issue_labels (issue_id TEXT, label_id TEXT, PRIMARY KEY(issue_id, label_id));
            INSERT INTO tracker_issue_labels VALUES ('i1','l1');
            INSERT INTO tracker_issue_labels VALUES ('gone','l1');
            CREATE TABLE tracker_activity (id INTEGER PRIMARY KEY, issue_id TEXT, actor_id TEXT, kind TEXT, created_at TEXT);
            INSERT INTO tracker_activity (issue_id, actor_id, kind, created_at) VALUES ('i1','u1','created','2020-01-01T00:00:00Z');
            INSERT INTO tracker_activity (issue_id, actor_id, kind, created_at) VALUES ('i1','ghost','edited','2020-01-01T00:00:00Z');
            INSERT INTO tracker_activity (issue_id, actor_id, kind, created_at) VALUES ('gone','u1','created','2020-01-01T00:00:00Z');
            ",
        )
        .await;
        let dest_path = dir.join("hub.sqlite");
        let dest = storage::connect(&dest_path).await.unwrap();
        let report = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(report.issues, 1);
        assert_eq!(report.activity, 2);
        assert_eq!(report.tables["tracker_issue_labels"], 1);
        assert_eq!(report.tables["orphans_skipped.tracker_issue_labels"], 1);
        assert_eq!(report.tables["orphans_skipped.tracker_activity"], 1);
        let nulled: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tracker_activity WHERE kind = 'edited' AND actor_id IS NULL",
        )
        .fetch_one(&dest)
        .await
        .unwrap();
        assert_eq!(nulled, 1, "a missing SET NULL parent is copied as NULL");
        let again = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(again.activity, 0);
        dest.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_second_import_refreshes_cron_issue_and_inbox_state() {
        let dir = scratch("refresh");
        let source = dir.join("omo.sqlite");
        raw_db(
            &source,
            "
            CREATE TABLE tracker_users (id TEXT PRIMARY KEY, name TEXT, display_name TEXT, email TEXT, avatar_color TEXT, kind TEXT, model TEXT, is_me INTEGER, created_at TEXT);
            INSERT INTO tracker_users VALUES ('u1','ada','Ada',NULL,'#000','human',NULL,1,'2020-01-01T00:00:00Z');
            CREATE TABLE tracker_teams (id TEXT PRIMARY KEY, key TEXT, name TEXT, icon TEXT, color TEXT, issue_seq INTEGER, system INTEGER, cycles_enabled INTEGER, cycle_weeks INTEGER, triage_enabled INTEGER, created_at TEXT, sort_order REAL);
            INSERT INTO tracker_teams VALUES ('t1','AGT','Agents','bot','#fff',1,0,0,2,0,'2020-01-01T00:00:00Z',0);
            CREATE TABLE tracker_states (id TEXT PRIMARY KEY, team_id TEXT, name TEXT, type TEXT, color TEXT, position REAL, lane_status TEXT, description TEXT);
            INSERT INTO tracker_states VALUES ('s1','t1','Todo','unstarted','#fff',0,NULL,NULL);
            INSERT INTO tracker_states VALUES ('s2','t1','Done','completed','#0f0',1,NULL,NULL);
            CREATE TABLE tracker_issues (id TEXT PRIMARY KEY, team_id TEXT, number INTEGER, identifier TEXT, title TEXT, description TEXT, state_id TEXT, priority INTEGER, creator_id TEXT, created_at TEXT, updated_at TEXT);
            INSERT INTO tracker_issues VALUES ('i1','t1',1,'AGT-1','Gate','body','s1',0,'u1','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z');
            CREATE TABLE cron_jobs (id TEXT, session_key TEXT, expression TEXT, payload_json TEXT, enabled INTEGER, next_run_at TEXT, created_at TEXT);
            INSERT INTO cron_jobs VALUES ('c1',NULL,'every 15m','{}',1,'2099-01-01T00:00:00Z','2020-01-01T00:00:00Z');
            CREATE TABLE inbox_items (id TEXT PRIMARY KEY, source TEXT, source_ref TEXT, thread_ref TEXT, author TEXT, title TEXT, snippet TEXT, received_at TEXT, link TEXT, raw_hash TEXT, collected_at TEXT);
            INSERT INTO inbox_items VALUES ('in1','github','ref-1',NULL,'octo','ci','red','2020-01-01T00:00:00Z',NULL,'hash','2020-01-01T00:00:00Z');
            CREATE TABLE inbox_decisions (item_id TEXT PRIMARY KEY, class TEXT, reason TEXT, status TEXT, decided_at TEXT);
            INSERT INTO inbox_decisions VALUES ('in1','owner','because','open','2020-01-01T00:00:00Z');
            ",
        )
        .await;
        let dest = storage::memory().await.unwrap();
        let first = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(first.issues, 1);
        assert_eq!(first.cron_jobs, 1);
        raw_db(
            &source,
            "
            UPDATE tracker_issues SET state_id = 's2', title = 'Moved' WHERE id = 'i1';
            UPDATE cron_jobs SET enabled = 0, next_run_at = '2020-06-01T00:00:00Z' WHERE id = 'c1';
            UPDATE inbox_decisions SET status = 'done' WHERE item_id = 'in1';
            ",
        )
        .await;
        let again = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(again.issues, 1);
        assert_eq!(again.cron_jobs, 1);
        assert_eq!(again.tables["inbox_decisions"], 1);
        let state: (String, String) =
            sqlx::query_as("SELECT state_id, title FROM tracker_issues WHERE id = 'i1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(state, ("s2".into(), "Moved".into()));
        let cron: (i64, String) =
            sqlx::query_as("SELECT enabled, next_run_at FROM scheduled_jobs WHERE id = 'c1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(cron, (0, "2020-06-01T00:00:00Z".into()));
        let status: String =
            sqlx::query_scalar("SELECT status FROM inbox_decisions WHERE item_id = 'in1'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(status, "done");
        let third = migrate_from_omo(&dest, &source, false).await.unwrap();
        assert_eq!(third.issues, 0);
        assert_eq!(third.cron_jobs, 0);
        assert_eq!(third.tables.get("inbox_decisions").copied().unwrap_or(0), 0);
        dest.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_dry_run_does_not_create_the_destination_and_cwd_can_be_recorded() {
        let dir = scratch("dry-dest");
        let source = dir.join("omo.sqlite");
        raw_db(&source, "CREATE TABLE unused (id INTEGER);").await;
        let dest = dir.join("missing").join("hub.sqlite");
        let report = open_and_migrate(&dest, &source, true, None, None)
            .await
            .unwrap();
        assert_eq!(report.issues, 0);
        assert!(!dest.exists());
        assert!(!dir.join("missing").exists());
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let home = dir.join("home");
        std::fs::create_dir_all(home.join("Library/LaunchAgents")).unwrap();
        std::fs::write(
            home.join("Library/LaunchAgents/ai.omon.gateway.plist"),
            format!(
                "<key>WorkingDirectory</key><string>{}</string>",
                work.display()
            ),
        )
        .unwrap();
        let snap = dir.join("snap.backup");
        raw_db(&snap, "CREATE TABLE unused (id INTEGER);").await;
        assert_eq!(omo_working_directory(&snap, Some(&home)), work);
        assert_eq!(
            omo_working_directory(&work.join("omon_gateway.db"), Some(&home)),
            work
        );
        let live = dir.join("omon_gateway.db");
        let imported = open_and_migrate(&dest, &snap, false, Some(&work), Some(&live))
            .await
            .unwrap();
        assert_eq!(imported.issues, 0);
        let pool = storage::connect(&dest).await.unwrap();
        let recorded: String =
            sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'omo_working_directory'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(recorded, work.to_string_lossy());
        let imported_from: String =
            sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'imported_from'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(imported_from, live.to_string_lossy());
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn lane_threads_and_the_plist_working_directory_are_the_live_defaults() {
        let dir = scratch("discover");
        let db_dir = dir.join("code/project/omon-gateway");
        std::fs::create_dir_all(&db_dir).unwrap();
        let live = db_dir.join("omon_gateway.db");
        raw_db(
            &live,
            "
            CREATE TABLE lanes (key TEXT, title TEXT, lead_session_key TEXT, cwd TEXT, brief_path TEXT, session_key TEXT, omo_thread_id TEXT, status TEXT, done_signals TEXT, claim_evidence TEXT, claim_at TEXT, turns_run INTEGER, last_activity_at TEXT, thread_channel_id TEXT);
            INSERT INTO lanes VALUES ('gate','Gate','meta','/tmp','','meta',NULL,'working','[]',NULL,NULL,0,'2020-01-01T00:00:00Z','slack:C0123:1700.0001');
            CREATE TABLE lane_thread_bindings (lane_key TEXT, bot_id TEXT, bound_at TEXT);
            INSERT INTO lane_thread_bindings VALUES ('gate','B1','2020-01-01T00:00:00Z');
            CREATE TABLE cron_jobs (id TEXT, session_key TEXT, expression TEXT, payload_json TEXT, enabled INTEGER, next_run_at TEXT, created_at TEXT);
            INSERT INTO cron_jobs VALUES ('on',NULL,'0 9 * * *','{}',1,NULL,'2020-01-01T00:00:00Z');
            INSERT INTO cron_jobs VALUES ('off',NULL,'0 9 * * *','{}',0,NULL,'2020-01-01T00:00:00Z');
            CREATE TABLE cron_problems (id INTEGER PRIMARY KEY, job_id TEXT, kind TEXT, signature TEXT, detail TEXT, status TEXT, created_at TEXT, updated_at TEXT);
            INSERT INTO cron_problems VALUES (4, 'on', 'failed', 'sig', 'boom', 'open', '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z');
            ",
        )
        .await;
        let agents = dir.join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("ai.omon.gateway.plist"),
            format!(
                r#"<?xml version="1.0"?><plist><dict><key>WorkingDirectory</key><string>{}</string></dict></plist>"#,
                db_dir.display()
            ),
        )
        .unwrap();
        let found = discover_omo_database(&dir).unwrap();
        assert_eq!(found, live);
        let dest = storage::memory().await.unwrap();
        let dry = migrate_from_omo(&dest, &live, true).await.unwrap();
        assert_eq!(dry.lane_threads, 1);
        assert_eq!(dry.cron_jobs, 2);
        assert_eq!(dry.cron_jobs_enabled, 1);
        let report = migrate_from_omo(&dest, &live, false).await.unwrap();
        assert_eq!(report.lane_threads, 1);
        let thread: String =
            sqlx::query_scalar("SELECT thread_channel_id FROM lanes WHERE key = 'gate'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(thread, "slack:C0123:1700.0001");
        let bot: String =
            sqlx::query_scalar("SELECT bot_id FROM lane_thread_bindings WHERE lane_key = 'gate'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(bot, "B1");
        let problem: (String, String) =
            sqlx::query_as("SELECT kind, detail FROM cron_problems WHERE id = '4'")
                .fetch_one(&dest)
                .await
                .unwrap();
        assert_eq!(problem, ("failed".into(), "boom".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
