//! Safari favicon parser (`Favicon Cache/favicons.db` + on-disk image store).
//!
//! Unlike Chromium, Safari does **not** store favicon bitmaps inside the
//! SQLite database. `favicons.db` only records the mapping between page URLs,
//! an internal UUID, and the favicon source URL. The actual image bytes live
//! as individual files under `Favicon Cache/favicons/`, each named by the
//! **uppercase hexadecimal MD5 of the UUID**.
//!
//! This layout is confirmed by Firefox's `SafariProfileMigrator`, which reads
//! `favicons.db` and then loads each image from
//! `~/Library/Safari/Favicon Cache/favicons/<MD5_HEX_UPPERCASE(uuid)>`.
//!
//! ## `favicons.db` schema (relevant tables)
//!
//! ```sql
//! CREATE TABLE icon_info (uuid TEXT PRIMARY KEY, url TEXT, timestamp INTEGER,
//!                         width INTEGER, height INTEGER, ...);
//! CREATE TABLE page_url  (url TEXT, uuid TEXT);
//! ```

use std::collections::HashMap;
use std::path::Path;

use md5::{Digest, Md5};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{ImportError, ImportResult};

/// Maximum favicon image size accepted (1 MB), matching the Chromium parser.
const MAX_FAVICON_SIZE: usize = 1024 * 1024;

/// A Safari favicon entry with its raw image data and associated page URLs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SafariFaviconEntry {
    /// The URL of the favicon image itself (from `icon_info.url`).
    pub favicon_url: String,
    /// Page URLs that use this favicon (from `page_url.url`).
    pub page_urls: Vec<String>,
    /// Raw image data loaded from the on-disk `favicons/<hash>` file.
    pub image_data: Vec<u8>,
}

