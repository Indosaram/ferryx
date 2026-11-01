use std::sync::mpsc;
use std::sync::Mutex;

use ferryx_browser_import::orchestrator::{ImportDestination, ImportProgress, Orchestrator};
use ferryx_browser_import::{BrowserType, DetectedBrowser, ImportServices};

#[derive(Debug, Clone)]
struct RecTab {
    space_id: String,
    url: String,
    title: String,
    folder_id: Option<String>,
}

#[derive(Debug, Clone)]
struct RecFolder {
    id: String,
    name: String,
}

#[derive(Default)]
struct Recorder {
    spaces: Mutex<Vec<(String, String, String, String)>>, // (id, name, theme, icon)
    tabs: Mutex<Vec<RecTab>>,
    folders: Mutex<Vec<RecFolder>>,
    pinned: Mutex<Vec<String>>,
    favorited: Mutex<Vec<String>>,
    active_space: Mutex<Option<String>>,
    next_id: Mutex<u32>,
}

impl Recorder {
    fn next(&self, prefix: &str) -> String {
        let mut n = self.next_id.lock().unwrap();
        *n += 1;
        format!("{prefix}-{}", *n)
    }

    fn space_name(&self, space_id: &str) -> String {
        self.spaces
            .lock()
            .unwrap()
            .iter()
            .find(|(id, ..)| id == space_id)
            .map(|(_, name, ..)| name.clone())
            .unwrap_or_default()
    }
}

impl ImportDestination for Recorder {
    fn create_space(&self, name: &str, theme: &str, icon: &str) -> Option<String> {
        let id = self.next("space");
        self.spaces.lock().unwrap().push((
            id.clone(),
            name.to_string(),
            theme.to_string(),
            icon.to_string(),
        ));
        Some(id)
    }

    fn create_tab(&self, space_id: &str, url: &str, title: &str) -> Option<String> {
        let id = self.next("tab");
        self.tabs.lock().unwrap().push(RecTab {
            space_id: space_id.to_string(),
            url: url.to_string(),
            title: title.to_string(),
            folder_id: None,
        });
        Some(id)
    }

    fn create_tab_in_folder(
        &self,
        space_id: &str,
        url: &str,
        title: &str,
        folder_id: &str,
    ) -> Option<String> {
        let id = self.next("tab");
        self.tabs.lock().unwrap().push(RecTab {
            space_id: space_id.to_string(),
            url: url.to_string(),
            title: title.to_string(),
            folder_id: Some(folder_id.to_string()),
        });
        Some(id)
    }

    fn create_folder(&self, space_id: &str, name: &str, parent_id: &str) -> Option<String> {
        let _ = (space_id, parent_id);
        let id = self.next("folder");
        self.folders.lock().unwrap().push(RecFolder {
            id: id.clone(),
            name: name.to_string(),
        });
        Some(id)
    }

    fn pin_tab(&self, tab_id: &str) {
        self.pinned.lock().unwrap().push(tab_id.to_string());
    }

    fn favorite_tab(&self, tab_id: &str) {
        self.favorited.lock().unwrap().push(tab_id.to_string());
    }

    fn activate_space(&self, space_id: &str) {
        *self.active_space.lock().unwrap() = Some(space_id.to_string());
    }

    fn get_active_space_id(&self) -> Option<String> {
        self.active_space.lock().unwrap().clone()
    }

    fn add_bookmark(&self, _t: &str, _u: &str, _p: &[String]) -> Option<String> {
        None
    }
    fn add_history(&self, _u: &str, _t: &str, _vt: f64, _vc: u32) -> bool {
        false
    }
    fn add_cookie(&self, _c: &ferryx_browser_import::CookieEntry) -> bool {
        false
    }
    fn add_autofill(&self, _f: &str, _v: &str, _tu: i32, _fu: i64, _lu: i64) -> bool {
        false
    }
    fn add_favicon(&self, _u: &str, _b: &[u8]) -> bool {
        false
    }
}

fn run_workspace_import(browser: &DetectedBrowser) -> std::sync::Arc<Recorder> {
    let dest = std::sync::Arc::new(Recorder::default());
    let orch = Orchestrator::new(dest.clone());
    let (tx, rx) = mpsc::channel();
    orch.start_import(browser, ImportServices::WORKSPACES, tx)
        .unwrap();

    let msgs: Vec<_> = rx.try_iter().collect();
    assert_eq!(
        msgs.last(),
        Some(&ImportProgress::AllComplete),
        "orchestrator must emit AllComplete"
    );
    dest
}

