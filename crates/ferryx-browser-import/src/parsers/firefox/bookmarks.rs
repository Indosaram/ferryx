//! Firefox bookmark parser — reads `places.sqlite` (moz_bookmarks + moz_places).
//!
//! Ports `maho-chromium/browser/importer/firefox_places_parser.cc`.

use std::collections::HashMap;
use std::path::Path;
use crate::{BookmarkEntry, ImportError, ImportResult};

const MAX_RECURSION_DEPTH: usize = 50;

const BOOKMARK_TYPE: i32 = 1;
const FOLDER_TYPE: i32 = 2;

const ROOT_ID: i64 = 1;
const MENU_ID: i64 = 2;
const TOOLBAR_ID: i64 = 3;
const OTHER_ID: i64 = 5;
const MOBILE_ID: i64 = 6;

struct BookmarkRow {
    id: i64,
    row_type: i32,
    parent: i64,
    title: String,
    url: String,
}

pub fn parse_firefox_bookmarks(profile_dir: &Path) -> ImportResult<Vec<BookmarkEntry>> {
    let db_path = profile_dir.join("places.sqlite");

    if !db_path.exists() {
        return Err(ImportError::FileNotFound(db_path.display().to_string()));
    }

    let conn = open_readonly(&db_path)?;

    let mut stmt = conn
        .prepare(
            "SELECT b.id, b.type, b.parent, b.title, p.url \
             FROM moz_bookmarks b \
             LEFT JOIN moz_places p ON b.fk = p.id \
             WHERE b.type IN (1, 2) \
             ORDER BY b.parent, b.position",
        )
        .map_err(|e| ImportError::Parse(format!("prepare: {e}")))?;

    let mut children_by_parent: HashMap<i64, Vec<BookmarkRow>> = HashMap::new();

    let rows = stmt
        .query_map([], |row| {
            Ok(BookmarkRow {
                id: row.get(0)?,
                row_type: row.get(1)?,
                parent: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                url: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        })
        .map_err(|e| ImportError::Parse(format!("query: {e}")))?;

    for row_result in rows {
        let row = row_result.map_err(|e| ImportError::Parse(format!("row: {e}")))?;
        children_by_parent.entry(row.parent).or_default().push(row);
    }

    let mut results = Vec::new();
    let root_folders = [MENU_ID, TOOLBAR_ID, OTHER_ID, MOBILE_ID];

    if let Some(root_children) = children_by_parent.get(&ROOT_ID) {
        for root_id in root_folders {
            if let Some(folder_row) = root_children
                .iter()
                .find(|r| r.id == root_id && r.row_type == FOLDER_TYPE)
            {
                let mut folder = BookmarkEntry {
                    title: folder_row.title.clone(),
                    url: String::new(),
                    is_folder: true,
                    children: Vec::new(),
                };
                build_tree(root_id, &children_by_parent, &mut folder.children, 1);
                results.push(folder);
            }
        }
    }

    Ok(results)
}

fn build_tree(
    parent_id: i64,
    children_by_parent: &HashMap<i64, Vec<BookmarkRow>>,
    out: &mut Vec<BookmarkEntry>,
    depth: usize,
) {
    if depth > MAX_RECURSION_DEPTH {
        return;
    }

    let Some(children) = children_by_parent.get(&parent_id) else {
        return;
    };

    for row in children {
        if row.row_type == BOOKMARK_TYPE {
            if row.url.is_empty() {
                continue;
            }
            if row.url.starts_with("javascript:") || row.url.starts_with("place:") {
                continue;
            }
            let title = if row.title.is_empty() {
                url::Url::parse(&row.url)
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .unwrap_or_default()
            } else {
                row.title.clone()
            };
            out.push(BookmarkEntry {
                title,
                url: row.url.clone(),
                is_folder: false,
                children: Vec::new(),
            });
        } else if row.row_type == FOLDER_TYPE {
            let mut folder = BookmarkEntry {
                title: row.title.clone(),
                url: String::new(),
                is_folder: true,
                children: Vec::new(),
            };
            build_tree(row.id, children_by_parent, &mut folder.children, depth + 1);
            out.push(folder);
        }
    }
}

/// Reads a private copy: the browser may hold the live file open or keep rows in its WAL.
fn open_readonly(path: &Path) -> ImportResult<crate::sqlite_snapshot::Snapshot> {
    crate::sqlite_snapshot::open_snapshot(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::fs;

    fn create_places_db(dir: &Path) {
        let db_path = dir.join("places.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE moz_bookmarks (
                 id INTEGER PRIMARY KEY, type INTEGER, parent INTEGER,
                 fk INTEGER, title TEXT, dateAdded INTEGER, position INTEGER
             );
             -- Root folder
             INSERT INTO moz_bookmarks (id, type, parent, title, dateAdded, position)
                 VALUES (1, 2, 0, 'Root', 0, 0);
             -- Bookmarks Menu
             INSERT INTO moz_bookmarks (id, type, parent, title, dateAdded, position)
                 VALUES (2, 2, 1, 'Bookmarks Menu', 1000000, 0);
             -- Bookmarks Toolbar
             INSERT INTO moz_bookmarks (id, type, parent, title, dateAdded, position)
                 VALUES (3, 2, 1, 'Bookmarks Toolbar', 1000000, 1);
             -- A place
             INSERT INTO moz_places (id, url) VALUES (1, 'https://example.com');
             -- A bookmark under Menu
             INSERT INTO moz_bookmarks (id, type, parent, fk, title, dateAdded, position)
                 VALUES (10, 1, 2, 1, 'Example', 1718000000000000, 0);
             -- A subfolder under Toolbar
             INSERT INTO moz_bookmarks (id, type, parent, title, dateAdded, position)
                 VALUES (11, 2, 3, 'Dev', 1718000000000000, 0);
             -- A bookmark in subfolder
             INSERT INTO moz_places (id, url) VALUES (2, 'https://rust-lang.org');
             INSERT INTO moz_bookmarks (id, type, parent, fk, title, dateAdded, position)
                 VALUES (12, 1, 11, 2, 'Rust', 1718000000000000, 0);",
        )
        .unwrap();
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-bk-missing");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let err = parse_firefox_bookmarks(&tmp).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)));
    }

    #[test]
    fn corrupted_db_returns_parse_error() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-bk-corrupt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("places.sqlite"), b"not a database").unwrap();
        let err = parse_firefox_bookmarks(&tmp).unwrap_err();
        assert!(
            matches!(err, ImportError::Parse(_) | ImportError::Io(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn parses_valid_bookmarks() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-bk-valid");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        create_places_db(&tmp);

        let entries = parse_firefox_bookmarks(&tmp).unwrap();
        assert_eq!(entries.len(), 2); // Menu + Toolbar (Other/Mobile absent)

        let menu = &entries[0];
        assert_eq!(menu.title, "Bookmarks Menu");
        assert!(menu.is_folder);
        assert_eq!(menu.children.len(), 1);
        assert_eq!(menu.children[0].title, "Example");
        assert_eq!(menu.children[0].url, "https://example.com");

        let toolbar = &entries[1];
        assert_eq!(toolbar.title, "Bookmarks Toolbar");
        assert!(toolbar.is_folder);
        assert_eq!(toolbar.children.len(), 1);
        let subfolder = &toolbar.children[0];
        assert_eq!(subfolder.title, "Dev");
        assert!(subfolder.is_folder);
        assert_eq!(subfolder.children.len(), 1);
        assert_eq!(subfolder.children[0].url, "https://rust-lang.org");
    }

    #[test]
    fn skips_javascript_and_place_urls() {
        let tmp = std::env::temp_dir().join("ferryx-import-ff-bk-scheme");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("places.sqlite");
        let _ = fs::remove_file(&db_path);
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE moz_bookmarks (
                 id INTEGER PRIMARY KEY, type INTEGER, parent INTEGER,
                 fk INTEGER, title TEXT, dateAdded INTEGER, position INTEGER
             );
             INSERT INTO moz_bookmarks VALUES (1, 2, 0, NULL, 'Root', 0, 0);
             INSERT INTO moz_bookmarks VALUES (2, 2, 1, NULL, 'Menu', 0, 0);
             INSERT INTO moz_places VALUES (1, 'javascript:alert(1)');
             INSERT INTO moz_places VALUES (2, 'place:sort=8&maxResults=10');
             INSERT INTO moz_bookmarks VALUES (10, 1, 2, 1, 'JS', 0, 0);
             INSERT INTO moz_bookmarks VALUES (11, 1, 2, 2, 'Place', 0, 1);",
        )
        .unwrap();
        drop(conn);

        let entries = parse_firefox_bookmarks(&tmp).unwrap();
        let menu = &entries[0];
        assert!(menu.children.is_empty());
    }
}
