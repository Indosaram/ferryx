//! Native, read-only machine directory browsing. No shell or desktop filesystem IPC.
use super::{
    auth::{DeviceAccessScope, DevicePermission},
    machine_protocol::{Directories, DirectoryEntry},
    state::RemoteGatewayState,
};
use axum::{
    extract::{Extension, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const SCAN_BUDGET: Duration = Duration::from_secs(5);
const JSON_LIMIT: usize = 256 * 1024;
type FsResult<T> = Result<T, FsError>;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FsError {
    Invalid,
    Missing,
    Denied,
    Unsupported,
    Timeout,
    Unavailable,
}
impl FsError {
    fn response(self) -> Response {
        let (status, code) = match self {
            Self::Invalid => (StatusCode::BAD_REQUEST, "INVALID_PATH"),
            Self::Missing => (StatusCode::NOT_FOUND, "DIRECTORY_NOT_FOUND"),
            Self::Denied => (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
            Self::Unsupported => (StatusCode::UNPROCESSABLE_ENTITY, "UNSUPPORTED_PATH"),
            Self::Timeout => (StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "HOST_UNAVAILABLE"),
        };
        super::server::machine_error(status, code)
    }
}
fn io_error(error: std::io::Error) -> FsError {
    match error.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => FsError::Missing,
        std::io::ErrorKind::PermissionDenied => FsError::Denied,
        std::io::ErrorKind::TimedOut => FsError::Timeout,
        _ => FsError::Unavailable,
    }
}

fn decode(value: &str) -> FsResult<String> {
    let mut output = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        output.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hi = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(FsError::Invalid)?;
                let lo = bytes
                    .next()
                    .and_then(|b| (b as char).to_digit(16))
                    .ok_or(FsError::Invalid)?;
                (hi * 16 + lo) as u8
            }
            byte => byte,
        });
    }
    String::from_utf8(output).map_err(|_| FsError::Invalid)
}
pub(super) fn query(raw: Option<&str>) -> FsResult<(String, bool)> {
    let raw = raw.unwrap_or("");
    if raw.len() > 3 * 4096 + 64 {
        return Err(FsError::Invalid);
    }
    let (mut path, mut hidden) = (None, None);
    for field in raw.split('&').filter(|field| !field.is_empty()) {
        let (key, value) = field.split_once('=').ok_or(FsError::Invalid)?;
        let value = decode(value)?;
        match decode(key)?.as_str() {
            "path" if path.is_none() => path = Some(value),
            "includeHidden" if hidden.is_none() => {
                hidden = Some(match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(FsError::Invalid),
                })
            }
            _ => return Err(FsError::Invalid),
        }
    }
    Ok((path.unwrap_or_default(), hidden.unwrap_or(false)))
}

fn validate(path: &str) -> FsResult<()> {
    if path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(FsError::Invalid);
    }
    #[cfg(windows)]
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err(FsError::Unsupported);
    }
    if path.contains("://") {
        return Err(FsError::Unsupported);
    }
    Ok(())
}
/// Canonicalizes a native directory; call only inside run_blocking. Home is not a jail.
/// Registration may reuse this resolver but must independently reject roots and revalidate.
pub(crate) fn resolve_directory(input: &str, home: &Path) -> FsResult<PathBuf> {
    validate(input)?;
    let expanded = match input {
        "" | "~" => home.to_owned(),
        value if value.starts_with("~/") => home.join(&value[2..]),
        value => PathBuf::from(value),
    };
    if !expanded.is_absolute() {
        return Err(FsError::Invalid);
    }
    validate(expanded.to_str().ok_or(FsError::Unsupported)?)?;
    let canonical = std::fs::canonicalize(expanded).map_err(io_error)?;
    // Windows canonicalize produces extended drive paths. Strip only the native
    // drive prefix, never a UNC prefix, before returning navigable paths.
    #[cfg(windows)]
    let canonical = {
        let text = canonical.to_str().ok_or(FsError::Unsupported)?;
        if text.starts_with(r"\\?\UNC\") {
            return Err(FsError::Unsupported);
        }
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text))
    };
    validate(canonical.to_str().ok_or(FsError::Unsupported)?)?;
    if !std::fs::metadata(&canonical).map_err(io_error)?.is_dir() {
        return Err(FsError::Missing);
    }
    Ok(canonical)
}

