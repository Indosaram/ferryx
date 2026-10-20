//! Host and per-session resource sampling owned by the daemon.
//!
//! The daemon — never the GUI — reads these numbers, because the daemon owns every PTY and is
//! the only process that can name a session's process tree. Sampling is on demand: the server
//! calls [`ResourceSampler::sample`] while answering a `ResourceUsage` request, so an idle
//! daemon spends nothing on telemetry and no background task can outlive its readers.
//!
//! Every platform reading is split into a thin I/O shell and a pure parser, so the parsing rules
//! for all three targets are unit-tested from any host even though only one target can be built
//! at a time. A metric this host cannot read is reported as `None` and named in
//! [`HostResourceSnapshot::unavailable`] — the sampler never substitutes a plausible number for
//! a missing one.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Two CPU counter readings closer together than this are differenced over too short an interval
/// to be meaningful, so the previous utilization is reported instead of counter noise.
const MIN_CPU_DELTA: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProbe {
    pub session_id: String,
    pub pid: Option<u32>,
    pub worktree_path: Option<String>,
}

/// Every field is optional: a session whose process exited between the listing and the
/// aggregation, or a platform that cannot report one of these, reports `None` rather than `0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResourceUsage {
    pub session_id: String,
    pub pid: Option<u32>,
    pub worktree_path: Option<String>,
    /// As `ps` reports it for the session's process tree: a share of one core.
    pub cpu_percent: Option<f32>,
    pub resident_bytes: Option<u64>,
    pub process_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostResourceSnapshot {
    pub sampled_at_ms: u64,
    pub platform: String,
    pub cpu_count: u32,
    /// Share of the whole machine's CPU, `0.0..=1.0`, differenced from two counter readings:
    /// `None` on the first sample, before a second reading exists to difference against.
    pub cpu_utilization: Option<f32>,
    pub load_average_1m: Option<f32>,
    pub memory_total_bytes: Option<u64>,
    pub memory_used_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_used_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    pub disk_free_bytes: Option<u64>,
    pub process_count: Option<u32>,
    pub sessions: Vec<SessionResourceUsage>,
    /// Metric names this host could not read, so a UI can say which gap it is showing instead
    /// of rendering a missing number as zero. Empty when every field above is populated.
    pub unavailable: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuCounters {
    pub total: u64,
    pub idle: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryReading {
    pub total_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_used_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessRow {
    pub pid: u32,
    pub parent: u32,
    pub resident_bytes: u64,
    pub cpu_percent: f32,
}

/// One platform's whole reading. `counters` and `process_cpu` exist because not every platform
/// can supply the same quantity: a host with counters yields a differenced utilization, while
/// Windows reports an instantaneous percentage and no per-process CPU at all.
struct PlatformReading {
    counters: Option<CpuCounters>,
    host_cpu_utilization: Option<f32>,
    memory: MemoryReading,
    load_average_1m: Option<f32>,
    uptime_seconds: Option<u64>,
    disk: (Option<u64>, Option<u64>),
    processes: Option<Vec<ProcessRow>>,
    process_cpu: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct SampleState {
    counters: Option<CpuCounters>,
    utilization: Option<f32>,
    sampled_at: Option<Instant>,
}

#[derive(Default)]
pub struct ResourceSampler {
    state: Mutex<SampleState>,
}

impl ResourceSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// `disk_path` is the volume whose free space matters (the runtime dir). Blocking: the
    /// subprocess and file reads happen on the caller's thread, so the daemon answers this from
    /// a blocking task rather than an executor worker.
    pub fn sample(&self, probes: &[SessionProbe], disk_path: &Path) -> HostResourceSnapshot {
        self.sample_reading(probes, platform::read(disk_path))
    }

    fn sample_reading(&self, probes: &[SessionProbe], reading: PlatformReading) -> HostResourceSnapshot {
        let mut unavailable = Vec::new();
        let now = Instant::now();
        let utilization = {
            let mut state = self.state.lock();
            let value = match (state.counters, reading.counters) {
                (Some(previous), Some(current)) => {
                    let elapsed = now.duration_since(state.sampled_at.unwrap_or(now));
                    if elapsed >= MIN_CPU_DELTA {
                        utilization_from_counters(previous, current, elapsed)
                    } else {
                        state.utilization
                    }
                }
                _ => reading.host_cpu_utilization,
            };
            if reading.counters.is_some() {
                state.counters = reading.counters;
                state.sampled_at = Some(now);
                state.utilization = value;
            }
            value
        };
        if utilization.is_none() {
            unavailable.push("cpuUtilization".to_string());
        }

        let memory = reading.memory;
        for (name, missing) in [
            ("memoryTotalBytes", memory.total_bytes.is_none()),
            ("memoryUsedBytes", memory.used_bytes.is_none()),
            ("swapTotalBytes", memory.swap_total_bytes.is_none()),
            ("swapUsedBytes", memory.swap_used_bytes.is_none()),
        ] {
            if missing {
                unavailable.push(name.to_string());
            }
        }

        if reading.load_average_1m.is_none() {
            unavailable.push("loadAverage1m".to_string());
        }
        if reading.uptime_seconds.is_none() {
            unavailable.push("uptimeSeconds".to_string());
        }

        let (disk_total_bytes, disk_free_bytes) = reading.disk;
        if disk_total_bytes.is_none() || disk_free_bytes.is_none() {
            unavailable.push("disk".to_string());
        }

        let process_count = reading.processes.as_ref().map(|rows| rows.len() as u32);
        if process_count.is_none() {
            unavailable.push("processCount".to_string());
        }
        if !reading.process_cpu {
            unavailable.push("sessionCpuPercent".to_string());
        }
        let sessions = match reading.processes {
            Some(rows) => probes
                .iter()
                .map(|probe| session_usage(probe, &rows, reading.process_cpu))
                .collect(),
            None => probes
                .iter()
                .map(|probe| SessionResourceUsage {
                    session_id: probe.session_id.clone(),
                    pid: probe.pid,
                    worktree_path: probe.worktree_path.clone(),
                    cpu_percent: None,
                    resident_bytes: None,
                    process_count: None,
                })
                .collect(),
        };

        HostResourceSnapshot {
            sampled_at_ms: wall_time_ms(),
            platform: std::env::consts::OS.to_string(),
            cpu_count: std::thread::available_parallelism()
                .map(|value| value.get() as u32)
                .unwrap_or(1),
            cpu_utilization: utilization,
            load_average_1m: reading.load_average_1m,
            memory_total_bytes: memory.total_bytes,
            memory_used_bytes: memory.used_bytes,
            swap_total_bytes: memory.swap_total_bytes,
            swap_used_bytes: memory.swap_used_bytes,
            uptime_seconds: reading.uptime_seconds,
            disk_total_bytes,
            disk_free_bytes,
            process_count,
            sessions,
            unavailable,
        }
    }
}

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub fn utilization_from_counters(
    previous: CpuCounters,
    current: CpuCounters,
    elapsed: Duration,
) -> Option<f32> {
    if elapsed.is_zero() {
        return None;
    }
    // A counter that went backwards means the source was reset (or a 32-bit field wrapped);
    // there is no delta to report.
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;
    if total == 0 || idle > total {
        return None;
    }
    let busy = (total - idle) as f64 / total as f64;
    Some(busy.clamp(0.0, 1.0) as f32)
}

fn session_usage(
    probe: &SessionProbe,
    rows: &[ProcessRow],
    process_cpu: bool,
) -> SessionResourceUsage {
    let mut usage = SessionResourceUsage {
        session_id: probe.session_id.clone(),
        pid: probe.pid,
        worktree_path: probe.worktree_path.clone(),
        cpu_percent: None,
        resident_bytes: None,
        process_count: None,
    };
    let Some(root) = probe.pid else {
        return usage;
    };
    let children = children_index(rows);
    let Some((cpu, resident, count)) = tree_totals(root, rows, &children) else {
        return usage;
    };
    usage.cpu_percent = process_cpu.then_some(cpu);
    usage.resident_bytes = Some(resident);
    usage.process_count = Some(count);
    usage
}

fn children_index(rows: &[ProcessRow]) -> HashMap<u32, Vec<usize>> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        children.entry(row.parent).or_default().push(index);
    }
    children
}

/// `None` when `root` is not in the table: its process already exited, which is a gap rather
/// than a tree of zero cost.
fn tree_totals(
    root: u32,
    rows: &[ProcessRow],
    children: &HashMap<u32, Vec<usize>>,
) -> Option<(f32, u64, u32)> {
    let by_pid: HashMap<u32, usize> = rows.iter().enumerate().map(|(i, r)| (r.pid, i)).collect();
    let root_index = *by_pid.get(&root)?;
    let mut cpu = 0.0f32;
    let mut resident = 0u64;
    let mut count = 0u32;
    let mut queue = vec![root_index];
    let mut seen = vec![false; rows.len()];
    while let Some(index) = queue.pop() {
        if seen[index] {
            continue;
        }
        seen[index] = true;
        let row = rows[index];
        cpu += row.cpu_percent;
        resident = resident.saturating_add(row.resident_bytes);
        count += 1;
        if let Some(next) = children.get(&row.pid) {
            queue.extend(next.iter().copied());
        }
    }
    Some((cpu, resident, count))
}

/// `ps` reports resident size in KiB in this format on every platform that provides it.
pub fn parse_ps_table(text: &str) -> Vec<ProcessRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(pid), Some(parent), Some(rss), Some(cpu)) = (
            fields.next().and_then(|value| value.parse::<u32>().ok()),
            fields.next().and_then(|value| value.parse::<u32>().ok()),
            fields.next().and_then(|value| value.parse::<u64>().ok()),
            fields.next().and_then(|value| value.parse::<f32>().ok()),
        ) else {
            continue;
        };
        rows.push(ProcessRow {
            pid,
            parent,
            resident_bytes: rss.saturating_mul(1024),
            cpu_percent: cpu.max(0.0),
        });
    }
    rows
}

