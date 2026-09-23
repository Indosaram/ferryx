#![cfg(feature = "native-terminal")]

use ferryx_lib::terminal::PtyManager;
use portable_pty::CommandBuilder;
use std::time::Duration;

async fn environment_from_child(overrides: &[(&str, &str)]) -> String {
    #[cfg(unix)]
    let mut command = {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args([
            "-c",
            "printf 'IMAGE_PROTOCOL=%s TERM_PROGRAM=%s:END\\n' \"$PI_IMAGE_PROTOCOL\" \"$TERM_PROGRAM\"",
        ]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = CommandBuilder::new("cmd.exe");
        command.args([
            "/D",
            "/C",
            "echo IMAGE_PROTOCOL=%PI_IMAGE_PROTOCOL% TERM_PROGRAM=%TERM_PROGRAM%:END",
        ]);
        command
    };
    command.env_remove("PI_IMAGE_PROTOCOL");
    command.env_remove("TERM_PROGRAM");
    for (name, value) in overrides {
        command.env(*name, *value);
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
    let output = environment_from_child(&[]).await;
    assert!(output.contains("IMAGE_PROTOCOL=kitty"), "{output:?}");
}

#[tokio::test]
async fn native_pty_advertises_the_terminal_identity_that_grants_placeholders() {
    let output = environment_from_child(&[]).await;
    assert!(output.contains("TERM_PROGRAM=ghostty"), "{output:?}");
}

#[tokio::test]
async fn native_pty_preserves_explicit_image_opt_out() {
    let output = environment_from_child(&[("PI_IMAGE_PROTOCOL", "none")]).await;
    assert!(output.contains("IMAGE_PROTOCOL=none "), "{output:?}");
}

#[tokio::test]
async fn native_pty_preserves_an_explicit_terminal_identity() {
    let output = environment_from_child(&[("TERM_PROGRAM", "my-terminal")]).await;
    assert!(output.contains("TERM_PROGRAM=my-terminal"), "{output:?}");
}
