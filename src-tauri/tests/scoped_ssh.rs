pub use ferryx_lib::scoped_contracts; // Compile the real shared DTOs.
#[path = "../src/ferryx_scope/ssh/mod.rs"]
pub mod ssh;

fn host() -> ssh::config::HostConfig {
    ssh::config::HostConfig { id: "qa".into(), name: "QA".into(), hostname: "127.0.0.1".into(), user: "indo".into(), port: 22222, identity_file: Some("fixture-key".into()), proxy_jump: Some("jump@bastion:2200".into()), known_hosts_file: "fixture-trust".into() }
}
#[test]
fn options_honor_port_key_jump_and_noninteractive_trust() {
    let args = host().argv().unwrap();
    for pair in [["-p", "22222"], ["-i", "fixture-key"], ["-J", "jump@bastion:2200"], ["-o", "StrictHostKeyChecking=yes"], ["-o", "ConnectTimeout=3"], ["-o", "BatchMode=yes"]] {
        assert!(args.windows(2).any(|w| w == pair), "missing {pair:?}: {args:?}");
    }
}
#[test]
fn corrupt_store_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("hosts.json");
    std::fs::write(&path, b"{broken").unwrap();
    let store = ssh::config::HostStore { path: path.clone(), lock: std::sync::Mutex::new(()) };
    assert!(store.update(&[host()]).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
}
