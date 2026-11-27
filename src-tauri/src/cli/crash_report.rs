//! `ferryx daemon crash-report` — collect the evidence needed to symbolicate a daemon crash.
//!
//! A daemon SIGSEGV leaves `ferryx +<offset>` frames in `~/Library/Logs/DiagnosticReports/`, which
//! name no function without the dSYM built from the same binary. This command gathers that case's
//! `.ips`, the build UUIDs the crash report names, a dSYM carrying those UUIDs, and the bounded
//! daemon log into `~/.ferryx/crash/<timestamp>/`, so a later `symbolicatecrash` run has every
//! input on disk instead of requiring the reporter's machine to still hold them.
//!
//! Read-only with respect to the daemon: it never contacts, starts, or stops it.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

pub const DAEMON_USAGE: &str =
    "expected `ferryx daemon crash-report [--ips <path>] [--dsym <dir>...] [--out <dir>] [--json]`";

/// Default cap on the daemon log copied into a crash bundle.
const LOG_TAIL_BYTES: u64 = 256 * 1024;

/// `daemon.log`, oldest rotation first; the newest generation is read last.
const LOG_GENERATIONS: [&str; 4] = ["daemon.log.3", "daemon.log.2", "daemon.log.1", "daemon.log"];

/// The detached fallback daemon's own stdout/stderr sink (`cli.rs` `spawn_detached_daemon`).
///
/// A daemon started that way never writes the structured log above: the fallback opens this file
/// directly and appends to it. A crash in that path would otherwise collect no diagnostics at all.
const DETACHED_LOG_FILE: &str = "daemon.log";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    /// An explicit `.ips` path; `None` selects the newest report the OS recorded.
    pub ips: Option<PathBuf>,
    /// Extra dSYM roots (directories, or a `.dSYM` bundle) searched for a UUID match.
    pub dsym_roots: Vec<PathBuf>,
    /// Bundle directory; `None` uses `<data>/crash/<timestamp>`.
    pub out: Option<PathBuf>,
    pub json: bool,
}

pub fn parse_daemon_cli<I, T>(args: I) -> Result<CrashReport, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.get(1).map(String::as_str) != Some("daemon") {
        return Err(DAEMON_USAGE.into());
    }
    if args.get(2).map(String::as_str) != Some("crash-report") {
        return Err(DAEMON_USAGE.into());
    }
    let mut report = CrashReport {
        ips: None,
        dsym_roots: Vec::new(),
        out: None,
        json: false,
    };
    let mut rest = args.iter().skip(3);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => report.json = true,
            "--ips" | "--dsym" | "--out" => {
                let value = rest
                    .next()
                    .ok_or_else(|| format!("`{arg}` requires a value\n{DAEMON_USAGE}"))?;
                if value.is_empty() {
                    return Err(format!("`{arg}` requires a value\n{DAEMON_USAGE}"));
                }
                match arg.as_str() {
                    "--ips" => report.ips = Some(PathBuf::from(value)),
                    "--dsym" => report.dsym_roots.push(PathBuf::from(value)),
                    _ => report.out = Some(PathBuf::from(value)),
                }
            }
            other => return Err(format!("unknown argument `{other}`\n{DAEMON_USAGE}")),
        }
    }
    Ok(report)
}

pub fn run_daemon_cli(args: &[String]) -> i32 {
    let command = match parse_daemon_cli(args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let json = command.json;
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start CLI runtime: {error}");
            return 1;
        }
    };
    let outcome = runtime.block_on(crate::ipc::run_blocking(move || {
        collect(command).map_err(crate::ipc::IpcError::internal)
    }));
    match outcome {
        Ok(value) => {
            if json {
                println!("{value}");
            } else {
                let dir = value["reportDir"].as_str().unwrap_or_default();
                let crash = value["crashReport"].as_str().unwrap_or("none");
                let dsym = value["dsym"].as_str().unwrap_or("none matched");
                println!("crash bundle: {dir}");
                println!("crash report: {crash}");
                println!("dSYM: {dsym}");
                if let Some(command) = value["symbolicate"].as_str() {
                    println!("symbolicate: {command}");
                }
            }
            0
        }
        Err(error) => {
            eprintln!("{}", error.message);
            1
        }
    }
}

