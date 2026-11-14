//! Machine-wide resource sample for an SSH host, which has no remote gateway to ask.

use std::time::Duration;

use super::runtime::{parse_fields, RemoteEnvironment, RemotePlatform};
use super::{direct, SshHost};
use crate::daemon::resource_usage::HostResourceSnapshot;
use crate::ipc::IpcError;

const PROBE_DEADLINE: Duration = Duration::from_secs(12);
const FIELD_COUNT: usize = 9;

pub async fn sample(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> Result<HostResourceSnapshot, IpcError> {
    let marker = format!("FERRYX_RESOURCES_V1_{}", uuid::Uuid::new_v4().simple());
    let script = probe_script(environment.platform, &marker);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, PROBE_DEADLINE)
        .await
        .map_err(|mut err| {
            let details = err.details.get_or_insert_with(|| serde_json::json!({}));
            details["stage"] = "resources".into();
            err
        })?;
    let fields = parse_fields(&output, &marker, FIELD_COUNT)?;
    Ok(snapshot_from_fields(environment.platform, &fields))
}

fn probe_script(platform: RemotePlatform, marker: &str) -> String {
    match platform {
        RemotePlatform::Posix => format!(
            concat!(
                "cpu=$( {{ getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null; }} | head -n1 ); \
                 load=$( awk '{{print $1}}' /proc/loadavg 2>/dev/null || sysctl -n vm.loadavg 2>/dev/null | awk '{{print $2}}' 2>/dev/null ); ",
                "mem_total=$( {{ awk '/^MemTotal:/ {{print $2*1024; exit}}' /proc/meminfo 2>/dev/null || \
                     sysctl -n hw.memsize 2>/dev/null; }} | head -n1 ); ",
                "mem_avail=$( awk '/^MemAvailable:/ {{print $2*1024; exit}}' /proc/meminfo 2>/dev/null ); ",
                "swap_total=$( {{ awk '/^SwapTotal:/ {{print $2*1024; exit}}' /proc/meminfo 2>/dev/null || \
                     sysctl -n vm.swapusage 2>/dev/null | sed -n 's/.*total = \\([0-9.]*\\)M.*/\\1/p' | \
                     awk '{{if ($1 != \"\") printf \"%.0f\", $1*1048576}}'; }} | head -n1 ); ",
                "swap_used=$( {{ awk '/^SwapFree:/ {{print $2*1024; exit}}' /proc/meminfo 2>/dev/null || \
                     sysctl -n vm.swapusage 2>/dev/null | sed -n 's/.*used = \\([0-9.]*\\)M.*/\\1/p' | \
                     awk '{{if ($1 != \"\") printf \"%.0f\", $1*1048576}}'; }} | head -n1 ); ",
                "procs=$( ps -A 2>/dev/null | wc -l | tr -d ' ' ); ",
                "disk=$( df -kP . 2>/dev/null | awk 'NR==2 {{print $2*1024\" \"$4*1024}}' ); ",
                "printf '{}\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000' \
                 \"$cpu\" \"$load\" \"$mem_total\" \"$mem_avail\" \"$swap_total\" \"$swap_used\" \"$procs\" \"$disk\" \"posix\""
            ),
            marker
        ),
        RemotePlatform::Windows => format!(
            concat!(
                "$cpu=''; $load=''; $mt=''; $ma=''; $st=''; $su=''; $pc=''; $disk=''; ",
                "try {{ $cpu=[string][Environment]::ProcessorCount }} catch {{}}; ",
                "try {{ $ma=[string][math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory*1024) }} catch {{}}; ",
                "try {{ $mt=[string][math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory) }} catch {{}}; ",
                "try {{
                     $page=(Get-CimInstance Win32_PageFileUsage -ErrorAction Stop | Measure-Object -Property AllocatedBaseSize -Sum).Sum;
                     if ($null -ne $page) {{ $st=[string]([double]$page*1048576) }}
                 }} catch {{}}; ",
                "try {{ $pc=[string](@(Get-Process).Count) }} catch {{}}; ",
                "try {{
                     $d=Get-CimInstance Win32_LogicalDisk -Filter \"DriveType=3\" -ErrorAction Stop |
                        Sort-Object -Property FreeSpace -Descending | Select-Object -First 1;
                     if ($d) {{ $disk=('' + [math]::Round($d.Size) + ' ' + [math]::Round($d.FreeSpace)) }}
                 }} catch {{}}; ",
                "[Console]::Write(('{}',$cpu,$load,$mt,$ma,$st,$su,$pc,$disk,'windows','' -join [char]0))"
            ),
            marker
        ),
    }
}

