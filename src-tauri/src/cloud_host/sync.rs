use std::path::{Path, PathBuf};
use std::process::Command;

pub const MAX_SINGLE_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
pub const REMOTE_WORKSPACES_REL: &str = ".ferryx-cloud/workspaces";

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".next",
    ".venv",
    "__pycache__",
    ".ferryx-worktrees",
    ".orca-worktrees",
];

#[derive(Debug, Clone)]
pub struct PackedWorkspace {
    pub archive_bytes: Vec<u8>,
    pub file_count: usize,
    pub skipped_large: usize,
}

pub fn sanitize_task_id(raw: &str) -> Result<String, String> {
    let clean: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    let trimmed = clean.trim_matches('.');
    if trimmed.is_empty() || trimmed.len() > 96 {
        return Err(format!("invalid task id `{raw}`"));
    }
    Ok(trimmed.to_string())
}

pub fn pack_local_workspace(local_cwd: &Path) -> Result<PackedWorkspace, String> {
    if !local_cwd.is_dir() {
        return Err(format!("local workspace does not exist: {}", local_cwd.display()));
    }
    let candidates = collect_workspace_files(local_cwd)?;
    let mut included: Vec<String> = Vec::new();
    let mut skipped_large = 0usize;

    for rel in candidates {
        if has_skipped_segment(&rel) {
            continue;
        }
        let full = local_cwd.join(&rel);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > MAX_SINGLE_FILE_BYTES {
            skipped_large += 1;
            continue;
        }
        included.push(rel);
    }

    let tmp_dir = tempfile::tempdir()
        .map_err(|error| format!("cannot create temp dir for workspace sync: {error}"))?;
    let archive_path = tmp_dir.path().join("workspace.tar.gz");

    if included.is_empty() {
        let empty_dir = tmp_dir.path().join("empty");
        std::fs::create_dir_all(&empty_dir)
            .map_err(|error| format!("cannot create empty sync dir: {error}"))?;
        let status = Command::new("tar")
            .args(["-C"])
            .arg(&empty_dir)
            .args(["-czf"])
            .arg(&archive_path)
            .arg(".")
            .status()
            .map_err(|error| format!("failed to run tar: {error}"))?;
        if !status.success() {
            return Err("tar failed creating empty workspace archive".to_string());
        }
    } else {
        let list_path = tmp_dir.path().join("files.nul");
        let mut list_bytes = Vec::new();
        for rel in &included {
            list_bytes.extend_from_slice(rel.as_bytes());
            list_bytes.push(0);
        }
        std::fs::write(&list_path, &list_bytes)
            .map_err(|error| format!("cannot write tar file list: {error}"))?;

        let output = Command::new("tar")
            .arg("-C")
            .arg(local_cwd)
            .arg("--null")
            .arg("-T")
            .arg(&list_path)
            .arg("-czf")
            .arg(&archive_path)
            .output()
            .map_err(|error| format!("failed to run tar: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("tar failed: {}", stderr.trim()));
        }
    }

    let archive_bytes = std::fs::read(&archive_path)
        .map_err(|error| format!("cannot read workspace archive: {error}"))?;
    if archive_bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "workspace archive is {} bytes, exceeding the {} byte sync limit",
            archive_bytes.len(),
            MAX_ARCHIVE_BYTES
        ));
    }

    Ok(PackedWorkspace {
        archive_bytes,
        file_count: included.len(),
        skipped_large,
    })
}

fn collect_workspace_files(local_cwd: &Path) -> Result<Vec<String>, String> {
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(local_cwd)
        .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        .output()
    {
        if output.status.success() {
            let mut files = Vec::new();
            for chunk in output.stdout.split(|b| *b == 0) {
                if chunk.is_empty() {
                    continue;
                }
                if let Ok(rel) = std::str::from_utf8(chunk) {
                    files.push(rel.to_string());
                }
            }
            return Ok(files);
        }
    }
    let mut files = Vec::new();
    walk_dir_relative(local_cwd, local_cwd, &mut files)?;
    Ok(files)
}

