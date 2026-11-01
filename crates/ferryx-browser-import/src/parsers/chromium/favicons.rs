//! Chromium Favicons SQLite parser.
//!
//! Ports `maho-chromium/browser/importer/favicon_parser.cc` to Rust.
//! Reads the `Favicons` SQLite database from a Chromium profile directory and
//! extracts favicon image data with associated page URLs.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{ImportError, ImportResult};

/// Maximum favicon data size (1 MB).
const MAX_FAVICON_SIZE: usize = 1024 * 1024;

/// A favicon entry with its image data and associated page URLs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaviconEntry {
    /// The URL of the favicon image itself.
    pub favicon_url: String,
    /// Page URLs that use this favicon.
    pub page_urls: Vec<String>,
    /// Raw image data (PNG, ICO, JPEG, GIF, WebP, or SVG).
    pub image_data: Vec<u8>,
}

/// Validates favicon data by checking magic bytes for known image formats.
fn is_valid_favicon_data(data: &[u8]) -> bool {
    if data.len() > MAX_FAVICON_SIZE {
        return false;
    }
    if !match_mime_type(data) {
        return false;
    }
    // PNG dimension check
    if data.len() >= 24 && data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47
    {
        let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        if width > 256 || height > 256 {
            return false;
        }
    }
    // ICO dimension check
    if data.len() >= 8 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 && data[3] == 0x00 {
        let num_images = u16::from_le_bytes([data[4], data[5]]) as usize;
        if data.len() >= 6 + num_images * 16 {
            for i in 0..num_images {
                let entry_offset = 6 + i * 16;
                let w_byte = data[entry_offset];
                let h_byte = data[entry_offset + 1];
                let width: u32 = if w_byte == 0 { 256 } else { w_byte as u32 };
                let height: u32 = if h_byte == 0 { 256 } else { h_byte as u32 };
                if width > 256 || height > 256 {
                    return false;
                }
            }
        }
    }
    true
}

