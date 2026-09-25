use ferryx_lib::paired_host::inventory::{GrantScope, Inventory, Pairing};

fn fixture_pairing(machine: &str) -> Pairing {
    Pairing {
        relay_origin: "https://relay.example.com/".into(),
        machine_id: machine.into(),
        display_label: format!("Machine {machine}"),
        grant_scope: GrantScope::Machine,
        device_token: "test-device-token".into(),
    }
}

#[test]
fn ssh_candidate_absent_without_explicit_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Inventory::open(dir.path()).unwrap();
    let row = store.pair(&fixture_pairing("alpha")).unwrap();
    assert_eq!(
        row.ssh_host_id, None,
        "without explicit link, ssh_host_id is absent"
    );

    let candidate = row.ssh_host_id.as_deref().map(|id| format!("ssh://{id}"));
    assert_eq!(
        candidate, None,
        "an SSH config host with matching hostname and no sshHostId produces no candidate"
    );

    let candidates = ferryx_lib::paired_host::attach::build_host_candidates(&row, Some("test-device-token"));
    assert_eq!(candidates.len(), 1, "only relay candidate when ssh_host_id is absent");
    assert_eq!(candidates[0].path, ferryx_lib::paired_host::path_select::AttachPath::Relay);
}

#[test]
fn ssh_candidate_present_with_explicit_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Inventory::open(dir.path()).unwrap();
    let row = store.pair(&fixture_pairing("beta")).unwrap();
    store
        .set_ssh_host_id(&row.host_id, row.generation, Some("saved-ssh-42".into()))
        .expect("set ssh host id");

    let list = store.list();
    assert_eq!(
        list[0].ssh_host_id.as_deref(),
        Some("saved-ssh-42"),
        "explicit link sets ssh_host_id"
    );

    let candidate = list[0]
        .ssh_host_id
        .as_deref()
        .map(|id| format!("127.0.0.1:43821 via {id}"));
    assert_eq!(
        candidate,
        Some("127.0.0.1:43821 via saved-ssh-42".into()),
        "SSH forward candidate points to daemon gateway forward"
    );

    let candidates = ferryx_lib::paired_host::attach::build_host_candidates(&list[0], Some("test-device-token"));
    assert_eq!(candidates.len(), 2, "both relay and SSH forward candidate when ssh_host_id is present");
    assert_eq!(candidates[0].path, ferryx_lib::paired_host::path_select::AttachPath::Relay);
    assert_eq!(candidates[1].path, ferryx_lib::paired_host::path_select::AttachPath::SshForward);
    assert_eq!(candidates[1].base_origin, "http://127.0.0.1:43821");

    store
        .set_ssh_host_id(&row.host_id, row.generation, None)
        .expect("clear ssh host id");
    assert_eq!(
        store.list()[0].ssh_host_id,
        None,
        "clearing link removes candidate"
    );

    let candidates_cleared = ferryx_lib::paired_host::attach::build_host_candidates(&store.list()[0], Some("test-device-token"));
    assert_eq!(candidates_cleared.len(), 1, "only relay candidate after link is cleared");
    assert_eq!(candidates_cleared[0].path, ferryx_lib::paired_host::path_select::AttachPath::Relay);
}
