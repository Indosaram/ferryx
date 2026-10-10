use std::path::PathBuf;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use snow::Builder;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedJson {
    machine_id: String,
    session_id: String,
    enrollment_epoch: String,
    frame: Vec<u8>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachIdentityFile {
    private_key: String,
}

fn decode_key(value: &str) -> Result<[u8; 32], String> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|e| format!("Base64 decode error: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "Key length must be exactly 32 bytes".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let captured_json_path = args.next().expect("Arg 1: path to captured-noise-msg1.json");
    let host_remote_dir = args.next().expect("Arg 2: canonical host remote dir (~/.ferryx/remote)");

    let json_bytes = std::fs::read(&captured_json_path)
        .map_err(|e| format!("Cannot read json at {}: {e}", captured_json_path))?;
    let captured: CapturedJson = serde_json::from_slice(&json_bytes)?;

    let host_dir = PathBuf::from(&host_remote_dir);
    let attach_path = host_dir.join("attach-identity.json");
    let auth_path = host_dir.join("remote-auth.json");

    let attach_bytes = std::fs::read(&attach_path)
        .map_err(|e| format!("Cannot read attach-identity at {}: {e}", attach_path.display()))?;
    let attach_file: AttachIdentityFile = serde_json::from_slice(&attach_bytes)?;
    let responder_private = decode_key(&attach_file.private_key)?;

    let msg1 = if captured.frame.len() == 100 {
        let len = u32::from_be_bytes(captured.frame[0..4].try_into().unwrap()) as usize;
        if len == 96 {
            &captured.frame[4..100]
        } else {
            &captured.frame[4..]
        }
    } else if captured.frame.len() == 96 {
        &captured.frame[..]
    } else {
        panic!("Captured frame length {} is unexpected", captured.frame.len());
    };

    let prologue = format!(
        "ferryx-attach-v1:{}:{}:{}",
        captured.machine_id, captured.session_id, captured.enrollment_epoch
    ).into_bytes();

    eprintln!("[PROBE]: Machine ID = {}", captured.machine_id);
    eprintln!("[PROBE]: Session ID = {}", captured.session_id);
    eprintln!("[PROBE]: Enrollment Epoch = {}", captured.enrollment_epoch);
    eprintln!("[PROBE]: Prologue String = {}", std::str::from_utf8(&prologue).unwrap_or("<binary>"));

    let params: snow::params::NoiseParams = "Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?;
    let mut responder = Builder::new(params)
        .local_private_key(&responder_private)
        .prologue(&prologue)
        .build_responder()?;

    let mut payload = vec![0u8; 65535];
    match responder.read_message(msg1, &mut payload) {
        Ok(read_len) => {
            println!("AEAD_DECRYPT: PASS (payload_len={})", read_len);
            if let Some(remote_static) = responder.get_remote_static() {
                let client_pub_b64 = STANDARD.encode(remote_static);
                println!("CLIENT_PUBLIC_KEY: {}", client_pub_b64);

                let auth_manager = ferryx_lib::remote::auth::AuthManager::with_persistence(Some(auth_path.clone()));
                let authorizes_key = auth_manager.authorizes_attach_key(&client_pub_b64);
                let authorizes_bytes = auth_manager.authorizes_attach_key_bytes(remote_static);
                let device_for_key = auth_manager.device_for_attach_key(&client_pub_b64);
                let live_pairing_accepts = auth_manager.live_pairing_capability_accepts_attach_key(&client_pub_b64);

                println!("AUTH_MANAGER_LOADED: {}", auth_path.display());
                println!("AUTH_MANAGER_DEVICE_MATCH: {}", device_for_key.is_some());
                println!("AUTH_MANAGER_LIVE_PAIRING_ACCEPTS: {}", live_pairing_accepts);
                println!("AUTH_MANAGER_AUTHORIZES_KEY: {}", authorizes_key);
                println!("AUTH_MANAGER_AUTHORIZES_BYTES: {}", authorizes_bytes);
            } else {
                println!("CLIENT_PUBLIC_KEY: FAIL (remote static absent)");
            }
        }
        Err(error) => {
            println!("AEAD_DECRYPT: FAIL ({:?})", error);
        }
    }

    Ok(())
}