pub(super) fn scan(
    input: &str,
    hidden: bool,
    home: &Path,
    cancelled: &AtomicBool,
    started: Instant,
) -> FsResult<Directories> {
    scan_after_resolution(input, hidden, home, cancelled, started, || {})
}

pub(super) fn scan_after_resolution(
    input: &str,
    hidden: bool,
    home: &Path,
    cancelled: &AtomicBool,
    started: Instant,
    resolved: impl FnOnce(),
) -> FsResult<Directories> {
    scan_with_elapsed(input, hidden, home, cancelled, || started.elapsed(), resolved)
}

// Keep native enumeration and all budget decisions shared; tests freeze only
// elapsed monotonic time so the child cutoff cannot be masked by the deadline.
pub(super) fn scan_with_elapsed(
    input: &str,
    hidden: bool,
    home: &Path,
    cancelled: &AtomicBool,
    elapsed: impl Fn() -> Duration,
    resolved: impl FnOnce(),
) -> FsResult<Directories> {
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    let home = resolve_directory("", home)?;
    let root = resolve_directory(input, &home)?;
    resolved();
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    let mut listing = Directories {
        path: root.to_str().ok_or(FsError::Unsupported)?.into(),
        parent_path: root
            .parent()
            .map(|p| p.to_str().map(str::to_owned).ok_or(FsError::Unsupported))
            .transpose()?,
        home_path: home.to_str().ok_or(FsError::Unsupported)?.into(),
        entries: vec![],
        truncated: elapsed() >= SCAN_BUDGET,
    };
    if listing.truncated {
        return Ok(listing);
    }
    let children = std::fs::read_dir(&root).map_err(io_error)?;
    let mut size = serde_json::to_vec(&listing)
        .map_err(|_| FsError::Unavailable)?
        .len();
    for (index, child) in children.enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Err(FsError::Timeout);
        }
        if elapsed() >= SCAN_BUDGET || index >= 10_000 {
            listing.truncated = true;
            break;
        }
        let child = match child {
            Ok(child) => child,
            Err(_) => {
                listing.truncated = true;
                continue;
            }
        };
        let name = match child.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                listing.truncated = true;
                continue;
            }
        };
        if name.chars().any(char::is_control) {
            listing.truncated = true;
            continue;
        }
        let metadata = match std::fs::metadata(child.path()) {
            Ok(meta) => meta,
            Err(_) => {
                listing.truncated = true;
                continue;
            }
        };
        if !metadata.is_dir() {
            continue;
        }
        let is_hidden = name.starts_with('.');
        #[cfg(windows)]
        let is_hidden = {
            use std::os::windows::fs::MetadataExt;
            is_hidden || metadata.file_attributes() & 2 != 0
        };
        if is_hidden && !hidden {
            continue;
        }
        let path =
            match resolve_directory(child.path().to_str().ok_or(FsError::Unsupported)?, &home) {
                Ok(path) => path,
                Err(_) => {
                    listing.truncated = true;
                    continue;
                }
            };
        if std::fs::read_dir(&path).is_err() {
            listing.truncated = true;
            continue;
        }
        let entry = DirectoryEntry {
            name,
            path: path.to_str().ok_or(FsError::Unsupported)?.into(),
            hidden: is_hidden,
        };
        let bytes = serde_json::to_vec(&entry)
            .map_err(|_| FsError::Unavailable)?
            .len()
            + 1;
        if listing.entries.len() == 1000 || size + bytes > JSON_LIMIT {
            listing.truncated = true;
            break;
        }
        size += bytes;
        listing.entries.push(entry);
    }
    listing.entries.sort_by(|a, b| a.name.cmp(&b.name));
    // An empty iterator (or the last blocking child probe) must not bypass
    // cancellation/deadline fencing at publication.
    if cancelled.load(Ordering::Acquire) {
        return Err(FsError::Timeout);
    }
    listing.truncated |= elapsed() >= SCAN_BUDGET;
    Ok(listing)
}

