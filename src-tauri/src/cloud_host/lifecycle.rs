//! The machine lifecycle: creating a machine under the catalog's rules, deleting one, and the sweep
//! that stops idle machines and deletes machines past their hard ceiling.
//!
//! Three invariants decide whether free credit survives a forgotten box, and every function here
//! exists to hold one of them:
//!
//! 1. **A status check never wakes a machine.** Nothing on the sweep path calls
//!    [`SandboxProvider::resume`]; `get` and `list` are the only reads, and the trait's own contract
//!    forbids a hidden wake inside them. A stopped machine therefore costs nothing while the sweep
//!    looks at it.
//! 2. **A provider failure never deletes a row.** A `Retryable` error leaves the row exactly as it
//!    was. Only two things turn a row into a `deleted` tombstone: the provider saying it is gone, or
//!    the sweep's own `delete` returning success (or `NotFound`).
//! 3. **The reaper is required exactly when nothing else will stop the machine.** A provider with
//!    `native_auto_stop` can be used without a scheduler entry; one without it cannot, and the
//!    refusal happens before any billable `create`, with the typed code
//!    [`LifecycleError::REAPER_REQUIRED`].
//!
//! Two windows apply to every row. The **idle window** (`ttl_seconds`, 30 minutes by default) is when
//! the sweep stops a machine nobody used; the **hard deadline** is `min(provider maximum, 6 h)` after
//! creation, when the sweep deletes it whatever the idle clock says. The provider is also given the
//! hard deadline as its own auto-stop, so a machine dies at that ceiling even if this Mac never runs
//! a ferryx command again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::ipc::IpcError;

use super::catalog::{seconds_to_ms, Catalog, CatalogRow, CatalogStore};
use super::reaper::{HostReaper, ReaperError, ReaperScheduler};
use super::slots::{SlotError, SlotLease, SlotRegistry};
use super::{
    Capabilities, CreateSandbox, ErrorKind, SandboxError, SandboxProvider, SandboxState, Transport,
};

/// Default idle window before a machine is stopped (30 minutes).
pub const DEFAULT_IDLE_STOP_SECONDS: u64 = 30 * 60;
/// Hard ceiling on any machine's life, whatever the provider allows (6 hours).
pub const HARD_MAX_SECONDS: u64 = 6 * 60 * 60;
/// Prefix every machine we create carries at the provider, so the orphan scan can recognise our own
/// machines on an account that also holds hand-made ones.
pub const TAG_PREFIX: &str = "fx-";

/// The typed failure of a lifecycle call. `code()` is the stable token a CLI or a test asserts on.
#[derive(Debug)]
pub enum LifecycleError {
    /// The catalog could not be read or written. Nothing at the provider was touched.
    Catalog(IpcError),
    /// The provider refused. `code()` is `PROVIDER_ERROR`; `kind()` carries the provider's kind.
    Provider(SandboxError),
    /// The scheduler entry could not be installed or removed when the call required it to work.
    Reaper(ReaperError),
    /// The provider has no native auto-stop and the local reaper is not loaded. Refused **before**
    /// any `create`, so no billable machine exists for a machine nothing would ever stop.
    /// `detail` carries why the reaper could not be installed, when an attempt was made.
    ReaperRequired { provider: String, detail: Option<String> },
    /// No provider of that name is registered in this build.
    UnknownProvider(String),
    /// No catalog row carries that name.
    NotInCatalog(String),
    /// The row exists but is already a `deleted` tombstone.
    AlreadyDeleted(String),
    /// A catalog row names a provider this build does not have, so the call cannot be made.
    ProviderNotConfigured { name: String, provider: String },
    /// The relay slot pool refused: no record is free (`NO_RELAY_SLOT`) or the one this machine
    /// holds cannot be read. Raised **before** any `create`, so no billable machine exists without
    /// the machine record it would connect through.
    Slot(SlotError),
}

impl LifecycleError {
    /// `REAPER_REQUIRED`, `UNKNOWN_PROVIDER`, ... - the token a CLI maps onto an exit code.
    pub fn code(&self) -> &'static str {
        match self {
            LifecycleError::Catalog(_) => "CATALOG_ERROR",
            LifecycleError::Provider(_) => "PROVIDER_ERROR",
            LifecycleError::Reaper(_) => "REAPER_ERROR",
            LifecycleError::ReaperRequired { .. } => "REAPER_REQUIRED",
            LifecycleError::UnknownProvider(_) => "UNKNOWN_PROVIDER",
            LifecycleError::NotInCatalog(_) => "NOT_IN_CATALOG",
            LifecycleError::AlreadyDeleted(_) => "ALREADY_DELETED",
            LifecycleError::ProviderNotConfigured { .. } => "PROVIDER_NOT_CONFIGURED",
            LifecycleError::Slot(error) => error.code(),
        }
    }

    /// The provider's own error kind, when a provider produced this.
    pub fn provider_kind(&self) -> Option<ErrorKind> {
        match self {
            LifecycleError::Provider(error) => Some(error.kind),
            _ => None,
        }
    }

    fn not_in_catalog(name: &str) -> Self {
        LifecycleError::NotInCatalog(format!("no cloud machine named {name} is in the catalog"))
    }
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleError::Catalog(error) => write!(f, "{error:?}"),
            LifecycleError::Provider(error) => write!(f, "{error}"),
            LifecycleError::Reaper(error) => write!(f, "{error}"),
            LifecycleError::ReaperRequired { provider, detail } => {
                write!(
                    f,
                    "REAPER_REQUIRED: provider {provider} has no native auto-stop and the 5-minute reaper \
                     (dev.ferryx.cloud-reaper) is not loaded; install it before creating a machine nothing \
                     would ever stop"
                )?;
                match detail {
                    Some(detail) => write!(f, " ({detail})"),
                    None => Ok(()),
                }
            }
            LifecycleError::UnknownProvider(provider) => write!(f, "unknown provider: {provider}"),
            LifecycleError::NotInCatalog(message) => f.write_str(message),
            LifecycleError::AlreadyDeleted(name) => write!(f, "cloud machine {name} is already deleted"),
            LifecycleError::ProviderNotConfigured { name, provider } => {
                write!(f, "cloud machine {name} names provider {provider}, which this build does not configure")
            }
            LifecycleError::Slot(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

impl From<IpcError> for LifecycleError {
    fn from(error: IpcError) -> Self {
        LifecycleError::Catalog(error)
    }
}

impl From<SandboxError> for LifecycleError {
    fn from(error: SandboxError) -> Self {
        LifecycleError::Provider(error)
    }
}

impl From<ReaperError> for LifecycleError {
    fn from(error: ReaperError) -> Self {
        LifecycleError::Reaper(error)
    }
}

impl From<SlotError> for LifecycleError {
    fn from(error: SlotError) -> Self {
        LifecycleError::Slot(error)
    }
}

/// The providers one reconcile can see, keyed by the registry name a catalog row stores.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    entries: BTreeMap<String, Arc<dyn SandboxProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `provider` under `name` (`"e2b"`, `"daytona"`, ...).
    pub fn insert(&mut self, name: impl Into<String>, provider: Arc<dyn SandboxProvider>) {
        self.entries.insert(name.into(), provider);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn SandboxProvider>> {
        self.entries.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// Names and providers in registry order (sorted by name), so a sweep is deterministic.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<dyn SandboxProvider>)> {
        self.entries.iter().map(|(name, provider)| (name.as_str(), provider))
    }
}

/// One completed lifecycle mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub row: CatalogRow,
    /// The scheduler entry was not loaded before this create and is now.
    pub reaper_installed: bool,
    /// The relay record this machine leased, when it connects through the relay. A provisioning
    /// step copies the record's identity onto the box; the lease's key material never leaves
    /// [`SlotLease`](super::slots::SlotLease), only its paths do.
    pub slot: Option<SlotLease>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deleted {
    pub row: CatalogRow,
    /// The scheduler entry was removed by this call (no live row is left).
    pub reaper_removed: bool,
}

/// What a sweep did to one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepAction {
    /// Idle past its window: the provider was told to stop it.
    Stopped,
    /// Past its hard deadline: the provider was told to delete it.
    Deleted,
    /// Absent from the provider's listing and confirmed gone by `get`.
    ConfirmedGone,
    /// A provider machine tagged `fx-` that no catalog row names, older than the hard ceiling.
    OrphanDeleted,
}

