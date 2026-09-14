#[path = "support/paired_daemon_fixture.rs"]
mod support;
use support::PairedDaemonFixture;

#[test]
fn equal_raw_ids_keep_host_output_separate() {
    let mut fixture = PairedDaemonFixture::new().unwrap();
    fixture.publish("relay/a", "session-1", b"A");
    fixture.publish("relay/b", "session-1", b"B");
    assert_eq!(fixture.output("relay/a", "session-1"), b"A");
    assert_eq!(fixture.output("relay/b", "session-1"), b"B");
}

#[cfg(unix)]
#[test]
fn socket_data_and_project_roots_are_private_and_cleaned() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::io::{Read, Write};
    let fixture = PairedDaemonFixture::new().unwrap();
    let owner = fixture.directory.path().to_owned();
    for path in [&fixture.runtime, &fixture.data, &fixture.root] {
        assert!(path.starts_with(&owner));
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o700);
        eprintln!("PRIVATE {} mode=700", path.display());
    }
    let socket = fixture.runtime.join("fixture.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut client = UnixStream::connect(&socket).unwrap();
    client.write_all(b"A01").unwrap();
    let (mut accepted, _) = listener.accept().unwrap();
    accepted.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    let mut bytes = [0; 3];
    accepted.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"A01");
    eprintln!("SURFACE unix socket {} payload=A01", socket.display());
    drop(accepted);
    drop(client);
    drop(listener);
    fixture.directory.close().unwrap();
    assert!(!owner.exists());
    eprintln!("CLEANUP {} absent", owner.display());
}