/// Synchronous collection; only ever invoked on a `run_blocking` worker.
fn collect(command: CrashReport) -> Result<Value, String> {
    collect_with(&command, &data_dir()?, home_dir().as_deref())
}

fn collect_with(command: &CrashReport, data_dir: &Path, home: Option<&Path>) -> Result<Value, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before the epoch: {error}"))?
        .as_secs();
    let stamp = utc_stamp(now);
    let out_dir = command
        .out
        .clone()
        .unwrap_or_else(|| data_dir.join("crash").join(&stamp));
    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("cannot create {}: {error}", out_dir.display()))?;

    let crash = locate_ips(command.ips.as_deref())?;
    let mut binary_uuids: Vec<String> = Vec::new();
    let mut copied_crash: Option<PathBuf> = None;
    if let Some(path) = &crash {
        let text = read_to_string_lossy(path);
        binary_uuids = ferryx_uuids(&text);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "crash.ips".to_string());
        let target = out_dir.join(&name);
        fs::copy(path, &target)
            .map_err(|error| format!("cannot copy {}: {error}", path.display()))?;
        copied_crash = Some(target);
    }

    let mut roots = command.dsym_roots.clone();
    for default in default_dsym_roots(data_dir, home) {
        if !roots.contains(&default) {
            roots.push(default);
        }
    }
    let mut matched: Vec<(String, PathBuf)> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in &roots {
        collect_dsym_bundles(root, 0, &mut candidates);
    }
    for candidate in &candidates {
        for uuid in dwarf_uuids(candidate) {
            if binary_uuids.contains(&uuid) {
                matched.push((uuid, candidate.clone()));
            }
        }
    }

    let log_tail = daemon_log_tail(data_dir, LOG_TAIL_BYTES);
    let log_path = out_dir.join("daemon.log");
    if !log_tail.is_empty() {
        fs::write(&log_path, log_tail.as_bytes())
            .map_err(|error| format!("cannot write {}: {error}", log_path.display()))?;
    }

    let symbolicate = copied_crash.as_ref().map(|path| {
        let dsym = matched
            .first()
            .map(|(_, dsym)| dsym.display().to_string())
            .unwrap_or_else(|| "<dSYM not found>".to_string());
        format!("symbolicatecrash {} {dsym}", path.display())
    });
    let report = json!({
        "schema": "ferryx-crash-report/v1",
        "collectedAt": stamp,
        "reportDir": out_dir.to_string_lossy(),
        "crashReport": copied_crash.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "sourceCrashReport": crash.as_ref().map(|path| path.to_string_lossy().into_owned()),
        "binaryUuids": binary_uuids,
        "dsym": matched.first().map(|(_, path)| path.to_string_lossy().into_owned()),
        "dsymUuids": matched.iter().map(|(uuid, _)| uuid.clone()).collect::<Vec<_>>(),
        "dsymCandidates": candidates
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        "daemonLog": (!log_tail.is_empty()).then(|| log_path.to_string_lossy().into_owned()),
        "symbolicate": symbolicate,
    });
    let report_path = out_dir.join("report.json");
    let rendered = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(&report_path, format!("{rendered}\n"))
        .map_err(|error| format!("cannot write {}: {error}", report_path.display()))?;
    Ok(report)
}

fn data_dir() -> Result<PathBuf, String> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .ok_or_else(|| "CRASH_REPORT_NO_DATA_DIR: set FERRYX_DATA_DIR or HOME".to_string())
}

fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .iter()
        .find_map(|key| std::env::var_os(key).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// Where a shipped dSYM archive may live on the reporting machine.
///
/// The release pipeline archives dSYMs under `<release run dir>/dsyms` (the run dirs live in
/// `~/ferryx-releases`), and an unpacked archive may sit under `<data>/dsyms`.
fn default_dsym_roots(data_dir: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = vec![data_dir.join("dsyms")];
    if let Some(home) = home {
        roots.push(home.join("ferryx-releases"));
    }
    roots
}

/// The newest `.ips` the OS recorded: a `ferryx*` report if one exists, else the newest overall.
fn locate_ips(explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    if let Some(path) = explicit {
        return explicit_ips(path).map(Some);
    }
    let Some(dir) = home_dir().map(|home| home.join("Library/Logs/DiagnosticReports")) else {
        return Err("CRASH_REPORT_NO_HOME: set HOME to locate DiagnosticReports".into());
    };
    locate_ips_in(&dir, None)
}

fn explicit_ips(path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!(
            "CRASH_REPORT_NOT_FOUND: {} is not a file",
            path.display()
        ));
    }
    Ok(path.to_path_buf())
}

