//! CLI side of controller pairing: the local credential store and
//! `ferryx browser controllers <pair|list|grant|revoke>`.
//!
//! A paired controller is `<data>/browser/controllers/client/<uuid>.json` (owner-only) holding
//! its id, label and secret; `<data>/browser/controllers/default.json` names the one browser
//! commands use unless `--controller <uuid>` (or `FERRYX_BROWSER_CONTROLLER`) picks another.
//! The store is a convenience for the same OS user, not a boundary against that user's other
//! processes. Secrets are never printed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::catalog::GrantScope;
use super::wire::{AgentRequest, AgentResponse};
use crate::ipc::browser_cli::ControllerAuth;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientRecord {
    id: String,
    label: String,
    secret: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultSelection {
    id: String,
}

/// `<data>/browser/controllers` for the current environment.
pub fn store_dir() -> Option<PathBuf> {
    crate::worktree::naming::data_dir_with_env(|key| std::env::var_os(key))
        .map(|dir| dir.join("browser").join("controllers"))
}

fn client_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("client").join(format!("{id}.json"))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Reads the credential `browser` commands send: `--controller`/`FERRYX_BROWSER_CONTROLLER`
/// when given, else the default selection. `Ok(None)` when nothing is paired yet.
pub fn load_auth(dir: &Path, explicit: Option<&str>) -> Result<Option<ControllerAuth>, String> {
    let id = match explicit {
        Some(id) => id.trim().to_string(),
        None => match std::fs::read(dir.join("default.json")) {
            Ok(bytes) => serde_json::from_slice::<DefaultSelection>(&bytes)
                .map_err(|error| format!("unreadable controller selection: {error}"))?
                .id,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        },
    };
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(format!("`{id}` is not a controller id"));
    }
    let bytes = std::fs::read(client_path(dir, &id))
        .map_err(|error| format!("controller {id} is not paired on this machine: {error}"))?;
    let record: ClientRecord =
        serde_json::from_slice(&bytes).map_err(|error| format!("unreadable controller {id}: {error}"))?;
    Ok(Some(ControllerAuth { id: record.id, secret: record.secret }))
}

/// Picks the `--controller <uuid>` option out of browser arguments.
pub fn take_controller_option(args: &mut Vec<String>) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == "--controller") else {
        return Ok(std::env::var("FERRYX_BROWSER_CONTROLLER").ok().filter(|value| !value.trim().is_empty()));
    };
    let value = args
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| "missing value for --controller".to_string())?;
    args.remove(index);
    args.remove(index);
    Ok(Some(value))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllersCommand {
    Pair { label: String },
    List,
    Grant { origin: Option<String>, scopes: Vec<GrantScope>, tool: Option<String> },
    Revoke,
}

pub const CONTROLLERS_USAGE: &str = "expected `ferryx browser controllers <pair --name <label>|list|grant (--origin <url> --scopes read,interact[,eval] | --tool <browser.tool>)|revoke> [--controller <uuid>]`";

fn option(args: &[String], name: &str) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == name) else { return Ok(None) };
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .map(Some)
        .ok_or_else(|| format!("missing value for {name}"))
}