/// Checks magic bytes for supported image formats.
fn match_mime_type(data: &[u8]) -> bool {
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
        && data[0] == b'G'
        && data[1] == b'I'
        && data[2] == b'F'
        && data[3] == b'8'
        && (data[4] == b'7' || data[4] == b'9')
        && data[5] == b'a'
    {
        return true;
    }
    // WebP
    if data.len() >= 12
        && data[0] == b'R'
        && data[1] == b'I'
        && data[2] == b'F'
        && data[3] == b'F'
        && data[8] == b'W'
        && data[9] == b'E'
        && data[10] == b'B'
        && data[11] == b'P'
    {
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

/// Parses Chromium favicons from a profile directory.
///
/// The `profile_dir` should contain a file named `Favicons`.
pub fn parse_chromium_favicons(profile_dir: &Path) -> ImportResult<Vec<FaviconEntry>> {
    let favicons_path = profile_dir.join("Favicons");

    if !favicons_path.exists() {
        return Err(ImportError::FileNotFound(
            favicons_path.display().to_string(),
        ));
    }

    let conn = open_readonly(&favicons_path)?;

    // Verify required tables exist
    if !table_exists(&conn, "icon_mapping")
        || !table_exists(&conn, "favicon_bitmaps")
        || !table_exists(&conn, "favicons")
    {
        return Err(ImportError::Parse(
            "required favicon tables not found".into(),
        ));
    }

    let mut stmt = conn
        .prepare(
            "SELECT f.url, im.page_url, fb.image_data \
             FROM icon_mapping im \
             JOIN favicons f ON f.id = im.icon_id \
             JOIN favicon_bitmaps fb ON fb.icon_id = f.id \
             WHERE fb.image_data IS NOT NULL AND length(fb.image_data) > 0 \
             ORDER BY f.url",
        )
        .map_err(|e| ImportError::Parse(format!("preparing favicons query: {e}")))?;

    let mut results: Vec<FaviconEntry> = Vec::new();
    let mut current_favicon_url = String::new();

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|e| ImportError::Parse(format!("querying favicons: {e}")))?;

    for row in rows {
        let (favicon_url, page_url, image_data) =
            row.map_err(|e| ImportError::Parse(format!("reading favicon row: {e}")))?;

        if !is_valid_favicon_data(&image_data) {
            continue;
        }

        if favicon_url != current_favicon_url {
            results.push(FaviconEntry {
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

    Ok(results)
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.prepare(&format!(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='{table_name}'"
    ))
    .and_then(|mut stmt| stmt.query_row([], |_| Ok(())))
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-import-test-chromium-favicons-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Minimal valid 1x1 PNG.
    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x01, // width = 1
            0x00, 0x00, 0x00, 0x01, // height = 1
            0x08, 0x02, // bit depth 8, color type 2 (RGB)
            0x00, 0x00, 0x00, // compression, filter, interlace
            0x90, 0x77, 0x53, 0xDE, // CRC
        ]
    }

    fn create_favicons_db(dir: &Path) {
        let db_path = dir.join("Favicons");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE favicons (id INTEGER PRIMARY KEY, url TEXT NOT NULL);
             CREATE TABLE favicon_bitmaps (id INTEGER PRIMARY KEY, icon_id INTEGER NOT NULL, image_data BLOB);
             CREATE TABLE icon_mapping (id INTEGER PRIMARY KEY, page_url TEXT NOT NULL, icon_id INTEGER NOT NULL);",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO favicons (id, url) VALUES (1, 'https://example.com/favicon.png')",
            [],
        )
        .unwrap();

        let png = minimal_png();
        conn.execute(
            "INSERT INTO favicon_bitmaps (icon_id, image_data) VALUES (1, ?1)",
            [&png],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO icon_mapping (page_url, icon_id) VALUES ('https://example.com', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO icon_mapping (page_url, icon_id) VALUES ('https://example.com/page', 1)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = create_test_dir("missing");
        let err = parse_chromium_favicons(&dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn corrupted_db_returns_error() {
        let dir = create_test_dir("corrupted");
        std::fs::write(dir.join("Favicons"), b"not a sqlite database").unwrap();
        let err = parse_chromium_favicons(&dir).unwrap_err();
        assert!(
            matches!(err, ImportError::Io(_) | ImportError::Parse(_)),
            "got {err:?}",
        );
    }

    #[test]
    fn parses_valid_favicons_db() {
        let dir = create_test_dir("valid");
        create_favicons_db(&dir);

        let entries = parse_chromium_favicons(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].favicon_url, "https://example.com/favicon.png");
        assert_eq!(entries[0].page_urls.len(), 2);
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com".to_string()));
        assert!(entries[0]
            .page_urls
            .contains(&"https://example.com/page".to_string()));
        assert!(!entries[0].image_data.is_empty());
    }

    #[test]
    fn rejects_invalid_image_data() {
        let dir = create_test_dir("invalid-img");
        let db_path = dir.join("Favicons");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE favicons (id INTEGER PRIMARY KEY, url TEXT NOT NULL);
             CREATE TABLE favicon_bitmaps (id INTEGER PRIMARY KEY, icon_id INTEGER, image_data BLOB);
             CREATE TABLE icon_mapping (id INTEGER PRIMARY KEY, page_url TEXT, icon_id INTEGER);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favicons (id, url) VALUES (1, 'https://bad.com/icon')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO favicon_bitmaps (icon_id, image_data) VALUES (1, X'DEADBEEF')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO icon_mapping (page_url, icon_id) VALUES ('https://bad.com', 1)",
            [],
        )
        .unwrap();

        let entries = parse_chromium_favicons(&dir).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn match_mime_type_detects_formats() {
        assert!(match_mime_type(&[0x89, 0x50, 0x4E, 0x47])); // PNG
        assert!(match_mime_type(&[0x00, 0x00, 0x01, 0x00])); // ICO
        assert!(match_mime_type(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
        assert!(match_mime_type(b"GIF87a")); // GIF87a
        assert!(match_mime_type(b"GIF89a")); // GIF89a
        assert!(!match_mime_type(&[0x01, 0x02, 0x03, 0x04])); // Random
    }
}
