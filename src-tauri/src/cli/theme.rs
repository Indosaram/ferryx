use std::path::Path;

use serde_json::json;

use crate::theme::{
    clear_environment_theme_in, default_theme_settings_path, default_themes_dir,
    read_published_themes_in, set_environment_theme_in, BUILT_IN_THEME_IDS, THEME_GUIDE,
};

pub const THEME_USAGE: &str = "Usage: ferryx-cli theme <show [--json] | set [<id-or-file.json>] [--appearance light|dark|system] [--json] | clear [--json] | guide>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeCliCommand {
    Show {
        json: bool,
    },
    Set {
        target: Option<String>,
        appearance: Option<String>,
        json: bool,
    },
    Clear {
        json: bool,
    },
    Guide,
}

pub fn parse_theme_cli(args: &[String]) -> Result<ThemeCliCommand, String> {
    let rest = if args.len() >= 2 && args[1] == "theme" {
        &args[2..]
    } else {
        return Err(THEME_USAGE.to_string());
    };
    let Some(sub) = rest.first().map(String::as_str) else {
        return Err(THEME_USAGE.to_string());
    };
    match sub {
        "show" => {
            let mut json_out = false;
            for flag in &rest[1..] {
                match flag.as_str() {
                    "--json" => json_out = true,
                    other => return Err(format!("Unknown flag `{other}`.\n{THEME_USAGE}")),
                }
            }
            Ok(ThemeCliCommand::Show { json: json_out })
        }
        "clear" => {
            let mut json_out = false;
            for flag in &rest[1..] {
                match flag.as_str() {
                    "--json" => json_out = true,
                    other => return Err(format!("Unknown flag `{other}`.\n{THEME_USAGE}")),
                }
            }
            Ok(ThemeCliCommand::Clear { json: json_out })
        }
        "guide" => {
            if rest.len() > 1 {
                return Err(format!("Unexpected arguments to `theme guide`.\n{THEME_USAGE}"));
            }
            Ok(ThemeCliCommand::Guide)
        }
        "set" => {
            let mut target: Option<String> = None;
            let mut appearance: Option<String> = None;
            let mut json_out = false;
            let mut idx = 1;
            while idx < rest.len() {
                match rest[idx].as_str() {
                    "--json" => {
                        json_out = true;
                        idx += 1;
                    }
                    "--appearance" => {
                        let Some(val) = rest.get(idx + 1) else {
                            return Err(format!("Missing value for `--appearance`.\n{THEME_USAGE}"));
                        };
                        appearance = Some(val.clone());
                        idx += 2;
                    }
                    arg if arg.starts_with("--") => {
                        return Err(format!("Unknown flag `{arg}`.\n{THEME_USAGE}"));
                    }
                    arg => {
                        if target.is_some() {
                            return Err(format!("Unexpected extra argument `{arg}`.\n{THEME_USAGE}"));
                        }
                        target = Some(arg.to_string());
                        idx += 1;
                    }
                }
            }
            if target.is_none() && appearance.is_none() {
                return Err(format!(
                    "Provide a theme id/file path, `--appearance`, or both.\n{THEME_USAGE}"
                ));
            }
            Ok(ThemeCliCommand::Set {
                target,
                appearance,
                json: json_out,
            })
        }
        other => Err(format!("Unknown theme subcommand `{other}`.\n{THEME_USAGE}")),
    }
}

