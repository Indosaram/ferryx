//! Safari Bookmarks.plist parser.
//!
//! Ports `maho-chromium/browser/importer/safari_bookmark_parser.cc` to Rust.
//! Reads `<safari_dir>/Bookmarks.plist` and recursively extracts bookmark tree.

use std::path::Path;

use plist::Value;

use crate::{BookmarkEntry, ImportError, ImportResult};

pub fn parse_safari_bookmarks(safari_dir: &Path) -> ImportResult<Vec<BookmarkEntry>> {
    let plist_path = safari_dir.join("Bookmarks.plist");

    let data = std::fs::read(&plist_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ImportError::FileNotFound(plist_path.display().to_string()),
        std::io::ErrorKind::PermissionDenied => {
            ImportError::PermissionDenied(plist_path.display().to_string())
        }
        _ => ImportError::Io(format!("{}: {}", plist_path.display(), e)),
    })?;

    if data.is_empty() {
        return Ok(Vec::new());
    }

    let plist = Value::from_reader(std::io::Cursor::new(&data))
        .map_err(|e| ImportError::Parse(format!("Bookmarks.plist: {}", e)))?;

    let root = plist
        .as_dictionary()
        .ok_or_else(|| ImportError::Parse("plist root is not a dictionary".into()))?;

    let children = match root.get("Children").and_then(|v| v.as_array()) {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };

    let mut result = Vec::new();
    walk_children(children, &mut result, 0);
    Ok(result)
}

const MAX_DEPTH: u32 = 32;

fn walk_children(children: &[Value], out: &mut Vec<BookmarkEntry>, depth: u32) {
    if depth >= MAX_DEPTH {
        return;
    }

    for item in children {
        let Some(dict) = item.as_dictionary() else {
            continue;
        };

        let bm_type = match dict.get("WebBookmarkType").and_then(|v| v.as_string()) {
            Some(t) => t,
            None => continue,
        };

        match bm_type {
            "WebBookmarkTypeList" => {
                let title = dict
                    .get("Title")
                    .and_then(|v| v.as_string())
                    .unwrap_or("")
                    .to_string();

                let mut folder = BookmarkEntry {
                    title,
                    url: String::new(),
                    is_folder: true,
                    children: Vec::new(),
                };

                if let Some(sub) = dict.get("Children").and_then(|v| v.as_array()) {
                    walk_children(sub, &mut folder.children, depth + 1);
                }

                out.push(folder);
            }
            "WebBookmarkTypeLeaf" => {
                let mut title = dict
                    .get("Title")
                    .and_then(|v| v.as_string())
                    .unwrap_or("")
                    .to_string();

                if let Some(uri_dict) = dict.get("URIDictionary").and_then(|v| v.as_dictionary()) {
                    if let Some(leaf_title) = uri_dict.get("title").and_then(|v| v.as_string()) {
                        title = leaf_title.to_string();
                    }
                }

                let url_str = match dict.get("URLString").and_then(|v| v.as_string()) {
                    Some(u) => u,
                    None => continue,
                };

                if url::Url::parse(url_str).is_err() {
                    continue;
                }

                out.push(BookmarkEntry {
                    title,
                    url: url_str.to_string(),
                    is_folder: false,
                    children: Vec::new(),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err = parse_safari_bookmarks(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {:?}", err);
    }

    #[test]
    fn empty_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Bookmarks.plist"), b"").unwrap();
        let result = parse_safari_bookmarks(tmp.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn corrupted_plist_returns_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Bookmarks.plist"), b"not a plist").unwrap();
        let err = parse_safari_bookmarks(tmp.path()).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {:?}", err);
    }

    #[test]
    fn parses_minimal_plist() {
        let tmp = tempfile::tempdir().unwrap();

        let plist_data = plist::Value::Dictionary({
            let mut root = plist::Dictionary::new();
            root.insert(
                "WebBookmarkType".into(),
                Value::String("WebBookmarkTypeList".into()),
            );

            let leaf = {
                let mut d = plist::Dictionary::new();
                d.insert(
                    "WebBookmarkType".into(),
                    Value::String("WebBookmarkTypeLeaf".into()),
                );
                d.insert(
                    "URLString".into(),
                    Value::String("https://example.com".into()),
                );
                let mut uri = plist::Dictionary::new();
                uri.insert("title".into(), Value::String("Example".into()));
                d.insert("URIDictionary".into(), Value::Dictionary(uri));
                Value::Dictionary(d)
            };

            let folder = {
                let mut d = plist::Dictionary::new();
                d.insert(
                    "WebBookmarkType".into(),
                    Value::String("WebBookmarkTypeList".into()),
                );
                d.insert("Title".into(), Value::String("My Folder".into()));
                d.insert("Children".into(), Value::Array(vec![leaf.clone()]));
                Value::Dictionary(d)
            };

            root.insert("Children".into(), Value::Array(vec![folder, leaf]));
            root
        });

        let path = tmp.path().join("Bookmarks.plist");
        let mut file = std::fs::File::create(&path).unwrap();
        plist::to_writer_xml(&mut file, &plist_data).unwrap();

        let entries = parse_safari_bookmarks(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2);

        assert!(entries[0].is_folder);
        assert_eq!(entries[0].title, "My Folder");
        assert_eq!(entries[0].children.len(), 1);
        assert_eq!(entries[0].children[0].url, "https://example.com");
        assert_eq!(entries[0].children[0].title, "Example");

        assert!(!entries[1].is_folder);
        assert_eq!(entries[1].url, "https://example.com");
    }

    #[test]
    fn rejects_invalid_url() {
        let tmp = tempfile::tempdir().unwrap();

        let plist_data = plist::Value::Dictionary({
            let mut root = plist::Dictionary::new();
            let leaf = {
                let mut d = plist::Dictionary::new();
                d.insert(
                    "WebBookmarkType".into(),
                    Value::String("WebBookmarkTypeLeaf".into()),
                );
                d.insert("URLString".into(), Value::String("not a url".into()));
                Value::Dictionary(d)
            };
            root.insert("Children".into(), Value::Array(vec![leaf]));
            root
        });

        let path = tmp.path().join("Bookmarks.plist");
        let mut file = std::fs::File::create(&path).unwrap();
        plist::to_writer_xml(&mut file, &plist_data).unwrap();

        let entries = parse_safari_bookmarks(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }
}
