#![cfg(desktop)]

use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use tauri_plugin_updater::UpdaterExt;

const SIGNED_ARTIFACT: &[u8] = include_bytes!("../../scripts/fixtures/updater/Ferryx.app.tar.gz");
const ARTIFACT_SIGNATURE: &str =
    include_str!("../../scripts/fixtures/updater/Ferryx.app.tar.gz.sig");

fn configured_pubkey() -> String {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse tauri.conf.json");
    config["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("plugins.updater.pubkey")
        .to_string()
}

// The manifest must embed the artifact URL of the very server that serves it, so the listener is
// bound first to learn its port, and only then is the manifest rendered and handed to the thread.
fn serve_release(version: &str, signature: &str, artifact: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let address = listener.local_addr().expect("local addr");
    let base = format!("http://{address}");
    let manifest = manifest_for(&base, version, signature);

    thread::spawn(move || {
        for stream in listener.incoming().take(16) {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 1024];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body: Vec<u8> = if request.contains("/latest.json") {
                manifest.clone().into_bytes()
            } else {
                artifact.clone()
            };
            respond(&mut stream, &body);
        }
    });

    format!("{base}/latest.json")
}

fn respond(stream: &mut TcpStream, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn manifest_for(base: &str, version: &str, signature: &str) -> String {
    let artifact_url = format!("{base}/Ferryx_universal.app.tar.gz");
    let platform = json!({ "signature": signature, "url": artifact_url });
    json!({
        "version": version,
        "notes": "endpoint contract fixture",
        "pub_date": "2026-08-26T00:00:00Z",
        "platforms": {
            "darwin-aarch64": platform,
            "darwin-x86_64": platform,
            "windows-x86_64": platform,
            "linux-x86_64": platform,
        }
    })
    .to_string()
}

fn updater_app() -> tauri::App<tauri::test::MockRuntime> {
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        json!({
            "endpoints": ["https://example.invalid/latest.json"],
            "pubkey": configured_pubkey(),
            "windows": { "installMode": "passive" },
        }),
    );

    tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .expect("build mock app with the updater plugin")
}

#[test]
fn a_newer_signed_release_is_offered_and_its_signature_accepted() {
    let app = updater_app();
    let manifest_url = serve_release(
        "9999.12.31",
        ARTIFACT_SIGNATURE.trim(),
        SIGNED_ARTIFACT.to_vec(),
    );

    tauri::async_runtime::block_on(async move {
        let updater = app
            .updater_builder()
            .pubkey(configured_pubkey())
            .endpoints(vec![manifest_url.parse().expect("endpoint url")])
            .expect("endpoints accepted")
            .build()
            .expect("updater built");

        let update = updater
            .check()
            .await
            .expect("check reaches the endpoint")
            .expect("a newer version is offered");

        assert_eq!(update.version, "9999.12.31");
        assert_eq!(
            update.current_version,
            app.package_info().version.to_string()
        );

        let bytes = update
            .download(|_, _| {}, || {})
            .await
            .expect("the plugin accepts the real minisign signature");

        assert_eq!(bytes, SIGNED_ARTIFACT);
    });
}

#[test]
fn an_older_release_is_not_offered() {
    let app = updater_app();
    let manifest_url = serve_release("0.0.1", ARTIFACT_SIGNATURE.trim(), SIGNED_ARTIFACT.to_vec());

    tauri::async_runtime::block_on(async move {
        let update = app
            .updater_builder()
            .pubkey(configured_pubkey())
            .endpoints(vec![manifest_url.parse().expect("endpoint url")])
            .expect("endpoints accepted")
            .build()
            .expect("updater built")
            .check()
            .await
            .expect("check reaches the endpoint");

        assert!(update.is_none(), "an older version must not be offered");
    });
}

#[test]
fn a_tampered_artifact_is_rejected_by_the_plugin() {
    let app = updater_app();
    let mut tampered = SIGNED_ARTIFACT.to_vec();
    tampered.extend_from_slice(b"tampered");
    let manifest_url = serve_release("9999.12.31", ARTIFACT_SIGNATURE.trim(), tampered);

    tauri::async_runtime::block_on(async move {
        let update = app
            .updater_builder()
            .pubkey(configured_pubkey())
            .endpoints(vec![manifest_url.parse().expect("endpoint url")])
            .expect("endpoints accepted")
            .build()
            .expect("updater built")
            .check()
            .await
            .expect("check reaches the endpoint")
            .expect("a newer version is offered");

        let outcome = update.download(|_, _| {}, || {}).await;

        assert!(
            outcome.is_err(),
            "a payload that does not match the signature must be refused"
        );
    });
}
