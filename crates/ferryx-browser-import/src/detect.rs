//! Installed browser detector (macOS Phase 1).
//!
//! Ports `maho-chromium/browser/importer/maho_browser_detector.cc` to Rust.
//! Enumerates Chromium-based, Gecko-based, and Safari browser installations.

use std::path::{Path, PathBuf};

use crate::{BrowserType, DetectedBrowser, ImportServices};

struct ChromiumBrowserInfo {
    browser_type: BrowserType,
    display_name: &'static str,
    has_profiles: bool,
    /// Arc's spaces live in one global `StorableSidebar.json` shared across all
    /// Chromium profiles, so per-profile entries import duplicate spaces behind
    /// an opaque profile picker. When true, collapse to a single representative
    /// profile and expose one plain-named entry.
    collapse_profiles: bool,
}

const CHROMIUM_BROWSERS: &[ChromiumBrowserInfo] = &[
    ChromiumBrowserInfo {
        browser_type: BrowserType::Chrome,
        display_name: "Google Chrome",
        has_profiles: true,
        collapse_profiles: false,
    },
    ChromiumBrowserInfo {
        browser_type: BrowserType::Arc,
        display_name: "Arc",
        has_profiles: true,
        collapse_profiles: true,
    },
    ChromiumBrowserInfo {
        browser_type: BrowserType::Brave,
        display_name: "Brave",
        has_profiles: true,
        collapse_profiles: false,
    },
    ChromiumBrowserInfo {
        browser_type: BrowserType::Edge,
        display_name: "Microsoft Edge",
        has_profiles: true,
        collapse_profiles: false,
    },
    ChromiumBrowserInfo {
        browser_type: BrowserType::Vivaldi,
        display_name: "Vivaldi",
        has_profiles: true,
        collapse_profiles: false,
    },
    ChromiumBrowserInfo {
        browser_type: BrowserType::Opera,
        display_name: "Opera",
        has_profiles: false,
        collapse_profiles: false,
    },
];

struct GeckoBrowserInfo {
    browser_type: BrowserType,
    display_name: &'static str,
}

const GECKO_BROWSERS: &[GeckoBrowserInfo] = &[
    GeckoBrowserInfo {
        browser_type: BrowserType::Firefox,
        display_name: "Firefox",
    },
    GeckoBrowserInfo {
        browser_type: BrowserType::Zen,
        display_name: "Zen",
    },
];

pub fn detect_installed_browsers() -> Vec<DetectedBrowser> {
    let mut results = Vec::new();

    if let Some(chromium_base) = chromium_base_dir() {
        detect_chromium_browsers(&chromium_base, &mut results);
    }
    if let Some(gecko_base) = gecko_base_dir() {
        detect_gecko_browsers(&gecko_base, &mut results);
    }
    #[cfg(target_os = "macos")]
    detect_safari(&mut results);

    results
}

fn chromium_base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
    }
    #[cfg(target_os = "linux")]
    {
        dirs::config_dir()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        dirs::data_dir()
    }
}

fn gecko_base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir()
    }
    #[cfg(not(target_os = "linux"))]
    {
        dirs::data_dir()
    }
}