#[test]
fn arc_real_sidebar_extracts_and_imports_spaces() {
    let tmp = tempfile::tempdir().unwrap();
    let user_data = tmp.path().join("Arc/User Data");
    let default_profile = user_data.join("Default");
    std::fs::create_dir_all(&default_profile).unwrap();
    std::fs::write(default_profile.join("Bookmarks"), "{}").unwrap();

    let sidebar = serde_json::json!({
        "firebaseSyncState": {
            "syncData": {
                "spaceModels": [
                    "space-uuid-work",
                    {
                        "title": "Work",
                        "customInfo": { "iconType": { "emoji_v2": "💼" } },
                        "containerIDs": ["pinned", "work-pin", "unpinned", "work-unpin"]
                    },
                    "space-uuid-home",
                    {
                        "title": "Personal",
                        "customInfo": { "iconType": { "icon": "house" } },
                        "containerIDs": ["pinned", "home-pin", "unpinned", "home-unpin"]
                    }
                ],
                "items": [
                    "i", { "id": "work-pin", "data": { "itemContainer": {} },
                           "childrenIds": ["c", "tab-gh", "c", "folder-docs"] },
                    "i", { "id": "tab-gh",
                           "data": { "tab": { "savedURL": "https://github.com", "savedTitle": "GitHub" } } },
                    "i", { "id": "folder-docs", "title": "Docs", "data": { "list": {} },
                           "childrenIds": ["c", "tab-docs"] },
                    "i", { "id": "tab-docs",
                           "data": { "tab": { "savedURL": "https://docs.rs", "savedTitle": "Docs.rs" } } },
                    "i", { "id": "work-unpin", "data": { "itemContainer": {} },
                           "childrenIds": ["c", "tab-hn"] },
                    "i", { "id": "tab-hn",
                           "data": { "tab": { "savedURL": "https://news.ycombinator.com", "savedTitle": "HN" } } },
                    "i", { "id": "home-pin", "data": { "itemContainer": {} },
                           "childrenIds": ["c", "tab-mail"] },
                    "i", { "id": "tab-mail",
                           "data": { "tab": { "savedURL": "https://mail.example.com", "savedTitle": "Mail" } } },
                    "i", { "id": "home-unpin", "data": { "itemContainer": {} }, "childrenIds": [] }
                ]
            }
        }
    });
    std::fs::write(
        user_data.join("StorableSidebar.json"),
        serde_json::to_string(&sidebar).unwrap(),
    )
    .unwrap();

    // Mirrors maho_browser_detector.cc: a data filename is appended to the
    // detected profile dir; the worker recovers the dir via parent().
    let browser = DetectedBrowser {
        browser_type: BrowserType::Arc,
        display_name: "Arc".to_string(),
        profile_path: default_profile.join("Bookmarks"),
        services_supported: ImportServices::WORKSPACES,
        requires_full_disk_access: false,
    };

    let dest = run_workspace_import(&browser);

    let spaces = dest.spaces.lock().unwrap();
    let names: Vec<&str> = spaces.iter().map(|(_, n, ..)| n.as_str()).collect();
    assert_eq!(spaces.len(), 2, "both Arc spaces imported");
    assert!(names.contains(&"Work"));
    assert!(names.contains(&"Personal"));

    let work_id = spaces
        .iter()
        .find(|(_, n, ..)| n == "Work")
        .map(|(id, ..)| id.clone())
        .unwrap();
    let personal_id = spaces
        .iter()
        .find(|(_, n, ..)| n == "Personal")
        .map(|(id, ..)| id.clone())
        .unwrap();
    let work_icon = spaces
        .iter()
        .find(|(_, n, ..)| n == "Work")
        .map(|(.., i)| i.clone());
    assert_eq!(work_icon.as_deref(), Some("💼"));
    let home_icon = spaces
        .iter()
        .find(|(_, n, ..)| n == "Personal")
        .map(|(.., i)| i.clone());
    assert_eq!(home_icon.as_deref(), Some("house"));
    drop(spaces);

    let tabs = dest.tabs.lock().unwrap();
    let find = |url: &str| {
        tabs.iter()
            .find(|t| t.url == url)
            .unwrap_or_else(|| panic!("missing tab {url}"))
    };

    assert_eq!(
        find("https://github.com").space_id,
        work_id,
        "github in Work"
    );
    assert_eq!(find("https://github.com").title, "GitHub", "github title");
    assert_eq!(find("https://docs.rs").space_id, work_id, "docs.rs in Work");
    assert_eq!(find("https://docs.rs").title, "Docs.rs", "docs.rs title");
    assert_eq!(
        find("https://news.ycombinator.com").space_id,
        work_id,
        "HN in Work"
    );
    assert_eq!(find("https://news.ycombinator.com").title, "HN", "HN title");
    assert_eq!(
        find("https://mail.example.com").space_id,
        personal_id,
        "mail in Personal"
    );
    assert_eq!(find("https://mail.example.com").title, "Mail", "mail title");

    // Pinned tabs (github + mail) are pinned in the destination.
    assert_eq!(dest.pinned.lock().unwrap().len(), 2, "two pinned tabs");

    // The Docs folder exists and docs.rs is nested inside it (not flat).
    let folders = dest.folders.lock().unwrap();
    let docs = folders
        .iter()
        .find(|f| f.name == "Docs")
        .expect("Docs folder created");
    assert_eq!(
        find("https://docs.rs").folder_id.as_deref(),
        Some(docs.id.as_str()),
        "docs.rs must be nested under the Docs folder"
    );

    // The HN regular tab is NOT in a folder.
    assert!(
        find("https://news.ycombinator.com").folder_id.is_none(),
        "regular tab is not foldered"
    );

    // Arc has no active-workspace pref → first space (Work) becomes active.
    let active = dest.active_space.lock().unwrap().clone().unwrap();
    assert_eq!(dest.space_name(&active), "Work");
}

