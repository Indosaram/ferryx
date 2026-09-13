#![cfg(feature = "native-terminal")]

use ferryx_lib::terminal::PtyManager;
use portable_pty::CommandBuilder;
use std::time::Duration;

async fn image_protocol_from_child(explicit: Option<&str>) -> String {
    #[cfg(unix)]
    let mut command = {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "printf 'IMAGE_PROTOCOL=%s:END\\n' \"$PI_IMAGE_PROTOCOL\""]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = CommandBuilder::new("cmd.exe");
        command.args(["/D", "/C", "echo IMAGE_PROTOCOL=%PI_IMAGE_PROTOCOL%:END"]);
        command
    };
    command.env_remove("PI_IMAGE_PROTOCOL");
    if let Some(value) = explicit {
        command.env("PI_IMAGE_PROTOCOL", value);
    }
    let manager = PtyManager::new();
    let spawn_manager = manager.clone();
    let (id, mut output) = tokio::task::spawn_blocking(move || spawn_manager.spawn(command, 80, 24))
        .await
        .expect("spawn task")
        .expect("isolated PTY");
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = output.recv().await {
            bytes.extend(chunk);
            if bytes.windows(4).any(|window| window == b":END") {
                break;
            }
        }
        String::from_utf8(bytes).expect("environment output")
    })
    .await;
    manager.close_session(&id).await.expect("close test PTY");
    result.expect("child output before deadline")
}

#[tokio::test]
async fn native_pty_advertises_supported_images_to_pi_clients() {
    // Given an unconfigured client, when it starts in a native PTY, then image output is enabled.
    let output = image_protocol_from_child(None).await;
    assert!(output.contains("IMAGE_PROTOCOL=kitty:END"), "{output:?}");
}

#[tokio::test]
async fn native_pty_preserves_explicit_image_opt_out() {
    // Given an explicit opt-out, when the client starts, then the terminal preserves that choice.
    let output = image_protocol_from_child(Some("none")).await;
    assert!(output.contains("IMAGE_PROTOCOL=none:END"), "{output:?}");
}
