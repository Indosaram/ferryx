use ferryx_lib::ssh::bridge::BridgeConnection;
use ferryx_lib::ssh::helper_setup::HelperLocation;
use ferryx_lib::ssh::runtime::{RemoteEnvironment, RemoteExecutor, RemotePlatform};
use ferryx_lib::ssh::direct;
use ferryx_lib::ssh::{SshAuthMethod, SshHost, SshHostSource};
use tokio::io::AsyncBufReadExt;
use std::process::Stdio;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = required(&mut args, "mode")?;
    let helper = required(&mut args, "helper")?;
    let root = required(&mut args, "root")?;
    let ssh_direct_args = if matches!(mode.as_str(), "ssh-direct" | "ssh") {
        Some((
            required(&mut args, "config")?,
            required(&mut args, "known_hosts")?,
            required(&mut args, "key")?,
        ))
    } else {
        None
    };
    let rounds: usize = args.next().unwrap_or_else(|| "1".into()).parse()?;
    if args.next().is_some() || rounds == 0 {
        return Err("usage: bridge-driver <direct|powershell-direct|ssh|ssh-direct> <helper> <root> [mode args] [rounds>=1]".into());
    }

    let location = HelperLocation { executable: helper, root };
    let direct_helper = if matches!(mode.as_str(), "direct" | "powershell-direct" | "ssh" | "ssh-direct") {
        Some(std::env::var("FERRYX_DRIVER_HELPER")?)
    } else {
        None
    };
    let mut daemon = if let Some(helper) = direct_helper.as_deref() {
        let mut child = tokio::process::Command::new(helper)
            .args(["daemon", "--root", &location.root, "--host-id", "bridge-driver-probe"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or("helper daemon stdout unavailable")?;
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let ready = tokio::time::timeout(DEADLINE, lines.next_line()).await??;
        if !ready.as_deref().unwrap_or_default().contains("\"event\":\"ready\"") {
            return Err(format!("helper daemon failed readiness: {ready:?}").into());
        }
        println!("DIRECT_DAEMON_READY {ready:?}");
        Some(child)
    } else {
        None
    };

    for round in 1..=rounds {
        tokio::time::timeout(DEADLINE, async {
            let mut connection = match mode.as_str() {
                "direct" => {
                    let child = tokio::process::Command::new(
                        direct_helper.as_deref().expect("direct helper was parsed"),
                    )
                        .args(["bridge", "--stdio", "--root", &location.root])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .spawn()?;
                    BridgeConnection::from_child(child)?
                }
                "powershell-direct" => {
                    let env = RemoteEnvironment {
                        platform: RemotePlatform::Windows,
                        executor: RemoteExecutor::Powershell,
                        version: "bridge-driver".into(),
                        home: r"C:\Users\sook".into(),
                        temp: r"C:\Windows\Temp".into(),
                        git: false,
                    };
                    let script = RemoteExecutor::Powershell.command(&direct::bridge_command(&env, &location));
                    let encoded = script
                        .split_once(" -EncodedCommand ")
                        .map(|(_, encoded)| encoded)
                        .expect("production PowerShell command contains encoded script");
                    let child = tokio::process::Command::new("powershell.exe")
                        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .spawn()?;
                    BridgeConnection::from_child(child)?
                }
                "ssh" => {
                    let (_, _, key) = ssh_direct_args.as_ref().expect("SSH args were parsed");
                    let host = SshHost {
                        id: "bridge-driver".into(),
                        label: "BridgeConnection driver".into(),
                        hostname: "127.0.0.1".into(),
                        username: Some("sook".into()),
                        port: Some(40222),
                        identity_file: Some(key.clone()),
                        jump_host: None,
                        source: SshHostSource::Manual,
                        auth_method: SshAuthMethod::Key,
                        disabled: Some(false),
                    };
                    BridgeConnection::spawn(&host, &windows_environment(), &location).await?
                }
                "ssh-direct" => {
                    let (config, known_hosts, key) = ssh_direct_args
                        .as_ref()
                        .expect("ssh-direct arguments were parsed");
                    let remote_command = RemoteExecutor::Powershell.command(&direct::bridge_command(&windows_environment(), &location));
                    let child = tokio::process::Command::new(r"C:\Windows\System32\OpenSSH\ssh.exe")
                        .args([
                            "-F",
                            config,
                            "-o",
                            &format!("UserKnownHostsFile={known_hosts}"),
                            "-p",
                            "40222",
                            "-i",
                            key,
                            "sook@127.0.0.1",
                            &remote_command,
                        ])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .spawn()?;
                    BridgeConnection::from_child(child)?
                }
                _ => return Err(std::io::Error::other(format!("unknown mode {mode}; expected direct, powershell-direct, ssh, or ssh-direct")).into()),
            };
            let pid = connection.child_id();
            let handshake = connection.handshake().await?;
            connection.close().await?;
            println!(
                "BRIDGE_OK round={round} pid={pid:?} host_id={} owner_id={} epoch={:?}",
                handshake.host_id, handshake.owner_id, handshake.epoch
            );
            Ok::<_, ferryx_lib::ssh::bridge::BridgeError>(())
        })
        .await??;
    }
    if let Some(daemon) = daemon.as_mut() {
        daemon.kill().await?;
        let status = daemon.wait().await?;
        println!("DIRECT_DAEMON_STOPPED status={status}");
    }
    Ok(())
}

fn required(
    args: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("missing {name} argument").into())
}

fn windows_environment() -> RemoteEnvironment {
    RemoteEnvironment {
        platform: RemotePlatform::Windows,
        executor: RemoteExecutor::Powershell,
        version: "bridge-driver".into(),
        home: r"C:\Users\sook".into(),
        temp: r"C:\Windows\Temp".into(),
        git: false,
    }
}
