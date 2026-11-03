use ferryx_rd_windows_stdio_contract::{
    agent_input::ScreenInfo, agent_server::AgentServerBackend, mcp_server,
};
use std::{io, process::Stdio, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
#[path = "../../../src/mcp_stdio.rs"]
mod native;
use ferryx_rd_windows_stdio_contract::agent_server;
mod observed;
use observed::run_mcp_io_until;

struct Backend;
impl AgentServerBackend for Backend {
    fn send_input_event(&self, _: ferryx_rd_proto::InputEvent) -> Result<(), String> {
        Ok(())
    }
    fn get_screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 1920,
            height: 1080,
            scale: 1.0,
            connected_host: "fixture".into(),
        }
    }
    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        None
    }
}

#[cfg(windows)]
#[path = "../../../src/mcp_windows_relay.rs"]
mod relay;

fn main() -> io::Result<()> {
    #[cfg(windows)]
    if relay::run_if_requested()? {
        return Ok(());
    }
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let mode = std::env::args().nth(1);
        if mode.as_deref() == Some("cancel-server") {
            let address = std::env::args().nth(2).unwrap();
            let mut control = tokio::net::TcpStream::connect(address).await?;
            let (tx, rx) = tokio::sync::watch::channel(false);
            let transport = native::run(Arc::new(Backend), rx);
            let cancel = async {
                control.read_u8().await?;
                tx.send(true).map_err(io::Error::other)?;
                Ok::<_,io::Error>(())
            };
            let (result, cancelled) = tokio::join!(transport, cancel);
            result?; cancelled?;
            return Ok(());
        }
        if mode.as_deref() == Some("server") {
            let (_tx, rx) = tokio::sync::watch::channel(false);
            return mcp_server::run_mcp_stdio_until(Arc::new(Backend), rx).await;
        }
        let mut child = tokio::process::Command::new(std::env::current_exe()?)
            .arg("server").stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn()?;
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let mut input = child.stdin.take().unwrap();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            for (id, request) in [(1, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#),
                (2, r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"remote_get_screen_info"}}"#)] {
                input.write_all(format!("{request}\n").as_bytes()).await?;
                let mut line = String::new();
                output.read_line(&mut line).await?;
                assert!(line.ends_with('\n'), "newline framing missing");
                let value: serde_json::Value = serde_json::from_str(&line)?;
                assert_eq!(value["id"], id);
                assert_eq!(value["jsonrpc"], "2.0");
                if id == 1 { assert_eq!(value["result"]["protocolVersion"], "2024-11-05"); }
                else {
                    let screen: serde_json::Value = serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap())?;
                    assert_eq!(screen["width"],1920);
                }
            }
            drop(input);
            let mut tail = Vec::new();
            output.read_to_end(&mut tail).await?;
            assert!(tail.is_empty());
            assert!(child.wait().await?.success());
            Ok::<_,io::Error>(())
        }).await;
        match result {
            Ok(result) => result?,
            Err(error) => { child.kill().await?; return Err(io::Error::other(error)); }
        }
        println!("PROTOCOL_EOF_GREEN");
        observed::cancellation_contract("READ").await?;
        observed::cancellation_contract("WRITE").await?;
        Ok(())
    })
}