fn snapshot_from_fields(platform: RemotePlatform, fields: &[&str]) -> HostResourceSnapshot {
    let cpu_count = parse_u32(fields[0]);
    let load_average_1m = parse_f32(fields[1]);
    let memory_total_bytes = parse_u64(fields[2]);
    let memory_available_bytes = parse_u64(fields[3]);
    let swap_total_bytes = parse_u64(fields[4]);
    let swap_used_bytes = parse_u64(fields[5]);
    let process_count = parse_u32(fields[6]);
    let disk = parse_disk(fields[7]);

    let memory_used_bytes = match (memory_total_bytes, memory_available_bytes) {
        (Some(total), Some(available)) => Some(total.saturating_sub(available)),
        _ => None,
    };

    let mut unavailable = Vec::new();
    let missing = [
        ("cpuCount", cpu_count.is_none()),
        ("loadAverage1m", load_average_1m.is_none()),
        ("memoryTotalBytes", memory_total_bytes.is_none()),
        ("memoryUsedBytes", memory_used_bytes.is_none()),
        ("swapTotalBytes", swap_total_bytes.is_none()),
        ("swapUsedBytes", swap_used_bytes.is_none()),
        ("processCount", process_count.is_none()),
        // The daemon's own sampler names the pair once, so both sources report the same metric.
        ("disk", disk.0.is_none() || disk.1.is_none()),
    ];
    for (name, absent) in missing {
        if absent {
            unavailable.push(name.to_string());
        }
    }
    unavailable.push("cpuUtilization".to_string());
    unavailable.push("uptimeSeconds".to_string());

    HostResourceSnapshot {
        sampled_at_ms: now_millis(),
        platform: platform_name(platform).to_string(),
        cpu_count: cpu_count.unwrap_or(0),
        cpu_utilization: None,
        load_average_1m,
        memory_total_bytes,
        memory_used_bytes,
        swap_total_bytes,
        swap_used_bytes,
        uptime_seconds: None,
        disk_total_bytes: disk.0,
        disk_free_bytes: disk.1,
        process_count,
        sessions: Vec::new(),
        unavailable,
    }
}

fn platform_name(platform: RemotePlatform) -> &'static str {
    match platform {
        RemotePlatform::Posix => "posix",
        RemotePlatform::Windows => "windows",
    }
}

fn parse_u64(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v.round() as u64)
}

fn parse_u32(raw: &str) -> Option<u32> {
    parse_u64(raw).and_then(|v| u32::try_from(v).ok())
}