fn walk_dir_relative(root: &Path, current: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(current)
        .map_err(|error| format!("cannot read {}: {error}", current.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_dir_relative(root, &path, out)?;
        } else if meta.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                if let Some(s) = rel.to_str() {
                    out.push(s.to_string());
                }
            }
        }
    }
    Ok(())
}

fn has_skipped_segment(rel: &str) -> bool {
    Path::new(rel).components().any(|comp| {
        comp.as_os_str()
            .to_str()
            .is_some_and(|seg| SKIP_DIRS.contains(&seg))
    })
}

pub fn remote_workspace_dir(remote_home: &str, task_id: &str) -> String {
    format!("{}/{REMOTE_WORKSPACES_REL}/{task_id}", remote_home.trim_end_matches('/'))
}

pub fn remote_archive_rel(task_id: &str) -> String {
    format!("{REMOTE_WORKSPACES_REL}/{task_id}.tar.gz")
}

pub fn build_remote_unpack_script(remote_home: &str, task_id: &str) -> String {
    let ws = crate::ssh::direct::quote_posix(&remote_workspace_dir(remote_home, task_id));
    let tar_path = crate::ssh::direct::quote_posix(&format!(
        "{}/{}",
        remote_home.trim_end_matches('/'),
        remote_archive_rel(task_id)
    ));
    format!(
        "set -e; \
         rm -rf {ws}; mkdir -p {ws}; \
         tar -xzf {tar_path} -C {ws}; \
         rm -f {tar_path}; \
         cd {ws}; \
         git init -q; \
         git config user.email 'ferryx-cloud@local'; \
         git config user.name 'ferryx-cloud'; \
         git add -A; \
         git commit -q --allow-empty -m baseline; \
         pwd"
    )
}

pub fn build_remote_diff_script(remote_home: &str, task_id: &str, cleanup: bool) -> String {
    let ws = crate::ssh::direct::quote_posix(&remote_workspace_dir(remote_home, task_id));
    let clean_cmd = if cleanup {
        format!("cd / && rm -rf {ws}")
    } else {
        "true".to_string()
    };
    format!(
        "set -e; \
         if [ ! -d {ws} ]; then exit 0; fi; \
         cd {ws}; \
         git add -A; \
         printf '__FX_STATUS_BEGIN__\\n'; \
         git diff --cached --name-status HEAD; \
         printf '__FX_PATCH_BEGIN__\\n'; \
         git diff --cached --binary HEAD; \
         {clean_cmd}"
    )
}

pub fn parse_remote_diff_output(stdout: &str) -> (Vec<String>, String) {
    let Some((_, after_status)) = stdout.split_once("__FX_STATUS_BEGIN__\n") else {
        return (Vec::new(), String::new());
    };
    let Some((status_block, patch_block)) = after_status.split_once("__FX_PATCH_BEGIN__\n") else {
        return (Vec::new(), String::new());
    };
    let changed_files = status_block
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    (changed_files, patch_block.to_string())
}

pub fn write_patch_file(path: &Path, patch: &str) -> Result<PathBuf, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut file = opts
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    file.write_all(patch.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync {}: {error}", path.display()))?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_task_id_cleans_unsafe_chars_and_rejects_empty() {
        assert_eq!(sanitize_task_id("st_abc-123").unwrap(), "st_abc-123");
        assert_eq!(sanitize_task_id("../weird/id").unwrap(), "_weird_id");
        assert!(sanitize_task_id("...").is_err());
    }

    #[test]
    fn pack_local_workspace_skips_ignored_and_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(dir.path().join("node_modules/pkg/index.js"), "x").unwrap();

        let packed = pack_local_workspace(dir.path()).unwrap();
        assert_eq!(packed.file_count, 1);
        assert!(!packed.archive_bytes.is_empty());
    }

    #[test]
    fn parse_remote_diff_output_splits_status_and_patch() {
        let raw = "__FX_STATUS_BEGIN__\nM\tsrc/a.rs\nA\tsrc/b.rs\n__FX_PATCH_BEGIN__\ndiff --git a/src/a.rs b/src/a.rs\n";
        let (files, patch) = parse_remote_diff_output(raw);
        assert_eq!(files, vec!["M\tsrc/a.rs", "A\tsrc/b.rs"]);
        assert!(patch.starts_with("diff --git"));
    }
}