fn chromium_subdir(info: &ChromiumBrowserInfo) -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    {
        Some(match info.browser_type {
            BrowserType::Chrome => "Google/Chrome/User Data",
            BrowserType::Arc => "Arc/User Data",
            BrowserType::Brave => "BraveSoftware/Brave-Browser/User Data",
            BrowserType::Edge => "Microsoft/Edge/User Data",
            BrowserType::Vivaldi => "Vivaldi/User Data",
            BrowserType::Opera => "Opera Software/Opera Stable",
            _ => return None,
        })
    }
    #[cfg(target_os = "linux")]
    {
        Some(match info.browser_type {
            BrowserType::Chrome => "google-chrome",
            BrowserType::Brave => "BraveSoftware/Brave-Browser",
            BrowserType::Edge => "microsoft-edge",
            BrowserType::Vivaldi => "vivaldi",
            BrowserType::Opera => "opera",
            _ => return None,
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Some(match info.browser_type {
            BrowserType::Chrome => "Google/Chrome",
            BrowserType::Arc => "Arc/User Data",
            BrowserType::Brave => "BraveSoftware/Brave-Browser",
            BrowserType::Edge => "Microsoft Edge",
            BrowserType::Vivaldi => "Vivaldi",
            BrowserType::Opera => "com.operasoftware.Opera",
            _ => return None,
        })
    }
}

fn gecko_subdir(info: &GeckoBrowserInfo) -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    {
        Some(match info.browser_type {
            BrowserType::Firefox => "Mozilla/Firefox",
            BrowserType::Zen => "zen",
            _ => return None,
        })
    }
    #[cfg(target_os = "linux")]
    {
        Some(match info.browser_type {
            BrowserType::Firefox => ".mozilla/firefox",
            BrowserType::Zen => ".zen",
            _ => return None,
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Some(match info.browser_type {
            BrowserType::Firefox => "Firefox",
            BrowserType::Zen => "zen",
            _ => return None,
        })
    }
}

fn detect_chromium_browsers(app_support: &Path, results: &mut Vec<DetectedBrowser>) {
    for info in CHROMIUM_BROWSERS {
        let Some(subdir) = chromium_subdir(info) else {
            continue;
        };
        let browser_dir = app_support.join(subdir);

        let profile_dirs: Vec<PathBuf> = if info.has_profiles {
            let mut dirs = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&browser_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if (name == "Default" || name.starts_with("Profile "))
                        && entry.path().is_dir()
                    {
                        dirs.push(entry.path());
                    }
                }
            }
            if dirs.is_empty() {
                dirs.push(browser_dir.join("Default"));
            }
            dirs
        } else {
            vec![browser_dir.clone()]
        };

        let installed_dirs: Vec<PathBuf> = profile_dirs
            .into_iter()
            .filter(|dir| {
                dir.join("Bookmarks").exists() || dir.join("Login Data").exists() || dir.is_dir()
            })
            .collect();

        let selected_dirs: Vec<PathBuf> = if info.collapse_profiles {
            pick_representative_profile(&installed_dirs)
                .into_iter()
                .collect()
        } else {
            installed_dirs
        };

        for profile_dir in selected_dirs {
            let has_bookmarks = profile_dir.join("Bookmarks").exists();
            let has_passwords = profile_dir.join("Login Data").exists();

            let mut services =
                ImportServices::HISTORY | ImportServices::COOKIES | ImportServices::FAVICONS;
            if has_bookmarks {
                services |= ImportServices::BOOKMARKS;
            }
            if has_passwords {
                services |= ImportServices::PASSWORDS;
            }
            if profile_dir.join("Web Data").exists() {
                services |= ImportServices::AUTOFILL;
            }
            if info.browser_type == BrowserType::Arc {
                services |= ImportServices::WORKSPACES;
            }

            let display_name = if info.collapse_profiles {
                info.display_name.to_string()
            } else {
                let profile_name = read_chromium_profile_name(&profile_dir).or_else(|| {
                    let dir_name = profile_dir
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if dir_name != "Default" && info.has_profiles {
                        Some(dir_name)
                    } else {
                        None
                    }
                });
                match profile_name {
                    Some(ref name) => format!("{} - {}", info.display_name, name),
                    None => info.display_name.to_string(),
                }
            };

            results.push(DetectedBrowser {
                browser_type: info.browser_type,
                display_name,
                profile_path: profile_dir,
                services_supported: services,
                requires_full_disk_access: false,
            });
        }
    }
}

/// Representative profile for `collapse_profiles` browsers: prefer `Default`,
/// then any profile with importable data, then the first available.
fn pick_representative_profile(dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(default_dir) = dirs.iter().find(|dir| {
        dir.file_name()
            .map(|n| n.to_string_lossy() == "Default")
            .unwrap_or(false)
    }) {
        return Some(default_dir.clone());
    }
    if let Some(with_data) = dirs
        .iter()
        .find(|dir| dir.join("Bookmarks").exists() || dir.join("Login Data").exists())
    {
        return Some(with_data.clone());
    }
    dirs.first().cloned()
}

