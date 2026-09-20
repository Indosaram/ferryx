use crate::{
    daemon::server::DaemonServer,
    remote::{server::create_remote_router, state::RemoteGatewayState},
    terminal::PtySession,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
pub const DEADLINE: Duration = Duration::from_secs(3);
pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub owner: DaemonServer,
    pub state: Arc<RemoteGatewayState>,
    pub base: String,
    pub token: String,
    pub device: String,
    tasks: tokio::task::JoinSet<()>,
}
impl Fixture {
    pub async fn new() -> Self {
        let (root, owner) = tokio::task::spawn_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let owner = DaemonServer::new_with_paths(
                Some(root.path().join("config")),
                Some(root.path().join("auth")),
            );
            (root, owner)
        })
        .await
        .unwrap();
        let state = owner.remote_state().clone();
        let pin = state
            .auth_manager
            .create_scoped_pairing_code(
                crate::remote::DevicePermission::Control,
                crate::remote::DeviceAccessScope::Machine,
            )
            .unwrap();
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "saturated")
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = create_remote_router(state.clone());
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            root,
            owner,
            state,
            base,
            token,
            device: device.id,
            tasks,
        }
    }
    pub async fn create(&self) -> Value {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let text = client
            .post(format!("{}/api/v1/workspace/projects", self.base))
            .bearer_auth(&self.token)
            .body(json!({"requestId":uuid::Uuid::new_v4(),"repoPath":self.root.path()}).to_string())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        let project: Value = serde_json::from_str(&text).unwrap();
        let text = client.post(format!("{}/api/v1/sessions", self.base)).bearer_auth(&self.token)
            .body(json!({"requestId":uuid::Uuid::new_v4(),"workspaceId":project["workspaceId"],"worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}}).to_string())
            .send().await.unwrap().error_for_status().unwrap().text().await.unwrap();
        serde_json::from_str(&text).unwrap()
    }
    pub async fn attach(&self, session: &Value) -> Socket {
        let target = &session["target"];
        let mut request = format!(
            "{}/api/v1/terminal/{}?daemonEpoch={}",
            self.base.replace("http:", "ws:"),
            target["sessionId"].as_str().unwrap(),
            target["daemonEpoch"].as_str().unwrap()
        )
        .into_client_request()
        .unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(request))
            .await
            .expect("new generation admission must not wait for saturated PTY")
            .unwrap();
        let first = tokio::time::timeout(DEADLINE, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, Message::Text(_)));
        socket
    }
    pub async fn cleanup(mut self) {
        self.state.auth_manager.revoke_device(&self.device);
        let backend = self.owner.terminal_service();
        for id in backend.list_sessions() {
            // A child may exit naturally when its control socket drops on a
            // failed assertion; lifecycle removal races this session listing.
            let pty = backend.get_session(&id);
            let pid = pty.as_ref().and_then(|pty| pty.pid());
            if pty.is_some() {
                backend.close_session(&id).await.unwrap();
            }
            self.state
                .machine_services
                .as_ref()
                .unwrap()
                .sessions
                .wait_machine_lifecycle(&id)
                .await
                .unwrap();
            if let Some(pty) = pty {
                assert!(pty.is_reaped());
            }
            eprintln!("A10 cleanup lifecycle drained pid={pid:?} id={id}");
        }
        self.tasks.shutdown().await;
        drop(self.owner);
        drop(self.state);
        tokio::task::spawn_blocking(move || self.root.close().unwrap())
            .await
            .unwrap();
        eprintln!("A10 cleanup listener joined; private root removed");
    }
}

pub struct Held {
    pub pty: Arc<PtySession>,
    pub control: tokio::net::UnixStream,
    pub accepted: u64,
    pub pid: u32,
}
impl Held {
    pub async fn start(fixture: &Fixture, session: &Value, socket: &mut Socket) -> Self {
        let path = fixture.root.path().join(uuid::Uuid::new_v4().to_string());
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let script = r#"use IO::Socket::UNIX; my $s=IO::Socket::UNIX->new(Peer=>$ARGV[0]) or die $!; $s->autoflush(1); print $s pack('L<',$$); read($s,my $go,1)==1 or die; my $count=0; my $bad=0; while (1) { my $n=sysread(STDIN,my $b,4096); $n or die; $count+=$n; $bad+=($b=~tr/LQZ/LQZ/); last if index($b,'!')>=0; } print $s pack('Q<Q<',$count,$bad); read($s,$go,1);"#;
        let command = format!(
            "stty raw -echo; exec /usr/bin/perl -e '{}' '{}'\r",
            script.replace('\'', "'\\''"),
            path.display()
        );
        socket
            .send(Message::Binary(command.into_bytes().into()))
            .await
            .unwrap();
        let (mut control, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let pid = control.read_u32_le().await.unwrap();
        drop(listener);
        std::fs::remove_file(path).unwrap();
        let pty = fixture
            .owner
            .terminal_service()
            .get_session(session["target"]["sessionId"].as_str().unwrap())
            .unwrap();
        assert_eq!(pty.pid(), Some(pid));
        let fd = pty.raw_master_fd().unwrap();
        let bytes = [b'x'; 65536];
        let mut accepted = 0u64;
        loop {
            // SAFETY: FFI buffer bounds: bytes is live for the call, fd is owned
            // by pty retained above, and write only reads bytes.len() bytes.
            let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
            if n < 0 {
                assert_eq!(
                    std::io::Error::last_os_error().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                break;
            }
            accepted += u64::try_from(n).unwrap();
            assert!(accepted < 4 * 1024 * 1024);
        }
        eprintln!("A10 kernel WouldBlock original_pid={pid} accepted={accepted}");
        Self {
            pty,
            control,
            accepted,
            pid,
        }
    }
    pub async fn drain(mut self) {
        self.control.write_all(&[1]).await.unwrap();
        self.pty.write_input_cancellable(b"!").await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(10), self.control.read_u64_le())
            .await
            .unwrap()
            .unwrap();
        let forbidden = self.control.read_u64_le().await.unwrap();
        assert_eq!(
            received,
            self.accepted + 1,
            "cancelled queued suffix reached original child"
        );
        assert_eq!(forbidden, 0);
        assert_eq!(self.pty.pid(), Some(self.pid));
        eprintln!(
            "A10 drained original_pid={} received={received} forbidden={forbidden}",
            self.pid
        );
    }
}