impl SweepAction {
    pub fn as_str(self) -> &'static str {
        match self {
            SweepAction::Stopped => "stopped",
            SweepAction::Deleted => "deleted",
            SweepAction::ConfirmedGone => "confirmed_gone",
            SweepAction::OrphanDeleted => "orphan_deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepRecord {
    pub provider: String,
    pub name: String,
    pub sandbox_id: String,
    pub action: SweepAction,
}

/// A provider machine we tagged but the catalog never heard of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanRecord {
    pub provider: String,
    pub sandbox_id: String,
    pub name: Option<String>,
    pub created_at: Option<i64>,
    /// Whether the sweep deleted it (only when its age is past the hard ceiling).
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepFailure {
    pub provider: String,
    pub name: String,
    pub detail: String,
}

/// Everything one sweep observed. Printed by the hidden `cloud reconcile` verb.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Catalog rows that still cost money and were examined.
    pub examined: usize,
    pub records: Vec<SweepRecord>,
    pub orphans: Vec<OrphanRecord>,
    pub failures: Vec<SweepFailure>,
    /// The sweep removed the scheduler entry because nothing live is left.
    pub reaper_removed: bool,
}

impl ReconcileReport {
    pub fn acted(&self, action: SweepAction) -> Vec<&SweepRecord> {
        self.records.iter().filter(|record| record.action == action).collect()
    }
}

/// The idle window a create lands on: the caller's request, clamped to the hard ceiling.
pub fn idle_seconds(capabilities: Capabilities, requested: Option<u64>) -> u64 {
    requested.unwrap_or(DEFAULT_IDLE_STOP_SECONDS).max(60).min(hard_max_seconds(capabilities))
}

/// `min(provider maximum, 6 h)`, in seconds.
pub fn hard_max_seconds(capabilities: Capabilities) -> u64 {
    capabilities.max_ttl_seconds.map_or(HARD_MAX_SECONDS, |maximum| maximum.min(HARD_MAX_SECONDS))
}