/// The selection itself, against an explicit directory so tests never read the user's own reports.
fn locate_ips_in(dir: &Path, explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    if let Some(path) = explicit {
        return explicit_ips(path).map(Some);
    }
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut reports: Vec<(bool, u64, String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension() != Some(std::ffi::OsStr::new("ips")) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        reports.push((name.to_ascii_lowercase().starts_with("ferryx"), modified, name, path));
    }
    // A ferryx report wins even when another app crashed more recently: the other one cannot
    // symbolicate this build.
    reports.sort_by(|a, b| (b.0, b.1, &b.2).cmp(&(a.0, a.1, &a.2)));
    Ok(reports.into_iter().next().map(|(_, _, _, path)| path))
}

fn read_to_string_lossy(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// Build UUIDs of the report's own images, in report order, deduplicated.
///
/// A `.ips` is a one-line JSON header followed by either a JSON body (`usedImages`) or a legacy
/// text body with a `Binary Images:` section. Both shapes are read; only images naming `ferryx`
/// are kept, because a dSYM can only match the crashed binary itself.
pub fn ferryx_uuids(text: &str) -> Vec<String> {
    let mut uuids: Vec<String> = Vec::new();
    // The body is the JSON after the one-line header; a report written on a single line is read as
    // a whole document instead.
    let payload = text.split_once('\n').map_or(text, |(_, body)| body);
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        if let Some(images) = value.get("usedImages").and_then(Value::as_array) {
            for image in images {
                let name = image
                    .get("name")
                    .or_else(|| image.get("path"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !name.to_ascii_lowercase().contains("ferryx") {
                    continue;
                }
                if let Some(uuid) = image
                    .get("uuid")
                    .and_then(Value::as_str)
                    .and_then(normalize_uuid)
                {
                    push_unique(&mut uuids, uuid);
                }
            }
        }
    }
    if !uuids.is_empty() {
        return uuids;
    }
    for line in text.lines() {
        if !line.to_ascii_lowercase().contains("ferryx") {
            continue;
        }
        if let Some(uuid) = angle_uuid(line) {
            push_unique(&mut uuids, uuid);
        }
    }
    uuids
}

fn push_unique(uuids: &mut Vec<String>, uuid: String) {
    if !uuids.contains(&uuid) {
        uuids.push(uuid);
    }
}

/// The `<UUID>` token of a legacy `Binary Images:` line, normalized.
fn angle_uuid(line: &str) -> Option<String> {
    let open = line.find('<')?;
    let rest = &line[open + 1..];
    let close = rest.find('>')?;
    normalize_uuid(&rest[..close])
}

/// `883bc83e-b8fb-373e-9a6c-14c1ff47a2f6` -> `883BC83E-B8FB-373E-9A6C-14C1FF47A2F6`; also accepts
/// the undashed 32-hex form. Anything else is not a UUID.
pub fn normalize_uuid(raw: &str) -> Option<String> {
    let hex: String = raw
        .chars()
        .filter(|ch| *ch != '-' && *ch != ' ' && *ch != '\t')
        .collect();
    if hex.len() != 32 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let hex = hex.to_ascii_uppercase();
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// `dwarfdump --uuid <path>` output -> UUIDs, normalized. Missing tool or a bad path reads empty.
pub fn dwarf_uuids(path: &Path) -> Vec<String> {
    uuids_from_dwarfdump(&run_dwarfdump(path))
}

pub fn uuids_from_dwarfdump(text: &str) -> Vec<String> {
    let mut uuids = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("UUID:") else {
            continue;
        };
        let token = rest.split_whitespace().next().unwrap_or_default();
        if let Some(uuid) = normalize_uuid(token) {
            push_unique(&mut uuids, uuid);
        }
    }
    uuids
}

fn run_dwarfdump(path: &Path) -> String {
    let output = Command::new("dwarfdump").arg("--uuid").arg(path).output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

/// Recursively collects `*.dSYM` bundles, without following directory symlinks.
fn collect_dsym_bundles(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 || !dir.is_dir() {
        return;
    }
    if dir.extension() == Some(std::ffi::OsStr::new("dSYM")) {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_dsym_bundles(&path, depth + 1, out);
        }
    }
}

/// The last `limit` bytes of the daemon log across its rotations, or empty when none exists.
///
/// Two sinks are read, because the daemon has two: the structured log lives in `<data>/logs`, while
/// the detached fallback daemon appends its stdout/stderr to `<data>/daemon.log`. Reading only the
/// first made a detached-path crash collect an empty tail, which is the case a bug report is least
/// likely to carry by hand.
fn daemon_log_tail(data_dir: &Path, limit: u64) -> String {
    let logs = data_dir.join("logs");
    // Newest file first, reading only the trailing bytes still needed, then
    // reassembled oldest-first so the output order is unchanged.
    let mut paths: Vec<PathBuf> = LOG_GENERATIONS.iter().map(|name| logs.join(name)).collect();
    paths.push(data_dir.join(DETACHED_LOG_FILE));
    let mut segments: Vec<Vec<u8>> = Vec::new();
    let mut remaining = limit;
    for path in paths.iter().rev() {
        if remaining == 0 {
            break;
        }
        if let Some(segment) = read_file_tail(path, remaining) {
            remaining -= segment.len() as u64;
            segments.push(segment);
        }
    }
    segments.reverse();
    String::from_utf8_lossy(&segments.concat()).into_owned()
}

/// The last `max` bytes of a file, or `None` when it cannot be read.
fn read_file_tail(path: &Path, max: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut buf = Vec::new();
    file.take(max).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// `20261116T085037Z` for the given Unix second, so bundle names sort chronologically.
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 -> (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn parses_crash_report_flags_and_rejects_other_shapes() {
        assert_eq!(
            parse_daemon_cli(argv(&["ferryx", "daemon", "crash-report"])),
            Ok(CrashReport { ips: None, dsym_roots: Vec::new(), out: None, json: false })
        );
        assert_eq!(
            parse_daemon_cli(argv(&[
                "ferryx", "daemon", "crash-report", "--json", "--ips", "/tmp/a.ips", "--dsym", "/tmp/d1", "--dsym", "/tmp/d2",
                "--out", "/tmp/bundle",
            ])),
            Ok(CrashReport {
                ips: Some(PathBuf::from("/tmp/a.ips")),
                dsym_roots: vec![PathBuf::from("/tmp/d1"), PathBuf::from("/tmp/d2")],
                out: Some(PathBuf::from("/tmp/bundle")),
                json: true,
            })
        );
        assert!(parse_daemon_cli(argv(&["ferryx", "daemon"])).is_err());
        assert!(parse_daemon_cli(argv(&["ferryx", "daemon", "restart"])).is_err());
        assert!(parse_daemon_cli(argv(&["ferryx", "daemon", "crash-report", "--ips"])).is_err());
        assert!(parse_daemon_cli(argv(&["ferryx", "daemon", "crash-report", "--bogus"])).is_err());
    }

    #[test]
    fn reads_the_uuids_a_modern_report_names_for_ferryx() {
        let text = concat!(
            "{\"app_name\":\"ferryx\",\"bug_type\":\"309\",\"timestamp\":\"2026-11-15 08:50:37.00 +0900\"}\n",
            "{\"usedImages\":[",
            "{\"name\":\"libsystem_kernel.dylib\",\"uuid\":\"11111111-1111-1111-1111-111111111111\"},",
            "{\"name\":\"ferryx\",\"uuid\":\"883bc83e-b8fb-373e-9a6c-14c1ff47a2f6\"},",
            "{\"path\":\"/Applications/Ferryx.app/Contents/MacOS/ferryx\",\"uuid\":\"883bc83eb8fb373e9a6c14c1ff47a2f6\"}]}"
        );
        // The binary is named twice; the same UUID is reported once.
        assert_eq!(ferryx_uuids(text), vec!["883BC83E-B8FB-373E-9A6C-14C1FF47A2F6".to_string()]);
    }

    #[test]
    fn reads_a_single_line_report_that_has_no_header_newline() {
        // Older reports are one JSON document with no separate header line; the whole text is the body.
        let text = concat!(
            "{\"usedImages\":[",
            "{\"name\":\"ferryx\",\"uuid\":\"883bc83e-b8fb-373e-9a6c-14c1ff47a2f6\"}]}"
        );
        assert_eq!(ferryx_uuids(text), vec!["883BC83E-B8FB-373E-9A6C-14C1FF47A2F6".to_string()]);
    }

    #[test]
    fn falls_back_to_the_legacy_binary_images_section() {
        let text = concat!(
            "{\"app_name\":\"ferryx\",\"bug_type\":\"309\"}\n",
            "Binary Images:\n",
            "       0x100000000 - 0x1009ffff +ferryx (2026.1113.2) <4a1b2c3d-0000-1111-2222-333344445555> /Applications/Ferryx.app/Contents/MacOS/ferryx\n",
            "       0x180000000 - 0x180ffffff  libsystem_kernel.dylib ( ) <99999999-0000-1111-2222-333344445555> /usr/lib/system/libsystem_kernel.dylib\n"
        );
        assert_eq!(ferryx_uuids(text), vec!["4A1B2C3D-0000-1111-2222-333344445555".to_string()]);
    }

    #[test]
    fn normalizes_uuids_and_refuses_anything_else() {
        assert_eq!(
            normalize_uuid("883bc83e-b8fb-373e-9a6c-14c1ff47a2f6").as_deref(),
            Some("883BC83E-B8FB-373E-9A6C-14C1FF47A2F6")
        );
        assert_eq!(
            normalize_uuid("883BC83EB8FB373E9A6C14C1FF47A2F6").as_deref(),
            Some("883BC83E-B8FB-373E-9A6C-14C1FF47A2F6")
        );
        assert_eq!(normalize_uuid("not-a-uuid"), None);
        assert_eq!(normalize_uuid("883BC83E-B8FB-373E-9A6C"), None);
    }

    #[test]
    fn parses_dwarfdump_output_for_both_architectures() {
        let text = concat!(
            "UUID: 8426AD72-1580-30BD-B8A2-0A05B4672A9F (x86_64) /tmp/a.dSYM/Contents/Resources/DWARF/ferryx\n",
            "UUID: 883BC83E-B8FB-373E-9A6C-14C1FF47A2F6 (arm64e) /tmp/a.dSYM/Contents/Resources/DWARF/ferryx\n",
            "warning: no debug symbols in executable\n"
        );
        assert_eq!(
            uuids_from_dwarfdump(text),
            vec![
                "8426AD72-1580-30BD-B8A2-0A05B4672A9F".to_string(),
                "883BC83E-B8FB-373E-9A6C-14C1FF47A2F6".to_string(),
            ]
        );
    }

    #[test]
    fn stamps_are_sortable_and_utc() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        // 2026-11-15T08:50:37Z, the timestamp of the daemon SIGSEGV this phase exists for.
        assert_eq!(utc_stamp(1_794_732_637), "20261115T085037Z".to_string());
        assert_eq!(utc_stamp(1_794_732_637).len(), 16);
    }

    #[test]
    fn log_tail_keeps_the_newest_bytes_across_rotations() {
        let root = tempfile::tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(logs.join("daemon.log.1"), b"old-record\n").unwrap();
        fs::write(logs.join("daemon.log"), b"new-record\n").unwrap();
        assert_eq!(daemon_log_tail(root.path(), 4096), "old-record\nnew-record\n");
        assert_eq!(daemon_log_tail(root.path(), 11), "new-record\n");
        assert_eq!(daemon_log_tail(&root.path().join("absent"), 11), "");
    }

    #[test]
    fn log_tail_reads_the_detached_fallback_daemons_stdout_log() {
        // `spawn_detached_daemon` appends its stdout/stderr to `<data>/daemon.log`, never to
        // `<data>/logs`. A crash in that path used to collect a tail with none of its diagnostics.
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("logs")).unwrap();
        fs::write(root.path().join("logs/daemon.log"), b"structured\n").unwrap();
        fs::write(root.path().join("daemon.log"), b"detached-panic\n").unwrap();
        assert_eq!(daemon_log_tail(root.path(), 4096), "structured\ndetached-panic\n");
        // Detached-only: the fallback is the whole log, so it must not read as empty.
        let detached = tempfile::tempdir().unwrap();
        fs::write(detached.path().join("daemon.log"), b"detached-only\n").unwrap();
        assert_eq!(daemon_log_tail(detached.path(), 4096), "detached-only\n");
        // The cap still applies to the combined tail, newest bytes last.
        assert_eq!(daemon_log_tail(root.path(), 9), "ed-panic\n");
    }

    #[test]
    fn collects_the_newest_ferryx_report_from_a_diagnostic_reports_dir() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Library/Logs/DiagnosticReports");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Safari-2026-11-16.ips"), b"{}\n{}").unwrap();
        fs::write(dir.join("ferryx-2026-11-15-085037.ips"), b"{}\n{}").unwrap();
        let found = locate_ips_in(&dir, None).unwrap().unwrap();
        assert_eq!(
            found.file_name().map(|name| name.to_string_lossy().into_owned()).as_deref(),
            Some("ferryx-2026-11-15-085037.ips")
        );
        assert!(locate_ips_in(&dir, Some(&dir.join("absent.ips"))).is_err());
    }

    #[test]
    fn collect_writes_a_bundle_that_names_the_crash_and_lifts_its_log() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(data.join("logs")).unwrap();
        fs::write(data.join("logs/daemon.log"), b"structured-record\n").unwrap();
        fs::write(data.join("daemon.log"), b"detached-panic\n").unwrap();
        let crash = root.path().join("ferryx-2026-11-16-101500.ips");
        fs::write(
            &crash,
            concat!(
                "{\"app_name\":\"ferryx\",\"bug_type\":\"309\"}\n",
                "{\"usedImages\":[{\"name\":\"ferryx\",\"uuid\":\"883bc83e-b8fb-373e-9a6c-14c1ff47a2f6\"}]}"
            ),
        )
        .unwrap();
        let no_dsyms = root.path().join("no-dsyms");
        fs::create_dir_all(&no_dsyms).unwrap();
        let out = root.path().join("bundle");
        let command = CrashReport {
            ips: Some(crash.clone()),
            dsym_roots: vec![no_dsyms],
            out: Some(out.clone()),
            json: true,
        };

        let report = collect_with(&command, &data, None).unwrap();

        assert_eq!(report["schema"], "ferryx-crash-report/v1");
        assert_eq!(report["reportDir"].as_str(), Some(out.to_string_lossy().as_ref()));
        assert_eq!(report["binaryUuids"], json!(["883BC83E-B8FB-373E-9A6C-14C1FF47A2F6"]));
        let copied = out.join("ferryx-2026-11-16-101500.ips");
        assert_eq!(fs::read(&copied).unwrap(), fs::read(&crash).unwrap());
        assert_eq!(report["crashReport"].as_str(), Some(copied.to_string_lossy().as_ref()));
        let tail = fs::read_to_string(out.join("daemon.log")).unwrap();
        assert!(tail.contains("structured-record"));
        assert!(tail.contains("detached-panic"));
        assert_eq!(
            report["daemonLog"].as_str(),
            Some(out.join("daemon.log").to_string_lossy().as_ref())
        );
        let written: Value = serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
        assert_eq!(written["schema"], report["schema"]);
        assert_eq!(written["binaryUuids"], report["binaryUuids"]);
    }

    #[test]
    fn daemon_log_tail_reads_only_the_trailing_bytes_in_order() {
        let dir = std::env::temp_dir().join(format!("ferryx-tail-{}-{}", std::process::id(), line!()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("logs")).unwrap();
        fs::write(dir.join("logs").join("daemon.log.1"), "old-old-old\n").unwrap();
        let mut big = vec![b'x'; 1_000_000];
        big.extend_from_slice(b"TAIL-END");
        fs::write(dir.join(DETACHED_LOG_FILE), &big).unwrap();
        let tail = daemon_log_tail(&dir, 8);
        assert_eq!(tail, "TAIL-END");
        let tail = daemon_log_tail(&dir, 12);
        assert_eq!(tail, "xxxxTAIL-END");
        let all = daemon_log_tail(&dir, 10_000_000);
        assert!(all.starts_with("old-old-old\n") && all.ends_with("TAIL-END"));
        let _ = fs::remove_dir_all(&dir);
    }
}
