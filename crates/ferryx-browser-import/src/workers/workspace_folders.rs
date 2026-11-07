use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::orchestrator::ImportDestination;
use crate::ParsedFolder;

use super::is_cancelled;

/// Import folders with parent-child relationships (recursive resolution).
pub(super) fn import_folders(
    destination: &dyn ImportDestination,
    space_id: &str,
    folders: &[ParsedFolder],
    cancelled: &Arc<AtomicBool>,
) {
    let mut id_map: HashMap<String, String> = HashMap::new();

    for folder in folders {
        if is_cancelled(cancelled) {
            break;
        }
        get_or_create_folder(destination, space_id, folders, &folder.id, &mut id_map);

        // Create tabs in this folder
        let app_folder_id = id_map.get(&folder.id).cloned().unwrap_or_default();
        for tab in &folder.tabs {
            if is_cancelled(cancelled) {
                break;
            }
            if app_folder_id.is_empty() {
                destination.create_tab(space_id, &tab.url, &tab.title);
            } else {
                destination.create_tab_in_folder(space_id, &tab.url, &tab.title, &app_folder_id);
            }
        }
    }
}

/// Recursively get or create a folder, resolving parent chains.
fn get_or_create_folder(
    destination: &dyn ImportDestination,
    space_id: &str,
    all_folders: &[ParsedFolder],
    folder_id: &str,
    id_map: &mut HashMap<String, String>,
) -> Option<String> {
    if folder_id.is_empty() {
        return None;
    }

    if let Some(existing) = id_map.get(folder_id) {
        return Some(existing.clone());
    }

    let folder_info = all_folders.iter().find(|f| f.id == folder_id)?;

    // Resolve parent first
    let parent_app_id = if !folder_info.parent_id.is_empty() {
        get_or_create_folder(
            destination,
            space_id,
            all_folders,
            &folder_info.parent_id,
            id_map,
        )
        .unwrap_or_default()
    } else {
        String::new()
    };

    let app_id = destination.create_folder(space_id, &folder_info.name, &parent_app_id)?;
    id_map.insert(folder_id.to_string(), app_id.clone());
    Some(app_id)
}

#[cfg(test)]
pub(in crate::workers) mod test_support {
    #[derive(Debug, Clone)]
    pub struct MockSpace {
        pub id: String,
        pub name: String,
        pub theme: String,
        pub icon: String,
    }

    #[derive(Debug, Clone)]
    pub struct MockTab {
        pub id: String,
        pub space_id: String,
        pub url: String,
        pub title: String,
        pub folder_id: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub struct MockFolder {
        pub id: String,
        pub space_id: String,
        pub name: String,
        pub parent_id: String,
    }

    #[derive(Debug, Clone)]
    pub struct MockBookmark {
        pub title: String,
        pub url: String,
        pub folder_path: Vec<String>,
    }

    #[derive(Debug, Clone)]
    pub struct MockHistory {
        pub url: String,
        pub title: String,
        pub visit_time: f64,
        pub visit_count: u32,
    }

    #[derive(Debug, Clone)]
    pub struct MockCookie {
        pub host: String,
        pub name: String,
        pub value: String,
        pub path: String,
    }

    #[derive(Debug, Clone)]
    pub struct MockAutofill {
        pub field_name: String,
        pub value: String,
    }

    #[derive(Debug, Clone)]
    pub struct MockFavicon {
        pub url: String,
        pub data_len: usize,
    }

    #[derive(Debug, Clone)]
    pub struct MockPassword {
        pub origin_url: String,
        pub username: String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::ImportDestination;
    use crate::workers::tests::MockDestination;
    use crate::{ParsedFolder, ParsedTab};

    #[test]
    fn test_import_folders_with_mock() {
        let dest = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));

        // Create a space first
        let space_id = dest.create_space("Test Space", "#ff0000", "star").unwrap();

        let folders = vec![
            ParsedFolder {
                id: "f1".to_string(),
                name: "Root Folder".to_string(),
                parent_id: String::new(),
                workspace_id: "ws1".to_string(),
                tabs: vec![ParsedTab {
                    url: "https://example.com".to_string(),
                    title: "Example".to_string(),
                    is_pinned: false,
                    is_essential: false,
                }],
            },
            ParsedFolder {
                id: "f2".to_string(),
                name: "Child Folder".to_string(),
                parent_id: "f1".to_string(),
                workspace_id: "ws1".to_string(),
                tabs: vec![ParsedTab {
                    url: "https://child.example.com".to_string(),
                    title: "Child".to_string(),
                    is_pinned: false,
                    is_essential: false,
                }],
            },
            ParsedFolder {
                id: String::new(),
                name: "Unresolved Folder".to_string(),
                parent_id: String::new(),
                workspace_id: "ws1".to_string(),
                tabs: vec![ParsedTab {
                    url: "https://fallback.example.com".to_string(),
                    title: "Folder Fallback".to_string(),
                    is_pinned: false,
                    is_essential: false,
                }],
            },
        ];

        import_folders(&*dest, &space_id, &folders, &cancelled);

        let created_folders = dest.folders.lock().unwrap();
        assert_eq!(created_folders.len(), 2);
        assert_eq!(created_folders[0].name, "Root Folder");
        assert!(created_folders[0].parent_id.is_empty());
        assert_eq!(created_folders[1].name, "Child Folder");
        // Child folder's parent is the root folder's app ID
        assert_eq!(created_folders[1].parent_id, created_folders[0].id);

        let tabs = dest.tabs.lock().unwrap();
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].title, "Example");
        assert_eq!(tabs[1].title, "Child");
        assert_eq!(tabs[2].title, "Folder Fallback");
        assert!(tabs[2].folder_id.is_none());
    }
}