/// The provider-side machine name: the operator's name with our tag prefix.
pub fn tagged_name(name: &str) -> String {
    if name.starts_with(TAG_PREFIX) {
        name.to_string()
    } else {
        format!("{TAG_PREFIX}{name}")
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Create a machine, install the reaper if this is the first live row, and refuse a provider whose
/// only shutdown guarantee would be a reaper that is not running.
///
/// Order matters twice over: the reaper question is settled before `create`, so a refusal leaves no
/// billable machine behind; and the relay slot is leased before `create` too, so a machine that has
/// no account record to connect through is refused rather than paid for.
pub async fn create_with_reaper(
    provider: &dyn SandboxProvider,
    provider_name: &str,
    request: CreateSandbox,
    catalog: &CatalogStore,
    slots: &SlotRegistry,
) -> Result<Created, LifecycleError> {
    let reaper = HostReaper::from_env().map_err(LifecycleError::Reaper)?;
    create_with_scheduler(provider, provider_name, request, catalog, slots, &reaper).await
}

/// [`create_with_reaper`] against an explicit scheduler, so a test can control what is loaded.
pub async fn create_with_scheduler(
    provider: &dyn SandboxProvider,
    provider_name: &str,
    request: CreateSandbox,
    catalog: &CatalogStore,
    slots: &SlotRegistry,
    scheduler: &dyn ReaperScheduler,
) -> Result<Created, LifecycleError> {
    let capabilities = provider.capabilities();
    let mut current = catalog.load().await?;

    // 1. The reaper: make sure the entry is loaded, then answer whether a provider without native
    //    auto-stop may proceed. An install failure is not fatal by itself - a provider that stops
    //    itself needs no reaper - so it is remembered and only decides this create when the
    //    provider has no native auto-stop.
    let mut reaper_installed = false;
    let mut loaded = scheduler.is_loaded().await.unwrap_or(false);
    let mut install_error: Option<String> = None;
    if !loaded {
        match scheduler.install().await {
            Ok(()) => {
                loaded = scheduler.is_loaded().await.unwrap_or(true);
                reaper_installed = loaded;
            }
            Err(error) => {
                loaded = false;
                install_error = Some(error.to_string());
            }
        }
    }
    if !capabilities.native_auto_stop && !loaded {
        return Err(LifecycleError::ReaperRequired {
            provider: provider_name.to_string(),
            detail: install_error,
        });
    }

    // 2. The relay slot, and only for a relay machine: a T2 box cannot carry a machineId of its
    //    own (the account is at its record limit, no route deletes a record, and the relay pins a
    //    machineId's key at its first connection), so it must claim a record from the pool. Leasing
    //    before `create` is what turns a dry pool into a refusal instead of a paid machine nobody
    //    can attach to. An SSH machine has its own route in and takes no record.
    let lease = if capabilities.transport == Transport::Relay {
        Some(slots.lease(&request.name).await?)
    } else {
        None
    };

    // 3. Two different windows: the provider is told the hard ceiling (its own auto-stop is the
    //    outer backstop), the catalog tracks the shorter idle window the sweep acts on.
    let idle = idle_seconds(capabilities, request.ttl_seconds);
    let hard_max = hard_max_seconds(capabilities);
    let sandbox_id = match provider
        .create(CreateSandbox {
            idempotency_key: request.idempotency_key,
            name: tagged_name(&request.name),
            ttl_seconds: Some(hard_max),
            env: request.env,
        })
        .await
    {
        Ok(sandbox_id) => sandbox_id,
        Err(error) => {
            // No machine exists, so the record must go straight back to the pool.
            release_slot(slots, lease.as_ref()).await;
            return Err(LifecycleError::Provider(error));
        }
    };

    // 4. Record what the provider says, without waking anything: a `Provisioning` machine stays
    //    `Provisioning` in the row until a later sweep observes the transition.
    let observed = match provider.get(&sandbox_id).await {
        Ok(info) => info.state,
        // A transient read failure must not lose the machine we just created.
        Err(_) => SandboxState::Provisioning,
    };
    let created_at = now_ms();
    let row = CatalogRow {
        name: request.name,
        provider: provider_name.to_string(),
        sandbox_id,
        transport: capabilities.transport,
        created_at,
        last_used_at: created_at,
        ttl_seconds: idle,
        hard_deadline: created_at.saturating_add(seconds_to_ms(hard_max)),
        state: observed,
        socket_path: None,
        // The record this machine connects through, so a later verb can prove the record is
        // reachable without guessing which slot it took. Kept on the tombstone by `delete`.
        slot: lease.as_ref().map(|lease| lease.slot().to_string()),
    };
    current.upsert(row.clone());
    if let Err(error) = catalog.save(&current).await {
        // The machine exists but nothing records it, so a later delete could not find it: give the
        // record back and report the catalog failure rather than leak both.
        release_slot(slots, lease.as_ref()).await;
        return Err(LifecycleError::Catalog(error));
    }
    Ok(Created { row, reaper_installed, slot: lease })
}

/// Give a held slot back, reporting a failure to do so without hiding the error that caused it.
async fn release_slot(slots: &SlotRegistry, lease: Option<&SlotLease>) {
    let Some(lease) = lease else { return };
    if let Err(error) = slots.release(lease).await {
        tracing::warn!(slot = lease.slot(), name = lease.name(), %error, "could not release a relay slot");
    }
}

/// Delete a machine and its row's claim on the scheduler: the reaper is removed as soon as no row
/// is left in a state other than `deleted`. Tombstones stay in the catalog, and the relay slot the
/// machine held goes back to the pool so the next T2 box can use that record.
pub async fn delete_with_reaper(
    provider: &dyn SandboxProvider,
    name: &str,
    catalog: &CatalogStore,
    slots: &SlotRegistry,
) -> Result<Deleted, LifecycleError> {
    let reaper = HostReaper::from_env().map_err(LifecycleError::Reaper)?;
    delete_with_scheduler(provider, name, catalog, slots, &reaper).await
}

/// [`delete_with_reaper`] against an explicit scheduler.
pub async fn delete_with_scheduler(
    provider: &dyn SandboxProvider,
    name: &str,
    catalog: &CatalogStore,
    slots: &SlotRegistry,
    scheduler: &dyn ReaperScheduler,
) -> Result<Deleted, LifecycleError> {
    let mut current = catalog.load().await?;
    let Some(existing) = current.row(name).cloned() else {
        return Err(LifecycleError::not_in_catalog(name));
    };
    if existing.is_tombstone() {
        // Deleting twice is already true; still make sure the reaper is not left behind, and that a
        // stale lease does not keep a record out of the pool forever.
        release_slot(slots, slots.lease_of(name).await?.as_ref()).await;
        let reaper_removed = uninstall_reaper_if_idle(catalog, &mut current, scheduler).await?;
        return Ok(Deleted { row: existing, reaper_removed });
    }

    // The provider may already be gone (a delete that landed but whose response was lost).
    let already_gone = match provider.get(&existing.sandbox_id).await {
        Ok(info) => info.state == SandboxState::Deleted,
        Err(error) if error.is_not_found() => true,
        Err(error) => return Err(LifecycleError::Provider(error)),
    };
    if !already_gone {
        match provider.delete(&existing.sandbox_id).await {
            Ok(()) => {}
            Err(error) if error.is_not_found() => {}
            Err(error) => {
                // The row keeps its state: the machine may still exist and still bill.
                return Err(LifecycleError::Provider(error));
            }
        }
    }

    let mut tombstone = existing;
    tombstone.state = SandboxState::Deleted;
    current.upsert(tombstone.clone());
    catalog.save(&current).await?;
    // Only now, with the provider confirmed gone and the tombstone written: a record returned to
    // the pool while the box still ran would be leased to a second machine at the same time.
    release_slot(slots, slots.lease_of(name).await?.as_ref()).await;
    let reaper_removed = uninstall_reaper_if_idle(catalog, &mut current, scheduler).await?;
    Ok(Deleted { row: tombstone, reaper_removed })
}

/// Remove the scheduler entry when the catalog no longer holds anything live.
async fn uninstall_reaper_if_idle(
    _catalog: &CatalogStore,
    current: &mut Catalog,
    scheduler: &dyn ReaperScheduler,
) -> Result<bool, LifecycleError> {
    if current.has_live() {
        return Ok(false);
    }
    if !scheduler.is_loaded().await.unwrap_or(false) {
        return Ok(false);
    }
    match scheduler.remove().await {
        Ok(()) => Ok(true),
        // A platform without a scheduler has nothing to remove.
        Err(error) if error.is_unsupported() => Ok(false),
        Err(error) => Err(LifecycleError::Reaper(error)),
    }
}

/// One sweep over every configured provider, at the wall clock.
pub async fn reconcile(
    catalog: &CatalogStore,
    providers: &ProviderRegistry,
) -> Result<ReconcileReport, LifecycleError> {
    let reaper = HostReaper::from_env().ok();
    let scheduler: Option<&dyn ReaperScheduler> =
        reaper.as_ref().map(|reaper| reaper as &dyn ReaperScheduler);
    Sweep { catalog, providers, scheduler, now_ms: now_ms() }.run().await
}

/// [`reconcile`] at an explicit instant, so a test never races a clock.
pub async fn reconcile_at(
    catalog: &CatalogStore,
    providers: &ProviderRegistry,
    now_ms: i64,
) -> Result<ReconcileReport, LifecycleError> {
    Sweep { catalog, providers, scheduler: None, now_ms }.run().await
}

/// [`reconcile`] with an explicit scheduler and instant: what the tests drive.
pub async fn reconcile_with(
    catalog: &CatalogStore,
    providers: &ProviderRegistry,
    scheduler: Option<&dyn ReaperScheduler>,
    now_ms: i64,
) -> Result<ReconcileReport, LifecycleError> {
    Sweep { catalog, providers, scheduler, now_ms }.run().await
}

struct Sweep<'a> {
    catalog: &'a CatalogStore,
    providers: &'a ProviderRegistry,
    scheduler: Option<&'a dyn ReaperScheduler>,
    now_ms: i64,
}

impl Sweep<'_> {
    async fn run(self) -> Result<ReconcileReport, LifecycleError> {
        let mut current = self.catalog.load().await?;
        let mut report = ReconcileReport::default();
        let mut changed = false;

        for (provider_name, provider) in self.providers.iter() {
            let capabilities = provider.capabilities();
            let hard_max = seconds_to_ms(hard_max_seconds(capabilities));

            // (a) One listing per provider. A failed listing proves nothing is gone: the rows of
            //     this provider are left exactly as they are.
            let listed = match provider.list().await {
                Ok(listed) => listed,
                Err(error) => {
                    report.failures.push(SweepFailure {
                        provider: provider_name.to_string(),
                        name: String::new(),
                        detail: format!("list failed: {error}"),
                    });
                    continue;
                }
            };
            let live_ids: BTreeSet<String> =
                listed.iter().map(|row| row.sandbox_id.clone()).collect();

            let rows: Vec<CatalogRow> = current
                .live()
                .filter(|row| row.provider == provider_name)
                .cloned()
                .collect();
            for row in rows {
                report.examined += 1;
                if !live_ids.contains(&row.sandbox_id) {
                    // (b) The provider's listing does not have it. Tombstone only on proof.
                    match provider.get(&row.sandbox_id).await {
                        Ok(info) if info.state == SandboxState::Deleted => {
                            mark_deleted(&mut current, &row.name);
                            report.records.push(record(provider_name, &row, SweepAction::ConfirmedGone));
                            changed = true;
                        }
                        Err(error) if error.is_not_found() => {
                            mark_deleted(&mut current, &row.name);
                            report.records.push(record(provider_name, &row, SweepAction::ConfirmedGone));
                            changed = true;
                        }
                        Ok(_) => {}
                        Err(error) => report.failures.push(SweepFailure {
                            provider: provider_name.to_string(),
                            name: row.name.clone(),
                            detail: format!("get failed: {error}"),
                        }),
                    }
                    continue;
                }

                // (d) Hard ceiling first: a machine past it is deleted even while it is in use.
                if row.is_past_hard_deadline(self.now_ms) {
                    match provider.delete(&row.sandbox_id).await {
                        Ok(()) => {
                            mark_deleted(&mut current, &row.name);
                            report.records.push(record(provider_name, &row, SweepAction::Deleted));
                            changed = true;
                        }
                        Err(error) if error.is_not_found() => {
                            mark_deleted(&mut current, &row.name);
                            report.records.push(record(provider_name, &row, SweepAction::Deleted));
                            changed = true;
                        }
                        Err(error) => report.failures.push(SweepFailure {
                            provider: provider_name.to_string(),
                            name: row.name.clone(),
                            detail: format!("delete past the hard deadline failed: {error}"),
                        }),
                    }
                    continue;
                }

                // (d) Idle: stop, never delete, so the disk and the home directory survive a resume.
                if row.is_idle(self.now_ms) {
                    match provider.stop(&row.sandbox_id).await {
                        Ok(()) => {
                            report.records.push(record(provider_name, &row, SweepAction::Stopped));
                        }
                        Err(error) => report.failures.push(SweepFailure {
                            provider: provider_name.to_string(),
                            name: row.name.clone(),
                            detail: format!("idle stop failed: {error}"),
                        }),
                    }
                }
            }

            // (c) Orphans: machines we tagged that no catalog row names. Reported always; deleted
            //     only when their age proves they are past the hard ceiling.
            for listed_row in &listed {
                if current.row_for_sandbox(&listed_row.sandbox_id).is_some() {
                    continue;
                }
                let Some(name) = listed_row.name.as_deref() else {
                    continue;
                };
                if !name.starts_with(TAG_PREFIX) {
                    continue;
                }
                let mut orphan = OrphanRecord {
                    provider: provider_name.to_string(),
                    sandbox_id: listed_row.sandbox_id.clone(),
                    name: Some(name.to_string()),
                    created_at: listed_row.created_at,
                    deleted: false,
                };
                let old_enough = listed_row
                    .created_at
                    .is_some_and(|created_at| self.now_ms.saturating_sub(created_at) > hard_max);
                if old_enough {
                    match provider.delete(&listed_row.sandbox_id).await {
                        Ok(()) | Err(SandboxError { kind: ErrorKind::NotFound, .. }) => {
                            orphan.deleted = true;
                        }
                        Err(error) => report.failures.push(SweepFailure {
                            provider: provider_name.to_string(),
                            name: name.to_string(),
                            detail: format!("orphan delete failed: {error}"),
                        }),
                    }
                }
                report.orphans.push(orphan);
            }
        }

        // A row naming a provider this build does not configure cannot be checked. It is reported
        // and left alone: a missing provider must never look like a deleted machine.
        let configured: BTreeSet<String> = self.providers.names().into_iter().map(str::to_string).collect();
        for row in current.live() {
            if !configured.contains(&row.provider) {
                report.failures.push(SweepFailure {
                    provider: row.provider.clone(),
                    name: row.name.clone(),
                    detail: "provider is not configured in this build; the row was left unchanged".to_string(),
                });
            }
        }

        if changed {
            self.catalog.save(&current).await?;
        }

        if let Some(scheduler) = self.scheduler {
            report.reaper_removed = uninstall_reaper_if_idle(self.catalog, &mut current, scheduler).await?;
        }
        Ok(report)
    }
}

