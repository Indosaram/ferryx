//! Owner-only init.luau. The VM never crosses its dedicated OS thread.
pub mod store;
mod vm;

use std::{path::PathBuf, sync::mpsc, time::{SystemTime, UNIX_EPOCH}};
use store::Registry;

pub const CAPABILITY: &str = "luauScriptingV1";
enum Request { Stop }
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

pub async fn start(registry: Registry) -> Result<Runtime, String> {
    let host = tokio::runtime::Handle::current();
    let (requests, receive) = mpsc::channel();
    std::thread::Builder::new().name("ferryx-luau".into()).spawn(move || {
        if let Err(error) = worker(registry, &host, receive) {
            let message = error.clone();
            if let Err(log_error) = blocking(&host, move || append(store::root()?, message)) {
                tracing::error!(%log_error, %error, "Script failed and its log could not be written");
            }
        }
    }).map_err(|error| error.to_string())?;
    Ok(Runtime { requests })
}

fn worker(registry: Registry, host: &tokio::runtime::Handle, receive: mpsc::Receiver<Request>) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let source = blocking(host, || {
        let root = store::root()?;
        store::source(store::enabled(&root)?, &store::config_path()?)
    })?;
    let Some(source) = source else { return Ok(()); };
    let identity = format!("{:x}", Sha256::digest(source.as_bytes()));
    let log_host = host.clone();
    let log_registry = registry.clone();
    let log_identity = identity.clone();
    let sequence = std::cell::Cell::new(0_u64);
    let vm = vm::Vm::new(identity, std::rc::Rc::new(now), move |message, startup| {
        let id = format!("{log_identity}/init-log/{}", sequence.get());
        sequence.set(sequence.get() + 1);
        let registry = log_registry.clone();
        blocking(&log_host, move || {
            if !startup || registry.store.lock().claim(&id, u64::MAX, now())? {
                append(store::root()?, message)?;
            }
            Ok(())
        })
    }).map_err(|error| error.to_string())?;
    vm.load(&source).map_err(|error| error.to_string())?;
    loop {
        match receive.recv_timeout(vm.next_wait()) {
            Ok(Request::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
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