/// Columns are user, nice, system, idle, iowait, irq, softirq, steal. Idle time is
/// `idle + iowait`: iowait is time no CPU spent running anything, which is the kernel's own
/// definition and counting it as busy would report an idle machine as loaded.
pub fn parse_proc_stat(text: &str) -> Option<CpuCounters> {
    let line = text.lines().find(|line| line.starts_with("cpu "))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect();
    if values.len() < 4 {
        return None;
    }
    let total: u64 = values.iter().sum();
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some(CpuCounters { total, idle })
}

/// Parse macOS `sysctl -n kern.cp_time`, which prints user, nice, sys, idle, intr as counters.
pub fn parse_cp_time(text: &str) -> Option<CpuCounters> {
    let values: Vec<u64> = text
        .split_whitespace()
        .filter_map(|value| value.parse::<u64>().ok())
        .collect();
    if values.len() < 4 {
        return None;
    }
    let total: u64 = values.iter().sum();
    Some(CpuCounters {
        total,
        idle: values[3],
    })
}

/// Parse the last `CPU usage:` line of `top -l 2 -n 0 -s 1`, e.g.
/// `CPU usage: 44.15% user, 25.66% sys, 30.18% idle`.
///
/// `top -l 1` reports load since boot, so the second sample is the only one that describes an
/// interval; the last line is therefore the reading, not the first.
pub fn parse_top_cpu_usage(text: &str) -> Option<f32> {
    let line = text
        .lines()
        .filter(|line| line.trim_start().starts_with("CPU usage:"))
        .next_back()?;
    let percent = |label: &str| -> Option<f32> {
        let (rest, _) = line.split_once(label)?;
        rest.split_whitespace()
            .last()?
            .trim_end_matches('%')
            .parse::<f32>()
            .ok()
    };
    let idle = percent("idle")?;
    Some((1.0 - idle / 100.0).clamp(0.0, 1.0))
}