fn record(provider: &str, row: &CatalogRow, action: SweepAction) -> SweepRecord {
    SweepRecord {
        provider: provider.to_string(),
        name: row.name.clone(),
        sandbox_id: row.sandbox_id.clone(),
        action,
    }
}

fn mark_deleted(catalog: &mut Catalog, name: &str) {
    if let Some(row) = catalog.rows.iter_mut().find(|row| row.name == name) {
        row.state = SandboxState::Deleted;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::super::provider::fake::FakeProvider;
    use super::*;
    use crate::cloud_host::Transport;
    use crate::ipc::IpcErrorCode;

    /// A scheduler whose loaded flag the test controls.
    #[derive(Default)]
    struct FakeScheduler {
        loaded: AtomicBool,
        installs: AtomicUsize,
        removes: AtomicUsize,
        unsupported: AtomicBool,
    }

    impl FakeScheduler {
        fn unloaded() -> Self {
            Self::default()
        }

        fn loaded() -> Self {
            let scheduler = Self::default();
            scheduler.loaded.store(true, Ordering::SeqCst);
            scheduler
        }

        fn on_unsupported() -> Self {
            let scheduler = Self::default();
            scheduler.unsupported.store(true, Ordering::SeqCst);
            scheduler
        }

        fn installs(&self) -> usize {
            self.installs.load(Ordering::SeqCst)
        }

        fn removes(&self) -> usize {
            self.removes.load(Ordering::SeqCst)
        }

        fn is_loaded_flag(&self) -> bool {
            self.loaded.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl ReaperScheduler for FakeScheduler {
        fn platform(&self) -> &'static str {
            "fake"
        }

        async fn is_loaded(&self) -> Result<bool, ReaperError> {
            Ok(self.loaded.load(Ordering::SeqCst))
        }

        async fn install(&self) -> Result<(), ReaperError> {
            if self.unsupported.load(Ordering::SeqCst) {
                return Err(ReaperError::Unsupported { platform: "fake", detail: "none".into() });
            }
            self.installs.fetch_add(1, Ordering::SeqCst);
            self.loaded.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn remove(&self) -> Result<(), ReaperError> {
            if self.unsupported.load(Ordering::SeqCst) {
                return Err(ReaperError::Unsupported { platform: "fake", detail: "none".into() });
            }
            self.removes.fetch_add(1, Ordering::SeqCst);
            self.loaded.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A [`FakeProvider`] with exactly one behaviour bent, so a test can drive a single failure
    /// path without hand-writing a whole provider.
    struct Delegating {
        inner: Arc<FakeProvider>,
        empty_listing: bool,
        stop_fails: bool,
    }

    impl Delegating {
        fn listing_empty(inner: Arc<FakeProvider>) -> Self {
            Self { inner, empty_listing: true, stop_fails: false }
        }

        fn failing_stop(inner: Arc<FakeProvider>) -> Self {
            Self { inner, empty_listing: false, stop_fails: true }
        }
    }

    #[async_trait::async_trait]
    impl SandboxProvider for Delegating {
        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
            self.inner.create(request).await
        }

        async fn get(&self, sandbox_id: &str) -> Result<super::super::SandboxInfo, SandboxError> {
            self.inner.get(sandbox_id).await
        }

        async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            if self.stop_fails {
                return Err(SandboxError::new(
                    ErrorKind::Retryable,
                    "fake",
                    "stop_unavailable",
                    "the provider is briefly unavailable",
                ));
            }
            self.inner.stop(sandbox_id).await
        }

        async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
            self.inner.resume(sandbox_id, ttl_seconds).await
        }

        async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
            self.inner.delete(sandbox_id).await
        }

        async fn extend_ttl(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), SandboxError> {
            self.inner.extend_ttl(sandbox_id, ttl_seconds).await
        }

        async fn exec(
            &self,
            sandbox_id: &str,
            command: &str,
            timeout_seconds: u64,
        ) -> Result<super::super::ExecResult, SandboxError> {
            self.inner.exec(sandbox_id, command, timeout_seconds).await
        }

        async fn list(&self) -> Result<Vec<super::super::ListedSandbox>, SandboxError> {
            if self.empty_listing {
                return Ok(Vec::new());
            }
            self.inner.list().await
        }
    }

    fn registry_of(provider: Arc<Delegating>) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.insert("fake", provider as Arc<dyn SandboxProvider>);
        registry
    }

    const T0: i64 = 1_735_689_600_000;
    const MINUTE_MS: i64 = 60_000;

    fn store(dir: &tempfile::TempDir) -> CatalogStore {
        CatalogStore::at(dir.path().join("state/cloud-hosts.json"))
    }

    /// A relay slot pool under the test's temp dir, holding one record per name. Each record gets a
    /// throwaway identity file, exactly the shape task 18 left in `~/.ferryx/cloud-slots/`.
    fn pool(dir: &tempfile::TempDir, records: &[&str]) -> SlotRegistry {
        let registry = SlotRegistry::at(super::super::slots::location_in(dir.path()));
        for record in records {
            let slot = registry.root().join(record);
            std::fs::create_dir_all(&slot).unwrap();
            std::fs::write(
                slot.join(super::super::slots::IDENTITY_FILE),
                format!(
                    "{{\"machineId\":\"{record}-machine\",\"displayName\":null,\"publicKey\":\"cHVi\",\"privateKey\":\"cHJpdg==\"}}"
                ),
            )
            .unwrap();
        }
        registry
    }

    /// A pool with no records at all, for a create that must not need one.
    fn empty_pool(dir: &tempfile::TempDir) -> SlotRegistry {
        pool(dir, &[])
    }

    fn request(name: &str) -> CreateSandbox {
        CreateSandbox {
            idempotency_key: format!("fx-{name}-1"),
            name: name.to_string(),
            ttl_seconds: None,
            env: BTreeMap::new(),
        }
    }

    fn row(name: &str, sandbox_id: &str, state: SandboxState) -> CatalogRow {
        CatalogRow {
            name: name.to_string(),
            provider: "fake".to_string(),
            sandbox_id: sandbox_id.to_string(),
            transport: Transport::Relay,
            created_at: T0,
            last_used_at: T0,
            ttl_seconds: 1_800,
            hard_deadline: T0 + seconds_to_ms(HARD_MAX_SECONDS),
            state,
            socket_path: None,
            slot: None,
        }
    }

    fn registry(provider: Arc<FakeProvider>) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.insert("fake", provider);
        registry
    }

    async fn seed(dir: &tempfile::TempDir, rows: Vec<CatalogRow>) -> CatalogStore {
        let store = store(dir);
        let mut catalog = Catalog::default();
        for row in rows {
            catalog.upsert(row);
        }
        store.save(&catalog).await.expect("seed the catalog");
        store
    }

    // ---- create -------------------------------------------------------------------------------

    #[tokio::test]
    async fn the_first_create_installs_the_reaper_and_records_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::unloaded();

        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("create");

        assert!(created.reaper_installed);
        assert_eq!(scheduler.installs(), 1);
        assert_eq!(created.row.name, "fx-a");
        assert_eq!(created.row.provider, "fake");
        assert_eq!(created.row.transport, Transport::Ssh);
        assert_eq!(created.row.ttl_seconds, DEFAULT_IDLE_STOP_SECONDS);
        assert_eq!(created.row.hard_deadline, created.row.created_at + seconds_to_ms(HARD_MAX_SECONDS));
        assert_eq!(provider.capabilities().native_auto_stop, true);

        let saved = store.load().await.expect("catalog");
        assert_eq!(saved.row("fx-a"), Some(&created.row));
        // The provider-side name carries our tag, so the orphan scan can recognise it.
        let listed = provider.list().await.expect("list");
        assert_eq!(listed[0].name.as_deref(), Some("fx-a"));
        assert_eq!(provider.ttl_of(&created.row.sandbox_id), Some(Some(HARD_MAX_SECONDS)));
        // Nothing about create resumed anything.
        assert!(!provider.call_methods().contains(&"resume"));
    }

    #[tokio::test]
    async fn a_provider_without_native_auto_stop_is_refused_before_any_create() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        // `FakeProvider::new` is a relay provider with `native_auto_stop: false`.
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::on_unsupported();

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect_err("REAPER_REQUIRED");

        assert_eq!(error.code(), "REAPER_REQUIRED");
        assert!(error.to_string().starts_with("REAPER_REQUIRED:"));
        assert_eq!(provider.call_methods(), Vec::<&'static str>::new(), "zero provider calls");
        assert!(!store.path().exists(), "a refused create writes no catalog row");
    }

    #[tokio::test]
    async fn a_provider_without_native_auto_stop_is_allowed_once_the_reaper_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("create with a loaded reaper");
        assert!(!created.reaper_installed, "the reaper was already loaded");
        assert_eq!(scheduler.installs(), 0);
        assert_eq!(provider.call_methods().first(), Some(&"create"));
    }

    #[tokio::test]
    async fn a_provider_with_native_auto_stop_creates_even_with_no_reaper_platform() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::on_unsupported();

        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("native auto-stop needs no reaper");
        assert!(!created.reaper_installed);
        assert_eq!(provider.live_ids().len(), 1);
    }

    #[tokio::test]
    async fn the_requested_ttl_is_the_idle_window_and_the_provider_gets_the_hard_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::loaded();
        let mut create = request("fx-a");
        create.ttl_seconds = Some(900);

        let created = create_with_scheduler(&*provider, "fake", create, &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("create");
        assert_eq!(created.row.ttl_seconds, 900);
        assert_eq!(provider.ttl_of(&created.row.sandbox_id), Some(Some(HARD_MAX_SECONDS)));
    }

    #[tokio::test]
    async fn a_provider_cap_shortens_the_hard_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::with_capabilities(Capabilities {
            ttl_required: true,
            max_ttl_seconds: Some(3_600),
            transport: Transport::Relay,
            native_auto_stop: true,
        }));
        let scheduler = FakeScheduler::loaded();
        let mut create = request("fx-a");
        create.ttl_seconds = Some(7_200);

        let created = create_with_scheduler(&*provider, "fake", create, &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("create");
        assert_eq!(created.row.ttl_seconds, 3_600, "the idle window cannot exceed the provider cap");
        assert_eq!(created.row.hard_deadline, created.row.created_at + seconds_to_ms(3_600));
        assert_eq!(provider.ttl_of(&created.row.sandbox_id), Some(Some(3_600)));
    }

    #[tokio::test]
    async fn a_relay_create_refused_for_a_reaper_takes_no_slot() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::on_unsupported();
        let slots = pool(&dir, &["record-1"]);

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect_err("REAPER_REQUIRED");
        assert_eq!(error.code(), "REAPER_REQUIRED");
        assert!(slots.lease_of("fx-a").await.unwrap().is_none(), "a refused create leases nothing");
    }

    // ---- relay slots --------------------------------------------------------------------------

    #[tokio::test]
    async fn a_relay_create_with_every_slot_leased_is_refused_before_any_create() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1"]);
        // Another machine already holds the only record in the pool.
        slots.lease("fx-other").await.expect("the other machine leases the only slot");
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect_err("NO_RELAY_SLOT");

        assert_eq!(error.code(), "NO_RELAY_SLOT");
        assert!(error.to_string().starts_with("NO_RELAY_SLOT:"), "{error}");
        assert_eq!(provider.call_methods(), Vec::<&'static str>::new(), "zero provider calls");
        assert!(!store.path().exists(), "a refused create writes no catalog row");
        assert!(slots.lease_of("fx-other").await.unwrap().is_some(), "the live lease is untouched");
    }

    #[tokio::test]
    async fn a_relay_create_with_an_empty_pool_is_refused_before_any_create() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = empty_pool(&dir);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect_err("NO_RELAY_SLOT");
        assert_eq!(error.code(), "NO_RELAY_SLOT");
        assert!(provider.call_methods().is_empty());
    }

    #[tokio::test]
    async fn a_relay_create_leases_a_slot_and_delete_returns_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1", "record-2"]);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("create");
        let lease = created.slot.clone().expect("a relay create leases a slot");
        assert_eq!(lease.name(), "fx-a");
        assert_eq!(lease.machine_id(), "record-1-machine");
        assert_eq!(
            slots.lease_of("fx-a").await.unwrap().map(|held| held.slot().to_string()),
            Some(lease.slot().to_string())
        );
        assert!(lease.identity_path().is_file(), "the record carries the pinned keypair");

        // A second machine takes the other record, not this one.
        let second = create_with_scheduler(&*provider, "fake", request("fx-b"), &store, &slots, &scheduler)
            .await
            .expect("second create");
        assert_eq!(second.slot.as_ref().map(|lease| lease.slot()), Some("record-2"));

        delete_with_scheduler(&*provider, "fx-a", &store, &slots, &scheduler).await.expect("delete");
        assert!(slots.lease_of("fx-a").await.unwrap().is_none(), "delete returns the slot");
        assert!(lease.identity_path().is_file(), "the record itself survives the delete");

        // The freed record is the next machine's.
        let third = create_with_scheduler(&*provider, "fake", request("fx-c"), &store, &slots, &scheduler)
            .await
            .expect("third create");
        assert_eq!(third.slot.as_ref().map(|lease| lease.slot()), Some("record-1"));
    }

    #[tokio::test]
    async fn a_failed_relay_create_returns_its_slot_to_the_pool() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1"]);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();
        // The injected failure lands on the first provider call, which is `create`.
        provider.fail_next(ErrorKind::Retryable);

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect_err("the provider failed");

        assert_eq!(error.code(), "PROVIDER_ERROR");
        assert!(slots.lease_of("fx-a").await.unwrap().is_none(), "no machine exists, so no lease");
        // The slot is immediately usable again.
        create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("a retried create takes the freed slot");
    }

    #[tokio::test]
    async fn an_ssh_create_takes_no_slot() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = empty_pool(&dir);
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::loaded();

        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("an ssh create needs no machine record");
        assert!(created.slot.is_none(), "an SSH machine leases nothing from the relay pool");
    }

    #[tokio::test]
    async fn an_ssh_create_is_unaffected_by_a_fully_leased_pool() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1"]);
        slots.lease("fx-other").await.expect("the pool is dry");
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::loaded();

        create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("an ssh machine never asks the relay pool");
    }

    #[tokio::test]
    async fn a_retried_relay_create_keeps_the_slot_it_already_holds() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1", "record-2"]);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let first = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("create");
        // A second create of the same name (the operator retried) must not take a second record.
        let retried = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("retried create");
        assert_eq!(retried.slot.as_ref().map(|lease| lease.slot()), first.slot.as_ref().map(|lease| lease.slot()));
        assert_eq!(
            slots.lease_of("fx-a").await.unwrap().map(|held| held.slot().to_string()),
            Some("record-1".to_string())
        );
    }

    // ---- delete -------------------------------------------------------------------------------

    #[tokio::test]
    async fn delete_tombstones_the_row_and_removes_the_reaper_once_nothing_is_live() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::unloaded();
        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("create");
        assert_eq!(created.row.name, "fx-a");
        assert!(scheduler.is_loaded_flag());

        let deleted = delete_with_scheduler(&*provider, "fx-a", &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("delete");
        assert_eq!(deleted.row.state, SandboxState::Deleted);
        assert!(deleted.reaper_removed);
        assert!(!scheduler.is_loaded_flag(), "nothing live is left, so the job is gone");
        assert_eq!(scheduler.removes(), 1);

        let saved = store.load().await.expect("catalog");
        assert_eq!(saved.rows.len(), 1, "the tombstone stays in the catalog");
        assert_eq!(saved.row("fx-a").map(|row| row.state), Some(SandboxState::Deleted));
        assert!(!saved.has_live());
        assert!(provider.live_ids().is_empty());
        assert!(!provider.call_methods().contains(&"resume"));
    }

    #[tokio::test]
    async fn delete_keeps_the_reaper_while_another_row_is_live() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::ssh());
        let scheduler = FakeScheduler::unloaded();
        let store = store(&dir);
        create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &pool(&dir, &["record-1"]), &scheduler).await.expect("a");
        create_with_scheduler(&*provider, "fake", request("fx-b"), &store, &pool(&dir, &["record-1", "record-2"]), &scheduler).await.expect("b");

        let deleted = delete_with_scheduler(&*provider, "fx-a", &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("delete a");
        assert!(!deleted.reaper_removed);
        assert!(scheduler.is_loaded_flag());
        let saved = store.load().await.expect("catalog");
        assert!(saved.has_live());
    }

    #[tokio::test]
    async fn delete_of_an_unknown_name_is_typed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let error = delete_with_scheduler(&*provider, "fx-missing", &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect_err("unknown name");
        assert_eq!(error.code(), "NOT_IN_CATALOG");
        assert!(error.to_string().contains("fx-missing"));
        assert!(provider.call_methods().is_empty());
    }

    #[tokio::test]
    async fn delete_of_a_machine_the_provider_already_lost_still_tombstones_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, vec![row("fx-a", "fake-404", SandboxState::Ready)]).await;
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let deleted = delete_with_scheduler(&*provider, "fx-a", &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("delete an already-gone machine");
        assert_eq!(deleted.row.state, SandboxState::Deleted);
        assert!(!provider.call_methods().contains(&"delete"), "nothing was there to delete");
        assert_eq!(store.load().await.unwrap().row("fx-a").unwrap().state, SandboxState::Deleted);
    }

    #[tokio::test]
    async fn deleting_twice_does_not_call_the_provider_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, vec![row("fx-a", "fake-1", SandboxState::Deleted)]).await;
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let deleted = delete_with_scheduler(&*provider, "fx-a", &store, &pool(&dir, &["record-1"]), &scheduler)
            .await
            .expect("idempotent delete");
        assert_eq!(deleted.row.state, SandboxState::Deleted);
        assert!(provider.call_methods().is_empty());
        // The tombstone is still the only row, and the reaper is removed because nothing is live.
        assert!(deleted.reaper_removed);
        assert_eq!(store.load().await.unwrap().rows.len(), 1);
    }

    #[tokio::test]
    async fn a_repeat_delete_returns_a_lease_the_first_delete_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let slots = pool(&dir, &["record-1"]);
        // The tombstone exists but the record is still held: a first delete that died between the
        // provider call and the release leaves exactly this. Without clearing it, the record would
        // stay out of the pool forever.
        let stale = slots.lease("fx-a").await.expect("the lease the first delete left behind");
        let store = seed(&dir, vec![row("fx-a", "fake-1", SandboxState::Deleted)]).await;
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let deleted = delete_with_scheduler(&*provider, "fx-a", &store, &slots, &scheduler)
            .await
            .expect("a repeat delete is already true");

        assert_eq!(deleted.row.state, SandboxState::Deleted);
        assert!(provider.call_methods().is_empty(), "a tombstone is never sent to the provider");
        assert!(slots.lease_of("fx-a").await.unwrap().is_none(), "the stale lease is cleared");
        assert!(!stale.dir().join(super::super::slots::LEASE_FILE).exists());
        // The record itself survives, still holding its keypair, and the next machine can use it.
        assert!(stale.identity_path().is_file());
        let next = slots.lease("fx-b").await.expect("the record went back to the pool");
        assert_eq!(next.slot(), "record-1");
    }

    #[tokio::test]
    async fn a_catalog_write_failure_returns_the_slot_and_reports_the_catalog_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let slots = pool(&dir, &["record-1"]);
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();
        // The atomic write publishes through `<catalog>.json.tmp.<pid>`; a directory there makes the
        // `open` fail with EISDIR, so `save` fails the way a full or read-only disk does. This works
        // even as root, where a permission bit would not.
        let blocked = store.path().with_extension(format!("json.tmp.{}", std::process::id()));
        std::fs::create_dir_all(&blocked).unwrap();

        let error = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect_err("the catalog could not be written");

        assert_eq!(error.code(), "CATALOG_ERROR");
        // The machine exists but nothing records it, so the record must not stay out of the pool.
        assert!(slots.lease_of("fx-a").await.unwrap().is_none(), "the slot went back");
        assert_eq!(provider.live_ids().len(), 1, "the machine itself was created");
        assert!(!store.path().exists(), "no catalog file was published");

        // With the disk writable again the freed record is usable, and the idempotent create
        // returns the machine it already made instead of a second one.
        std::fs::remove_dir_all(&blocked).unwrap();
        let retried = create_with_scheduler(&*provider, "fake", request("fx-a"), &store, &slots, &scheduler)
            .await
            .expect("a retried create after the catalog is writable");
        assert_eq!(retried.slot.as_ref().map(|lease| lease.slot()), Some("record-1"));
        assert_eq!(provider.live_ids().len(), 1, "the retry reused the machine");
        assert_eq!(store.load().await.unwrap().row("fx-a").map(|row| row.slot.as_deref()), Some(Some("record-1")), "the row records the slot");
    }

    // ---- reconcile ----------------------------------------------------------------------------

    #[tokio::test]
    async fn an_idle_row_is_stopped_and_never_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        let created = create_with_scheduler(&*provider, "fake", request("fx-a"), &store(&dir), &pool(&dir, &["record-1"]), &FakeScheduler::loaded())
            .await
            .expect("create");        // 31 minutes of no use on a 30-minute idle window.
        let mut idle = created.row.clone();
        idle.last_used_at = T0;
        idle.state = SandboxState::Ready;
        let store = seed(&dir, vec![idle]).await;

        let report = reconcile_at(&store, &registry(provider.clone()), T0 + 31 * MINUTE_MS)
            .await
            .expect("reconcile");

        assert_eq!(report.acted(SweepAction::Stopped).len(), 1);
        assert!(report.records.iter().all(|record| record.action != SweepAction::Deleted));
        assert!(provider.call_methods().contains(&"stop"));
        assert!(!provider.call_methods().contains(&"delete"));
        let saved = store.load().await.expect("catalog");
        assert!(saved.has_live(), "a stopped machine is still a row");
        assert_eq!(saved.row("fx-a").unwrap().state, SandboxState::Ready);
        assert_eq!(provider.live_ids().len(), 1);
    }

    #[tokio::test]
    async fn a_row_inside_its_idle_window_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        let store = seed(&dir, vec![row("fx-a", "fake-1", SandboxState::Ready)]).await;
        // The fake has no machine `fake-1`, so keep the row out of the "absent" branch by creating
        // the same id through its own idempotency key path: `create` returns `fake-1` first.
        provider
            .create(CreateSandbox {
                idempotency_key: "seed".into(),
                name: "fx-a".into(),
                ttl_seconds: Some(600),
                env: BTreeMap::new(),
            })
            .await
            .expect("seed the provider");
        provider.get("fake-1").await.expect("promote to ready");

        let report = reconcile_at(&store, &registry(provider.clone()), T0 + MINUTE_MS)
            .await
            .expect("reconcile");

        assert!(report.records.is_empty(), "{:?}", report.records);
        assert_eq!(report.examined, 1);
        assert!(!provider.call_methods().contains(&"stop"));
        assert!(!provider.call_methods().contains(&"delete"));
    }

    #[tokio::test]
    async fn a_row_past_the_hard_deadline_is_deleted_even_while_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        provider
            .create(CreateSandbox {
                idempotency_key: "seed".into(),
                name: "fx-a".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("seed");
        provider.get("fake-1").await.expect("promote");
        let mut stale = row("fx-a", "fake-1", SandboxState::Ready);
        stale.last_used_at = T0; // idle too, but the hard ceiling wins
        let store = seed(&dir, vec![stale]).await;

        let past = T0 + seconds_to_ms(HARD_MAX_SECONDS) + 1;
        let report = reconcile_at(&store, &registry(provider.clone()), past).await.expect("reconcile");

        assert_eq!(report.acted(SweepAction::Deleted).len(), 1);
        assert!(provider.call_methods().contains(&"delete"));
        assert!(!provider.call_methods().contains(&"stop"), "a machine past the ceiling is deleted, not stopped");
        let saved = store.load().await.expect("catalog");
        assert!(!saved.has_live());
        assert_eq!(saved.row("fx-a").unwrap().state, SandboxState::Deleted);
    }

    #[tokio::test]
    async fn a_retryable_row_is_kept_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        provider
            .create(CreateSandbox {
                idempotency_key: "seed".into(),
                name: "fx-a".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("seed");
        provider.get("fake-1").await.expect("promote");
        let mut stale = row("fx-a", "fake-1", SandboxState::Ready);
        stale.last_used_at = T0;
        let store = seed(&dir, vec![stale.clone()]).await;

        // A failed listing proves nothing is gone: nothing is stopped, deleted or tombstoned.
        provider.fail_next(ErrorKind::Retryable);
        let report = reconcile_at(&store, &registry(provider.clone()), T0 + 31 * MINUTE_MS)
            .await
            .expect("reconcile");
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].detail.contains("list failed"));
        assert!(report.records.is_empty());
        assert_eq!(store.load().await.unwrap().row("fx-a"), Some(&stale));

        // A failed idle stop is the same: the row keeps its state, so the next sweep retries it.
        let stop_fails = Arc::new(Delegating::failing_stop(provider.clone()));
        let report = reconcile_at(&store, &registry_of(stop_fails), T0 + 31 * MINUTE_MS)
            .await
            .expect("reconcile again");
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].detail.contains("idle stop failed"), "{:?}", report.failures);
        assert!(report.records.is_empty());
        assert_eq!(store.load().await.unwrap().row("fx-a"), Some(&stale));
        assert!(!provider.call_methods().contains(&"resume"));
        assert!(!provider.call_methods().contains(&"delete"));
    }

    #[tokio::test]
    async fn a_row_missing_from_the_listing_is_tombstoned_only_when_get_says_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, vec![row("fx-gone", "fake-404", SandboxState::Ready)]).await;
        let provider = Arc::new(FakeProvider::new());

        let report = reconcile_at(&store, &registry(provider.clone()), T0 + MINUTE_MS)
            .await
            .expect("reconcile");

        assert_eq!(report.acted(SweepAction::ConfirmedGone).len(), 1);
        let saved = store.load().await.expect("catalog");
        assert!(!saved.has_live());
        assert_eq!(saved.rows.len(), 1, "the tombstone is kept");
        assert!(!provider.call_methods().contains(&"delete"), "a machine the provider lost is not deleted twice");
    }

    #[tokio::test]
    async fn a_row_missing_from_the_listing_but_still_reported_by_get_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        provider
            .create(CreateSandbox {
                idempotency_key: "seed".into(),
                name: "fx-a".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("seed");
        provider.get("fake-1").await.expect("promote");
        let store = seed(&dir, vec![row("fx-a", "fake-1", SandboxState::Ready)]).await;

        // An empty listing with a `get` that still reports the machine: a paging bug at the
        // provider must never read as "deleted".
        let listing_empty = Arc::new(Delegating::listing_empty(provider.clone()));
        let report = reconcile_at(&store, &registry_of(listing_empty), T0 + MINUTE_MS)
            .await
            .expect("reconcile");

        assert!(report.records.is_empty(), "{:?}", report.records);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert!(store.load().await.unwrap().has_live(), "a live machine stays live");
        assert!(!provider.call_methods().contains(&"delete"));
    }

    #[tokio::test]
    async fn a_provider_machine_tagged_ours_but_unknown_to_the_catalog_is_an_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, Vec::new()).await;
        let provider = Arc::new(FakeProvider::new());
        // Two machines we tagged, one hand-made machine on the same account.
        let young = provider
            .create(CreateSandbox {
                idempotency_key: "young".into(),
                name: "fx-young".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("young");
        let old = provider
            .create(CreateSandbox {
                idempotency_key: "old".into(),
                name: "fx-old".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("old");
        provider
            .create(CreateSandbox {
                idempotency_key: "theirs".into(),
                name: "bong-i-1".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("theirs");

        // The fake's clock starts at T0 and advances one second per machine.
        let report = reconcile_at(&store, &registry(provider.clone()), T0 + seconds_to_ms(HARD_MAX_SECONDS) + 5_000)
            .await
            .expect("reconcile");

        let names: Vec<&str> = report.orphans.iter().filter_map(|orphan| orphan.name.as_deref()).collect();
        assert!(names.contains(&"fx-young") && names.contains(&"fx-old"), "{names:?}");
        assert!(!names.contains(&"bong-i-1"), "a machine we did not tag is never touched");
        assert!(report.orphans.iter().all(|orphan| orphan.deleted), "both are past the ceiling");
        assert!(!provider.live_ids().contains(&young));
        assert!(!provider.live_ids().contains(&old));
        assert_eq!(provider.live_ids().len(), 1, "the hand-made machine survives");
    }

    #[tokio::test]
    async fn a_young_orphan_is_reported_and_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, Vec::new()).await;
        let provider = Arc::new(FakeProvider::new());
        provider
            .create(CreateSandbox {
                idempotency_key: "young".into(),
                name: "fx-young".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("young");

        let report = reconcile_at(&store, &registry(provider.clone()), T0 + MINUTE_MS).await.expect("reconcile");

        assert_eq!(report.orphans.len(), 1);
        assert!(!report.orphans[0].deleted);
        assert_eq!(provider.live_ids().len(), 1);
        assert!(!provider.call_methods().contains(&"delete"));
    }

    #[tokio::test]
    async fn a_row_whose_provider_is_not_configured_is_reported_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut unknown = row("fx-a", "fake-1", SandboxState::Ready);
        unknown.provider = "daytona".to_string();
        let store = seed(&dir, vec![unknown.clone()]).await;
        let provider = Arc::new(FakeProvider::new());

        let report = reconcile_at(&store, &registry(provider), T0 + MINUTE_MS).await.expect("reconcile");

        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].detail.contains("not configured"));
        assert_eq!(store.load().await.unwrap().row("fx-a"), Some(&unknown));
    }

    #[tokio::test]
    async fn the_sweep_removes_the_reaper_once_the_last_row_is_confirmed_gone() {
        let dir = tempfile::tempdir().unwrap();
        let store = seed(&dir, vec![row("fx-a", "fake-404", SandboxState::Ready)]).await;
        let provider = Arc::new(FakeProvider::new());
        let scheduler = FakeScheduler::loaded();

        let report = reconcile_with(&store, &registry(provider), Some(&scheduler), T0 + MINUTE_MS)
            .await
            .expect("reconcile");

        assert_eq!(report.acted(SweepAction::ConfirmedGone).len(), 1);
        assert!(report.reaper_removed);
        assert!(!scheduler.is_loaded_flag());
        assert_eq!(store.load().await.unwrap().rows.len(), 1, "the tombstone stays");
    }

    #[tokio::test]
    async fn the_sweep_keeps_the_reaper_while_a_row_is_live() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        provider
            .create(CreateSandbox {
                idempotency_key: "seed".into(),
                name: "fx-a".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("seed");
        provider.get("fake-1").await.expect("promote");
        let store = seed(&dir, vec![row("fx-a", "fake-1", SandboxState::Ready)]).await;
        let scheduler = FakeScheduler::loaded();

        let report = reconcile_with(&store, &registry(provider), Some(&scheduler), T0 + MINUTE_MS)
            .await
            .expect("reconcile");
        assert!(!report.reaper_removed);
        assert!(scheduler.is_loaded_flag());
        assert_eq!(scheduler.removes(), 0);
    }

    #[tokio::test]
    async fn no_reconcile_path_ever_resumes_a_machine() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(FakeProvider::new());
        // One row of each shape a sweep can meet, all stopped, idle, or gone.
        let ids = [
            provider
                .create(CreateSandbox {
                    idempotency_key: "ready".into(),
                    name: "fx-ready".into(),
                    ttl_seconds: None,
                    env: BTreeMap::new(),
                })
                .await
                .unwrap(),
            provider
                .create(CreateSandbox {
                    idempotency_key: "stopped".into(),
                    name: "fx-stopped".into(),
                    ttl_seconds: None,
                    env: BTreeMap::new(),
                })
                .await
                .unwrap(),
        ];
        provider.get(&ids[0]).await.expect("promote ready");
        provider.stop(&ids[1]).await.expect("stop the second");
        provider.get(&ids[1]).await.expect("settle to stopped");

        let mut ready = row("fx-ready", &ids[0], SandboxState::Ready);
        ready.last_used_at = T0;
        let mut stopped = row("fx-stopped", &ids[1], SandboxState::Stopped);
        stopped.last_used_at = T0;
        let mut over = row("fx-over", "fake-404", SandboxState::Ready);
        over.hard_deadline = T0;
        let store = seed(&dir, vec![ready, stopped, over]).await;

        let report = reconcile_at(&store, &registry(provider.clone()), T0 + 31 * MINUTE_MS)
            .await
            .expect("reconcile");

        assert!(!report.records.is_empty());
        assert!(
            !provider.call_methods().contains(&"resume"),
            "a status check must never wake a machine: {:?}",
            provider.call_methods()
        );
    }

    // ---- helpers ----------------------------------------------------------------------------

    #[test]
    fn idle_and_hard_max_clamp_independently() {
        let unbounded = Capabilities {
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Relay,
            native_auto_stop: true,
        };
        assert_eq!(hard_max_seconds(unbounded), HARD_MAX_SECONDS);
        assert_eq!(idle_seconds(unbounded, None), DEFAULT_IDLE_STOP_SECONDS);
        assert_eq!(idle_seconds(unbounded, Some(60 * 60 * 24)), HARD_MAX_SECONDS);

        let short = Capabilities { max_ttl_seconds: Some(600), ..unbounded };
        assert_eq!(hard_max_seconds(short), 600);
        assert_eq!(idle_seconds(short, None), 600);
        assert_eq!(idle_seconds(short, Some(1)), 60, "a one-second idle window is nonsense");
    }

    #[test]
    fn names_are_tagged_once() {
        assert_eq!(tagged_name("fx-a"), "fx-a");
        assert_eq!(tagged_name("a"), "fx-a");
        assert_eq!(tagged_name("fx-fx-a"), "fx-fx-a");
    }

    #[test]
    fn every_error_code_is_stable() {
        assert_eq!(
            LifecycleError::ReaperRequired { provider: "e2b".into(), detail: None }.code(),
            "REAPER_REQUIRED"
        );
        assert_eq!(LifecycleError::UnknownProvider("nope".into()).code(), "UNKNOWN_PROVIDER");
        assert_eq!(LifecycleError::NotInCatalog("x".into()).code(), "NOT_IN_CATALOG");
        assert_eq!(LifecycleError::AlreadyDeleted("x".into()).code(), "ALREADY_DELETED");
        assert_eq!(
            LifecycleError::ProviderNotConfigured { name: "x".into(), provider: "p".into() }.code(),
            "PROVIDER_NOT_CONFIGURED"
        );
        assert_eq!(
            LifecycleError::Provider(SandboxError::new(ErrorKind::Retryable, "fake", "", "boom")).code(),
            "PROVIDER_ERROR"
        );
        assert_eq!(
            LifecycleError::Provider(SandboxError::new(ErrorKind::Retryable, "fake", "", "boom")).provider_kind(),
            Some(ErrorKind::Retryable)
        );
        assert_eq!(LifecycleError::Reaper(ReaperError::failed("install", "boom")).code(), "REAPER_ERROR");
        assert_eq!(
            LifecycleError::Catalog(IpcError::new(IpcErrorCode::IoError, "boom")).code(),
            "CATALOG_ERROR"
        );
    }
}
