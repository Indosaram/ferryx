use super::runtime::{
    parse_fields, powershell_data, RemoteEnvironment, RemotePlatform, POWERSHELL_GIT,
};
use super::{direct, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::worktree::{BranchDeletionPreview, DirtyFile, DirtyState, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
}

pub fn parse_worktree_porcelain(output: &str) -> Vec<RemoteWorktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<RemoteWorktree> = None;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(entry) = current.take() {
                worktrees.push(entry);
            }
            continue;
        }
        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            None => (line, None),
        };
        let entry = match key {
            "worktree" => {
                let Some(path) = value else { continue };
                current.get_or_insert_with(|| RemoteWorktree {
                    path: path.to_string(),
                    head: None,
                    branch: None,
                    bare: false,
                    detached: false,
                })
            }
            _ => match current.as_mut() {
                Some(entry) => entry,
                None => continue,
            },
        };
        match (key, value) {
            ("HEAD", Some(value)) => entry.head = Some(value.to_string()),
            ("branch", Some(value)) => {
                entry.branch = Some(value.trim_start_matches("refs/heads/").to_string())
            }
            ("bare", _) => entry.bare = true,
            ("detached", _) => entry.detached = true,
            _ => {}
        }
    }
    if let Some(entry) = current.take() {
        worktrees.push(entry);
    }
    worktrees
}

/// Derives a sanitized worktree branch namespace segment from a remote workspace ID (`ssh:<hex>`).
/// Takes up to 12 leading hex characters and formats as `ssh-{segment}`.
pub fn derive_ws_segment(workspace_id: &str) -> Result<String, IpcError> {
    let hex = workspace_id
        .strip_prefix(crate::ssh::projects::REMOTE_PREFIX)
        .ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::InvalidNamespace,
                format!(
                    "Remote workspace ID must start with '{}'",
                    crate::ssh::projects::REMOTE_PREFIX
                ),
            )
        })?;
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(IpcError::new(
            IpcErrorCode::InvalidNamespace,
            "Remote workspace ID contains invalid hex characters",
        ));
    }
    let segment = if hex.len() > 12 { &hex[..12] } else { hex };
    Ok(format!("ssh-{segment}"))
}

/// `<repo>/<WORKTREE_DIR>/<ws_segment>/<slug>`, the same shape as local managed worktrees.
pub fn remote_worktree_path(
    platform: RemotePlatform,
    repo_root: &str,
    ws_segment: &str,
    slug: &str,
) -> String {
    let dir = crate::worktree::naming::WORKTREE_DIR;
    match platform {
        RemotePlatform::Posix => {
            let root = repo_root.trim_end_matches('/');
            format!("{root}/{dir}/{ws_segment}/{slug}")
        }
        RemotePlatform::Windows => {
            let root = repo_root.trim_end_matches(['/', '\\']);
            let slug = slug.replace('/', "\\");
            format!("{root}\\{dir}\\{ws_segment}\\{slug}")
        }
    }
}

/// The path segment a remote identity's `ws_id` maps to: raw `ssh:<hex>` workspace IDs use
/// [`derive_ws_segment`], anything else already is a segment.
pub fn remote_ws_segment(ws_id: &str) -> Result<String, IpcError> {
    if ws_id.starts_with(crate::ssh::projects::REMOTE_PREFIX) {
        derive_ws_segment(ws_id)
    } else {
        Ok(ws_id.to_string())
    }
}


pub fn validate_base_ref(base_ref: Option<&str>) -> Result<(), IpcError> {
    if let Some(base) = base_ref {
        if base.starts_with('-') {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "base_ref must not start with '-'",
            ));
        }
    }
    Ok(())
}

pub fn validate_path_inside_root(
    platform: RemotePlatform,
    root: &str,
    path: &str,
) -> Result<(), IpcError> {
    match platform {
        RemotePlatform::Posix => {
            let norm_root = root.trim_end_matches('/');
            let norm_path = path.trim_end_matches('/');
            if norm_path.split('/').any(|seg| seg == "..")
                || !norm_path.starts_with(&format!("{norm_root}/"))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Worktree path must be inside the project repository root",
                ));
            }
        }
        RemotePlatform::Windows => {
            let norm_root = root.replace('\\', "/").trim_end_matches('/').to_lowercase();
            let norm_path = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
            if norm_path.split('/').any(|seg| seg == "..")
                || !norm_path.starts_with(&format!("{norm_root}/"))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Worktree path must be inside the project repository root",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_cwd_inside_root(
    platform: RemotePlatform,
    root: &str,
    path: &str,
) -> Result<(), IpcError> {
    match platform {
        RemotePlatform::Posix => {
            let norm_root = root.trim_end_matches('/');
            let norm_path = path.trim_end_matches('/');
            if norm_path.split('/').any(|seg| seg == "..")
                || (norm_path != norm_root && !norm_path.starts_with(&format!("{norm_root}/")))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Working directory must be inside the project repository root",
                ));
            }
        }
        RemotePlatform::Windows => {
            let norm_root = root.replace('\\', "/").trim_end_matches('/').to_lowercase();
            let norm_path = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
            if norm_path.split('/').any(|seg| seg == "..")
                || (norm_path != norm_root && !norm_path.starts_with(&format!("{norm_root}/")))
            {
                return Err(IpcError::new(
                    IpcErrorCode::InvalidPath,
                    "Working directory must be inside the project repository root",
                ));
            }
        }
    }
    Ok(())
}

pub fn resolve_remote_spawn_root(
    platform: RemotePlatform,
    repo_root: &str,
    worktree: Option<&WorktreeIdentity>,
    cwd: Option<&str>,
) -> Result<String, IpcError> {
    if let Some(cwd) = cwd {
        validate_cwd_inside_root(platform, repo_root, cwd)?;
        let is_root = match platform {
            RemotePlatform::Posix => cwd.trim_end_matches('/') == repo_root.trim_end_matches('/'),
            RemotePlatform::Windows => {
                cwd.replace('\\', "/").trim_end_matches('/').to_lowercase()
                    == repo_root
                        .replace('\\', "/")
                        .trim_end_matches('/')
                        .to_lowercase()
            }
        };
        if is_root {
            Ok(repo_root.to_string())
        } else {
            Ok(cwd.to_string())
        }
    } else if let Some(identity) = worktree {
        if identity.slug.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Worktree slug cannot be empty",
            ));
        }
        if identity.slug.contains("..") {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                "Worktree slug must not contain '..'",
            ));
        }
        // Identities name the workspace; the managed path uses its sanitized segment, exactly as
        // `cmd_ssh_create_remote_worktree` laid it out.
        let ws_segment = remote_ws_segment(&identity.ws_id)?;
        let path = remote_worktree_path(platform, repo_root, &ws_segment, &identity.slug);
        validate_path_inside_root(platform, repo_root, &path)?;
        Ok(path)
    } else {
        Ok(repo_root.to_string())
    }
}

pub fn worktree_list_script(platform: RemotePlatform, repo_root: &str, marker: &str) -> String {
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            format!(
                "out=$(git -C {quoted_root} worktree list --porcelain 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '%s\\n' \"$out\" >&2; exit $rc; }}; printf '{marker}\\000%s\\000' \"$out\""
            )
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            format!(
                "{POWERSHELL_GIT}\n$p={p}; \
                 $g=Invoke-FerryxGit @('-C',$p,'worktree','list','--porcelain'); \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree list failed' }}; throw $err }}; \
                 [Console]::Write(('{marker}',$g.Output,'' -join [char]0))"
            )
        }
    }
}