fn make_mozlz4(json: &str) -> Vec<u8> {
    let compressed = lz4_flex::block::compress(json.as_bytes());
    let size = json.len() as u32;
    let mut data = Vec::new();
    data.extend_from_slice(b"mozLz40\0");
    data.extend_from_slice(&size.to_le_bytes());
    data.extend_from_slice(&compressed);
    data
}

#[test]
fn zen_real_session_extracts_and_imports_spaces() {
    let tmp = tempfile::tempdir().unwrap();
    let profile = tmp.path().join("zen/abcd1234.default");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("places.sqlite"), "fake").unwrap();

    let session = serde_json::json!({
        "spaces": [
            {
                "uuid": "zws-dev",
                "name": "Dev",
                "userContextId": 1,
                "icon": "🦊",
                "theme": { "gradientColors": [{ "color": "#aa00ff" }] }
            },
            { "uuid": "zws-media", "name": "Media", "userContextId": 2 }
        ],
        "tabs": [
            { "zenWorkspace": "zws-dev",
              "entries": [{ "url": "https://rust-lang.org", "title": "Rust" }],
              "pinned": true, "zenEssential": false },
            { "zenWorkspace": "zws-dev",
              "entries": [{ "url": "https://crates.io", "title": "crates.io" }],
              "folderId": "zf-crates", "pinned": false, "zenEssential": false },
            { "zenWorkspace": "zws-dev",
              "entries": [{ "url": "https://lib.rs", "title": "" }],
              "pinned": false, "zenEssential": true },
            { "zenWorkspace": "zws-media",
              "entries": [{ "url": "https://youtube.com", "title": "YouTube" }],
              "pinned": false, "zenEssential": false }
        ],
        "folders": [
            { "id": "zf-crates", "name": "Crates", "workspaceId": "zws-dev" }
        ]
    });
    std::fs::write(
        profile.join("zen-sessions.jsonlz4"),
        make_mozlz4(&serde_json::to_string(&session).unwrap()),
    )
    .unwrap();
    std::fs::write(
        profile.join("prefs.js"),
        "user_pref(\"zen.workspaces.active\", \"zws-media\");\n",
    )
    .unwrap();

    let browser = DetectedBrowser {
        browser_type: BrowserType::Zen,
        display_name: "Zen".to_string(),
        profile_path: profile.join("places.sqlite"),
        services_supported: ImportServices::WORKSPACES,
        requires_full_disk_access: false,
    };

    let dest = run_workspace_import(&browser);

    let spaces = dest.spaces.lock().unwrap();
    assert_eq!(spaces.len(), 2, "both Zen workspaces imported");
    let dev = spaces
        .iter()
        .find(|(_, n, ..)| n == "Dev")
        .expect("Dev space");
    let dev_id = dev.0.clone();
    assert_eq!(dev.2, "#aa00ff", "Dev theme color imported");
    assert_eq!(dev.3, "🦊", "Dev icon imported");
    let media_id = spaces
        .iter()
        .find(|(_, n, ..)| n == "Media")
        .map(|(id, ..)| id.clone())
        .expect("Media space");
    drop(spaces);

    let tabs = dest.tabs.lock().unwrap();
    let find = |url: &str| {
        tabs.iter()
            .find(|t| t.url == url)
            .unwrap_or_else(|| panic!("missing tab {url}"))
    };
    assert_eq!(
        find("https://rust-lang.org").space_id,
        dev_id,
        "rust in Dev"
    );
    assert_eq!(find("https://rust-lang.org").title, "Rust", "pinned title");
    assert_eq!(find("https://crates.io").space_id, dev_id, "crates in Dev");
    assert_eq!(
        find("https://crates.io").title,
        "crates.io",
        "foldered title"
    );
    assert_eq!(find("https://lib.rs").space_id, dev_id, "lib.rs in Dev");
    assert_eq!(find("https://lib.rs").title, "", "empty favorite title");
    assert_eq!(
        find("https://youtube.com").space_id,
        media_id,
        "youtube in Media"
    );
    assert_eq!(
        find("https://youtube.com").title,
        "YouTube",
        "regular title"
    );

    assert_eq!(dest.pinned.lock().unwrap().len(), 1, "rust-lang is pinned");
    assert_eq!(
        dest.favorited.lock().unwrap().len(),
        1,
        "lib.rs is favorited (essential)"
    );

    let folders = dest.folders.lock().unwrap();
    let crates = folders
        .iter()
        .find(|f| f.name == "Crates")
        .expect("Crates folder");
    assert_eq!(
        find("https://crates.io").folder_id.as_deref(),
        Some(crates.id.as_str()),
        "crates.io nested under Crates folder"
    );

    // prefs.js active workspace = zws-media → Media becomes the active space.
    let active = dest.active_space.lock().unwrap().clone().unwrap();
    assert_eq!(dest.space_name(&active), "Media");
}
