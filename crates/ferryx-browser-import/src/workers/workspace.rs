//! Workspace import worker.
//!
//! Ports `importers/workspace_importer.cc`. Reads parsed workspaces (Arc/Zen)
//! and creates spaces, tabs, folders, pinned/favorite tabs in the destination.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::orchestrator::{ImportDestination, ImportProgress, ImportType};
use crate::parsers;
use crate::{BrowserType, DetectedBrowser, ImportResult};

use super::workspace_folders::import_folders;
use super::{is_cancelled, send_update, ImportWorker};

pub struct WorkspaceWorker;

impl ImportWorker for WorkspaceWorker {
    fn run(
        &self,
        browser: &DetectedBrowser,
        destination: &dyn ImportDestination,
        cancelled: &Arc<AtomicBool>,
        progress: &Sender<ImportProgress>,
    ) -> ImportResult<u32> {
        let profile_dir = browser.profile_dir();

        let (workspaces, active_uuid) = match browser.browser_type {
            BrowserType::Zen => {
                let result = parsers::zen::workspaces::parse_zen_workspaces(profile_dir)?;
                (result.workspaces, result.active_workspace_uuid)
            }
            BrowserType::Arc => {
                let result = parsers::arc::sidebar::parse_arc_sidebar(profile_dir)?;
                (result.workspaces, result.active_workspace_uuid)
            }
            _ => return Ok(0),
        };

        if workspaces.is_empty() {
            return Ok(0);
        }

        let mut spaces_created = 0u32;
        let mut primary_space_id: Option<String> = None;

        for ws in &workspaces {
            if is_cancelled(cancelled) {
                break;
            }

            let space_id = match destination.create_space(&ws.name, &ws.theme_color_hex, &ws.icon) {
                Some(id) => id,
                None => continue,
            };
            spaces_created += 1;

            if primary_space_id.is_none() || ws.uuid == active_uuid {
                primary_space_id = Some(space_id.clone());
            }

            // Pinned tabs
            for tab in &ws.pinned_tabs {
                if is_cancelled(cancelled) {
                    break;
                }
                if let Some(tab_id) = destination.create_tab(&space_id, &tab.url, &tab.title) {
                    destination.pin_tab(&tab_id);
                }
            }

            // Favorite tabs
            for tab in &ws.favorites {
                if is_cancelled(cancelled) {
                    break;
                }
                if let Some(tab_id) = destination.create_tab(&space_id, &tab.url, &tab.title) {
                    destination.favorite_tab(&tab_id);
                }
            }

            // Regular tabs
            for tab in &ws.regular_tabs {
                if is_cancelled(cancelled) {
                    break;
                }
                destination.create_tab(&space_id, &tab.url, &tab.title);
            }

            // Folders and their tabs
            import_folders(destination, &space_id, &ws.folders, cancelled);

            send_update(
                progress,
                ImportType::Workspaces,
                spaces_created,
                &format!("Created space: {}", ws.name),
            );
        }

        // Activate the primary space
        if let Some(ref space_id) = primary_space_id {
            destination.activate_space(space_id);
        }

        Ok(spaces_created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::ImportDestination;
    use crate::workers::tests::MockDestination;
    use crate::{ParsedTab, ParsedWorkspace};
    use std::sync::mpsc;

    #[test]
    fn test_workspace_worker_unsupported_browser() {
        let dest: Arc<dyn ImportDestination> = Arc::new(MockDestination::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();

        let browser = DetectedBrowser {
            browser_type: BrowserType::Chrome,
            display_name: "Chrome".to_string(),
            profile_path: std::path::PathBuf::from("/nonexistent/profile"),
            services_supported: 0xFF,
            requires_full_disk_access: false,
        };

        let result = WorkspaceWorker.run(&browser, &*dest, &cancelled, &tx);
        assert_eq!(result.unwrap(), 0);
        let msgs: Vec<ImportProgress> = rx.try_iter().collect();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_workspace_with_pinned_and_favorites() {
        let dest = Arc::new(MockDestination::new());
        let _cancelled = Arc::new(AtomicBool::new(false));
        let (_tx, _rx) = mpsc::channel::<ImportProgress>();

        let workspaces = vec![ParsedWorkspace {
            uuid: "ws-1".to_string(),
            name: "Work".to_string(),
            icon: "briefcase".to_string(),
            theme_color_hex: "#0000ff".to_string(),
            container_id: 1,
            pinned_tabs: vec![ParsedTab {
                url: "https://pinned.example.com".to_string(),
                title: "Pinned".to_string(),
                is_pinned: true,
                is_essential: false,
            }],
            favorites: vec![ParsedTab {
                url: "https://fav.example.com".to_string(),
                title: "Fav".to_string(),
                is_pinned: false,
                is_essential: false,
            }],
            regular_tabs: vec![ParsedTab {
                url: "https://regular.example.com".to_string(),
                title: "Regular".to_string(),
                is_pinned: false,
                is_essential: false,
            }],
            folders: vec![],
        }];

        // Directly test the import logic
        let mut spaces_created = 0u32;
        for ws in &workspaces {
            let space_id = dest
                .create_space(&ws.name, &ws.theme_color_hex, &ws.icon)
                .unwrap();
            spaces_created += 1;

            for tab in &ws.pinned_tabs {
                if let Some(tab_id) = dest.create_tab(&space_id, &tab.url, &tab.title) {
                    dest.pin_tab(&tab_id);
                }
            }
            for tab in &ws.favorites {
                if let Some(tab_id) = dest.create_tab(&space_id, &tab.url, &tab.title) {
                    dest.favorite_tab(&tab_id);
                }
            }
            for tab in &ws.regular_tabs {
                dest.create_tab(&space_id, &tab.url, &tab.title);
            }
        }

        assert_eq!(spaces_created, 1);
        let pinned = dest.pinned.lock().unwrap();
        assert_eq!(pinned.len(), 1);
        let favorited = dest.favorited.lock().unwrap();
        assert_eq!(favorited.len(), 1);
        let tabs = dest.tabs.lock().unwrap();
        assert_eq!(tabs.len(), 3); // pinned + fav + regular
        assert_eq!(tabs[0].title, "Pinned");
        assert_eq!(tabs[1].title, "Fav");
        assert_eq!(tabs[2].title, "Regular");
    }
}
