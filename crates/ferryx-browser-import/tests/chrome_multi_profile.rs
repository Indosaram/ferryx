//! Verifies the Chromium profile enumeration handles "Profile 1", "Profile 2",
//! etc. alongside the "Default" profile (Tier-A #2: non-Default profile selector).

#[test]
fn detector_enumerates_default_and_numbered_profiles() {
    let tmp = tempfile::tempdir().unwrap();
    let chrome_dir = tmp.path().join("Google").join("Chrome");

    for (subdir, name) in [
        ("Default", "Personal"),
        ("Profile 1", "Work"),
        ("Profile 2", "Side"),
    ] {
        let pdir = chrome_dir.join(subdir);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("Bookmarks"), b"{}").unwrap();
        std::fs::write(
            pdir.join("Preferences"),
            serde_json::json!({"profile": {"name": name}}).to_string(),
        )
        .unwrap();
    }

    let profiles: Vec<_> = std::fs::read_dir(&chrome_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n == "Default" || n.starts_with("Profile "))
        .collect();

    assert!(profiles.contains(&"Default".to_string()));
    assert!(profiles.contains(&"Profile 1".to_string()));
    assert!(profiles.contains(&"Profile 2".to_string()));
    assert_eq!(profiles.len(), 3);

    for subdir in ["Default", "Profile 1", "Profile 2"] {
        let prefs_path = chrome_dir.join(subdir).join("Preferences");
        let prefs: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&prefs_path).unwrap()).unwrap();
        let name = prefs["profile"]["name"].as_str().unwrap();
        assert!(!name.is_empty());
    }
}
