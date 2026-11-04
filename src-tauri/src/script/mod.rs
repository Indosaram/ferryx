//! Owner-only init.luau. The VM never crosses its dedicated OS thread.
pub mod store;
pub mod api;
pub mod control;
mod vm;

use std::{path::PathBuf, sync::mpsc, time::{SystemTime, UNIX_EPOCH}};
use store::Registry;

pub const CAPABILITY: &str = "luauScriptingV1";
use control::Request;
pub struct Runtime { requests: mpsc::Sender<Request> }
impl Drop for Runtime {
    fn drop(&mut self) {
        if self.requests.send(Request::Stop).is_err() {
            tracing::debug!("Script worker already stopped");
        }
    }
}

pub(super) fn blocking<T: Send + 'static>(
    host: &tokio::runtime::Handle,
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    host.block_on(crate::ipc::run_blocking(move || operation().map_err(crate::ipc::IpcError::internal)))
        .map_err(|error| error.to_string())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn append(root: PathBuf, message: String) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true)
        .open(root.join("script.log")).map_err(|error| error.to_string())?;
    writeln!(file, "{message}").map_err(|error| error.to_string())
}

pub async fn start(registry: Registry, effects: control::Host) -> Result<Runtime, String> {
    let host = tokio::runtime::Handle::current();
    let (requests, receive) = mpsc::channel();
    control::publish(requests.clone())?;
    std::thread::Builder::new().name("ferryx-luau".into()).spawn(move || {
        if let Err(error) = worker(registry, &host, receive, effects) {
            let message = error.clone();
            if let Err(log_error) = blocking(&host, move || append(store::root()?, message)) {
                tracing::error!(%log_error, %error, "Script failed and its log could not be written");
            }
        }
    }).map_err(|error| error.to_string())?;
    Ok(Runtime { requests })
}

fn load(registry: &Registry, host: &tokio::runtime::Handle, effects: control::Host) -> Result<Option<vm::Vm>, String> {
    use sha2::{Digest, Sha256};
    let source = blocking(host, || {
        let root = store::root()?;
        store::source(store::enabled(&root)?, &store::config_path()?)
    })?;
    let Some(source) = source else { return Ok(None); };
    let identity = format!("{:x}", Sha256::digest(source.as_bytes()));
    let log_host = host.clone();
    let log_registry = registry.clone();
    let log_identity = identity.clone();
    let sequence = std::cell::Cell::new(0_u64);
    let effect_registry = registry.clone();
    let effect_host = host.clone();
    let effect_identity = identity.clone();
    let effect_sequence = std::cell::Cell::new(0_u64);
    let vm = vm::Vm::with_host(identity, std::rc::Rc::new(now), move |message, startup| {
        let id = format!("{log_identity}/init-log/{}", sequence.get());
        sequence.set(sequence.get() + 1);
        let registry = log_registry.clone();
        blocking(&log_host, move || {
            if !startup || registry.store.lock().claim(&id, u64::MAX, now())? {
                append(store::root()?, message)?;
            }
            Ok(())
        })
    }, std::rc::Rc::new(move |name, value| {
        let id = format!("{effect_identity}/effect/{}", effect_sequence.get());
        effect_sequence.set(effect_sequence.get() + 1);
        let registry = effect_registry.clone();
        if name != "read" && !blocking(&effect_host, move || registry.store.lock().claim(&id, u64::MAX, now()))? {
            return Ok(serde_json::Value::Null);
        }
        effects(name, value)
    })).map_err(|error| error.to_string())?;
    {
        let _execution = registry.execution.lock();
        vm.load(&source).map_err(|error| error.to_string())?;
    }
    Ok(Some(vm))
}

fn worker(registry: Registry, host: &tokio::runtime::Handle, receive: mpsc::Receiver<Request>, effects: control::Host) -> Result<(), String> {
    let mut vm = match load(&registry, host, effects.clone()) {
        Ok(vm) => vm,
        Err(error) => { blocking(host, move || append(store::root()?, error))?; None }
    };
    loop {
        let wait = vm.as_ref().map(vm::Vm::next_wait).unwrap_or(std::time::Duration::from_secs(3600));
        match receive.recv_timeout(wait) {
            Ok(Request::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Request::Control(action, reply)) => {
                let result = (|| {
                    match action {
                        control::Action::SetEnabled { enabled } => {
                            blocking(host, move || control::set_enabled(enabled))?;
                            vm = load(&registry, host, effects.clone())?;
                        }
                        control::Action::Reload => { vm = load(&registry, host, effects.clone())?; }
                        control::Action::Key { keys, session } => {
                            let _execution = registry.execution.lock();
                            if !registry.store.lock().paused {
                                if let Some(vm) = &vm { vm.key(&keys, &session).map_err(|e| e.to_string())?; }
                            }
                        }
                        control::Action::Status => {}
                    }
                    let bindings: Vec<_> = vm.as_ref().map(|vm| vm.api.bindings.borrow().iter()
                        .map(|(keys, (overrides, _))| serde_json::json!({"keys":keys,"override":overrides})).collect()).unwrap_or_default();
                    let enabled = blocking(host, || store::enabled(&store::root()?))?;
                    let log = blocking(host, || match std::fs::read_to_string(store::root()?.join("script.log")) {
                        Ok(log) => Ok(log),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                        Err(e) => Err(e.to_string()),
                    })?;
                    Ok(serde_json::json!({"enabled":enabled,"bindings":bindings,"log":log}))
                })();
                if reply.send(result).is_err() { tracing::debug!("Script caller disconnected"); }
            }
            Ok(Request::Event(name, value)) => {
                let _execution = registry.execution.lock();
                if !registry.store.lock().paused {
                    if let Some(vm) = &vm {
                        if let Err(error) = vm.event(&name, &value) {
                            let error = error.to_string();
                            blocking(host, move || append(store::root()?, error))?;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let Some(vm) = &vm else { continue; };
                if let Err(error) = vm.fire(&registry, host) {
                    blocking(host, move || append(store::root()?, error))?;
                }
            }
        }
    }
}

pub fn run_cli(args: &[String]) -> i32 {
    if args.get(2).map(String::as_str) != Some("log") || args.len() != 3 {
        eprintln!("Usage: ferryx-cli script log");
        return 2;
    }
    let result = store::root().and_then(|root| match std::fs::read_to_string(root.join("script.log")) {
        Ok(log) => Ok(log),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    });
    match result {
        Ok(log) => { print!("{log}"); 0 }
        Err(error) => { eprintln!("{error}"); 1 }
    }
}

#[cfg(test)]
mod tests;