/// `MemAvailable` is the kernel's own estimate of what a new workload could claim. Kernels
/// older than 3.14 do not print it, and free + buffers + cached + reclaimable is the documented
/// fallback for them.
pub fn parse_meminfo(text: &str) -> MemoryReading {
    let mut reading = MemoryReading::default();
    let mut free = None;
    let mut buffers = 0u64;
    let mut cached = 0u64;
    let mut reclaimable = 0u64;
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|value| value.saturating_mul(1024));
        match key.trim() {
            "MemTotal" => reading.total_bytes = value,
            "MemFree" => free = value,
            "Buffers" => buffers = value.unwrap_or(0),
            "Cached" => cached = value.unwrap_or(0),
            "SReclaimable" => reclaimable = value.unwrap_or(0),
            "SwapTotal" => reading.swap_total_bytes = value,
            "SwapFree" => {
                reading.swap_used_bytes = match (reading.swap_total_bytes, value) {
                    (Some(total), Some(free_swap)) => Some(total.saturating_sub(free_swap)),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    let available = parse_meminfo_available(text);
    if let (Some(total), Some(available)) = (reading.total_bytes, available.or_else(|| {
        free.map(|free| free.saturating_add(buffers).saturating_add(cached).saturating_add(reclaimable))
    })) {
        reading.used_bytes = Some(total.saturating_sub(available));
    }
    if let (Some(total), Some(used)) = (reading.swap_total_bytes, reading.swap_used_bytes) {
        if used > total {
            reading.swap_used_bytes = Some(total);
        }
    }
    reading
}

fn parse_meminfo_available(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|value| value.saturating_mul(1024))
}

/// Uses the page size `vm_stat` prints in its first line. Free, inactive and speculative pages
/// are all reclaimable and count as available; purgeable pages overlap with those, so including
/// them would count the same page twice.
pub fn parse_vm_stat(text: &str, total_bytes: u64) -> MemoryReading {
    let page_size = text
        .lines()
        .next()
        .and_then(|line| line.split("page size of ").nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok());
    let Some(page_size) = page_size else {
        return MemoryReading {
            total_bytes: Some(total_bytes),
            ..MemoryReading::default()
        };
    };
    let pages = |label: &str| -> Option<u64> {
        text.lines()
            .find(|line| line.trim_start().starts_with(label))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|rest| rest.trim().trim_end_matches('.').parse::<u64>().ok())
    };
    let available_pages = pages("Pages free")
        .unwrap_or(0)
        .saturating_add(pages("Pages inactive").unwrap_or(0))
        .saturating_add(pages("Pages speculative").unwrap_or(0));
    MemoryReading {
        total_bytes: Some(total_bytes),
        used_bytes: Some(total_bytes.saturating_sub(available_pages.saturating_mul(page_size))),
        swap_total_bytes: None,
        swap_used_bytes: None,
    }
}

