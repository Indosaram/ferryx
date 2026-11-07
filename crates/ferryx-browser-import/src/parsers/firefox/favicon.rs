//! Firefox favicon parser (`favicons.sqlite`).
//!
//! Firefox stores favicon bitmaps inline in the `favicons.sqlite` database
//! (Places subsystem), unlike Safari which keeps images on disk. Three tables
//! are relevant:
//!
//! ```sql
//! CREATE TABLE moz_icons (id INTEGER PRIMARY KEY, icon_url TEXT, ...,
//!                         data BLOB, ...);
//! CREATE TABLE moz_pages_w_icons (id INTEGER PRIMARY KEY, page_url TEXT, ...);
//! CREATE TABLE moz_icons_to_pages (page_id INTEGER, icon_id INTEGER, ...);
//! ```
//!
//! `data` holds the raw image bytes (typically PNG, sometimes SVG). This mirrors
//! the Chromium `Favicons` parser: one entry per icon URL, aggregating the page
//! URLs that reference it.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{ImportError, ImportResult};

/// Maximum favicon image size accepted (1 MB), matching the other parsers.
const MAX_FAVICON_SIZE: usize = 1024 * 1024;

/// A Firefox favicon entry with its raw image data and associated page URLs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirefoxFaviconEntry {
    /// The URL of the favicon image itself (from `moz_icons.icon_url`).
    pub favicon_url: String,
    /// Page URLs that use this favicon (from `moz_pages_w_icons.page_url`).
    pub page_urls: Vec<String>,
    /// Raw image data (PNG, ICO, JPEG, GIF, WebP, or SVG).
    pub image_data: Vec<u8>,
}

