//! Chromium Bookmarks JSON parser.
//!
//! Ports `maho-chromium/browser/importer/chromium_bookmark_parser.cc` to Rust.
//! Reads the `Bookmarks` JSON file from a Chromium profile directory and
//! extracts bookmark entries recursively.

use std::path::Path;

use serde_json::Value;

use crate::{BookmarkEntry, ImportError, ImportResult};

/// Maximum file size (100 MB).
const MAX_FILE_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// Maximum recursion depth for folder nesting.
const MAX_RECURSION_DEPTH: usize = 50;

fn is_importable_url(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    if url.starts_with("javascript:") || url.starts_with("chrome:") {
        return false;
    }
    url::Url::parse(url).is_ok()
}

fn parse_node_recursive(
    node: &serde_json::Map<String, Value>,
    out: &mut Vec<BookmarkEntry>,
    depth: usize,
) {
    if depth > MAX_RECURSION_DEPTH {
        return;
    }

    let Some(node_type) = node.get("type").and_then(|v| v.as_str()) else {
        return;
    };

    let title = node
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    match node_type {
        "url" => {
            let Some(url_str) = node.get("url").and_then(|v| v.as_str()) else {
                return;
            };
            if !is_importable_url(url_str) {
                return;
            }
            out.push(BookmarkEntry {
                title,
                url: url_str.to_string(),
                is_folder: false,
                children: Vec::new(),
            });
        }
        "folder" => {
            let mut folder = BookmarkEntry {
                title,
                url: String::new(),
                is_folder: true,
                children: Vec::new(),
            };

            if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
                for child in children {
                    if let Some(child_obj) = child.as_object() {
                        parse_node_recursive(child_obj, &mut folder.children, depth + 1);
                    }
                }
            }

            out.push(folder);
        }
        _ => {}
    }
}

fn parse_root_folder(
    roots: &serde_json::Map<String, Value>,
    root_name: &str,
    out: &mut Vec<BookmarkEntry>,
) {
    let Some(root) = roots.get(root_name).and_then(|v| v.as_object()) else {
        return;
    };
    let Some(children) = root.get("children").and_then(|v| v.as_array()) else {
        return;
    };
    for child in children {
        if let Some(child_obj) = child.as_object() {
            parse_node_recursive(child_obj, out, 1);
        }
    }
}

/// Parses Chromium bookmarks from a profile directory.
///
/// The `profile_dir` should contain a file named `Bookmarks`.
pub fn parse_chromium_bookmarks(profile_dir: &Path) -> ImportResult<Vec<BookmarkEntry>> {
    let bookmarks_path = profile_dir.join("Bookmarks");

    if !bookmarks_path.exists() {
        return Err(ImportError::FileNotFound(
            bookmarks_path.display().to_string(),
        ));
    }

    let metadata = std::fs::metadata(&bookmarks_path)
        .map_err(|e| ImportError::Io(format!("{}: {e}", bookmarks_path.display())))?;

    if metadata.len() > MAX_FILE_SIZE_BYTES {
        return Err(ImportError::Parse("Bookmarks file too large".into()));
    }

    let content = std::fs::read_to_string(&bookmarks_path)
        .map_err(|e| ImportError::Io(format!("{}: {e}", bookmarks_path.display())))?;

    if content.is_empty() {
        return Ok(Vec::new());
    }

    let root: Value = serde_json::from_str(&content)
        .map_err(|e| ImportError::Parse(format!("Bookmarks JSON: {e}")))?;

    let Some(root_obj) = root.as_object() else {
        return Err(ImportError::Parse("root is not a JSON object".into()));
    };

    let Some(roots) = root_obj.get("roots").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
    };

    let mut results = Vec::new();
    parse_root_folder(roots, "bookmark_bar", &mut results);
    parse_root_folder(roots, "other", &mut results);
    parse_root_folder(roots, "synced", &mut results);

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-import-test-chromium-bookmarks-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_returns_file_not_found() {
        let dir = create_test_dir("missing");
        let err = parse_chromium_bookmarks(&dir).unwrap_err();
        assert!(matches!(err, ImportError::FileNotFound(_)), "got {err:?}");
    }

    #[test]
    fn empty_file_returns_empty() {
        let dir = create_test_dir("empty");
        fs::write(dir.join("Bookmarks"), "").unwrap();
        let res = parse_chromium_bookmarks(&dir).unwrap();
        assert!(res.is_empty());
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let dir = create_test_dir("badjson");
        fs::write(dir.join("Bookmarks"), "{invalid json!!").unwrap();
        let err = parse_chromium_bookmarks(&dir).unwrap_err();
        assert!(matches!(err, ImportError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn parses_minimal_bookmarks() {
        let dir = create_test_dir("valid");
        let fixture = serde_json::json!({
            "roots": {
                "bookmark_bar": {
                    "children": [
                        {
                            "type": "url",
                            "name": "Rust",
                            "url": "https://rust-lang.org",
                            "date_added": "13350000000000000"
                        },
                        {
                            "type": "folder",
                            "name": "Work",
                            "date_added": "13349000000000000",
                            "children": [
                                {
                                    "type": "url",
                                    "name": "GitHub",
                                    "url": "https://github.com",
                                    "date_added": "13348000000000000"
                                }
                            ]
                        }
                    ]
                },
                "other": { "children": [] },
                "synced": { "children": [] }
            }
        });
        fs::write(
            dir.join("Bookmarks"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let entries = parse_chromium_bookmarks(&dir).unwrap();
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].title, "Rust");
        assert_eq!(entries[0].url, "https://rust-lang.org");
        assert!(!entries[0].is_folder);

        assert_eq!(entries[1].title, "Work");
        assert!(entries[1].is_folder);
        assert_eq!(entries[1].children.len(), 1);
        assert_eq!(entries[1].children[0].title, "GitHub");
    }

    #[test]
    fn rejects_javascript_urls() {
        let dir = create_test_dir("jsurl");
        let fixture = serde_json::json!({
            "roots": {
                "bookmark_bar": {
                    "children": [
                        {
                            "type": "url",
                            "name": "XSS",
                            "url": "javascript:alert(1)",
                            "date_added": "0"
                        }
                    ]
                }
            }
        });
        fs::write(
            dir.join("Bookmarks"),
            serde_json::to_string(&fixture).unwrap(),
        )
        .unwrap();

        let entries = parse_chromium_bookmarks(&dir).unwrap();
        assert!(entries.is_empty());
    }
}