fn parse_f32(raw: &str) -> Option<f32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn parse_disk(raw: &str) -> (Option<u64>, Option<u64>) {
    let mut parts = raw.split_whitespace();
    let total = parts.next().and_then(parse_u64);
    let free = parts.next().and_then(parse_u64);
    (total, free)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posix_fields() -> Vec<&'static str> {
        vec![
            "10",
            "1.25",
            "17179869184",
            "8589934592",
            "2147483648",
            "1073741824",
            "412",
            "500000 250000",
            "posix",
        ]
    }

    #[test]
    fn a_complete_probe_reports_every_metric_it_can_prove() {
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &posix_fields());
        assert_eq!(snapshot.cpu_count, 10);
        assert_eq!(snapshot.load_average_1m, Some(1.25));
        assert_eq!(snapshot.memory_total_bytes, Some(17_179_869_184));
        assert_eq!(
            snapshot.memory_used_bytes,
            Some(17_179_869_184 - 8_589_934_592)
        );
        assert_eq!(snapshot.swap_total_bytes, Some(2_147_483_648));
        assert_eq!(snapshot.swap_used_bytes, Some(1_073_741_824));
        assert_eq!(snapshot.process_count, Some(412));
        assert_eq!(snapshot.disk_total_bytes, Some(500_000));
        assert_eq!(snapshot.disk_free_bytes, Some(250_000));
        assert_eq!(snapshot.platform, "posix");
        assert!(snapshot.sessions.is_empty());
    }

    #[test]
    fn a_single_probe_names_the_metrics_it_cannot_measure() {
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &posix_fields());
        assert_eq!(snapshot.cpu_utilization, None);
        assert_eq!(snapshot.uptime_seconds, None);
        assert!(snapshot.unavailable.contains(&"cpuUtilization".to_string()));
        assert!(snapshot.unavailable.contains(&"uptimeSeconds".to_string()));
        for measured in [
            "cpuCount",
            "loadAverage1m",
            "memoryTotalBytes",
            "memoryUsedBytes",
            "swapTotalBytes",
            "swapUsedBytes",
            "processCount",
            "disk",
        ] {
            assert!(
                !snapshot.unavailable.contains(&measured.to_string()),
                "{measured} was read but reported as unavailable"
            );
        }
    }

    #[test]
    fn an_empty_field_is_absent_and_named_rather_than_zero() {
        let fields = vec!["", "", "", "", "", "", "", "", "posix"];
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &fields);
        assert_eq!(
            snapshot.cpu_count, 0,
            "a missing processor count is not one core"
        );
        assert_eq!(snapshot.load_average_1m, None);
        assert_eq!(snapshot.memory_total_bytes, None);
        assert_eq!(snapshot.memory_used_bytes, None);
        assert_eq!(snapshot.swap_total_bytes, None);
        assert_eq!(snapshot.swap_used_bytes, None);
        assert_eq!(snapshot.process_count, None);
        assert_eq!(snapshot.disk_total_bytes, None);
        assert_eq!(snapshot.disk_free_bytes, None);
        for name in [
            "cpuCount",
            "loadAverage1m",
            "memoryTotalBytes",
            "memoryUsedBytes",
            "swapTotalBytes",
            "swapUsedBytes",
            "processCount",
            "disk",
        ] {
            assert!(
                snapshot.unavailable.contains(&name.to_string()),
                "{name} was not named missing"
            );
        }
    }

    #[test]
    fn memory_without_an_available_figure_does_not_invent_a_used_figure() {
        let fields = vec!["8", "", "8589934592", "", "", "", "", "", "posix"];
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &fields);
        assert_eq!(snapshot.memory_total_bytes, Some(8_589_934_592));
        assert_eq!(snapshot.memory_used_bytes, None);
        assert!(snapshot
            .unavailable
            .contains(&"memoryUsedBytes".to_string()));
    }

    #[test]
    fn swap_reports_each_half_independently() {
        let fields = vec!["4", "", "", "", "2147483648", "", "100", "", "posix"];
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &fields);
        assert_eq!(snapshot.swap_total_bytes, Some(2_147_483_648));
        assert_eq!(snapshot.swap_used_bytes, None);
        assert!(snapshot.unavailable.contains(&"swapUsedBytes".to_string()));
        assert!(!snapshot.unavailable.contains(&"swapTotalBytes".to_string()));
    }

    #[test]
    fn a_disk_pair_missing_its_free_half_still_reports_the_total() {
        let fields = vec!["2", "", "", "", "", "", "", "1000", "posix"];
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &fields);
        assert_eq!(snapshot.disk_total_bytes, Some(1000));
        assert_eq!(snapshot.disk_free_bytes, None);
        // One half of the pair is unusable, and the metric is named the same way by both samplers.
        assert!(snapshot.unavailable.contains(&"disk".to_string()));
    }

    #[test]
    fn windows_platform_is_reported_as_windows() {
        let fields = vec!["8", "", "", "", "", "", "", "", "windows"];
        assert_eq!(
            snapshot_from_fields(RemotePlatform::Windows, &fields).platform,
            "windows"
        );
    }

    #[test]
    fn garbage_numbers_are_treated_as_absent() {
        let fields = vec![
            "abc", "NaN", "12abc", "-", "Infinity", "x", "y", "z", "posix",
        ];
        let snapshot = snapshot_from_fields(RemotePlatform::Posix, &fields);
        assert_eq!(snapshot.cpu_count, 0);
        assert_eq!(snapshot.load_average_1m, None);
        assert_eq!(snapshot.memory_total_bytes, None);
        assert_eq!(snapshot.swap_total_bytes, None);
        assert_eq!(snapshot.process_count, None);
        assert_eq!(snapshot.disk_total_bytes, None);
    }

    #[test]
    fn the_posix_script_emits_the_marker_and_every_field() {
        let script = probe_script(RemotePlatform::Posix, "MARKER_X");
        assert!(script.contains("MARKER_X"));
        assert!(
            script.contains("\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000")
        );
        assert!(script.contains("getconf _NPROCESSORS_ONLN"));
        assert!(script.contains("/proc/meminfo"));
        assert!(script.contains("hw.memsize"));
    }

    #[test]
    fn the_windows_script_emits_the_marker_and_every_field() {
        let script = probe_script(RemotePlatform::Windows, "MARKER_W");
        assert!(script.contains("MARKER_W"));
        assert!(script.contains("Environment]::ProcessorCount"));
        // `parse_fields` reads count+1 fields and requires the last one empty, so the join must
        // carry a trailing separator: without it the frame is rejected as malformed.
        assert!(
            script.contains("'windows','' -join [char]0)"),
            "the Windows frame needs its trailing empty field: {script}"
        );
    }

    #[test]
    fn every_field_the_probes_emit_lines_up_with_the_declared_count() {
        // `parse_fields` wants FIELD_COUNT values after the marker plus a trailing empty one, so
        // a probe that drifts from FIELD_COUNT would fail every sample at parse time instead.
        assert_eq!(
            probe_script(RemotePlatform::Posix, "M")
                .matches("%s")
                .count(),
            FIELD_COUNT
        );

        let windows = probe_script(RemotePlatform::Windows, "M");
        let start = windows
            .find("::Write((")
            .expect("the Windows script writes a frame")
            + "::Write((".len();
        let frame = &windows[start..];
        let frame = &frame[..frame
            .find("-join")
            .expect("the Windows frame is null-joined")];
        // marker + FIELD_COUNT values + the trailing empty field.
        assert_eq!(
            frame.matches(',').count() + 1,
            FIELD_COUNT + 2,
            "frame: {frame}"
        );
    }
}