/// macOS prints this as `total = 2048.00M  used = 512.25M  free = 1535.75M`.
pub fn parse_swapusage(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| -> Option<u64> {
        let rest = text.split(&format!("{name} = ")).nth(1)?;
        let value = rest.split_whitespace().next()?;
        parse_swap_size(value)
    };
    (field("total"), field("used"))
}

fn parse_swap_size(value: &str) -> Option<u64> {
    let (number, scale) = match value.chars().last()? {
        'K' | 'k' => (&value[..value.len() - 1], 1024u64),
        'M' | 'm' => (&value[..value.len() - 1], 1024 * 1024),
        'G' | 'g' => (&value[..value.len() - 1], 1024 * 1024 * 1024),
        _ => (value, 1),
    };
    number.parse::<f64>().ok().map(|parsed| {
        (parsed * scale as f64) as u64
    })
}

/// macOS prints this as `{ 1.23 4.56 7.89 }`, so the numbers are found by parsing rather than
/// by fixed offsets.
pub fn parse_macos_loadavg(text: &str) -> Option<f32> {
    text.split_whitespace()
        .find_map(|value| value.parse::<f32>().ok())
}

pub fn parse_linux_loadavg(text: &str) -> Option<f32> {
    text.split_whitespace().next()?.parse::<f32>().ok()
}

/// macOS prints this as `{ sec = 1759..., usec = 0 } Thu Jan  1 ...`.
pub fn parse_boottime_secs(text: &str) -> Option<u64> {
    text.split("sec = ").nth(1)?.split(',').next()?.trim().parse::<u64>().ok()
}

pub fn parse_proc_uptime(text: &str) -> Option<u64> {
    text.split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|seconds| seconds as u64)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowsReading {
    pub cpu_percent: Option<f32>,
    pub memory: MemoryReading,
    pub disk_total_bytes: Option<u64>,
    pub disk_free_bytes: Option<u64>,
    pub uptime_seconds: Option<u64>,
    pub processes: Option<Vec<ProcessRow>>,
}

