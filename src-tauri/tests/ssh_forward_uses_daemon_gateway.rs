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

    store
        .set_ssh_host_id(&row.host_id, row.generation, None)
        .expect("clear ssh host id");
    assert_eq!(
        store.list()[0].ssh_host_id,
        None,
        "clearing link removes candidate"
    );
}