fn read_chromium_profile_name(profile_dir: &Path) -> Option<String> {
    let prefs_path = profile_dir.join("Preferences");
    let content = std::fs::read_to_string(&prefs_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let name = parsed.pointer("/profile/name").and_then(|v| v.as_str())?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn detect_gecko_browsers(app_support: &Path, results: &mut Vec<DetectedBrowser>) {
    for info in GECKO_BROWSERS {
        let Some(subdir) = gecko_subdir(info) else {
            continue;
        };
        let browser_dir = app_support.join(subdir);
        let profiles_ini = browser_dir.join("profiles.ini");

        if !profiles_ini.exists() {
            continue;
        }

        let profile_dir = match resolve_default_gecko_profile(&browser_dir) {
            Some(p) => p,
            None => continue,
        };

        let places_db = profile_dir.join("places.sqlite");
        if !places_db.exists() {
            continue;
        }

        let mut services = ImportServices::BOOKMARKS
            | ImportServices::HISTORY
            | ImportServices::COOKIES
            | ImportServices::FAVICONS;

        if profile_dir.join("logins.json").exists() {
            services |= ImportServices::PASSWORDS;
        }
        if info.browser_type == BrowserType::Zen {
            services |= ImportServices::WORKSPACES;
        }

        results.push(DetectedBrowser {
            browser_type: info.browser_type,
            display_name: info.display_name.to_string(),
            profile_path: profile_dir,
            services_supported: services,
            requires_full_disk_access: false,
        });
    }
}

fn resolve_default_gecko_profile(browser_data_dir: &Path) -> Option<PathBuf> {
    let profiles_ini = browser_data_dir.join("profiles.ini");
    let contents = std::fs::read_to_string(&profiles_ini).ok()?;

    #[derive(PartialEq)]
    enum SectionKind {
        None,
        Install,
        Profile,
    }

    let mut section_kind = SectionKind::None;
    let mut install_default_paths: Vec<PathBuf> = Vec::new();
    let mut profile_default_paths: Vec<PathBuf> = Vec::new();
    let mut all_profile_paths: Vec<PathBuf> = Vec::new();

    let mut section_path = String::new();
    let mut section_is_relative = true;
    let mut section_marked_default = false;

    let resolve_path = |raw: &str, is_relative: bool| -> PathBuf {
        if is_relative {
            browser_data_dir.join(raw)
        } else {
            PathBuf::from(raw)
        }
    };

    let lines: Vec<&str> = contents.lines().collect();

    let flush_profile = |kind: &SectionKind,
                         path: &str,
                         is_relative: bool,
                         marked_default: bool,
                         all: &mut Vec<PathBuf>,
                         defaults: &mut Vec<PathBuf>| {
        if *kind != SectionKind::Profile || path.is_empty() {
            return;
        }
        let p = resolve_path(path, is_relative);
        all.push(p.clone());
        if marked_default {
            defaults.push(p);
        }
    };

    for line in &lines {
        let line = line.trim();
        if line.starts_with('[') {
            flush_profile(
                &section_kind,
                &section_path,
                section_is_relative,
                section_marked_default,
                &mut all_profile_paths,
                &mut profile_default_paths,
            );

            if line.starts_with("[Install") {
                section_kind = SectionKind::Install;
            } else if line.starts_with("[Profile") {
                section_kind = SectionKind::Profile;
            } else {
                section_kind = SectionKind::None;
            }
            section_path.clear();
            section_is_relative = true;
            section_marked_default = false;
            continue;
        }

        if section_kind == SectionKind::Install {
            if let Some(val) = line.strip_prefix("Default=") {
                if !val.is_empty() {
                    install_default_paths.push(browser_data_dir.join(val));
                }
            }
        } else if section_kind == SectionKind::Profile {
            if let Some(val) = line.strip_prefix("Path=") {
                section_path = val.to_string();
            } else if line == "IsRelative=0" {
                section_is_relative = false;
            } else if line == "Default=1" {
                section_marked_default = true;
            }
        }
    }

    flush_profile(
        &section_kind,
        &section_path,
        section_is_relative,
        section_marked_default,
        &mut all_profile_paths,
        &mut profile_default_paths,
    );

    let pick_first_with_places = |paths: &[PathBuf]| -> Option<PathBuf> {
        paths
            .iter()
            .find(|p| p.join("places.sqlite").exists())
            .cloned()
    };

    pick_first_with_places(&install_default_paths)
        .or_else(|| pick_first_with_places(&profile_default_paths))
        .or_else(|| pick_first_with_places(&all_profile_paths))
}

#[cfg(target_os = "macos")]
fn detect_safari(results: &mut Vec<DetectedBrowser>) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };
    let safari_bookmarks = home.join("Library/Safari/Bookmarks.plist");
    if safari_bookmarks.exists() {
        results.push(DetectedBrowser {
            browser_type: BrowserType::Safari,
            display_name: "Safari".to_string(),
            profile_path: safari_bookmarks,
            services_supported: ImportServices::BOOKMARKS
                | ImportServices::HISTORY
                | ImportServices::COOKIES
                | ImportServices::PASSWORDS
                | ImportServices::FAVICONS,
            requires_full_disk_access: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_browsers_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let results = detect_chromium_browsers_in(tmp.path());
        assert!(results.is_empty());
    }

    fn detect_chromium_browsers_in(app_support: &Path) -> Vec<DetectedBrowser> {
        let mut results = Vec::new();
        detect_chromium_browsers(app_support, &mut results);
        results
    }

    fn detect_gecko_browsers_in(app_support: &Path) -> Vec<DetectedBrowser> {
        let mut results = Vec::new();
        detect_gecko_browsers(app_support, &mut results);
        results
    }

    #[test]
    fn detects_chrome_with_default_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let chrome_dir = tmp.path().join("Google/Chrome/Default");
        std::fs::create_dir_all(&chrome_dir).unwrap();
        std::fs::write(chrome_dir.join("Bookmarks"), "{}").unwrap();

        let results = detect_chromium_browsers_in(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].browser_type, BrowserType::Chrome);
        assert_eq!(results[0].display_name, "Google Chrome");
        assert!(results[0].services_supported & ImportServices::BOOKMARKS != 0);
        assert!(!results[0].requires_full_disk_access);
    }

    #[test]
    fn detects_chrome_with_custom_profile_name() {
        let tmp = tempfile::tempdir().unwrap();
        let profile_dir = tmp.path().join("Google/Chrome/Profile 1");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("Bookmarks"), "{}").unwrap();

        let prefs = serde_json::json!({"profile": {"name": "Personal"}});
        std::fs::write(
            profile_dir.join("Preferences"),
            serde_json::to_string(&prefs).unwrap(),
        )
        .unwrap();

        let results = detect_chromium_browsers_in(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].display_name, "Google Chrome - Personal");
    }

    #[test]
    fn arc_collapses_multiple_profiles_to_single_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("Arc/User Data");
        for (dir, name) in [
            ("Default", "ts"),
            ("Profile 1", "Your Chromium"),
            ("Profile 2", "asd"),
        ] {
            let p = base.join(dir);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("Bookmarks"), "{}").unwrap();
            let prefs = serde_json::json!({"profile": {"name": name}});
            std::fs::write(
                p.join("Preferences"),
                serde_json::to_string(&prefs).unwrap(),
            )
            .unwrap();
        }

        let results = detect_chromium_browsers_in(tmp.path());
        let arc: Vec<_> = results
            .iter()
            .filter(|b| b.browser_type == BrowserType::Arc)
            .collect();
        assert_eq!(arc.len(), 1, "Arc must collapse to a single entry");
        assert_eq!(arc[0].display_name, "Arc", "no profile-name suffix");
        assert!(arc[0].services_supported & ImportServices::WORKSPACES != 0);
        assert!(
            arc[0].profile_path.ends_with("Default"),
            "Default is the preferred representative profile"
        );
    }

    #[test]
    fn arc_single_default_profile_has_plain_name() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Arc/User Data/Default");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("Bookmarks"), "{}").unwrap();

        let results = detect_chromium_browsers_in(tmp.path());
        let arc: Vec<_> = results
            .iter()
            .filter(|b| b.browser_type == BrowserType::Arc)
            .collect();
        assert_eq!(arc.len(), 1);
        assert_eq!(arc[0].display_name, "Arc");
    }

    #[test]
    fn arc_without_default_picks_profile_with_data() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("Arc/User Data/Profile 1");
        std::fs::create_dir_all(&p1).unwrap();
        std::fs::write(p1.join("Bookmarks"), "{}").unwrap();

        let results = detect_chromium_browsers_in(tmp.path());
        let arc: Vec<_> = results
            .iter()
            .filter(|b| b.browser_type == BrowserType::Arc)
            .collect();
        assert_eq!(arc.len(), 1);
        assert_eq!(arc[0].display_name, "Arc");
        assert!(arc[0].profile_path.ends_with("Profile 1"));
    }

    #[test]
    fn arc_without_default_or_data_picks_first_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("Arc/User Data");
        std::fs::create_dir_all(base.join("Profile 1")).unwrap();
        std::fs::create_dir_all(base.join("Profile 2")).unwrap();

        let results = detect_chromium_browsers_in(tmp.path());
        let arc: Vec<_> = results
            .iter()
            .filter(|b| b.browser_type == BrowserType::Arc)
            .collect();
        assert_eq!(
            arc.len(),
            1,
            "empty Arc profiles still collapse to one entry"
        );
        assert_eq!(arc[0].display_name, "Arc");
        assert!(arc[0].services_supported & ImportServices::WORKSPACES != 0);
    }

    #[test]
    fn detects_firefox_with_profiles_ini() {
        let tmp = tempfile::tempdir().unwrap();
        let ff_dir = tmp.path().join("Firefox");
        let profile_dir = ff_dir.join("Profiles/abc123.default");
        std::fs::create_dir_all(&profile_dir).unwrap();

        let ini = "[Profile0]\nPath=Profiles/abc123.default\nIsRelative=1\nDefault=1\n";
        std::fs::write(ff_dir.join("profiles.ini"), ini).unwrap();
        std::fs::write(profile_dir.join("places.sqlite"), "fake").unwrap();

        let results = detect_gecko_browsers_in(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].browser_type, BrowserType::Firefox);
        assert_eq!(results[0].display_name, "Firefox");
        assert!(results[0].services_supported & ImportServices::BOOKMARKS != 0);
        assert!(results[0].services_supported & ImportServices::HISTORY != 0);
    }

    #[test]
    fn detects_zen_with_workspaces_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let zen_dir = tmp.path().join("zen");
        let profile_dir = zen_dir.join("abc.default");
        std::fs::create_dir_all(&profile_dir).unwrap();

        let ini =
            "[Install123]\nDefault=abc.default\n\n[Profile0]\nPath=abc.default\nIsRelative=1\n";
        std::fs::write(zen_dir.join("profiles.ini"), ini).unwrap();
        std::fs::write(profile_dir.join("places.sqlite"), "fake").unwrap();

        let results = detect_gecko_browsers_in(tmp.path());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].browser_type, BrowserType::Zen);
        assert!(results[0].services_supported & ImportServices::WORKSPACES != 0);
    }

    #[test]
    fn gecko_without_places_sqlite_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let ff_dir = tmp.path().join("Firefox");
        let profile_dir = ff_dir.join("Profiles/abc.default");
        std::fs::create_dir_all(&profile_dir).unwrap();

        let ini = "[Profile0]\nPath=Profiles/abc.default\nIsRelative=1\nDefault=1\n";
        std::fs::write(ff_dir.join("profiles.ini"), ini).unwrap();
        // No places.sqlite

        let results = detect_gecko_browsers_in(tmp.path());
        assert!(results.is_empty());
    }

    #[test]
    fn resolve_gecko_profile_install_section_preferred() {
        let tmp = tempfile::tempdir().unwrap();
        let profile_a = tmp.path().join("Profiles/a.default");
        let profile_b = tmp.path().join("Profiles/b.release");
        std::fs::create_dir_all(&profile_a).unwrap();
        std::fs::create_dir_all(&profile_b).unwrap();
        std::fs::write(profile_a.join("places.sqlite"), "db").unwrap();
        std::fs::write(profile_b.join("places.sqlite"), "db").unwrap();

        let ini = "[Install123]\nDefault=Profiles/b.release\n\n[Profile0]\nPath=Profiles/a.default\nIsRelative=1\nDefault=1\n[Profile1]\nPath=Profiles/b.release\nIsRelative=1\n";
        std::fs::write(tmp.path().join("profiles.ini"), ini).unwrap();

        let resolved = resolve_default_gecko_profile(tmp.path()).unwrap();
        assert_eq!(resolved, profile_b);
    }
}