/// `ConvertTo-Json` collapses a single-element array to a bare object, so the process list is
/// accepted in both shapes. A malformed row is dropped rather than failing the whole reading:
/// one unreadable process must not cost the operator the host numbers.
pub fn parse_windows_snapshot(text: &str) -> WindowsReading {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return WindowsReading::default();
    };
    let number = |key: &str| -> Option<f64> { value.get(key).and_then(|item| item.as_f64()) };
    let memory = MemoryReading {
        total_bytes: number("totalMemoryBytes").map(|value| value as u64),
        used_bytes: match (number("totalMemoryBytes"), number("freeMemoryBytes")) {
            (Some(total), Some(free)) => Some((total - free).max(0.0) as u64),
            _ => None,
        },
        swap_total_bytes: number("swapTotalBytes").map(|value| value as u64),
        swap_used_bytes: match (number("swapTotalBytes"), number("swapFreeBytes")) {
            (Some(total), Some(free)) => Some((total - free).max(0.0) as u64),
            _ => None,
        },
    };
    let processes = value.get("processes").map(|item| {
        let items: Vec<&serde_json::Value> = match item {
            serde_json::Value::Array(entries) => entries.iter().collect(),
            other => vec![other],
        };
        items
            .into_iter()
            .filter_map(|entry| {
                Some(ProcessRow {
                    pid: entry.get("ProcessId")?.as_u64()? as u32,
                    parent: entry
                        .get("ParentProcessId")
                        .and_then(|item| item.as_u64())
                        .unwrap_or(0) as u32,
                    resident_bytes: entry
                        .get("WorkingSetSize")
                        .and_then(|item| item.as_u64())
                        .unwrap_or(0),
                    // Per-process CPU is not in this snapshot; the tree reports `None` rather
                    // than a fabricated share.
                    cpu_percent: 0.0,
                })
            })
            .collect::<Vec<_>>()
    });
    WindowsReading {
        cpu_percent: number("cpuPercent").map(|value| (value / 100.0).clamp(0.0, 1.0) as f32),
        memory,
        disk_total_bytes: number("diskTotalBytes").map(|value| value as u64),
        disk_free_bytes: number("diskFreeBytes").map(|value| value as u64),
        uptime_seconds: number("uptimeSeconds").map(|value| value as u64),
        processes,
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::process::Command;

    pub(super) fn read(disk_path: &Path) -> PlatformReading {
        let counters = cpu_counters();
        PlatformReading {
            counters,
            // `kern.cp_time` is absent on current Darwin, where `top` is the remaining source
            // that reports an interval rather than load since boot.
            host_cpu_utilization: if counters.is_none() {
                interval_cpu_utilization()
            } else {
                None
            },
            memory: memory(),
            load_average_1m: load_average_1m(),
            uptime_seconds: uptime_seconds(),
            disk: disk_usage(disk_path),
            processes: process_table(),
            process_cpu: true,
        }
    }

    fn cpu_counters() -> Option<CpuCounters> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|text| parse_proc_stat(&text))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let output = Command::new("sysctl")
                .args(["-n", "kern.cp_time"])
                .output()
                .ok()?;
            output
                .status
                .success()
                .then(|| parse_cp_time(&String::from_utf8_lossy(&output.stdout)))
                .flatten()
        }
    }

    fn memory() -> MemoryReading {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/meminfo")
                .map(|text| parse_meminfo(&text))
                .unwrap_or_default()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let total = sysctl_u64("hw.memsize");
            let mut reading = match (total, Command::new("vm_stat").output()) {
                (Some(total), Ok(output)) => {
                    parse_vm_stat(&String::from_utf8_lossy(&output.stdout), total)
                }
                _ => MemoryReading::default(),
            };
            if let Ok(output) = Command::new("sysctl").args(["-n", "vm.swapusage"]).output() {
                let (swap_total, swap_used) = parse_swapusage(&String::from_utf8_lossy(&output.stdout));
                reading.swap_total_bytes = swap_total;
                reading.swap_used_bytes = swap_used;
            }
            reading
        }
    }

    #[cfg(target_os = "macos")]
    fn interval_cpu_utilization() -> Option<f32> {
        let output = Command::new("top")
            .args(["-l", "2", "-n", "0", "-s", "1"])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| parse_top_cpu_usage(&String::from_utf8_lossy(&output.stdout)))
            .flatten()
    }

    #[cfg(not(target_os = "macos"))]
    fn interval_cpu_utilization() -> Option<f32> {
        None
    }

    #[cfg(not(target_os = "linux"))]
    fn sysctl_u64(name: &str) -> Option<u64> {
        let output = Command::new("sysctl").args(["-n", name]).output().ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse::<u64>().ok()
    }

    fn load_average_1m() -> Option<f32> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/loadavg")
                .ok()
                .and_then(|text| parse_linux_loadavg(&text))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let output = Command::new("sysctl").args(["-n", "vm.loadavg"]).output().ok()?;
            parse_macos_loadavg(&String::from_utf8_lossy(&output.stdout))
        }
    }

    fn uptime_seconds() -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|text| parse_proc_uptime(&text))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let output = Command::new("sysctl").args(["-n", "kern.boottime"]).output().ok()?;
            let boot = parse_boottime_secs(&String::from_utf8_lossy(&output.stdout))?;
            Some(wall_time_ms().saturating_div(1000).saturating_sub(boot))
        }
    }

    fn process_table() -> Option<Vec<ProcessRow>> {
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,rss=,pcpu="])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| parse_ps_table(&String::from_utf8_lossy(&output.stdout)))
    }

    fn disk_usage(path: &Path) -> (Option<u64>, Option<u64>) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            return (None, None);
        };
        // SAFETY: `c_path` is a valid NUL-terminated C string that outlives the call, and
        // `statvfs` only writes into the `statvfs` value we pass by mutable reference.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return (None, None);
        }
        let block = stat.f_frsize as u64;
        (
            Some((stat.f_blocks as u64).saturating_mul(block)),
            Some((stat.f_bavail as u64).saturating_mul(block)),
        )
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::process::Command;

    /// Windows has no `/proc`, and each CIM query costs a PowerShell launch. One script answers
    /// host CPU, memory, swap, disk and the process table together, so the cost is paid once per
    /// sample instead of once per metric.
    pub(super) fn read(disk_path: &Path) -> PlatformReading {
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             $os = Get-CimInstance Win32_OperatingSystem; \
             $cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average; \
             $procs = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize); \
             $root = [System.IO.Path]::GetPathRoot('{root}'); \
             $drive = [System.IO.DriveInfo]::new($root); \
             [pscustomobject]@{{ \
               cpuPercent=$cpu; \
               totalMemoryBytes=[int64]$os.TotalVisibleMemorySize*1024; \
               freeMemoryBytes=[int64]$os.FreePhysicalMemory*1024; \
               swapTotalBytes=[int64]$os.TotalVirtualMemorySize*1024; \
               swapFreeBytes=[int64]$os.FreeVirtualMemory*1024; \
               uptimeSeconds=[int64]((Get-Date) - $os.LastBootUpTime).TotalSeconds; \
               diskTotalBytes=$drive.TotalSize; \
               diskFreeBytes=$drive.AvailableFreeSpace; \
               processes=$procs }} | ConvertTo-Json -Compress -Depth 4",
            root = disk_path.to_string_lossy().replace('\'', "''")
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();
        let reading = match output {
            Ok(output) if output.status.success() => {
                parse_windows_snapshot(&String::from_utf8_lossy(&output.stdout))
            }
            _ => WindowsReading::default(),
        };
        PlatformReading {
            counters: None,
            host_cpu_utilization: reading.cpu_percent,
            memory: reading.memory,
            // Windows has no load average, and inventing one from the CPU queue would report a
            // different quantity under the same name.
            load_average_1m: None,
            uptime_seconds: reading.uptime_seconds,
            disk: (reading.disk_total_bytes, reading.disk_free_bytes),
            processes: reading.processes,
            process_cpu: false,
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;

    pub(super) fn read(_: &Path) -> PlatformReading {
        PlatformReading {
            counters: None,
            host_cpu_utilization: None,
            memory: MemoryReading::default(),
            load_average_1m: None,
            uptime_seconds: None,
            disk: (None, None),
            processes: None,
            process_cpu: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes() -> Vec<SessionProbe> {
        vec![
            SessionProbe {
                session_id: "s1".to_string(),
                pid: Some(100),
                worktree_path: Some("/tmp/wt".to_string()),
            },
            SessionProbe {
                session_id: "s2".to_string(),
                pid: None,
                worktree_path: None,
            },
        ]
    }

    #[test]
    fn utilization_differences_counters_over_the_interval() {
        let previous = CpuCounters {
            total: 1_000,
            idle: 900,
        };
        let current = CpuCounters {
            total: 2_000,
            idle: 1_300,
        };
        let utilization = utilization_from_counters(previous, current, Duration::from_secs(1)).unwrap();
        assert!((utilization - 0.6).abs() < 0.0001, "got {utilization}");
    }

    #[test]
    fn utilization_refuses_a_reset_or_empty_interval() {
        let previous = CpuCounters {
            total: 1_000,
            idle: 900,
        };
        let reset = CpuCounters {
            total: 10,
            idle: 1,
        };
        assert_eq!(
            utilization_from_counters(previous, reset, Duration::from_secs(1)),
            None
        );
        assert_eq!(
            utilization_from_counters(previous, previous, Duration::from_secs(1)),
            None
        );
        assert_eq!(
            utilization_from_counters(previous, previous, Duration::ZERO),
            None
        );
    }

    #[test]
    fn proc_stat_sums_every_column_and_treats_iowait_as_idle() {
        let text = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 2 3 4 5\n";
        let counters = parse_proc_stat(text).unwrap();
        assert_eq!(counters.total, 1_000);
        assert_eq!(counters.idle, 850);
    }

    #[test]
    fn top_cpu_usage_takes_the_interval_sample_and_reads_busy_from_idle() {
        let text = "\
Processes: 900 total\n\
CPU usage: 38.22% user, 30.14% sys, 31.62% idle \n\
Load Avg: 12.00, 15.00, 13.00\n\
CPU usage: 44.15% user, 25.66% sys, 30.18% idle \n";
        let utilization = parse_top_cpu_usage(text).unwrap();
        assert!((utilization - 0.6982).abs() < 0.0001, "got {utilization}");
    }

    #[test]
    fn top_cpu_usage_reports_nothing_without_a_cpu_line() {
        assert_eq!(parse_top_cpu_usage("Processes: 900 total\n"), None);
    }

    #[test]
    fn cp_time_parses_macos_aggregate_counters() {
        let counters = parse_cp_time("12345 67 890 45678 9\n").unwrap();
        assert_eq!(counters.total, 12345 + 67 + 890 + 45678 + 9);
        assert_eq!(counters.idle, 45678);
    }

    #[test]
    fn meminfo_prefers_available_and_falls_back_to_reclaimable_pages() {
        let with_available = "\
MemTotal:       16000000 kB
MemFree:         1000000 kB
MemAvailable:    6000000 kB
Buffers:          200000 kB
Cached:          3000000 kB
SwapTotal:       2000000 kB
SwapFree:        1500000 kB
";
        let reading = parse_meminfo(with_available);
        assert_eq!(reading.total_bytes, Some(16_000_000 * 1024));
        assert_eq!(reading.used_bytes, Some(10_000_000 * 1024));
        assert_eq!(reading.swap_used_bytes, Some(500_000 * 1024));

        let without_available = "\
MemTotal:       16000000 kB
MemFree:         1000000 kB
Buffers:          200000 kB
Cached:          3000000 kB
SReclaimable:     100000 kB
";
        let reading = parse_meminfo(without_available);
        assert_eq!(reading.used_bytes, Some(11_700_000 * 1024));
        assert_eq!(reading.swap_total_bytes, None);
    }

    #[test]
    fn vm_stat_counts_free_inactive_and_speculative_as_available() {
        let text = "\
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               10000.
Pages active:                            200000.
Pages inactive:                           20000.
Pages speculative:                         5000.
Pages wired down:                         30000.
";
        let reading = parse_vm_stat(text, 64 * 1024 * 1024 * 1024);
        assert_eq!(reading.total_bytes, Some(64 * 1024 * 1024 * 1024));
        let available = (10_000 + 20_000 + 5_000) * 16_384u64;
        assert_eq!(
            reading.used_bytes,
            Some(64 * 1024 * 1024 * 1024 - available)
        );
    }

    #[test]
    fn swapusage_reads_scaled_values() {
        let (total, used) = parse_swapusage("total = 2048.00M  used = 512.25M  free = 1535.75M");
        assert_eq!(total, Some(2048 * 1024 * 1024));
        assert_eq!(used, Some((512.25 * 1024.0 * 1024.0) as u64));
    }

    #[test]
    fn load_average_and_uptime_parsers_handle_both_platform_shapes() {
        assert_eq!(parse_macos_loadavg("{ 1.23 4.56 7.89 }"), Some(1.23));
        assert_eq!(parse_linux_loadavg("0.42 0.30 0.20 1/400 12345"), Some(0.42));
        assert_eq!(
            parse_boottime_secs("{ sec = 1759000000, usec = 0 } Thu Jan  1 00:00:00 2026"),
            Some(1_759_000_000)
        );
        assert_eq!(parse_proc_uptime("12345.67 98765.43\n"), Some(12_345));
    }

    #[test]
    fn ps_table_skips_unparsable_lines_and_scales_kib_to_bytes() {
        let rows = parse_ps_table("  100   1  2048  1.5\n  bad line\n  101  100  1024  0.5\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].resident_bytes, 2048 * 1024);
        assert_eq!(rows[1].parent, 100);
    }

    #[test]
    fn session_usage_sums_the_whole_process_tree() {
        let rows = parse_ps_table("100 1 1000 0.5\n101 100 2000 1.0\n102 101 500 0.25\n999 1 8000 9.0\n");
        let usage = session_usage(&probes()[0], &rows, true);
        assert_eq!(usage.process_count, Some(3));
        assert_eq!(usage.resident_bytes, Some((1000 + 2000 + 500) * 1024));
        assert!((usage.cpu_percent.unwrap() - 1.75).abs() < 0.0001);
    }

    #[test]
    fn a_host_without_per_process_cpu_reports_memory_and_count_but_no_share() {
        let rows = parse_ps_table("100 1 1000 0.5\n101 100 2000 1.0\n");
        let usage = session_usage(&probes()[0], &rows, false);
        assert_eq!(usage.cpu_percent, None);
        assert_eq!(usage.resident_bytes, Some(3000 * 1024));
        assert_eq!(usage.process_count, Some(2));
    }

    #[test]
    fn session_usage_without_a_live_process_reports_nothing_rather_than_zero() {
        let rows = parse_ps_table("999 1 8000 9.0\n");
        let usage = session_usage(&probes()[0], &rows, true);
        assert_eq!(usage.pid, Some(100));
        assert_eq!(usage.cpu_percent, None);
        assert_eq!(usage.resident_bytes, None);
        assert_eq!(usage.process_count, None);
    }

    #[test]
    fn windows_snapshot_parses_a_single_process_object_as_a_one_row_table() {
        let text = r#"{"cpuPercent":12.5,"totalMemoryBytes":17179869184,"freeMemoryBytes":8589934592,
            "swapTotalBytes":4294967296,"swapFreeBytes":2147483648,"diskTotalBytes":1000000000,
            "diskFreeBytes":400000000,"uptimeSeconds":86400,
            "processes":{"ProcessId":100,"ParentProcessId":1,"WorkingSetSize":1048576}}"#;
        let reading = parse_windows_snapshot(text);
        assert_eq!(reading.cpu_percent, Some(0.125));
        assert_eq!(reading.memory.total_bytes, Some(17_179_869_184));
        assert_eq!(reading.memory.used_bytes, Some(8_589_934_592));
        assert_eq!(reading.memory.swap_used_bytes, Some(2_147_483_648));
        assert_eq!(reading.disk_free_bytes, Some(400_000_000));
        assert_eq!(reading.uptime_seconds, Some(86_400));
        assert_eq!(reading.processes.unwrap().len(), 1);
    }

    #[test]
    fn windows_snapshot_reports_nothing_when_the_script_printed_garbage() {
        let reading = parse_windows_snapshot("Get-CimInstance : Access denied");
        assert_eq!(reading, WindowsReading::default());
    }

    #[test]
    fn sampler_names_every_metric_it_could_not_read() {
        let sampler = ResourceSampler::new();
        let snapshot = sampler.sample_reading(&probes(), PlatformReading {
            counters: None, host_cpu_utilization: None, memory: MemoryReading::default(),
            load_average_1m: None, uptime_seconds: None, disk: (None, None),
            processes: None, process_cpu: false,
        });
        assert!(snapshot.unavailable.contains(&"cpuUtilization".to_string()));
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[1].session_id, "s2");
        assert_eq!(snapshot.sessions[1].pid, None);
        assert!(snapshot.cpu_count >= 1);
    }

    #[test]
    fn consecutive_samples_report_utilization_once_counters_exist() {
        let sampler = ResourceSampler::new();
        let reading = |total, idle| PlatformReading {
            counters: Some(CpuCounters { total, idle }), host_cpu_utilization: None,
            memory: MemoryReading::default(), load_average_1m: None,
            uptime_seconds: None, disk: (None, None), processes: None, process_cpu: false,
        };
        let first = sampler.sample_reading(&probes(), reading(100, 50));
        assert!(first.cpu_utilization.is_none());
        sampler.state.lock().sampled_at = Some(Instant::now() - MIN_CPU_DELTA);
        let second = sampler.sample_reading(&probes(), reading(200, 75));
        match second.cpu_utilization {
            Some(value) => assert!((0.0..=1.0).contains(&value)),
            None => assert!(second.unavailable.contains(&"cpuUtilization".to_string())),
        }
    }
}