/// Parses `controllers ...` (arguments after `ferryx browser`).
pub fn parse_controllers(args: &[String]) -> Result<ControllersCommand, String> {
    match args.first().map(String::as_str) {
        Some("pair") => Ok(ControllersCommand::Pair {
            label: option(args, "--name")?.ok_or_else(|| "missing required option --name".to_string())?,
        }),
        Some("list") => Ok(ControllersCommand::List),
        Some("revoke") => Ok(ControllersCommand::Revoke),
        Some("grant") => {
            let origin = option(args, "--origin")?;
            let tool = option(args, "--tool")?;
            let scopes = option(args, "--scopes")?
                .map(|list| {
                    list.split(',')
                        .map(|scope| {
                            serde_json::from_value::<GrantScope>(serde_json::Value::String(scope.trim().to_string()))
                                .map_err(|_| format!("unknown scope `{scope}` (read, interact, eval)"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            match (&origin, &tool) {
                (Some(_), None) if !scopes.is_empty() => Ok(ControllersCommand::Grant { origin, scopes, tool }),
                (None, Some(_)) => Ok(ControllersCommand::Grant { origin, scopes, tool }),
                _ => Err(CONTROLLERS_USAGE.into()),
            }
        }
        _ => Err(CONTROLLERS_USAGE.into()),
    }
}

fn new_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn response_error(response: AgentResponse) -> String {
    match response {
        AgentResponse::Error { code, message, .. } => format!("{code}: {message}"),
        other => format!("unexpected response: {other:?}"),
    }
}

/// Runs a controllers command against the running app. `send` delivers one sub-protocol
/// request (with the credential when given) and returns the app's answer.
pub async fn run_controllers<F, Fut>(
    command: ControllersCommand,
    explicit: Option<&str>,
    send: F,
) -> Result<String, String>
where
    F: Fn(Option<ControllerAuth>, AgentRequest) -> Fut,
    Fut: std::future::Future<Output = Result<AgentResponse, String>>,
{
    let dir = store_dir().ok_or_else(|| "cannot locate the Ferryx data directory".to_string())?;
    let auth = || -> Result<ControllerAuth, String> {
        load_auth(&dir, explicit)?.ok_or_else(|| {
            format!("no paired browser controller: run `{}`", super::protocol::PAIRING_COMMAND)
        })
    };
    match command {
        ControllersCommand::Pair { label } => {
            let record = ClientRecord { id: uuid::Uuid::new_v4().to_string(), label: label.clone(), secret: new_secret() };
            let path = client_path(&dir, &record.id);
            let bytes = serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?;
            write_private(&path, &bytes).map_err(|error| error.to_string())?;
            let request = AgentRequest::Pair { id: record.id.clone(), label, secret: record.secret.clone() };
            let approval = match send(None, request).await? {
                AgentResponse::Pending { approval_id } => approval_id,
                other => {
                    let _ = std::fs::remove_file(&path);
                    return Err(response_error(other));
                }
            };
            eprintln!("waiting for approval in the Ferryx app (request {approval})");
            match send(None, AgentRequest::Await { approval_id: approval }).await? {
                AgentResponse::Resolved { resolution } if resolution.starts_with("approve") => {
                    let selection = serde_json::to_vec(&DefaultSelection { id: record.id.clone() })
                        .map_err(|error| error.to_string())?;
                    write_private(&dir.join("default.json"), &selection).map_err(|error| error.to_string())?;
                    Ok(serde_json::json!({ "type": "paired", "controllerId": record.id }).to_string())
                }
                AgentResponse::Resolved { resolution } => {
                    let _ = std::fs::remove_file(&path);
                    Err(format!("pairing was not approved ({resolution})"))
                }
                other => {
                    let _ = std::fs::remove_file(&path);
                    Err(response_error(other))
                }
            }
        }
        ControllersCommand::Grant { origin, scopes, tool } => {
            let auth = auth()?;
            let approval = match send(Some(auth.clone()), AgentRequest::Grant { origin, scopes, tool }).await? {
                AgentResponse::Pending { approval_id } => approval_id,
                other => return Err(response_error(other)),
            };
            eprintln!("waiting for approval in the Ferryx app (request {approval})");
            match send(Some(auth), AgentRequest::Await { approval_id: approval }).await? {
                AgentResponse::Resolved { resolution } => {
                    Ok(serde_json::json!({ "type": "grant", "resolution": resolution }).to_string())
                }
                other => Err(response_error(other)),
            }
        }
        ControllersCommand::List => match send(Some(auth()?), AgentRequest::Controllers).await? {
            AgentResponse::Controllers { controllers } => Ok(controllers.to_string()),
            other => Err(response_error(other)),
        },
        ControllersCommand::Revoke => {
            let auth = auth()?;
            match send(Some(auth.clone()), AgentRequest::Revoke).await? {
                // The local record stays: later commands get BROWSER_CONTROLLER_REVOKED from the
                // app instead of silently falling back to an unpaired request. Pairing again
                // replaces the default selection.
                AgentResponse::Revoked => {
                    Ok(serde_json::json!({ "type": "revoked", "controllerId": auth.id }).to_string())
                }
                other => Err(response_error(other)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn controllers_commands_parse() {
        assert_eq!(
            parse_controllers(&args(&["pair", "--name", "qa-a"])),
            Ok(ControllersCommand::Pair { label: "qa-a".into() })
        );
        assert_eq!(
            parse_controllers(&args(&["grant", "--origin", "http://127.0.0.1:18761", "--scopes", "read,interact"])),
            Ok(ControllersCommand::Grant {
                origin: Some("http://127.0.0.1:18761".into()),
                scopes: vec![GrantScope::Read, GrantScope::Interact],
                tool: None,
            })
        );
        assert!(parse_controllers(&args(&["grant", "--origin", "http://a.test"])).is_err());
        assert!(parse_controllers(&args(&["grant", "--origin", "http://a.test", "--scopes", "admin"])).is_err());
        assert!(parse_controllers(&args(&["pair"])).is_err());
        assert!(parse_controllers(&args(&["dance"])).is_err());
    }

    #[test]
    fn the_controller_option_is_taken_out_of_the_arguments() {
        let mut list = args(&["ferryx", "browser", "list", "--controller", "abc"]);
        assert_eq!(take_controller_option(&mut list), Ok(Some("abc".into())));
        assert_eq!(list, args(&["ferryx", "browser", "list"]));
        assert!(take_controller_option(&mut args(&["--controller"])).is_err());
    }

    #[test]
    fn the_default_or_explicit_credential_is_loaded_and_a_missing_one_is_none() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        assert!(load_auth(dir, None).unwrap().is_none());
        let id = uuid::Uuid::new_v4().to_string();
        let record = ClientRecord { id: id.clone(), label: "qa".into(), secret: new_secret() };
        write_private(&client_path(dir, &id), &serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(load_auth(dir, Some(&id)).unwrap().unwrap().secret, record.secret);
        assert!(load_auth(dir, None).unwrap().is_none());
        write_private(&dir.join("default.json"), &serde_json::to_vec(&DefaultSelection { id: id.clone() }).unwrap())
            .unwrap();
        assert_eq!(load_auth(dir, None).unwrap().unwrap().id, id);
        assert!(load_auth(dir, Some("not-a-uuid")).is_err());
        assert!(load_auth(dir, Some(&uuid::Uuid::new_v4().to_string())).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(client_path(dir, &id)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(new_secret().len(), 64);
        assert!(!format!("{:?}", load_auth(dir, None).unwrap().unwrap()).contains(&record.secret));
    }
}
