//! Single source for managed worktree names.
//!
//! New managed worktrees live at `<repo>/.ferryx-worktrees/<ws>/<slug>` on branch
//! `<prefix>/<ws>/<slug>`, where `<prefix>` defaults to `ferryx` and can be configured per
//! repository in `<data>/worktree-settings.json`. Everything that parses names also accepts the
//! legacy Orca layout (`.orca-worktrees`, `orca/<ws>/<slug>`) and the old SSH `wt-<slug>`
//! directory, so existing checkouts and branches keep working without being moved.
//!
//! This file depends only on `std`, `serde` and `serde_json` because the standalone SSH helper
//! includes it with `#[path]`.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Directory, under the repository root, that new managed worktrees are created in.
pub const WORKTREE_DIR: &str = ".ferryx-worktrees";
/// Directories older builds created managed worktrees in. Recognized, never created anew.
pub const LEGACY_WORKTREE_DIRS: [&str; 1] = [".orca-worktrees"];
/// Branch prefix for repositories with no managed worktrees and no configured prefix.
pub const DEFAULT_BRANCH_PREFIX: &str = "ferryx";
/// Branch prefixes older builds used, index-aligned with [`LEGACY_WORKTREE_DIRS`].
pub const LEGACY_BRANCH_PREFIXES: [&str; 1] = ["orca"];
/// The old SSH layout named a worktree directory `wt-<slug>` directly under the managed dir.
pub const LEGACY_SLUG_DIR_PREFIX: &str = "wt-";
/// File name of the backend-side worktree settings, inside the Ferryx data directory.
pub const SETTINGS_FILE_NAME: &str = "worktree-settings.json";
const MAX_PREFIX_LEN: usize = 31;

/// Every managed worktree directory name, current first.
pub fn all_worktree_dirs() -> impl Iterator<Item = &'static str> {
    std::iter::once(WORKTREE_DIR).chain(LEGACY_WORKTREE_DIRS)
}

pub fn is_worktree_dir(name: &str) -> bool {
    all_worktree_dirs().any(|dir| dir == name)
}

/// The built-in branch prefixes, current first.
pub fn builtin_branch_prefixes() -> impl Iterator<Item = &'static str> {
    std::iter::once(DEFAULT_BRANCH_PREFIX).chain(LEGACY_BRANCH_PREFIXES)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixError {
    pub reason: String,
}

impl std::fmt::Display for PrefixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for PrefixError {}

/// Accepts `^[a-z0-9][a-z0-9._-]{0,30}$` that is also a valid Git ref component.
pub fn validate_branch_prefix(prefix: &str) -> Result<(), PrefixError> {
    let bytes = prefix.as_bytes();
    let first_ok = bytes
        .first()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    let rest_ok = bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'));
    if !first_ok || !rest_ok || bytes.len() > MAX_PREFIX_LEN {
        return Err(PrefixError {
            reason: format!("Branch prefix '{prefix}' must match ^[a-z0-9][a-z0-9._-]{{0,30}}$"),
        });
    }
    if prefix.contains("..") || prefix.ends_with('.') || prefix.ends_with(".lock") {
        return Err(PrefixError {
            reason: format!("Branch prefix '{prefix}' is not a valid Git ref component"),
        });
    }
    Ok(())
}

/// Splits `[refs/heads/]<prefix>/<ws>/<slug...>` when `<prefix>` is one of `prefixes`.
pub fn parse_branch<'a>(
    branch: &str,
    prefixes: impl IntoIterator<Item = &'a str>,
) -> Option<(String, String)> {
    let short = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let mut parts = short.splitn(3, '/');
    let prefix = parts.next()?;
    let ws = parts.next()?;
    let slug = parts.next()?;
    if ws.is_empty() || slug.is_empty() {
        return None;
    }
    prefixes
        .into_iter()
        .any(|known| known == prefix)
        .then(|| (ws.to_string(), slug.to_string()))
}

/// Parses a branch with every prefix Ferryx knows about: built-in, legacy and configured.
pub fn parse_known_branch(branch: &str) -> Option<(String, String)> {
    let known = known_branch_prefixes();
    parse_branch(branch, known.iter().map(String::as_str))
}

/// Built-in prefixes plus every prefix configured in the settings file of this environment.
pub fn known_branch_prefixes() -> Vec<String> {
    let mut known: Vec<String> = builtin_branch_prefixes().map(str::to_string).collect();
    for prefix in configured_prefixes(settings_path().as_deref()) {
        if !known.contains(&prefix) {
            known.push(prefix);
        }
    }
    known
}

/// Where new worktrees of one repository go and which branch prefix they use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeLayout {
    pub branch_prefix: String,
    pub worktree_dir: &'static str,
}