pub fn worktree_create_script(
    platform: RemotePlatform,
    repo_root: &str,
    branch: &str,
    path: &str,
    base_ref: Option<&str>,
) -> String {
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            let quoted_branch = direct::quote_posix(branch);
            let quoted_path = direct::quote_posix(path);
            let quoted_base = match base_ref {
                Some(base) => format!(" {}", direct::quote_posix(base)),
                None => String::new(),
            };
            format!(
                "out=$(git -C {quoted_root} worktree add -b {quoted_branch} {quoted_path}{quoted_base} 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '{WORKTREE_ERR_MARKER}\\n%s\\n' \"$out\"; printf '%s\\n' \"$out\" >&2; exit $rc; }}"
            ) + "\n" + &posix_exclude_script(&quoted_root) + &format!("\nprintf '{WORKTREE_DONE_MARKER}\\n'")
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            let wt = powershell_data(path);
            let br = powershell_data(branch);
            let base_code = match base_ref {
                Some(base) => format!(" $args += {};", powershell_data(base)),
                None => String::new(),
            };
            format!(
                "{POWERSHELL_GIT}\n$p={p}; $wt={wt}; $br={br}; \
                 $args=@('-C',$p,'worktree','add','-b',$br,$wt);{base_code} \
                 $g=Invoke-FerryxGit $args; \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree add failed' }}; [Console]::Out.WriteLine('{WORKTREE_ERR_MARKER}'); [Console]::Out.WriteLine($err); throw $err }}"
            ) + "\n" + &windows_exclude_script() + &format!("\n[Console]::Out.WriteLine('{WORKTREE_DONE_MARKER}')")
        }
    }
}

/// Printed last by the create and remove scripts once git has succeeded. Some SSH servers
/// (Tailscale SSH among them) report every remote command as exit 0, so a failed `git worktree`
/// would otherwise look like a success.
pub const WORKTREE_DONE_MARKER: &str = "FERRYX_WT_DONE";

/// Printed on stdout immediately before the git error body when a create or remove command fails,
/// so an exit-0 SSH server still surfaces the real git diagnostic (including dirty-worktree text).
const WORKTREE_ERR_MARKER: &str = "FERRYX_WT_ERR";

fn is_dirty_worktree_message(msg: &str) -> bool {
    msg.contains("modified or untracked files") || msg.contains("--force")
}

fn require_done(output: &[u8], stage: &'static str) -> Result<(), IpcError> {
    let text = String::from_utf8_lossy(output);
    if text.lines().any(|line| line.trim() == WORKTREE_DONE_MARKER) {
        return Ok(());
    }
    let diagnostic = text
        .split_once(WORKTREE_ERR_MARKER)
        .map(|(_, rest)| rest.trim())
        .filter(|s| !s.is_empty());
    let code = if stage == "worktree-remove" && diagnostic.is_some_and(is_dirty_worktree_message) {
        IpcErrorCode::DirtyWorktree
    } else {
        IpcErrorCode::GitError
    };
    let message = match diagnostic {
        Some(detail) => format!("SSH git {stage} failed: {detail}"),
        None => "The remote git worktree command did not confirm success".to_string(),
    };
    Err(tag_error(
        IpcError::new(code, message).with_details(serde_json::json!({
            "stage": "execution",
            "stderr": diagnostic,
        })),
        stage,
    ))
}

/// Printed on stdout by the create script when the best-effort `info/exclude` update fails.
pub const EXCLUDE_FAILED_MARKER: &str = "FERRYX_WT_EXCLUDE_FAILED";