/// Parses Firefox favicons from a profile directory containing `favicons.sqlite`.
///
/// Entries whose image bytes are missing or not a recognized image format are
/// skipped. Never panics on malformed input.
pub fn parse_firefox_favicons(profile_dir: &Path) -> ImportResult<Vec<FirefoxFaviconEntry>> {
    let db_path = profile_dir.join("favicons.sqlite");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    if !table_exists(&conn, "moz_icons")
        || !table_exists(&conn, "moz_pages_w_icons")
        || !table_exists(&conn, "moz_icons_to_pages")
    {
        return Err(ImportError::Parse(
            "required favicon tables not found".into(),
        ));
    }

    let mut stmt = conn
        .prepare(
            "SELECT i.icon_url, p.page_url, i.data \
             FROM moz_icons i \
             JOIN moz_icons_to_pages itp ON itp.icon_id = i.id \
             JOIN moz_pages_w_icons p ON p.id = itp.page_id \
             WHERE i.data IS NOT NULL AND length(i.data) > 0 \
             ORDER BY i.icon_url",
        )
        .map_err(|e| ImportError::Parse(format!("preparing favicons query: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|e| ImportError::Parse(format!("querying favicons: {e}")))?;

    let mut results: Vec<FirefoxFaviconEntry> = Vec::new();
    let mut current_favicon_url = String::new();

    for row in rows {
        let (favicon_url, page_url, image_data) =
            row.map_err(|e| ImportError::Parse(format!("reading favicon row: {e}")))?;

        if !is_valid_favicon_data(&image_data) {
            continue;
        }

        if favicon_url != current_favicon_url || results.is_empty() {
            results.push(FirefoxFaviconEntry {
                favicon_url: favicon_url.clone(),
                page_urls: Vec::new(),
                image_data,
            });
            current_favicon_url = favicon_url;
        }

        if let Some(entry) = results.last_mut() {
            if url::Url::parse(&page_url).is_ok() && !entry.page_urls.contains(&page_url) {
                entry.page_urls.push(page_url);
            }
        }
    }

    results.retain(|e| !e.page_urls.is_empty());
    Ok(results)
}

fn is_valid_favicon_data(data: &[u8]) -> bool {
    if data.is_empty() || data.len() > MAX_FAVICON_SIZE {
        return false;
    }
    matches_image_magic(data)
}

fn matches_image_magic(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    // PNG
    if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
        return true;
    }
    // ICO
    if data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 && data[3] == 0x00 {
        return true;
    }
    // JPEG
    if data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return true;
    }
    // GIF
    if data.len() >= 6
        && &data[0..4] == b"GIF8"
        && (data[4] == b'7' || data[4] == b'9')
        && data[5] == b'a'
    {
        return true;
    }
    // WebP (RIFF....WEBP)
    if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return true;
    }
    // SVG (XML or <svg tag)
    let check_len = data.len().min(100);
    if let Ok(text) = std::str::from_utf8(&data[..check_len]) {
        if text.contains("<svg") || text.contains("<?xml") {
            return true;
        }
    }
    false
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.prepare(&format!(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='{table}'"
    ))
    .and_then(|mut s| s.query_row([], |_| Ok(())))
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid 1x1 PNG.
    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x01, // width = 1
            0x00, 0x00, 0x00, 0x01, // height = 1
            0x08, 0x02, 0x00, 0x00, 0x00, // bit depth, color, misc
            0x90, 0x77, 0x53, 0xDE, // CRC
        ]
    }

    /// Builds `favicons.sqlite` with the three required tables and the given
    /// (icon_url, image_data, page_url) rows (one page mapping per row).
    fn create_favicons_db(profile_dir: &Path, rows: &[(&str, &[u8], &str)]) {
        let conn = Connection::open(profile_dir.join("favicons.sqlite")).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_icons (id INTEGER PRIMARY KEY, icon_url TEXT NOT NULL, \
                 fixed_icon_url_hash INTEGER, width INTEGER DEFAULT 0, root INTEGER DEFAULT 0, \
                 color INTEGER, data BLOB, expire_ms INTEGER DEFAULT 0);
             CREATE TABLE moz_pages_w_icons (id INTEGER PRIMARY KEY, page_url TEXT NOT NULL, \
                 page_url_hash INTEGER);
             CREATE TABLE moz_icons_to_pages (page_id INTEGER NOT NULL, icon_id INTEGER NOT NULL, \
                 expire_ms INTEGER DEFAULT 0, PRIMARY KEY (page_id, icon_id));",
        )
        .unwrap();

        let mut next_icon_id = 1i64;
        for (page_id, (icon_url, data, page_url)) in (1i64..).zip(rows) {
            let icon_id: i64 = match conn.query_row(
                "SELECT id FROM moz_icons WHERE icon_url = ?1",
                [icon_url],
                |r| r.get(0),
            ) {
                Ok(id) => id,
                Err(_) => {
                    let id = next_icon_id;
                    next_icon_id += 1;
                    conn.execute(
                        "INSERT INTO moz_icons (id, icon_url, data) VALUES (?1, ?2, ?3)",
                        rusqlite::params![id, icon_url, data],
                    )
                    .unwrap();
                    id
                }
            };

            conn.execute(
                "INSERT INTO moz_pages_w_icons (id, page_url) VALUES (?1, ?2)",
                rusqlite::params![page_id, page_url],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO moz_icons_to_pages (page_id, icon_id) VALUES (?1, ?2)",
                rusqlite::params![page_id, icon_id],
            )
            .unwrap();
        }
    }

    #[test]
    fn missing_db_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = parse_firefox_favicons(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupt_db_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("favicons.sqlite"), b"not a sqlite db").unwrap();
        let err = parse_firefox_favicons(tmp.path()).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn parses_favicon_with_multiple_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let png = minimal_png();
        create_favicons_db(
            tmp.path(),
            &[
                (
                    "https://example.com/favicon.ico",
                    &png,
                    "https://example.com",
                ),
                (
                    "https://example.com/favicon.ico",
                    &png,
                    "https://example.com/page",
                ),
            ],
        );

        let entries = parse_firefox_favicons(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].favicon_url, "https://example.com/favicon.ico");
        assert_eq!(entries[0].page_urls.len(), 2);
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com".to_string()));
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com/page".to_string()));
        assert_eq!(entries[0].image_data, png);
    }

    #[test]
    fn skips_invalid_image_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        create_favicons_db(
            tmp.path(),
            &[(
                "https://bad.com/fav",
                &[0xDE, 0xAD, 0xBE, 0xEF],
                "https://bad.com",
            )],
        );
        let entries = parse_firefox_favicons(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn skips_non_http_page_urls() {
        let tmp = tempfile::tempdir().unwrap();
        let png = minimal_png();
        create_favicons_db(tmp.path(), &[("https://x.com/fav", &png, "not a url")]);
        let entries = parse_firefox_favicons(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn empty_db_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        create_favicons_db(tmp.path(), &[]);
        let entries = parse_firefox_favicons(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn distinct_icons_produce_distinct_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let png = minimal_png();
        create_favicons_db(
            tmp.path(),
            &[
                ("https://a.com/fav", &png, "https://a.com"),
                ("https://b.com/fav", &png, "https://b.com"),
            ],
        );
        let entries = parse_firefox_favicons(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }
}