/// Parses Safari favicons from a Safari profile directory (`~/Library/Safari`).
///
/// Reads `Favicon Cache/favicons.db`, then loads each referenced image from
/// `Favicon Cache/favicons/<MD5_HEX_UPPERCASE(uuid)>`. Entries whose image file
/// is missing or not a recognized image format are skipped. Never panics.
pub fn parse_safari_favicons(safari_dir: &Path) -> ImportResult<Vec<SafariFaviconEntry>> {
    let cache_dir = safari_dir.join("Favicon Cache");
    let db_path = cache_dir.join("favicons.db");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    if !table_exists(&conn, "icon_info") || !table_exists(&conn, "page_url") {
        return Err(ImportError::Parse(
            "required favicon tables not found".into(),
        ));
    }

    let mut stmt = conn
        .prepare(
            "SELECT i.uuid, i.url, p.url \
             FROM icon_info i \
             INNER JOIN page_url p ON i.uuid = p.uuid \
             ORDER BY i.uuid",
        )
        .map_err(|e| ImportError::Parse(format!("favicons query: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| ImportError::Parse(format!("favicons rows: {e}")))?;

    let images_dir = cache_dir.join("favicons");
    let mut results: Vec<SafariFaviconEntry> = Vec::new();
    // Maps uuid -> index into `results`, or None if the image was missing/bad
    // (so we only attempt to read each uuid's file once).
    let mut seen: HashMap<String, Option<usize>> = HashMap::new();

    for row in rows {
        let (uuid, favicon_url, page_url) = match row {
            Ok(r) => r,
            Err(_) => continue,
        };

        if url::Url::parse(&page_url).is_err() {
            continue;
        }

        let slot = seen.entry(uuid.clone()).or_insert_with(|| {
            let hashed = md5_hex_uppercase(&uuid);
            let image_path = images_dir.join(&hashed);
            match std::fs::read(&image_path) {
                Ok(data) if is_valid_favicon_data(&data) => {
                    results.push(SafariFaviconEntry {
                        favicon_url: favicon_url.clone(),
                        page_urls: Vec::new(),
                        image_data: data,
                    });
                    Some(results.len() - 1)
                }
                _ => None,
            }
        });

        if let Some(index) = *slot {
            let entry = &mut results[index];
            if !entry.page_urls.contains(&page_url) {
                entry.page_urls.push(page_url);
            }
        }
    }

    // Drop any entries that ended up with no valid page URLs.
    results.retain(|e| !e.page_urls.is_empty());
    Ok(results)
}

/// Computes the uppercase hex MD5 of a UUID string — the on-disk file name
/// Safari uses for the corresponding favicon image.
fn md5_hex_uppercase(uuid: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(uuid.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(32);
    for byte in digest {
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// Validates favicon image data by checking magic bytes for known formats.
fn is_valid_favicon_data(data: &[u8]) -> bool {
    if data.is_empty() || data.len() > MAX_FAVICON_SIZE {
        return false;
    }
    matches_image_magic(data)
}

/// Checks the leading bytes for supported raster/vector image formats.
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

    /// Creates `Favicon Cache/favicons.db` with the two required tables and the
    /// given (uuid, favicon_url, page_url) rows.
    fn create_favicons_db(safari_dir: &Path, rows: &[(&str, &str, &str)]) {
        let cache = safari_dir.join("Favicon Cache");
        std::fs::create_dir_all(&cache).unwrap();
        let conn = Connection::open(cache.join("favicons.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE icon_info (uuid TEXT PRIMARY KEY NOT NULL, url TEXT NOT NULL, \
                 timestamp INTEGER, width INTEGER DEFAULT 0, height INTEGER DEFAULT 0, \
                 has_generated_representations INTEGER DEFAULT 0);
             CREATE TABLE page_url (url TEXT NOT NULL, uuid TEXT NOT NULL);",
        )
        .unwrap();
        for (uuid, favicon_url, page_url) in rows {
            conn.execute(
                "INSERT OR IGNORE INTO icon_info (uuid, url) VALUES (?1, ?2)",
                [uuid, favicon_url],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO page_url (url, uuid) VALUES (?1, ?2)",
                [page_url, uuid],
            )
            .unwrap();
        }
    }

    /// Writes an image file for `uuid` into `Favicon Cache/favicons/`.
    fn write_image(safari_dir: &Path, uuid: &str, data: &[u8]) {
        let dir = safari_dir.join("Favicon Cache").join("favicons");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(md5_hex_uppercase(uuid)), data).unwrap();
    }

    #[test]
    fn missing_db_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let safari_dir = tmp.path().join("Safari");
        std::fs::create_dir_all(&safari_dir).unwrap();
        let err = parse_safari_favicons(&safari_dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupt_db_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("Favicon Cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("favicons.db"), b"not a sqlite db").unwrap();
        let err = parse_safari_favicons(tmp.path()).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn parses_favicon_with_image_file() {
        let tmp = tempfile::tempdir().unwrap();
        let safari = tmp.path();
        create_favicons_db(
            safari,
            &[
                (
                    "uuid-1",
                    "https://example.com/favicon.ico",
                    "https://example.com",
                ),
                (
                    "uuid-1",
                    "https://example.com/favicon.ico",
                    "https://example.com/page",
                ),
            ],
        );
        write_image(safari, "uuid-1", &minimal_png());

        let entries = parse_safari_favicons(safari).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].favicon_url, "https://example.com/favicon.ico");
        assert_eq!(entries[0].page_urls.len(), 2);
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com".to_string()));
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com/page".to_string()));
        assert_eq!(entries[0].image_data, minimal_png());
    }

    #[test]
    fn skips_entry_when_image_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let safari = tmp.path();
        create_favicons_db(
            safari,
            &[("uuid-x", "https://no-image.com/fav", "https://no-image.com")],
        );
        // No image file written for uuid-x.
        let entries = parse_safari_favicons(safari).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn skips_entry_with_invalid_image_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let safari = tmp.path();
        create_favicons_db(
            safari,
            &[("uuid-bad", "https://bad.com/fav", "https://bad.com")],
        );
        write_image(safari, "uuid-bad", &[0xDE, 0xAD, 0xBE, 0xEF]);

        let entries = parse_safari_favicons(safari).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn empty_db_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let safari = tmp.path();
        create_favicons_db(safari, &[]);
        let entries = parse_safari_favicons(safari).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn md5_matches_known_vector() {
        // MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
        assert_eq!(md5_hex_uppercase("abc"), "900150983CD24FB0D6963F7D28E17F72");
    }

    #[test]
    fn image_magic_detection() {
        assert!(matches_image_magic(&[0x89, 0x50, 0x4E, 0x47])); // PNG
        assert!(matches_image_magic(&[0x00, 0x00, 0x01, 0x00])); // ICO
        assert!(matches_image_magic(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
        assert!(matches_image_magic(b"GIF89a")); // GIF
        assert!(!matches_image_magic(&[0x01, 0x02, 0x03, 0x04])); // junk
    }
}