fn managed_dirs_words(quote: impl Fn(&str) -> String) -> String {
    crate::worktree::naming::all_worktree_dirs()
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs after a successful `worktree add`: appends `/<dir>/` to the repository's `info/exclude`
/// once for every managed worktree dir present. A failure prints [`EXCLUDE_FAILED_MARKER`] and
/// never fails the script. `.gitignore` is never touched.
fn posix_exclude_script(quoted_root: &str) -> String {
    let dirs = managed_dirs_words(direct::quote_posix);
    let marker = EXCLUDE_FAILED_MARKER;
    format!(
        "( gcd=$(git -C {quoted_root} rev-parse --git-common-dir) && \
         case \"$gcd\" in /*) ;; *) gcd={quoted_root}/\"$gcd\" ;; esac && \
         mkdir -p \"$gcd/info\" && ex=\"$gcd/info/exclude\" && \
         for d in {dirs}; do [ -d {quoted_root}/\"$d\" ] || continue; pat=\"/$d/\"; \
         grep -qxF -- \"$pat\" \"$ex\" 2>/dev/null && continue; \
         if [ -s \"$ex\" ] && [ -n \"$(tail -c 1 \"$ex\")\" ]; then printf '\\n' >> \"$ex\" || exit 1; fi; \
         printf '%s\\n' \"$pat\" >> \"$ex\" || exit 1; done ) 2>/dev/null || printf '{marker}\\n'"
    )
}

fn windows_exclude_script() -> String {
    let dirs = managed_dirs_words(|dir| format!("'{dir}'")).replace(' ', ",");
    r#"try { $c=Invoke-FerryxGit @('-C',$p,'rev-parse','--git-common-dir'); if ($c.Code -ne 0) { throw 'rev-parse failed' }; $cd=$c.Output.Trim(); if (-not [IO.Path]::IsPathRooted($cd)) { $cd=Join-Path $p $cd }; $info=Join-Path $cd 'info'; [void](New-Item -ItemType Directory -Force -Path $info); $ex=Join-Path $info 'exclude'; $have=if (Test-Path -LiteralPath $ex) { [IO.File]::ReadAllText($ex) } else { '' }; foreach ($d in @(__DIRS__)) { if (-not (Test-Path -LiteralPath (Join-Path $p $d))) { continue }; $pat='/'+$d+'/'; if (($have -split "`r?`n") -contains $pat) { continue }; $pre=if ($have.Length -gt 0 -and -not $have.EndsWith("`n")) { "`n" } else { '' }; [IO.File]::AppendAllText($ex, $pre+$pat+"`n"); $have+=$pre+$pat+"`n" } } catch { [Console]::Out.WriteLine('__MARKER__') }"#
        .replace("__DIRS__", &dirs)
        .replace("__MARKER__", EXCLUDE_FAILED_MARKER)
}

pub fn worktree_remove_script(
    platform: RemotePlatform,
    repo_root: &str,
    path: &str,
    force: bool,
) -> String {
    let force_arg = if force { " --force" } else { "" };
    match platform {
        RemotePlatform::Posix => {
            let quoted_root = direct::quote_posix(repo_root);
            let quoted_path = direct::quote_posix(path);
            format!(
                "out=$(git -C {quoted_root} worktree remove{force_arg} {quoted_path} 2>&1) || {{ rc=$?; [ \"$rc\" -ne 0 ] || rc=1; printf '{WORKTREE_ERR_MARKER}\\n%s\\n' \"$out\"; printf '%s\\n' \"$out\" >&2; exit $rc; }}; printf '{WORKTREE_DONE_MARKER}\\n'"
            )
        }
        RemotePlatform::Windows => {
            let p = powershell_data(repo_root);
            let wt = powershell_data(path);
            let force_code = if force { " $args += '--force';" } else { "" };
            format!(
                "{POWERSHELL_GIT}\n$p={p}; $wt={wt}; \
                 $args = @('-C',$p,'worktree','remove');{force_code} $args += $wt; \
                 $g=Invoke-FerryxGit $args; \
                 if ($g.Code -ne 0) {{ $err = if ($g.Error) {{ $g.Error }} elseif ($g.Output) {{ $g.Output }} else {{ 'git worktree remove failed' }}; [Console]::Out.WriteLine('{WORKTREE_ERR_MARKER}'); [Console]::Out.WriteLine($err); throw $err }}; \
                 [Console]::Out.WriteLine('{WORKTREE_DONE_MARKER}')"
            )
        }
    }
}

/// Status is base64 encoded before entering the NUL-delimited response frame.
/// Four arguments mirror the other platform script builders in this module.
pub fn worktree_delete_preview_script(
    platform: RemotePlatform,
    repo_root: &str,
    path: &str,
    marker: &str,
) -> String {
    match platform {
        RemotePlatform::Posix => {
            let root = direct::quote_posix(repo_root);
            let path = direct::quote_posix(path);
            r#"set -eu
root=__ROOT__; wt=__PATH__
tmp=$(mktemp -d) || exit 1
trap 'rm -rf "$tmp"' EXIT
# -z disables Git path quoting, preserving spaces, quotes and Unicode.
git -C "$root" worktree list --porcelain -z > "$tmp/list"
# Paths accepted by validate_path contain no control characters.
tr '\000' '\n' < "$tmp/list" > "$tmp/lines"
branch=$(WT="$wt" awk 'BEGIN { RS="\n\n"; FS="\n" } $1 == "worktree " ENVIRON["WT"] { for(i=2;i<=NF;i++) if(index($i,"branch refs/heads/")==1) print substr($i,19); found=1 } END { if(!found) exit 1 }' "$tmp/lines") || { echo 'Worktree not found' >&2; exit 1; }
if [ -z "$branch" ]; then printf '__MARKER__\000detached\000'; exit 0; fi
ref="refs/heads/$branch"
head=$(git -C "$root" rev-parse --verify --end-of-options "$ref^{commit}")
missing=false
: > "$tmp/status"
if [ -e "$wt" ]; then git -C "$wt" status --porcelain=v1 -z > "$tmp/status"; else missing=true; fi
merged=false
if git -C "$root" merge-base --is-ancestor -- "$ref" HEAD; then merged=true; else rc=$?; [ "$rc" -eq 1 ] || exit "$rc"; fi
upstream=''; counts=''
# --end-of-options protects revisions; -- would instead begin path arguments.
if upstream=$(git -C "$root" rev-parse --abbrev-ref --symbolic-full-name --verify --end-of-options "$branch@{upstream}" 2>/dev/null); then
    counts=$(git -C "$root" rev-list --left-right --count --end-of-options "$upstream...$ref" --)
else upstream=''; fi
base64 < "$tmp/status" > "$tmp/encoded"
status=$(tr -d '\r\n' < "$tmp/encoded")
printf '__MARKER__\000ok\000%s\000%s\000%s\000%s\000%s\000%s\000%s\000' "$branch" "$head" "$missing" "$merged" "$upstream" "$counts" "$status"
"#
                .replace("__ROOT__", &root)
                .replace("__PATH__", &path)
                .replace("__MARKER__", marker)
        }
        RemotePlatform::Windows => {
            let root = powershell_data(repo_root);
            let path = powershell_data(path);
            let body = r#"
$root=__ROOT__; $wt=__PATH__
$list=Invoke-FerryxGit @('-C',$root,'worktree','list','--porcelain','-z')
if ($list.Code -ne 0) { throw $list.Error }
$nul=[string][char]0; $wanted='worktree '+$wt.Replace('\','/').TrimEnd('/')
$record=@($list.Output -split ($nul+$nul) | Where-Object { (($_ -split $nul)[0]).Replace('\','/').TrimEnd('/') -eq $wanted })
if ($record.Count -ne 1) { throw 'Worktree not found' }
$branch=@($record[0] -split $nul | Where-Object { $_.StartsWith('branch refs/heads/') })
if ($branch.Count -eq 0) { [Console]::Write(('__MARKER__','detached','' -join [char]0)); return }
$branch=$branch[0].Substring(18); $ref='refs/heads/'+$branch
$head=Invoke-FerryxGit @('-C',$root,'rev-parse','--verify','--end-of-options',($ref+'^{commit}'))
if ($head.Code -ne 0) { throw $head.Error }
$missing=-not (Test-Path -LiteralPath $wt); $status=''
if (-not $missing) {
    $g=Invoke-FerryxGit @('-C',$wt,'status','--porcelain=v1','-z')
    if ($g.Code -ne 0) { throw $g.Error }
    $status=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($g.Output))
}
$merge=Invoke-FerryxGit @('-C',$root,'merge-base','--is-ancestor','--',$ref,'HEAD')
if ($merge.Code -ne 0 -and $merge.Code -ne 1) { throw $merge.Error }
$up=Invoke-FerryxGit @('-C',$root,'rev-parse','--abbrev-ref','--symbolic-full-name','--verify','--end-of-options',($branch+'@{upstream}'))
$upstream=''; $counts=''
if ($up.Code -eq 0) {
    $upstream=$up.Output.Trim()
    $g=Invoke-FerryxGit @('-C',$root,'rev-list','--left-right','--count','--end-of-options',($upstream+'...'+$ref),'--')
    if ($g.Code -ne 0) { throw $g.Error }; $counts=$g.Output.Trim()
}
[Console]::Write(('__MARKER__','ok',$branch,$head.Output.Trim(),$missing.ToString().ToLowerInvariant(),($merge.Code -eq 0).ToString().ToLowerInvariant(),$upstream,$counts,$status,'' -join [char]0))
"#;
            format!(
                "{POWERSHELL_GIT}\n{}",
                body.replace("__ROOT__", &root)
                    .replace("__PATH__", &path)
                    .replace("__MARKER__", marker)
            )
        }
    }
}

pub fn parse_delete_preview_output(
    output: &[u8],
    marker: &str,
) -> Result<BranchDeletionPreview, IpcError> {
    let invalid = || IpcError::new(IpcErrorCode::ParseError, "Malformed worktree delete preview");
    if let Ok(fields) = parse_fields(output, marker, 1) {
        if fields[0] == "detached" {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Detached worktree has no branch",
            ));
        }
    }
    let fields = parse_fields(output, marker, 8)?;
    if fields[0] != "ok" || fields[1].is_empty() || fields[2].is_empty() {
        return Err(invalid());
    }
    let missing = fields[3].parse::<bool>().map_err(|_| invalid())?;
    let merged = fields[4].parse::<bool>().map_err(|_| invalid())?;
    let upstream = (!fields[5].is_empty()).then(|| fields[5].to_owned());
    let (behind, ahead) = match upstream.as_ref() {
        Some(_) => {
            let counts = fields[6]
                .split_whitespace()
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| invalid())?;
            match counts.as_slice() {
                [behind, ahead] => (Some(*behind), Some(*ahead)),
                _ => return Err(invalid()),
            }
        }
        None => {
            if !fields[6].is_empty() {
                return Err(invalid());
            }
            (None, None)
        }
    };
    let status = STANDARD.decode(fields[7]).map_err(|_| invalid())?;
    let mut records = status.split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            if records.next().is_some() {
                return Err(invalid());
            }
            break;
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(invalid());
        }
        let status_code = std::str::from_utf8(&record[..2])
            .map_err(|_| invalid())?
            .to_owned();
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        if status_code.contains(['R', 'C'])
            && records.next().is_none_or(|source| source.is_empty())
        {
            return Err(invalid());
        }
        files.push(DirtyFile { status_code, path });
    }
    if !status.is_empty() && status.last() != Some(&0) {
        return Err(invalid());
    }
    if missing && !files.is_empty() {
        return Err(invalid());
    }
    Ok(BranchDeletionPreview {
        dirty_state: DirtyState::dirty(files),
        missing,
        branch: fields[1].into(),
        head: fields[2].into(),
        upstream,
        merged,
        ahead,
        behind,
    })
}

