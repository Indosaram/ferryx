//! Task 5 acceptance: the real macOS launchd job, driven through the public lifecycle API.
//!
//! The unit tests above run the lifecycle against a fake scheduler, which proves the decision rules
//! but not that the plist this build renders is one launchd accepts. This binary does the second
//! half: it creates a machine through [`create_with_reaper`] with the real [`HostReaper`], asks
//! `launchctl print` what launchd loaded, then deletes the machine and requires the job to be gone.
//!
//! Nothing here touches the operator's catalog: both the catalog and the plist path are pointed at a
//! temporary directory, so a run cannot leave a real LaunchAgent or a real machine row behind. The
//! FakeProvider provisions nothing, so no provider is called and no credit is spent.
//!
//! `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored launchd_acceptance`

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::cloud_host::catalog::{CatalogStore, CATALOG_FILE};
use crate::cloud_host::lifecycle::{create_with_reaper, delete_with_reaper};
use crate::cloud_host::provider::fake::FakeProvider;
use crate::cloud_host::slots::SlotRegistry;
use crate::cloud_host::{CreateSandbox, SandboxState};

/// Read `run interval` out of `launchctl print` for the job.
fn printed_interval(target: &str) -> Option<u64> {
    let output = std::process::Command::new("launchctl").args(["print", target]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.lines()
        .find(|line| line.trim_start().starts_with("run interval ="))
        .and_then(|line| {
            line.split('=')
                .nth(1)?
                .trim()
                .trim_end_matches("seconds")
                .trim()
                .parse()
                .ok()
        })
}

fn job_loaded(target: &str) -> bool {
    std::process::Command::new("launchctl")
        .args(["print", target])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore = "touches the real launchctl; run explicitly on maho-mac"]
async fn launchd_acceptance_the_job_loads_with_interval_300_and_leaves_with_the_last_row() {
    // `FERRYX_CLOUD_REAPER_HOME` and `FERRYX_CLOUD_REAPER_PLIST_PATH` move both artifacts into a
    // temp directory, so the operator's `~/Library/LaunchAgents` is never written.
    let root = tempfile::tempdir().expect("a temp home for the reaper artifacts");
    let home = root.path().to_path_buf();
    let plist = home.join("LaunchAgents").join("dev.ferryx.cloud-reaper.plist");
    std::env::set_var("FERRYX_CLOUD_REAPER_HOME", &home);
    std::env::set_var("FERRYX_CLOUD_REAPER_PLIST_PATH", &plist);
    std::env::set_var("FERRYX_CLI_PATH", "/usr/bin/true");

    let uid = unsafe { libc::getuid() };
    let target = format!("gui/{uid}/dev.ferryx.cloud-reaper");
    // A leftover job from an aborted run would make the first assertion meaningless.
    if job_loaded(&target) {
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &target])
            .output();
    }
    assert!(!job_loaded(&target), "the job must start absent");

    let store = CatalogStore::at(home.join("state").join(CATALOG_FILE));
    // An SSH provider takes no relay slot, so this acceptance run needs no record in the pool.
    let slots = SlotRegistry::at(home.join("cloud-slots"));
    // `FakeProvider::ssh()` has native auto-stop, so this create exercises the "install the reaper
    // on the first create" path without needing the reaper for permission.
    let provider = Arc::new(FakeProvider::ssh());

    let created = create_with_reaper(
        &*provider,
        "fake",
        CreateSandbox {
            idempotency_key: "fx-acceptance-1".into(),
            name: "fx-acceptance".into(),
            ttl_seconds: Some(1_800),
            env: BTreeMap::new(),
        },
        &store,
        &slots,
    )
    .await
    .expect("create_with_reaper");
    assert!(created.reaper_installed, "the first create installs the reaper");
    assert!(plist.is_file(), "the plist artifact exists at the overridden path: {}", plist.display());

    let interval = printed_interval(&target);
    println!("launchctl print {target}: run interval = {interval:?}");
    assert_eq!(interval, Some(300), "the loaded job runs every 300 s");

    // The transcripts below are the evidence: launchd's own words, then the plist it accepted.
    let transcript = std::process::Command::new("launchctl")
        .args(["print", &target])
        .output()
        .expect("launchctl print");
    println!("--- launchctl print {target} ---\n{}", String::from_utf8_lossy(&transcript.stdout));

    // The plist names the reconcile argv, and the job's own log paths live under the temp home.
    let rendered = std::fs::read_to_string(&plist).expect("read the plist");
    println!("--- {} ---\n{rendered}", plist.display());
    for expected in ["<string>cloud</string>", "<string>reconcile</string>", "<integer>300</integer>"] {
        assert!(rendered.contains(expected), "the plist carries {expected}:\n{rendered}");
    }

    let deleted = delete_with_reaper(&*provider, "fx-acceptance", &store, &slots)
        .await
        .expect("delete_with_reaper");
    assert_eq!(deleted.row.state, SandboxState::Deleted);
    assert!(deleted.reaper_removed, "no live row is left, so the job is removed");
    assert!(!job_loaded(&target), "launchctl print fails after removal");
    assert_eq!(printed_interval(&target), None, "the job is gone, not merely stopped");

    // The tombstone stays in the catalog.
    let catalog = store.load().await.expect("catalog");
    assert_eq!(catalog.rows.len(), 1);
    assert_eq!(catalog.row("fx-acceptance").map(|row| row.state), Some(SandboxState::Deleted));
    assert!(!catalog.has_live());

    println!("ACCEPTANCE OK: job loaded with run interval 300, then removed; tombstone kept");
}