/// Resolves the layout for `repo_root`.
///
/// A configured prefix wins. Without one, a repository that already has a legacy managed dir
/// keeps the legacy prefix and dir, so one repository never ends up with mixed layouts unless
/// the user changes the setting. Everything else gets the Ferryx names.
pub fn layout_for_repo(settings: Option<&Path>, repo_root: &Path) -> WorktreeLayout {
    let configured = configured_prefix(settings, repo_root);
    let legacy = LEGACY_WORKTREE_DIRS
        .into_iter()
        .zip(LEGACY_BRANCH_PREFIXES)
        .find(|(dir, _)| repo_root.join(dir).is_dir());
    match (configured, legacy) {
        (Some(prefix), Some((dir, legacy_prefix))) if prefix == legacy_prefix => WorktreeLayout {
            branch_prefix: prefix,
            worktree_dir: dir,
        },
        (Some(prefix), _) => WorktreeLayout {
            branch_prefix: prefix,
            worktree_dir: WORKTREE_DIR,
        },
        (None, Some((dir, legacy_prefix))) => WorktreeLayout {
            branch_prefix: legacy_prefix.to_string(),
            worktree_dir: dir,
        },
        (None, None) => WorktreeLayout {
            branch_prefix: DEFAULT_BRANCH_PREFIX.to_string(),
            worktree_dir: WORKTREE_DIR,
        },
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSettings {
    #[serde(default)]
    pub version: u32,
    /// Keyed by the canonical repository root.
    #[serde(default)]
    pub repos: BTreeMap<String, RepoWorktreeSettings>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoWorktreeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_prefix: Option<String>,
}

/// The Ferryx data directory: `FERRYX_DATA_DIR`, else the per-user platform location.
pub fn data_dir_with_env(lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(dir) = lookup("FERRYX_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    {
        lookup("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("Ferryx"))
            .or_else(|| lookup("USERPROFILE").map(|base| PathBuf::from(base).join(".ferryx")))
    }
    #[cfg(not(windows))]
    {
        lookup("HOME")
            .or_else(|| lookup("USERPROFILE"))
            .map(|base| PathBuf::from(base).join(".ferryx"))
    }
}

/// `<data>/worktree-settings.json` for the current environment.
pub fn settings_path() -> Option<PathBuf> {
    data_dir_with_env(|key| std::env::var_os(key)).map(|dir| dir.join(SETTINGS_FILE_NAME))
}

/// Reads the settings; a missing or malformed file reads as empty settings.
pub fn load_settings(path: &Path) -> WorktreeSettings {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn repo_key(repo_root: &Path) -> String {
    repo_root.to_string_lossy().into_owned()
}

/// The valid prefix configured for `repo_root`, if any.
pub fn configured_prefix(settings: Option<&Path>, repo_root: &Path) -> Option<String> {
    let settings = load_settings(settings?);
    settings
        .repos
        .get(&repo_key(repo_root))
        .and_then(|repo| repo.branch_prefix.clone())
        .filter(|prefix| validate_branch_prefix(prefix).is_ok())
}

/// Stores (or clears, with `None`) the prefix for `repo_root`. Validate before calling.
pub fn write_configured_prefix(
    path: &Path,
    repo_root: &Path,
    prefix: Option<&str>,
) -> std::io::Result<()> {
    let mut settings = load_settings(path);
    settings.version = 1;
    let key = repo_key(repo_root);
    match prefix {
        Some(prefix) => {
            settings.repos.insert(
                key,
                RepoWorktreeSettings {
                    branch_prefix: Some(prefix.to_string()),
                },
            );
        }
        None => {
            settings.repos.remove(&key);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)?;
    // In-process writes invalidate directly; the mtime check covers writes by other processes.
    *PREFIX_CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = None;
    Ok(())
}

type PrefixCache = Option<(PathBuf, Option<SystemTime>, Vec<String>)>;
static PREFIX_CACHE: Mutex<PrefixCache> = Mutex::new(None);

/// Every valid prefix configured for any repository, cached by file modification time.
pub fn configured_prefixes(settings: Option<&Path>) -> Vec<String> {
    let Some(path) = settings else {
        return Vec::new();
    };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut cache = PREFIX_CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some((cached_path, cached_modified, prefixes)) = cache.as_ref() {
        if cached_path == path && *cached_modified == modified && modified.is_some() {
            return prefixes.clone();
        }
    }
    let mut prefixes: Vec<String> = load_settings(path)
        .repos
        .into_values()
        .filter_map(|repo| repo.branch_prefix)
        .filter(|prefix| validate_branch_prefix(prefix).is_ok())
        .collect();
    prefixes.sort();
    prefixes.dedup();
    *cache = Some((path.to_path_buf(), modified, prefixes.clone()));
    prefixes
}

/// `<dir>/<ws>/<slug>` with `/` separators.
pub fn worktree_rel_path(dir: &str, ws: &str, slug: &str) -> String {
    format!("{dir}/{ws}/{slug}")
}

/// The root-anchored `info/exclude` pattern for one directory under the repository root.
pub fn exclude_pattern(rel_dir: &str) -> String {
    format!("/{}/", rel_dir.trim_matches('/'))
}

/// Exclude patterns for a new managed worktree under `current_dir`: that dir, plus every legacy
/// managed dir already present in `repo_root`.
pub fn managed_exclude_patterns(repo_root: &Path, current_dir: &str) -> Vec<String> {
    let mut patterns = vec![exclude_pattern(current_dir)];
    for dir in all_worktree_dirs() {
        if dir != current_dir && repo_root.join(dir).is_dir() {
            patterns.push(exclude_pattern(dir));
        }
    }
    patterns
}

/// `<common_dir>/info/exclude`, resolving a relative `git rev-parse --git-common-dir` result
/// against the directory git ran in.
pub fn exclude_file(git_cwd: &Path, common_dir_output: &str) -> PathBuf {
    let common = PathBuf::from(common_dir_output.trim());
    let common = if common.is_absolute() {
        common
    } else {
        git_cwd.join(common)
    };
    common.join("info").join("exclude")
}

/// Appends each pattern missing from the repository-local `info/exclude` file and returns how
/// many were added. Running it again adds nothing. Only `info/exclude` is written; tracked
/// files such as `.gitignore` are never touched.
pub fn ensure_excluded(exclude_file: &Path, patterns: &[String]) -> std::io::Result<usize> {
    let existing = match std::fs::read_to_string(exclude_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let present = |pattern: &str| {
        let bare = pattern.trim_matches('/');
        existing.lines().map(str::trim).any(|line| {
            line == pattern
                || line == bare
                || line.strip_prefix('/') == Some(bare)
                || line.strip_suffix('/') == Some(bare)
        })
    };
    let mut addition = String::new();
    for pattern in patterns {
        if !present(pattern) && !addition.lines().any(|line| line == pattern) {
            addition.push_str(pattern);
            addition.push('\n');
        }
    }
    if addition.is_empty() {
        return Ok(0);
    }
    if !existing.is_empty() && !existing.ends_with('\n') {
        addition.insert(0, '\n');
    }
    if let Some(parent) = exclude_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(exclude_file)?;
    file.write_all(addition.as_bytes())?;
    Ok(addition.lines().filter(|line| !line.is_empty()).count())
}

/// The text before the first managed worktree dir component of `path` (either separator).
pub fn split_repo_root(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    let mut best: Option<usize> = None;
    for dir in all_worktree_dirs() {
        let mut from = 0;
        while let Some(found) = path[from..].find(dir) {
            let index = from + found;
            let end = index + dir.len();
            let before_ok = index > 0 && matches!(bytes[index - 1], b'/' | b'\\');
            let after_ok = end == bytes.len() || matches!(bytes[end], b'/' | b'\\');
            if before_ok && after_ok {
                let root_end = index - 1;
                best = Some(best.map_or(root_end, |b| b.min(root_end)));
                break;
            }
            from = index + 1;
        }
    }
    best.map(|end| &path[..end])
}

fn components(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).collect()
}

fn subpath(rest: &[&str]) -> Option<String> {
    let sub: Vec<&str> = rest.iter().copied().filter(|s| !s.is_empty()).collect();
    (!sub.is_empty()).then(|| sub.join("/"))
}

/// When `path` lies inside the managed worktree for `slug` (`<dir>/<ws>/<slug>` in any managed
/// dir, or the legacy `<dir>/wt-<slug>`), returns the subdirectory below the worktree root
/// (`None` for the root itself). Returns `None` when `path` is not inside such a worktree.
pub fn managed_worktree_subdir(path: &str, slug: &str) -> Option<Option<String>> {
    let comps = components(path);
    let slug_parts: Vec<&str> = slug.split('/').collect();
    let legacy = format!("{LEGACY_SLUG_DIR_PREFIX}{slug}");
    let legacy_parts: Vec<&str> = legacy.split('/').collect();
    for (index, comp) in comps.iter().enumerate() {
        if !is_worktree_dir(comp) {
            continue;
        }
        let rest = &comps[index + 1..];
        if rest.len() >= legacy_parts.len() && rest[..legacy_parts.len()] == legacy_parts[..] {
            return Some(subpath(&rest[legacy_parts.len()..]));
        }
        let n = slug_parts.len();
        if rest.len() > n && !rest[0].is_empty() && rest[1..=n] == slug_parts[..] {
            return Some(subpath(&rest[n + 1..]));
        }
    }
    None
}

/// The first slug segment of a path inside a managed worktree, from either layout.
pub fn infer_slug(path: &str) -> Option<String> {
    let comps = components(path);
    for (index, comp) in comps.iter().enumerate() {
        if !is_worktree_dir(comp) {
            continue;
        }
        let rest = &comps[index + 1..];
        if let Some(slug) = rest
            .first()
            .and_then(|first| first.strip_prefix(LEGACY_SLUG_DIR_PREFIX))
            .filter(|slug| !slug.is_empty())
        {
            return Some(slug.to_string());
        }
        if rest.len() >= 2 && !rest[0].is_empty() && !rest[1].is_empty() {
            return Some(rest[1].to_string());
        }
    }
    None
}

/// The full slug of a managed worktree root path, from either layout.
pub fn slug_from_worktree_path(path: &str) -> Option<String> {
    let comps = components(path);
    let index = comps.iter().rposition(|comp| is_worktree_dir(comp))?;
    let rest: Vec<&str> = comps[index + 1..]
        .iter()
        .copied()
        .filter(|s| !s.is_empty())
        .collect();
    match rest.as_slice() {
        [single] => single
            .strip_prefix(LEGACY_SLUG_DIR_PREFIX)
            .filter(|slug| !slug.is_empty())
            .map(str::to_string),
        [_ws, slug @ ..] if !slug.is_empty() => Some(slug.join("/")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_validation_follows_the_documented_pattern() {
        for ok in ["ferryx", "orca", "a", "team-1", "x.y_z", "0abc"] {
            assert!(validate_branch_prefix(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "Bad Prefix!",
            "-x",
            ".x",
            "_x",
            "UPPER",
            "a/b",
            "a..b",
            "a.",
            "a.lock",
            "abcdefghijklmnopqrstuvwxyzabcdef",
        ] {
            assert!(validate_branch_prefix(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn branch_parsing_accepts_current_and_legacy_prefixes() {
        let known = || builtin_branch_prefixes();
        assert_eq!(
            parse_branch("refs/heads/ferryx/ws/a/b", known()),
            Some(("ws".into(), "a/b".into()))
        );
        assert_eq!(
            parse_branch("orca/ws/a", known()),
            Some(("ws".into(), "a".into()))
        );
        assert_eq!(parse_branch("feature/ws/a", known()), None);
        assert_eq!(parse_branch("ferryx/ws", known()), None);
    }

    #[test]
    fn path_helpers_recognize_every_layout() {
        assert_eq!(
            split_repo_root("/srv/repo/.ferryx-worktrees/ws/a"),
            Some("/srv/repo")
        );
        assert_eq!(
            split_repo_root("/srv/repo/.orca-worktrees/wt-a"),
            Some("/srv/repo")
        );
        assert_eq!(
            split_repo_root(r"C:\repo\.orca-worktrees\wt-a\sub"),
            Some(r"C:\repo")
        );
        assert_eq!(split_repo_root("/srv/repo/x.ferryx-worktrees/a"), None);
        assert_eq!(split_repo_root("/srv/repo"), None);

        assert_eq!(
            managed_worktree_subdir("/r/.ferryx-worktrees/ws/feat/src/lib", "feat"),
            Some(Some("src/lib".into()))
        );
        assert_eq!(
            managed_worktree_subdir(".ferryx-worktrees/ws/feat", "feat"),
            Some(None)
        );
        assert_eq!(
            managed_worktree_subdir(r"C:\r\.orca-worktrees\wt-feat\sub", "feat"),
            Some(Some("sub".into()))
        );
        assert_eq!(
            managed_worktree_subdir("/r/.orca-worktrees/ws/feat", "feat"),
            Some(None)
        );
        assert_eq!(
            managed_worktree_subdir("/r/.ferryx-worktrees/ws/other", "feat"),
            None
        );

        assert_eq!(
            infer_slug("/r/.ferryx-worktrees/ws/feat/sub").as_deref(),
            Some("feat")
        );
        assert_eq!(
            infer_slug("/r/.orca-worktrees/wt-feat/sub").as_deref(),
            Some("feat")
        );
        assert_eq!(infer_slug("/r/src").as_deref(), None);

        assert_eq!(
            slug_from_worktree_path("/r/.ferryx-worktrees/ssh-ab/a/b").as_deref(),
            Some("a/b")
        );
        assert_eq!(
            slug_from_worktree_path(r"C:\r\.orca-worktrees\wt-feat").as_deref(),
            Some("feat")
        );
        assert_eq!(slug_from_worktree_path("/r/plain").as_deref(), None);
    }
}