pub async fn preview_remote_delete(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
    path: &str,
) -> Result<BranchDeletionPreview, IpcError> {
    environment.platform.validate_path(repo_root)?;
    environment.platform.validate_path(path)?;
    validate_path_inside_root(environment.platform, repo_root, path)?;
    let marker = format!("FERRYX_WT_PREVIEW_V1_{}", uuid::Uuid::new_v4().simple());
    let script = worktree_delete_preview_script(environment.platform, repo_root, path, &marker);
    let mut plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let master = super::master::acquire(host).await.ok().flatten();
    if let Some(lease) = master.as_ref() {
        direct::use_master(&mut plan, lease.socket());
    }
    let output = direct::bounded_output_with_limit(&plan, Duration::from_secs(30), 8 * 1024 * 1024)
        .await?;
    parse_delete_preview_output(&output, &marker)
}

fn tag_error(mut err: IpcError, stage: &'static str) -> IpcError {
    match err.details.as_mut() {
        Some(details) => {
            details["stage"] = stage.into();
        }
        None => {
            err = err.with_details(serde_json::json!({"stage": stage}));
        }
    }
    err
}

pub async fn list_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
) -> Result<Vec<RemoteWorktree>, IpcError> {
    environment.platform.validate_path(repo_root)?;
    let marker = format!("FERRYX_WT_LIST_V1_{}", uuid::Uuid::new_v4().simple());
    let script = worktree_list_script(environment.platform, repo_root, &marker);
    let mut plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    // Read-only inventory: a reused master removes one cold handshake per call and cannot
    // change the result, so a shared connection is safe here in a way it is not for a
    // mutation. The master is an optimisation, never a requirement - any failure to
    // establish one keeps the cold plan - and the lease is held for the whole call so idle
    // expiry sees this operation as active rather than guessing from elapsed time.
    let master = super::master::acquire(host).await.ok().flatten();
    if let Some(lease) = master.as_ref() {
        direct::use_master(&mut plan, lease.socket());
    }
    super::master::note_path("worktree_list", master.is_some());
    list_output(&plan, &marker).await
}

async fn list_output(
    plan: &crate::terminal::shell::ShellCommandPlan,
    marker: &str,
) -> Result<Vec<RemoteWorktree>, IpcError> {
    // A complete porcelain inventory grows with the number and path length of worktrees;
    // it is not a small probe report. Keep a bounded 8 MiB budget without truncating entries.
    let output = direct::bounded_output_with_limit(plan, Duration::from_secs(30), 8 * 1024 * 1024)
        .await
        .map_err(|err| tag_error(err, "worktree-list"))?;
    let fields =
        parse_fields(&output, marker, 1).map_err(|err| tag_error(err, "worktree-list"))?;
    Ok(parse_worktree_porcelain(fields[0]))
}

pub async fn create_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
    ws_segment: &str,
    slug: &str,
    base_ref: Option<&str>,
    path: &str,
) -> Result<(), IpcError> {
    validate_base_ref(base_ref)?;
    environment.platform.validate_path(repo_root)?;
    environment.platform.validate_path(path)?;
    let is_windows = environment.platform == RemotePlatform::Windows;
    let branch = crate::worktree::manager::WorktreeManager::format_branch_name_platform(
        ws_segment, slug, is_windows,
    )
    .map_err(|err| IpcError::new(IpcErrorCode::InvalidNamespace, err.to_string()))?;
    let script = worktree_create_script(environment.platform, repo_root, &branch, path, base_ref);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(30))
        .await
        .map_err(|err| tag_error(err, "worktree-create"))?;
    require_done(&output, "worktree-create")?;
    if String::from_utf8_lossy(&output)
        .lines()
        .any(|line| line.trim() == EXCLUDE_FAILED_MARKER)
    {
        tracing::warn!(
            target: "worktree",
            kind = "worktree_exclude_failed",
            repo = %repo_root,
            "could not add managed worktree dir to the remote info/exclude"
        );
    }
    Ok(())
}

