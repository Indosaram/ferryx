use super::*;
fn host() -> SshHost {
    serde_json::from_value(serde_json::json!({"id":uuid::Uuid::new_v4().to_string(),"hostname":format!("{}.invalid",uuid::Uuid::new_v4()),"authMethod":"password"})).unwrap()
}

#[test]
fn credentials_are_transient_endpoint_scoped_and_redacted() {
    let host = host();
    let secret = "unique test password";
    assert_eq!(
        require(&host).unwrap_err().details.unwrap()["stage"],
        "authentication"
    );
    set(&host, Password::new(secret.into())).unwrap();
    let mut renamed = host.clone();
    renamed.id = "different-id".into();
    assert!(require(&renamed).is_ok());
    renamed.port = Some(2222);
    assert!(require(&renamed).is_err());
    let request = crate::daemon::protocol::DaemonRequest::SshPassword {
        host: host.clone(),
        password: Some(Password::new(secret.into())),
    };
    assert!(!format!("{request:?}").contains(secret));
    assert!(!serde_json::to_string(&host).unwrap().contains(secret));
    let plan = super::super::direct::ssh_plan(&host, "true".into(), false).unwrap();
    assert!(!format!("{plan:?}").contains(secret));
    let env = environment(&plan.args).unwrap();
    assert!(!format!("{env:?}").contains(secret));
    let port: u16 = env
        .iter()
        .find(|v| v.0 == "FERRYX_SSH_ASKPASS_PORT")
        .unwrap()
        .1
        .parse()
        .unwrap();
    let token = &env
        .iter()
        .find(|v| v.0 == "FERRYX_SSH_ASKPASS_TOKEN")
        .unwrap()
        .1;
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(token.as_bytes()).unwrap();
    let mut received = String::new();
    stream.read_to_string(&mut received).unwrap();
    assert_eq!(received, secret);
    clear(&host).unwrap();
    assert!(require(&host).is_err());
}

#[test]
fn stale_generation_cannot_clear_replacement() {
    let host = host();
    set(&host, Password::new("first".into())).unwrap();
    let old = generation(&host).unwrap().unwrap();
    set(&host, Password::new("second".into())).unwrap();
    assert!(!clear_generation(&host, &old).unwrap());
    assert!(require(&host).is_ok());
    assert!(clear_generation(&host, &generation(&host).unwrap().unwrap()).unwrap());
    assert!(require(&host).is_err());
}

#[test]
fn argument_endpoint_lookup_matches_explicit_default_user_ipv6_and_jump() {
    for (hostname, username, port, jump) in [
        ("ssh-alias", None, None, None),
        ("2001:db8::1", Some("user"), Some(2222), None),
        (
            "destination",
            Some("user"),
            Some(22),
            Some("jump@bastion:2200"),
        ),
    ] {
        let mut host = host();
        host.hostname = hostname.into();
        host.username = username.map(Into::into);
        host.port = port;
        host.jump_host = jump.map(Into::into);
        set(&host, Password::new("endpoint-test".into())).unwrap();
        let plan = super::super::direct::ssh_plan(&host, "true".into(), false).unwrap();
        assert!(environment(&plan.args).is_ok());
        clear(&host).unwrap();
    }
}

#[test]
fn invalid_passwords_do_not_enter_store() {
    let host = host();
    for value in ["", "line\nbreak", "nul\0byte"] {
        assert!(set(&host, Password::new(value.into())).is_err());
    }
    assert!(require(&host).is_err());
}