pub fn execute_theme_cli_in(
    cmd: ThemeCliCommand,
    themes_dir: &Path,
    settings_path: &Path,
) -> Result<String, String> {
    match cmd {
        ThemeCliCommand::Guide => Ok(THEME_GUIDE.to_string()),
        ThemeCliCommand::Show { json: as_json } => {
            let snap = read_published_themes_in(themes_dir, settings_path);
            if as_json {
                let payload = json!({
                    "themesDir": snap.themes_dir,
                    "defaultThemeId": snap.default_theme_id,
                    "defaultAppearance": snap.default_appearance,
                    "revision": snap.revision,
                    "builtIn": BUILT_IN_THEME_IDS,
                    "published": snap.themes.iter().map(|t| json!({
                        "id": t.id,
                        "name": t.name,
                        "appearance": t.appearance,
                        "hasCss": t.has_css,
                        "hasBackground": t.has_background,
                    })).collect::<Vec<_>>(),
                });
                serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
            } else {
                let mut lines = vec![
                    format!("Themes dir: {}", snap.themes_dir),
                    format!(
                        "Default theme: {}",
                        snap.default_theme_id.as_deref().unwrap_or("(none)")
                    ),
                    format!(
                        "Default appearance: {}",
                        snap.default_appearance.as_deref().unwrap_or("(none)")
                    ),
                    format!("Built-in themes: {}", BUILT_IN_THEME_IDS.join(", ")),
                ];
                if snap.themes.is_empty() {
                    lines.push("Published themes: (none)".to_string());
                } else {
                    lines.push("Published themes:".to_string());
                    for t in &snap.themes {
                        let mut tags = vec![t.appearance.as_str()];
                        if t.has_css {
                            tags.push("css");
                        }
                        if t.has_background {
                            tags.push("background");
                        }
                        lines.push(format!("  - {} ({}) [{}]", t.id, t.name, tags.join(", ")));
                    }
                }
                Ok(lines.join("\n"))
            }
        }
        ThemeCliCommand::Set {
            target,
            appearance,
            json: as_json,
        } => {
            let snap = set_environment_theme_in(
                themes_dir,
                settings_path,
                target.as_deref(),
                appearance.as_deref(),
            )?;
            if as_json {
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "defaultThemeId": snap.default_theme_id,
                    "defaultAppearance": snap.default_appearance,
                    "revision": snap.revision,
                }))
                .map_err(|e| e.to_string())
            } else {
                Ok(format!(
                    "Theme updated (default={}, appearance={}, revision={}).",
                    snap.default_theme_id.as_deref().unwrap_or("(none)"),
                    snap.default_appearance.as_deref().unwrap_or("(unchanged)"),
                    snap.revision
                ))
            }
        }
        ThemeCliCommand::Clear { json: as_json } => {
            let snap = clear_environment_theme_in(themes_dir, settings_path)?;
            if as_json {
                serde_json::to_string_pretty(&json!({
                    "ok": true,
                    "defaultThemeId": snap.default_theme_id,
                    "defaultAppearance": snap.default_appearance,
                    "revision": snap.revision,
                }))
                .map_err(|e| e.to_string())
            } else {
                Ok(format!(
                    "Cleared environment default theme (revision={}).",
                    snap.revision
                ))
            }
        }
    }
}

pub fn run_theme_cli(args: &[String]) -> i32 {
    let cmd = match parse_theme_cli(args) {
        Ok(cmd) => cmd,
        Err(err) => {
            eprintln!("{err}");
            return 2;
        }
    };
    match execute_theme_cli_in(cmd, &default_themes_dir(), &default_theme_settings_path()) {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_all_theme_subcommands() {
        assert_eq!(
            parse_theme_cli(&argv(&["ferryx-cli", "theme", "show", "--json"])).unwrap(),
            ThemeCliCommand::Show { json: true }
        );
        assert_eq!(
            parse_theme_cli(&argv(&[
                "ferryx-cli",
                "theme",
                "set",
                "nightfall",
                "--appearance",
                "dark"
            ]))
            .unwrap(),
            ThemeCliCommand::Set {
                target: Some("nightfall".to_string()),
                appearance: Some("dark".to_string()),
                json: false,
            }
        );
        assert_eq!(
            parse_theme_cli(&argv(&["ferryx-cli", "theme", "clear"])).unwrap(),
            ThemeCliCommand::Clear { json: false }
        );
        assert_eq!(
            parse_theme_cli(&argv(&["ferryx-cli", "theme", "guide"])).unwrap(),
            ThemeCliCommand::Guide
        );
    }

    #[test]
    fn runs_set_show_clear_round_trip() {
        let dir = tempdir().expect("tempdir");
        let themes_dir = dir.path().join("themes");
        let settings_path = dir.path().join("theme-settings.json");
        let file = dir.path().join("solar.json");
        fs::write(
            &file,
            r##"{"name":"Solar","appearance":"dark","canvas":"#002b36","accent":"#268bd2"}"##,
        )
        .expect("write");

        let set_out = execute_theme_cli_in(
            ThemeCliCommand::Set {
                target: Some(file.to_string_lossy().into_owned()),
                appearance: Some("dark".to_string()),
                json: true,
            },
            &themes_dir,
            &settings_path,
        )
        .expect("set");
        assert!(set_out.contains("\"solar\""));

        let show_out = execute_theme_cli_in(
            ThemeCliCommand::Show { json: true },
            &themes_dir,
            &settings_path,
        )
        .expect("show");
        assert!(show_out.contains("\"Solar\""));

        let clear_out = execute_theme_cli_in(
            ThemeCliCommand::Clear { json: true },
            &themes_dir,
            &settings_path,
        )
        .expect("clear");
        assert!(clear_out.contains("\"defaultThemeId\": null"));
    }
}