pub async fn remove_remote(
    host: &SshHost,
    environment: &RemoteEnvironment,
    repo_root: &str,
    path: &str,
    force: bool,
) -> Result<(), IpcError> {
    environment.platform.validate_path(repo_root)?;
    environment.platform.validate_path(path)?;
    let script = worktree_remove_script(environment.platform, repo_root, path, force);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let result = direct::bounded_output(&plan, Duration::from_secs(30)).await;
    match result {
        Ok(output) => require_done(&output, "worktree-remove"),
        Err(mut err) => {
            let is_dirty = err
                .details
                .as_ref()
                .and_then(|d| d.get("stderr"))
                .and_then(|s| s.as_str())
                .map(is_dirty_worktree_message)
                .unwrap_or(false);
            if is_dirty {
                err.code = IpcErrorCode::DirtyWorktree;
            }
            Err(tag_error(err, "worktree-remove"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::manager::WorktreeManager;

    #[cfg(unix)]
    fn preview_fixture() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().to_str().unwrap().to_owned();
        preview_git(&root, &["init", "--initial-branch=main"]);
        preview_git(&root, &["config", "user.name", "Preview Test"]);
        preview_git(&root, &["config", "user.email", "preview@example.test"]);
        std::fs::write(dir.path().join("tracked"), "original").unwrap();
        preview_git(&root, &["add", "tracked"]);
        preview_git(&root, &["commit", "-m", "initial"]);
        let path = format!("{root}/checkout with 'quotes' 日本語");
        preview_git(&root, &["worktree", "add", "-b", "feature", &path]);
        (dir, root, path)
    }

    #[cfg(unix)]
    fn preview_git(root: &str, args: &[&str]) {
        let output = std::process::Command::new("git").args(["-C", root]).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    #[cfg(unix)]
    async fn execute_preview(root: &str, path: &str) -> Result<BranchDeletionPreview, IpcError> {
        let marker = "FERRYX_PREVIEW_TEST";
        let script = worktree_delete_preview_script(RemotePlatform::Posix, root, path, marker);
        let plan = crate::terminal::shell::ShellCommandPlan { program: "/bin/sh".into(), args: vec!["-c".into(), script] };
        let output = direct::bounded_output_with_limit(&plan, Duration::from_secs(10), 8 * 1024 * 1024).await?;
        parse_delete_preview_output(&output, marker)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_is_clean_and_merged_when_branch_matches_head() {
        // Given: a real linked checkout at repository HEAD.
        let (_dir, root, path) = preview_fixture();
        // When: executing the generated script through the real shell.
        let preview = execute_preview(&root, &path).await.unwrap();
        // Then: its checkout/ref state is reported without a fabricated upstream.
        assert!(!preview.missing);
        assert!(!preview.dirty_state.is_dirty);
        assert!(preview.merged);
        assert_eq!(preview.branch, "feature");
        assert_eq!((preview.upstream, preview.ahead, preview.behind), (None, None, None));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_preserves_dirty_paths_when_status_has_renames_and_control_characters() {
        // Given: a staged rename, modification and untracked special-character paths.
        let (_dir, root, path) = preview_fixture();
        let renamed = "renamed 'quoted' 日本語\nfile";
        preview_git(&path, &["mv", "tracked", renamed]);
        std::fs::write(std::path::Path::new(&path).join(renamed), "modified").unwrap();
        let untracked = "untracked spaces \"quotes\" 日本語\nFERRYX_PREVIEW_TEST";
        std::fs::write(std::path::Path::new(&path).join(untracked), "untracked").unwrap();
        // When: inspecting the checkout via the generated script and frame parser.
        let preview = execute_preview(&root, &path).await.unwrap();
        // Then: paths survive NUL status framing, base64 and the outer marker unchanged.
        assert!(preview.dirty_state.is_dirty);
        assert_eq!(preview.dirty_state.files.len(), 2);
        assert!(preview.dirty_state.files.iter().any(|file| file.path == renamed && file.status_code == "RM"));
        assert!(preview.dirty_state.files.iter().any(|file| file.path == untracked && file.status_code == "??"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_reports_divergence_when_branch_and_upstream_have_unique_commits() {
        // Given: unequal unique commit counts on each side of a local upstream.
        let (_dir, root, path) = preview_fixture();
        preview_git(&path, &["commit", "--allow-empty", "-m", "feature"]);
        preview_git(&root, &["commit", "--allow-empty", "-m", "main"]);
        preview_git(&root, &["commit", "--allow-empty", "-m", "main again"]);
        preview_git(&root, &["branch", "--set-upstream-to=main", "feature"]);
        // When: inspecting the branch against repository HEAD.
        let preview = execute_preview(&root, &path).await.unwrap();
        // Then: merge state and count direction match the local preview contract.
        assert!(!preview.merged);
        assert_eq!(preview.upstream.as_deref(), Some("main"));
        assert_eq!((preview.behind, preview.ahead), (Some(2), Some(1)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_is_merged_when_repository_head_contains_branch() {
        // Given: a feature commit merged into repository HEAD.
        let (_dir, root, path) = preview_fixture();
        preview_git(&path, &["commit", "--allow-empty", "-m", "feature"]);
        preview_git(&root, &["merge", "--no-ff", "-m", "merge feature", "feature"]);
        // When: inspecting the feature checkout.
        let preview = execute_preview(&root, &path).await.unwrap();
        // Then: the branch is merged into the repository, not merely its own checkout HEAD.
        assert!(preview.merged);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_recovers_refs_when_checkout_is_missing() {
        // Given: Git still knows a checkout whose directory has disappeared.
        let (_dir, root, path) = preview_fixture();
        preview_git(&path, &["commit", "--allow-empty", "-m", "feature"]);
        let expected = std::process::Command::new("git").args(["-C", &root, "rev-parse", "refs/heads/feature"]).output().unwrap();
        assert!(expected.status.success());
        std::fs::remove_dir_all(&path).unwrap();
        // When: inspecting it through repository metadata.
        let preview = execute_preview(&root, &path).await.unwrap();
        // Then: branch/head survive and missing checkout is not treated as merged.
        assert!(preview.missing);
        assert!(!preview.merged);
        assert!(!preview.dirty_state.is_dirty);
        assert_eq!(preview.branch, "feature");
        assert_eq!(preview.head, String::from_utf8(expected.stdout).unwrap().trim());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_returns_structured_error_when_checkout_is_detached() {
        // Given: a registered checkout without a branch ref.
        let (_dir, root, path) = preview_fixture();
        preview_git(&path, &["checkout", "--detach"]);
        // When: requesting a deletion preview.
        let error = execute_preview(&root, &path).await.unwrap_err();
        // Then: no branch or merge state is fabricated.
        assert_eq!(error.code, IpcErrorCode::InvalidArgument);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_fails_when_existing_checkout_cannot_run_git_status() {
        // Given: a registered directory whose .git pointer is corrupt.
        let (_dir, root, path) = preview_fixture();
        std::fs::write(std::path::Path::new(&path).join(".git"), "gitdir: /does-not-exist").unwrap();
        // When: status inspection fails.
        let result = execute_preview(&root, &path).await;
        // Then: the failure cannot be reported as a clean checkout.
        assert!(result.is_err());
    }

    #[test]
    fn preview_parser_rejects_malformed_frames_and_status() {
        // Given: malformed wire responses, including invalid counts and rename records.
        for frame in [
            b"wrong\0ok\0".as_slice(),
            b"M\0ok\0feature\0abc\0false\0true\0\0\0!!!\0",
            b"M\0ok\0feature\0abc\0false\0true\0main\0bad\0\0",
        ] {
            // When: parsing the untrusted frame.
            let result = parse_delete_preview_output(frame, "M");
            // Then: corruption never becomes a successful preview.
            assert!(result.is_err());
        }
        let malformed_status = STANDARD.encode(b"R  destination\0");
        let frame = format!("M\0ok\0feature\0abc\0false\0false\0\0\0{malformed_status}\0");
        assert!(parse_delete_preview_output(frame.as_bytes(), "M").is_err());
    }

    #[test]
    fn windows_preview_script_encodes_paths_and_uses_checked_git_calls() {
        // Given: Windows paths with PowerShell metacharacters.
        let script = worktree_delete_preview_script(RemotePlatform::Windows, r"C:\repo '日本語'", r"C:\repo '日本語'\checkout", "M");
        // When: preparing the PowerShell transport.
        // Then: data is encoded, status is NUL-safe, and Git errors are checked.
        assert!(script.contains("FromBase64String"));
        assert!(script.contains("Invoke-FerryxGit"));
        assert!(script.contains("'--porcelain=v1','-z'"));
        assert!(script.contains("ToBase64String"));
        assert!(script.contains("'merge-base','--is-ancestor','--',$ref,'HEAD'"));
        assert!(script.contains("$g.Code -ne 0"));
    }

    #[test]
    fn preview_parser_reads_windows_frame_and_count_direction() {
        // Given: a frame in the exact format emitted by PowerShell.
        let status = STANDARD.encode("?? file with spaces 日本語\0".as_bytes());
        let frame = format!("M\0ok\0feature\0abc\0false\0false\0origin/main\02\t3\0{status}\0");
        // When: parsing Windows output.
        let preview = parse_delete_preview_output(frame.as_bytes(), "M").unwrap();
        // Then: counts and paths retain their semantic direction/content.
        assert_eq!((preview.behind, preview.ahead), (Some(2), Some(3)));
        assert_eq!(preview.dirty_state.files[0].path, "file with spaces 日本語");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preview_rejects_traversal_before_ssh() {
        // Given: an otherwise valid endpoint and a traversal path.
        let host: SshHost = serde_json::from_value(serde_json::json!({"id":"preview-jail","hostname":"never-contact.invalid"})).unwrap();
        let environment = RemoteEnvironment { platform: RemotePlatform::Posix, executor: super::super::runtime::RemoteExecutor::Sh, version: "test".into(), home: "/tmp".into(), temp: "/tmp".into(), git: true };
        // When: previewing outside the repository jail.
        let error = preview_remote_delete(&host, &environment, "/repo", "/repo/../escape").await.unwrap_err();
        // Then: it fails validation without attempting any SSH connection.
        assert_eq!(error.code, IpcErrorCode::InvalidPath);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worktree_listing_larger_than_probe_limit_is_complete() {
        let marker = "FERRYX_WT_LIST_LARGE";
        let script = format!(
            "printf '{marker}\\000'; i=0; while [ $i -lt 300 ]; do printf 'worktree /srv/repo/worktree-%s\\nHEAD 0123456789012345678901234567890123456789\\nbranch refs/heads/branch-%s\\n\\n' \"$i\" \"$i\"; i=$((i+1)); done; printf '\\000'"
        );
        let plan = crate::terminal::shell::ShellCommandPlan {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script],
        };

        let entries = list_output(&plan, marker).await.expect("complete listing");

        assert_eq!(entries.len(), 300);
        assert_eq!(entries[299].path, "/srv/repo/worktree-299");
        assert_eq!(entries[299].branch.as_deref(), Some("branch-299"));
    }

    #[test]
    fn red_parse_porcelain_entries() {
        // Legacy input (.orca-worktrees/wt-<slug> on orca/...) next to the Ferryx layout.
        let output = "worktree /home/sook/repo\nHEAD abc1234\nbranch refs/heads/main\n\nworktree /home/sook/repo/.orca-worktrees/wt-feat\nHEAD def5678\nbranch refs/heads/orca/ws1/wt-feat\n\nworktree /tmp/detached-wt\ndetached\n\nworktree /home/sook/repo/.ferryx-worktrees/ssh-ab/new\nHEAD 0a0b0c0\nbranch refs/heads/ferryx/ssh-ab/new\n\n";
        let parsed = parse_worktree_porcelain(output);
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        let legacy_branch = format!(
            "{}/ws1/wt-feat",
            crate::worktree::naming::LEGACY_BRANCH_PREFIXES[0]
        );
        assert_eq!(parsed[1].branch.as_deref(), Some(legacy_branch.as_str()));
        assert!(parsed[2].detached);
        assert_eq!(parsed[3].branch.as_deref(), Some("ferryx/ssh-ab/new"));
        assert_eq!(
            crate::worktree::naming::slug_from_worktree_path(&parsed[1].path).as_deref(),
            Some("feat")
        );
        assert_eq!(
            crate::worktree::naming::slug_from_worktree_path(&parsed[3].path).as_deref(),
            Some("new")
        );
        assert!(!parsed[0].bare);
    }

    #[test]
    fn red_malformed_porcelain_tolerated() {
        let parsed = parse_worktree_porcelain(
            "noworktreekey here\n\nworktree /only/path\n\nworktree /b\nbare\n",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].path, "/only/path");
        assert!(parsed[1].bare);
    }

    #[test]
    fn red_list_script_marker_framed_both_platforms() {
        let posix = worktree_list_script(RemotePlatform::Posix, "/home/user/repo", "MARKER_LIST");
        assert!(posix.contains("git -C '/home/user/repo' worktree list --porcelain"));
        assert!(posix.contains("MARKER_LIST\\000%s\\000"));

        let windows =
            worktree_list_script(RemotePlatform::Windows, r"C:\Users\sook\repo", "MARKER_WIN");
        assert!(windows.contains("Invoke-FerryxGit"));
        assert!(windows.contains("'worktree','list','--porcelain'"));
        assert!(windows.contains("MARKER_WIN"));
    }

    #[test]
    fn red_worktree_remove_script_supports_force_both_platforms() {
        let posix_safe = worktree_remove_script(
            RemotePlatform::Posix,
            "/srv/repo",
            "/srv/repo/.orca-worktrees/wt-1",
            false,
        );
        assert!(posix_safe
            .contains("git -C '/srv/repo' worktree remove '/srv/repo/.orca-worktrees/wt-1'"));
        assert!(!posix_safe.contains("--force"));

        let posix_force = worktree_remove_script(
            RemotePlatform::Posix,
            "/srv/repo",
            "/srv/repo/.orca-worktrees/wt-1",
            true,
        );
        assert!(posix_force.contains(
            "git -C '/srv/repo' worktree remove --force '/srv/repo/.orca-worktrees/wt-1'"
        ));

        let win_safe = worktree_remove_script(
            RemotePlatform::Windows,
            r"C:\repo",
            r"C:\repo\.orca-worktrees\wt-1",
            false,
        );
        assert!(!win_safe.contains("'--force'"));

        let win_force = worktree_remove_script(
            RemotePlatform::Windows,
            r"C:\repo",
            r"C:\repo\.orca-worktrees\wt-1",
            true,
        );
        assert!(win_force.contains("$args += '--force';"));
    }

    #[test]
    fn remote_worktree_cross_platform_namespace_selection() {
        let ws_segment = "ssh-0123456789ab";
        // POSIX remote accepts names that are reserved devices only on Windows
        assert!(WorktreeManager::format_branch_name_platform(ws_segment, "CON", false).is_ok());
        assert!(WorktreeManager::format_branch_name_platform(ws_segment, "aux.txt", false).is_ok());
        // Windows remote strictly rejects Windows reserved device names
        assert!(WorktreeManager::format_branch_name_platform(ws_segment, "CON", true).is_err());
        assert!(WorktreeManager::format_branch_name_platform(ws_segment, "aux.txt", true).is_err());
    }

    #[test]
    fn worktree_commands_need_their_done_marker_not_just_exit_zero() {
        // A server that reports every command as exit 0 returns a failed git as empty output.
        assert!(require_done(b"", "worktree-remove").is_err());
        assert!(require_done(b"fatal: not a working tree\n", "worktree-create").is_err());
        let dirty = require_done(
            b"FERRYX_WT_ERR\nfatal: '/r/wt' contains modified or untracked files, use --force to delete it\n",
            "worktree-remove",
        )
        .expect_err("dirty worktree error must surface even on exit-0 servers");
        assert_eq!(dirty.code, IpcErrorCode::DirtyWorktree);
        assert!(dirty.message.contains("modified or untracked files"));
        assert!(require_done(b"FERRYX_WT_EXCLUDE_FAILED\nFERRYX_WT_DONE\n", "worktree-create").is_ok());
        for script in [
            worktree_create_script(RemotePlatform::Posix, "/r", "ferryx/a/b", "/r/.ferryx-worktrees/a/b", None),
            worktree_create_script(RemotePlatform::Windows, r"C:\r", "ferryx/a/b", r"C:\r\.ferryx-worktrees\a\b", None),
            worktree_remove_script(RemotePlatform::Posix, "/r", "/r/.ferryx-worktrees/a/b", false),
            worktree_remove_script(RemotePlatform::Windows, r"C:\r", r"C:\r\.ferryx-worktrees\a\b", true),
        ] {
            assert!(script.contains(WORKTREE_DONE_MARKER), "{script}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_posix_remove_never_prints_the_done_marker() {
        let script = worktree_remove_script(RemotePlatform::Posix, "/nonexistent-ferryx-repo", "/nonexistent-ferryx-repo/wt", false);
        let output = std::process::Command::new("sh").args(["-c", &script]).output().unwrap();
        assert!(!output.status.success());
        assert!(require_done(&output.stdout, "worktree-remove").is_err());
    }

    #[test]
    fn red_create_script_quoting_and_sanitized_branch() {
        let raw_ws_id = "ssh:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let ws_segment = derive_ws_segment(raw_ws_id).expect("valid segment");
        assert_eq!(ws_segment, "ssh-0123456789ab");

        let branch =
            WorktreeManager::format_branch_name(&ws_segment, "my-feature").expect("valid branch");
        assert_eq!(branch, "ferryx/ssh-0123456789ab/my-feature");

        // Posix create script with base_ref
        let posix = worktree_create_script(
            RemotePlatform::Posix,
            "/home/user/repo with spaces",
            &branch,
            "/home/user/repo with spaces/.ferryx-worktrees/ssh-0123456789ab/my-feature",
            Some("origin/main"),
        );
        // The raw hash with colon must never reach the script
        assert!(!posix.contains("ssh:0123456789ab"));
        assert!(posix.contains("ferryx/ssh-0123456789ab/my-feature"));
        assert!(posix.contains("'/home/user/repo with spaces'"));
        assert!(posix
            .contains("'/home/user/repo with spaces/.ferryx-worktrees/ssh-0123456789ab/my-feature'"));
        assert!(posix.contains("'origin/main'"));

        // Windows create script without base_ref
        let win = worktree_create_script(
            RemotePlatform::Windows,
            r"C:\Users\sook\repo",
            &branch,
            r"C:\Users\sook\repo\.ferryx-worktrees\ssh-0123456789ab\my-feature",
            None,
        );
        assert!(!win.contains("ssh:0123456789ab"));
        assert!(win.contains("Invoke-FerryxGit"));
    }

    #[test]
    fn red_derive_ws_segment_malformed_rejected() {
        assert!(derive_ws_segment("not-ssh:1234").is_err());
        assert!(derive_ws_segment("ssh:").is_err());
        assert!(derive_ws_segment("ssh:bad..hex").is_err());
        assert!(derive_ws_segment("ssh:0123456789abcdef").is_ok());
        assert_eq!(
            derive_ws_segment("ssh:0123456789abcdef").unwrap(),
            "ssh-0123456789ab"
        );
        assert_eq!(derive_ws_segment("ssh:abc").unwrap(), "ssh-abc");
    }

    #[test]
    fn red_path_inside_root_guard() {
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/home/user/repo",
            "/home/user/repo/.orca-worktrees/wt-1"
        )
        .is_ok());
        assert!(validate_path_inside_root(
            RemotePlatform::Windows,
            r"C:\Users\sook\repo",
            r"C:\Users\sook\repo\.orca-worktrees\wt-1"
        )
        .is_ok());
        assert!(validate_path_inside_root(
            RemotePlatform::Windows,
            r"C:/Users/sook/repo",
            r"C:\Users\sook\repo\.orca-worktrees\wt-1"
        )
        .is_ok());

        // Repo root itself cannot be deleted
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/home/user/repo",
            "/home/user/repo"
        )
        .is_err());
        // Sibling folder cannot be deleted
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/home/user/repo",
            "/home/user/repo-other/wt"
        )
        .is_err());
        // Traversal cannot be deleted
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/home/user/repo",
            "/home/user/repo/../secret"
        )
        .is_err());
        assert!(
            validate_path_inside_root(RemotePlatform::Posix, "/home/user/repo", "/etc/passwd")
                .is_err()
        );

        // Fix 1 Regression tests:
        // Posix: no backslash translation
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/srv/repo",
            r"/srv/repo\outside"
        )
        .is_err());
        assert!(
            validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", "/srv/repo-other/x")
                .is_err()
        );
        assert!(validate_path_inside_root(
            RemotePlatform::Posix,
            "/srv/repo",
            "/srv/repo/.orca-worktrees/wt-1"
        )
        .is_ok());
        assert!(
            validate_path_inside_root(RemotePlatform::Posix, "/srv/repo", "/srv/repo").is_err()
        );
        // Windows: case and separator insensitive, traversal rejected
        assert!(validate_path_inside_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            r"c:\repo\.orca-worktrees\wt-1"
        )
        .is_ok());
        assert!(validate_path_inside_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            r"C:\Repo\..\secret"
        )
        .is_err());
    }

    #[test]
    fn red_create_remote_rejects_base_ref_option_injection() {
        assert_eq!(
            validate_base_ref(Some("--no-checkout")).unwrap_err().code,
            IpcErrorCode::InvalidArgument
        );
        assert!(validate_base_ref(Some("origin/main")).is_ok());
        assert!(validate_base_ref(None).is_ok());
    }

    #[tokio::test]
    async fn red_create_remote_option_injection_async_guard() {
        use crate::ssh::runtime::RemoteExecutor;
        use crate::ssh::{SshAuthMethod, SshHostSource};

        let host = SshHost {
            id: "h1".into(),
            label: "test".into(),
            hostname: "localhost".into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Config,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            shell: None,
        };
        let env = RemoteEnvironment {
            platform: RemotePlatform::Posix,
            executor: RemoteExecutor::Sh,
            version: "test".into(),
            home: "/home/user".into(),
            temp: "/tmp".into(),
            git: true,
        };
        let err = create_remote(
            &host,
            &env,
            "/srv/repo",
            "ssh-abc",
            "wt-1",
            Some("--no-checkout"),
            "/srv/repo/.orca-worktrees/wt-1",
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
    }

    #[test]
    fn red_posix_script_builders_executable_behavior() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo_root_path = dir.path().canonicalize().expect("canonicalize");
        let portable_root =
            crate::worktree::strip_verbatim_prefix(repo_root_path.to_str().expect("repo root str"));
        let portable_root = if cfg!(windows) {
            portable_root.replace('\\', "/")
        } else {
            portable_root
        };
        let repo_root = portable_root.as_str();

        let init_status = std::process::Command::new("git")
            .args(["init", repo_root])
            .status()
            .expect("git init");
        assert!(init_status.success());

        let name_status = std::process::Command::new("git")
            .args(["-C", repo_root, "config", "user.name", "test"])
            .status()
            .expect("git config user.name");
        assert!(name_status.success());

        let email_status = std::process::Command::new("git")
            .args(["-C", repo_root, "config", "user.email", "test@example.com"])
            .status()
            .expect("git config user.email");
        assert!(email_status.success());

        let commit_status = std::process::Command::new("git")
            .args(["-C", repo_root, "commit", "--allow-empty", "-m", "init"])
            .status()
            .expect("git commit");
        assert!(commit_status.success());

        let ws_segment = derive_ws_segment("ssh:0123456789abcdef").expect("valid segment");
        let branch =
            WorktreeManager::format_branch_name(&ws_segment, "feat-test").expect("valid branch");
        let wt_path =
            remote_worktree_path(RemotePlatform::Posix, repo_root, &ws_segment, "feat-test");
        assert_eq!(wt_path, format!("{repo_root}/.ferryx-worktrees/ssh-0123456789ab/feat-test"));

        let create_script =
            worktree_create_script(RemotePlatform::Posix, repo_root, &branch, &wt_path, None);

        // Git for Windows places git.exe under cmd/, bin/ or mingw64/bin/ depending on the
        // installation, so bash.exe is found by walking up from git.exe rather than assuming a
        // single layout. A hosted runner image puts git.exe under mingw64/bin, where the plain
        // `<root>/bin/bash.exe` guess does not exist and the script never runs.
        let bash = if cfg!(windows) {
            let git = std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
                .map(|dir| dir.join("git.exe"))
                .find(|path| path.is_file())
                .expect("Git for Windows on PATH");
            let mut candidates: Vec<std::path::PathBuf> = Vec::new();
            let mut cursor = git.parent();
            while let Some(dir) = cursor {
                candidates.push(dir.join("bin").join("bash.exe"));
                candidates.push(dir.join("usr").join("bin").join("bash.exe"));
                cursor = dir.parent();
            }
            candidates
                .into_iter()
                .find(|candidate| candidate.is_file())
                .expect("bash.exe inside the Git for Windows installation")
        } else {
            std::path::PathBuf::from("bash")
        };
        let create_output = std::process::Command::new(&bash)
            .args(["-c", &create_script])
            .output()
            .expect("run create_script");
        assert!(
            create_output.status.success(),
            "create_script failed: status={:?} stdout={} stderr={}",
            create_output.status,
            String::from_utf8_lossy(&create_output.stdout),
            String::from_utf8_lossy(&create_output.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&create_output.stdout).contains(EXCLUDE_FAILED_MARKER),
            "exclude step failed"
        );
        let status = std::process::Command::new("git")
            .args(["-C", repo_root, "status", "--porcelain"])
            .output()
            .expect("git status");
        assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "");
        let exclude = std::fs::read_to_string(repo_root_path.join(".git/info/exclude"))
            .expect("info/exclude");
        assert_eq!(
            exclude
                .lines()
                .filter(|line| *line == "/.ferryx-worktrees/")
                .count(),
            1
        );

        let marker = format!("FERRYX_WT_LIST_V1_{}", uuid::Uuid::new_v4().simple());
        let list_script = worktree_list_script(RemotePlatform::Posix, repo_root, &marker);

        let list_output = std::process::Command::new(&bash)
            .args(["-c", &list_script])
            .output()
            .expect("run list_script");
        assert!(
            list_output.status.success(),
            "list_script failed: {}",
            String::from_utf8_lossy(&list_output.stderr)
        );

        let fields = parse_fields(&list_output.stdout, &marker, 1).expect("parse_fields");
        let worktrees = parse_worktree_porcelain(fields[0]);

        let created_wt = worktrees
            .iter()
            .find(|wt| wt.path == wt_path)
            .expect("created worktree in porcelain listing");
        assert_eq!(
            created_wt.branch.as_deref(),
            Some("ferryx/ssh-0123456789ab/feat-test")
        );
    }

    #[test]
    fn red_porcelain_parsing_of_created_output() {
        let output = "worktree /srv/repo/.ferryx-worktrees/ssh-abc/new\nHEAD e4d8c2\nbranch refs/heads/ferryx/ssh-abc/new\n\n";
        let parsed = parse_worktree_porcelain(output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "/srv/repo/.ferryx-worktrees/ssh-abc/new");
        assert_eq!(parsed[0].head.as_deref(), Some("e4d8c2"));
        assert_eq!(parsed[0].branch.as_deref(), Some("ferryx/ssh-abc/new"));
        assert!(!parsed[0].bare);
        assert!(!parsed[0].detached);
    }

    #[test]
    fn test_resolve_remote_spawn_root() {
        // cwd inside root ok
        let res = resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo/.orca-worktrees/wt-1"),
        )
        .unwrap();
        assert_eq!(res, "/srv/repo/.orca-worktrees/wt-1");

        // cwd outside (posix backslash-sibling /srv/repo\outside) rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some(r"/srv/repo\outside"),
        )
        .is_err());

        // cwd outside (/srv/repo-other/x) rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo-other/x"),
        )
        .is_err());

        // .. segment rejected
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            None,
            Some("/srv/repo/../outside"),
        )
        .is_err());
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Windows,
            r"C:\Repo",
            None,
            Some(r"C:\Repo\..\outside"),
        )
        .is_err());

        // worktree-derived path equals <root>/.ferryx-worktrees/<ws>/<slug> (posix)
        let wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "feature-1".into(),
        };
        let res_posix =
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", Some(&wt), None).unwrap();
        assert_eq!(res_posix, "/srv/repo/.ferryx-worktrees/agent/feature-1");

        // backslash variant (windows)
        let res_win =
            resolve_remote_spawn_root(RemotePlatform::Windows, r"C:\Repo", Some(&wt), None)
                .unwrap();
        assert_eq!(res_win, r"C:\Repo\.ferryx-worktrees\agent\feature-1");

        // A raw remote workspace ID maps to the same sanitized segment creation uses.
        let raw = WorktreeIdentity {
            ws_id: "ssh:0123456789abcdef".into(),
            slug: "feature-1".into(),
        };
        assert_eq!(
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", Some(&raw), None)
                .unwrap(),
            "/srv/repo/.ferryx-worktrees/ssh-0123456789ab/feature-1"
        );

        // empty slug rejected with IpcErrorCode::InvalidArgument
        let empty_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "".into(),
        };
        let err =
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", Some(&empty_wt), None)
                .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);

        // slug containing .. rejected
        let dotdot_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "../escape".into(),
        };
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&dotdot_wt),
            None,
        )
        .is_err());

        let dotdot_mid_wt = WorktreeIdentity {
            ws_id: "agent".into(),
            slug: "sub/../escape".into(),
        };
        assert!(resolve_remote_spawn_root(
            RemotePlatform::Posix,
            "/srv/repo",
            Some(&dotdot_mid_wt),
            None,
        )
        .is_err());

        // none -> repo_root
        let res_none =
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", None, None).unwrap();
        assert_eq!(res_none, "/srv/repo");

        // cwd equal to repo_root ok (posix and windows)
        let res_root_posix =
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", None, Some("/srv/repo"))
                .unwrap();
        assert_eq!(res_root_posix, "/srv/repo");

        let res_root_slash =
            resolve_remote_spawn_root(RemotePlatform::Posix, "/srv/repo", None, Some("/srv/repo/"))
                .unwrap();
        assert_eq!(res_root_slash, "/srv/repo");

        let res_root_win =
            resolve_remote_spawn_root(RemotePlatform::Windows, r"C:\Repo", None, Some(r"C:\Repo"))
                .unwrap();
        assert_eq!(res_root_win, r"C:\Repo");

        let res_root_win_ci =
            resolve_remote_spawn_root(RemotePlatform::Windows, r"C:\Repo", None, Some(r"c:\repo\"))
                .unwrap();
        assert_eq!(res_root_win_ci, r"C:\Repo");
    }
}
