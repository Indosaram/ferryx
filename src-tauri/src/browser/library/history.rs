//! Host-side browsing history in SQLite (`<data>/browser/history.sqlite`): one row per URL with
//! its visit count and last visit, plus a visits table for clear-by-range. Private tabs are never
//! recorded: [`HistoryStore::record_visit`] refuses the private profile. All functions do blocking
//! I/O: call them through `crate::ipc::run_blocking`.
//!
//! Import lane contract: [`HistoryStore::import`] takes plain [`ImportedVisit`] values (url,
//! title, visit time in ms since the Unix epoch, visit count).

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "history.sqlite";
/// Per-URL visit rows kept for range deletes; older rows are trimmed per URL beyond this.
const VISITS_PER_URL: i64 = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub url: String,
    pub title: Option<String>,
    pub visit_count: u32,
    /// Milliseconds since the Unix epoch.
    pub last_visited_at: i64,
    /// Frecency score (visit count weighted by recency) at query time.
    pub frecency: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedVisit {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    pub visited_at: i64,
    #[serde(default = "one")]
    pub visit_count: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("history store: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("history store io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct HistoryStore {
    conn: Connection,
}

/// Only http(s) and file pages are history; about:, data:, chrome: and similar are not.
pub fn is_recordable_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
}

/// Recency-weighted visit count, Firefox-style buckets.
pub fn frecency(visit_count: u32, last_visited_at: i64, now: i64) -> f64 {
    let age_days = (now.saturating_sub(last_visited_at)).max(0) as f64 / 86_400_000.0;
    let weight = if age_days < 4.0 {
        100.0
    } else if age_days < 14.0 {
        70.0
    } else if age_days < 31.0 {
        50.0
    } else if age_days < 90.0 {
        30.0
    } else {
        10.0
    };
    f64::from(visit_count.max(1)) * weight
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

impl HistoryStore {
    /// `<data>/browser/history.sqlite`.
    pub fn default_path(data_dir: &Path) -> PathBuf {
        data_dir.join("browser").join(FILE_NAME)
    }

    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::with_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, HistoryError> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> Result<Self, HistoryError> {
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS urls (
                 id INTEGER PRIMARY KEY,
                 url TEXT NOT NULL UNIQUE,
                 title TEXT,
                 visit_count INTEGER NOT NULL DEFAULT 0,
                 last_visited_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS visits (
                 id INTEGER PRIMARY KEY,
                 url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
                 visited_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS visits_by_time ON visits(visited_at);
             CREATE INDEX IF NOT EXISTS visits_by_url ON visits(url_id);
             CREATE INDEX IF NOT EXISTS urls_by_time ON urls(last_visited_at);
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
    }

    /// Records one visit. Returns false (and stores nothing) for a private tab or a URL that is
    /// not history (about:blank, data:, ...).
    pub fn record_visit(
        &mut self,
        url: &str,
        title: Option<&str>,
        visited_at: i64,
        private: bool,
    ) -> Result<bool, HistoryError> {
        if private || !is_recordable_url(url) {
            return Ok(false);
        }
        self.insert(url.trim(), title, visited_at, 1)?;
        Ok(true)
    }

    fn insert(&mut self, url: &str, title: Option<&str>, visited_at: i64, count: u32) -> Result<(), HistoryError> {
        let title = title.map(str::trim).filter(|title| !title.is_empty());
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO urls (url, title, visit_count, last_visited_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(url) DO UPDATE SET
                 title = COALESCE(excluded.title, urls.title),
                 visit_count = urls.visit_count + excluded.visit_count,
                 last_visited_at = MAX(urls.last_visited_at, excluded.last_visited_at)",
            params![url, title, count.max(1), visited_at],
        )?;
        let url_id: i64 = tx.query_row("SELECT id FROM urls WHERE url = ?1", [url], |row| row.get(0))?;
        tx.execute("INSERT INTO visits (url_id, visited_at) VALUES (?1, ?2)", params![url_id, visited_at])?;
        tx.execute(
            "DELETE FROM visits WHERE url_id = ?1 AND id NOT IN
                 (SELECT id FROM visits WHERE url_id = ?1 ORDER BY visited_at DESC LIMIT ?2)",
            params![url_id, VISITS_PER_URL],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Updates the title of a URL already in history (titles often arrive after the address).
    pub fn update_title(&mut self, url: &str, title: &str) -> Result<(), HistoryError> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(());
        }
        self.conn.execute("UPDATE urls SET title = ?2 WHERE url = ?1", params![url.trim(), title])?;
        Ok(())
    }

    fn map_entry(row: &rusqlite::Row<'_>, now: i64) -> rusqlite::Result<HistoryEntry> {
        let visit_count: u32 = row.get(2)?;
        let last_visited_at: i64 = row.get(3)?;
        Ok(HistoryEntry {
            url: row.get(0)?,
            title: row.get(1)?,
            visit_count,
            last_visited_at,
            frecency: frecency(visit_count, last_visited_at, now),
        })
    }

    /// Newest first; `query` filters by every whitespace-separated term in url or title;
    /// `before` pages by last visit time (exclusive).
    pub fn list(&self, query: &str, before: Option<i64>, limit: usize) -> Result<Vec<HistoryEntry>, HistoryError> {
        let now = now_ms();
        let terms: Vec<String> = query.split_whitespace().map(|t| format!("%{}%", escape_like(&t.to_lowercase()))).collect();
        let mut sql = String::from("SELECT url, title, visit_count, last_visited_at FROM urls WHERE last_visited_at < ?1");
        for index in 0..terms.len() {
            let n = index + 2;
            sql.push_str(&format!(
                " AND (lower(url) LIKE ?{n} ESCAPE '\\' OR lower(COALESCE(title, '')) LIKE ?{n} ESCAPE '\\')"
            ));
        }
        sql.push_str(&format!(" ORDER BY last_visited_at DESC LIMIT {}", limit.min(10_000)));
        let mut statement = self.conn.prepare(&sql)?;
        let mut values: Vec<rusqlite::types::Value> = vec![before.unwrap_or(i64::MAX).into()];
        values.extend(terms.into_iter().map(rusqlite::types::Value::from));
        let rows = statement.query_map(rusqlite::params_from_iter(values), |row| Self::map_entry(row, now))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Omnibox candidates: entries matching the query, ranked by frecency (match quality is
    /// scored in the UI, which merges these with bookmarks and open tabs).
    pub fn suggest(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, HistoryError> {
        let mut entries = self.list(query, None, 500)?;
        entries.sort_by(|a, b| b.frecency.total_cmp(&a.frecency).then(b.last_visited_at.cmp(&a.last_visited_at)));
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn delete_url(&mut self, url: &str) -> Result<bool, HistoryError> {
        Ok(self.conn.execute("DELETE FROM urls WHERE url = ?1", [url])? > 0)
    }

    /// Removes every visit in `[from, to)` (ms); URLs left with no visit disappear, the rest get
    /// their last visit and count recomputed. Returns the number of URLs removed.
    pub fn clear_range(&mut self, from: i64, to: i64) -> Result<usize, HistoryError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM visits WHERE visited_at >= ?1 AND visited_at < ?2", params![from, to])?;
        // URLs touched in the range that kept no visit row.
        let removed = tx.execute(
            "DELETE FROM urls WHERE last_visited_at >= ?1 AND last_visited_at < ?2
                 AND NOT EXISTS (SELECT 1 FROM visits WHERE visits.url_id = urls.id)",
            params![from, to],
        )?;
        tx.execute(
            "UPDATE urls SET
                 last_visited_at = (SELECT MAX(visited_at) FROM visits WHERE visits.url_id = urls.id),
                 visit_count = MIN(visit_count, (SELECT COUNT(*) FROM visits WHERE visits.url_id = urls.id))
             WHERE last_visited_at >= ?1 AND last_visited_at < ?2",
            params![from, to],
        )?;
        tx.commit()?;
        Ok(removed)
    }

    pub fn clear_all(&mut self) -> Result<(), HistoryError> {
        self.conn.execute_batch("DELETE FROM visits; DELETE FROM urls;")?;
        Ok(())
    }

    /// Bulk insert for importers and the one-time localStorage migration. Returns rows inserted.
    pub fn import(&mut self, visits: &[ImportedVisit]) -> Result<usize, HistoryError> {
        let mut inserted = 0;
        for visit in visits {
            if !is_recordable_url(&visit.url) || visit.visited_at <= 0 {
                continue;
            }
            self.insert(visit.url.trim(), visit.title.as_deref(), visit.visited_at, visit.visit_count)?;
            inserted += 1;
        }
        Ok(inserted)
    }

    /// [`Self::import`] for another browser's history: a visit whose url already has a visit at
    /// the same time is a duplicate and is not added again. Returns (inserted, duplicates).
    pub fn import_new(&mut self, visits: &[ImportedVisit]) -> Result<(usize, usize), HistoryError> {
        let (mut inserted, mut duplicates) = (0, 0);
        for visit in visits {
            let url = visit.url.trim();
            if !is_recordable_url(url) || visit.visited_at <= 0 {
                continue;
            }
            let seen: bool = self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM urls WHERE url = ?1 AND (last_visited_at = ?2
                     OR EXISTS (SELECT 1 FROM visits WHERE visits.url_id = urls.id AND visited_at = ?2)))",
                params![url, visit.visited_at],
                |row| row.get(0),
            )?;
            if seen {
                duplicates += 1;
                continue;
            }
            self.insert(url, visit.title.as_deref(), visit.visited_at, visit.visit_count)?;
            inserted += 1;
        }
        Ok((inserted, duplicates))
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>, HistoryError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<(), HistoryError> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}