struct DeviceBudget {
    tokens: f64,
    updated: Instant,
    slots: Arc<tokio::sync::Semaphore>,
}
pub(crate) struct BrowseLimits(
    parking_lot::Mutex<HashMap<String, DeviceBudget>>,
    #[cfg(test)] pub(super) parking_lot::Mutex<Option<tokio::sync::mpsc::UnboundedSender<(&'static str, bool)>>>,
    Arc<tokio::sync::Semaphore>,
    #[cfg(test)] pub(super) parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)] pub(super) parking_lot::Mutex<Option<tokio::sync::mpsc::UnboundedSender<bool>>>,
);
impl Default for BrowseLimits {
    fn default() -> Self {
        Self(
            parking_lot::Mutex::default(),
            #[cfg(test)] parking_lot::Mutex::default(),
            Arc::new(tokio::sync::Semaphore::new(16)),
            #[cfg(test)] parking_lot::Mutex::default(),
            #[cfg(test)] parking_lot::Mutex::default(),
        )
    }
}

#[cfg(test)]
struct RequestCompleted {
    events: Option<tokio::sync::mpsc::UnboundedSender<(&'static str, bool)>>,
    flag: Arc<AtomicBool>,
}
#[cfg(test)]
struct AuthCompleted(Option<tokio::sync::mpsc::UnboundedSender<bool>>);
#[cfg(test)]
impl Drop for AuthCompleted {
    fn drop(&mut self) {
        if let Some(events) = &self.0 { let _ = events.send(true); }
    }
}
#[cfg(test)]
impl Drop for RequestCompleted {
    fn drop(&mut self) {
        if let Some(events) = &self.events {
            let _ = events.send(("request_dropped", self.flag.load(Ordering::Acquire)));
        }
    }
}
impl BrowseLimits {
    pub(super) fn auth_slots(&self) -> &Arc<tokio::sync::Semaphore> {
        #[cfg(test)]
        { &self.2 }
        #[cfg(not(test))]
        { &self.1 }
    }
    pub(super) fn acquire(
        &self,
        device: &str,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, Response> {
        self.acquire_with_clock(device, Instant::now)
    }

    // Explicit monotonic admission time permits deterministic refill tests of
    // the same bucket/permit path used by the HTTP handler.
    #[cfg(test)]
    pub(super) fn acquire_at(
        &self,
        device: &str,
        now: Instant,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, Response> {
        self.acquire_with_clock(device, || now)
    }

    fn acquire_with_clock(
        &self,
        device: &str,
        clock: impl FnOnce() -> Instant,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, Response> {
        let mut budgets = self.0.lock();
        let now = clock();
        budgets.retain(|_, b| {
            now.duration_since(b.updated) < Duration::from_secs(60) || b.slots.available_permits() != 4
        });
        if !budgets.contains_key(device) && budgets.len() >= 4096 {
            return Err(super::server::machine_error(
                StatusCode::TOO_MANY_REQUESTS,
                "CAPACITY_EXCEEDED",
            ));
        }
        let budget = budgets
            .entry(device.into())
            .or_insert_with(|| DeviceBudget {
                tokens: 20.0,
                updated: now,
                slots: Arc::new(tokio::sync::Semaphore::new(4)),
            });
        budget.tokens = (budget.tokens + now.duration_since(budget.updated).as_secs_f64() * 10.0).min(20.0);
        budget.updated = now;
        if budget.tokens < 1.0 {
            return Err(super::server::machine_error(
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
            ));
        }
        let permit = budget.slots.clone().try_acquire_owned().map_err(|_| {
            super::server::machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED")
        })?;
        budget.tokens -= 1.0;
        Ok(permit)
    }
}
pub(super) struct CancelOnDrop(pub(super) Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) async fn directories(
    State(state): State<Arc<RemoteGatewayState>>,
    Extension(limits): Extension<Arc<BrowseLimits>>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Response, Response> {
    // Start the complete read budget before admission or blocking auth I/O.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let flag = Arc::new(AtomicBool::new(false));
    #[cfg(test)]
    let events = limits.1.lock().clone();
    #[cfg(test)]
    let _completed = RequestCompleted { events: events.clone(), flag: flag.clone() };
    let cancel = CancelOnDrop(flag);
    let auth_flag = cancel.0.clone();
    let auth_permit = limits.auth_slots().clone().try_acquire_owned().map_err(|_| {
        super::server::machine_error(StatusCode::TOO_MANY_REQUESTS, "CAPACITY_EXCEEDED")
    })?;
    let auth_state = state.clone();
    let auth_headers = headers.clone();
    #[cfg(test)]
    let auth_limits = limits.clone();
    // Capture before scheduling: channel closure then proves every scheduled
    // worker has finished, including queued/excess workers and panic unwinds.
    #[cfg(test)]
    let auth_completed = AuthCompleted(limits.4.lock().clone());
    let auth_work = crate::ipc::run_blocking(move || {
        #[cfg(test)]
        let _completed = auth_completed;
        let permit = auth_permit;
        #[cfg(test)]
        let probe = auth_limits.3.lock().clone();
        #[cfg(test)]
        if let Some(probe) = probe { probe(); }
        let result = if auth_flag.load(Ordering::Acquire) || tokio::time::Instant::now() >= deadline {
            Err(FsError::Timeout.response())
        } else { super::server::authenticate_machine_request(
            &auth_state,
            &auth_headers,
        ) };
        drop(permit);
        Ok(result)
    });
    let device = tokio::time::timeout_at(deadline, auth_work)
    .await
    .map_err(|_| FsError::Timeout.response())?
    .map_err(|_| FsError::Unavailable.response())??;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err(super::server::machine_error(
            StatusCode::FORBIDDEN,
            "MACHINE_ACCESS_REQUIRED",
        ));
    }
    if state.machine_services.is_none() {
        return Err(super::server::machine_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE",
        ));
    }
    let mut revoked = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| super::server::machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED"))?;
    let (path, hidden) = query(raw.as_deref()).map_err(FsError::response)?;
    if tokio::time::Instant::now() >= deadline {
        return Err(FsError::Timeout.response());
    }
    let permit = limits.acquire(&device.id)?;
    let flag = cancel.0.clone();

    let started = Instant::now();
    let work = crate::ipc::run_blocking(move || {
        let _permit = permit; // Cancellation cannot free a still-blocked worker slot.
        #[cfg(test)]
        let home = state.browse_home.read().clone();
        #[cfg(not(test))]
        let home: Option<PathBuf> = None;
        #[cfg(test)]
        let probe = state.browse_probe.read().clone();
        #[cfg(test)]
        if let Some(probe) = probe {
            probe();
        }
        let home = home.or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
        });
        let result = home
            .ok_or(FsError::Unavailable)
            .and_then(|home| scan(&path, hidden, &home, &flag, started));
        drop(_permit);
        #[cfg(test)]
        if let Some(events) = events {
            let _ = events.send(("worker_completed", matches!(result, Err(FsError::Timeout))));
        }
        Ok(result)
    });
    let listing = tokio::select! {
        biased;
        _ = revoked.wait_for(|value| *value) => return Err(super::server::machine_error(StatusCode::UNAUTHORIZED, "UNAUTHORIZED")),
        result = tokio::time::timeout_at(deadline, work) => result.map_err(|_| FsError::Timeout.response())?.map_err(|_| FsError::Unavailable.response())?.map_err(FsError::response)?,
    };
    if *revoked.borrow() {
        return Err(super::server::machine_error(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
        ));
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(FsError::Timeout.response());
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(listing)).into_response())
}
