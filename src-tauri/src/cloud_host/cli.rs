//! `ferryx-cli cloud ...`: the operator surface over the provider seam, the catalog and the
//! dispatch transports.
//!
//! Dispatch is MODE=A, as task 1 proved (`.omo/evidence/ferryx-cloud-host-sandbox/task-1-spike-r2.txt`):
//! [`socket`](CloudCliCommand::Socket) publishes a local unix socket that the box's RPC daemon
//! answers through, and the existing `host session` verbs run on this Mac against it. MODE=B -
//! running the verbs *on* the box and streaming their output back - is kept in the parser, but in
//! MODE=A [`Mhc`](CloudCliCommand::Mhc) refuses with exit 3 and [`Socket`](CloudCliCommand::Socket)
//! is the only dispatch verb that answers. [`DISPATCH_MODE`] is the single place that flips.
//!
//! Four rules this file follows, because they are what the rest of the module already guarantees:
//!
//! 1. **A secret never touches argv, stdout or the catalog.** The agent's auth file is filtered
//!    here in memory to its `hms` key and pushed as a 0600 file over the file API (relay
//!    transport) or `scp` over the per-machine conf (SSH transport). The bootstrap's own
//!    environment is written to a 0600 file and sourced by the script, which is why a provider
//!    that refuses create-time environment (Sprites) still bootstraps.
//! 2. **Every verb ends in a reconcile.** The reaper is the only thing that stops a forgotten
//!    machine without the operator, so a verb that could leave the catalog stale also runs the
//!    sweep, and a sweep failure is reported without hiding the verb's own outcome.
//! 3. **Status never wakes a machine.** `list` joins the catalog with `provider.get` and nothing
//!    else; no path here calls `resume` except the `resume` verb.
//! 4. **A machine that failed after `create` is rolled back.** The provider bills from the moment
//!    it answers, and `provision_relay` never releases the relay slot, so every step after the
//!    create sits inside one guard: any failure deletes the machine and gives the slot back. That
//!    is one `Err` arm ([`rollback_on_failure`]) around one future ([`run_steps`]), never a
//!    `discard` beside each step - a step that escapes the arm leaves a machine billing with
//!    nothing that can attach to it.
//!
//! # Model egress: one recorded decision per provider (todo 12, D1)
//!
//! A machine whose engine cannot reach the model endpoint boots into an engine whose first model
//! call fails. Task 1's spike proved this is the normal case, not an edge case: from a fresh
//! Daytona sandbox `curl https://100milionseek.top/` returned `000` while `registry.npmjs.org` and
//! `github.com` answered `200`, so the bootstrap installs fine and the model call does not
//! (`.omo/evidence/ferryx-cloud-host-sandbox/task-1-spike-r2.txt`). `cloud create` therefore has an
//! explicit egress decision per provider, taken by [`model_egress`] and recorded here:
//!
//! | provider | mechanism | why |
//! | --- | --- | --- |
//! | `daytona` | [`ModelEgress::Bridge`] | live 2026-11-21: egress to the endpoint is blocked (`000`), and its create API takes no network policy, so nothing can allowlist it at create time |
//! | `codespaces` | [`ModelEgress::Direct`] | a GitHub-hosted VM with unfiltered outbound egress, and its create body has no network field. Task 14 confirms it on a live box |
//! | `freestyle` | [`ModelEgress::Direct`] | the only provider whose create body takes a firewall: its one outbound rule (`{destination:{public:true}}`) *is* the allowlist-at-create answer, and it is required on every create |
//! | `instabox` | [`ModelEgress::Direct`] | unfiltered egress on the fixed host; no allowlist call exists. Task 14 confirms it |
//! | `tensorlake` | [`ModelEgress::Direct`] | unfiltered egress; no allowlist call exists. Task 14 confirms it |
//! | `e2b` | [`ModelEgress::Direct`] | Firecracker VM with the provider's default open egress; its create body has no network policy. Task 13 confirms it end to end |
//! | `blaxel` | [`ModelEgress::Direct`] | open egress; no allowlist call exists. Task 14 confirms it |
//! | `opencomputer` | [`ModelEgress::Direct`] | open egress; no allowlist call exists. Task 14 confirms it |
//! | `prized` | [`ModelEgress::Direct`] | open egress; no allowlist call exists. Task 14 confirms it |
//! | `sprites` | [`ModelEgress::Direct`] | open egress; no allowlist call exists. Task 14 confirms it |
//! | `vercel` | [`ModelEgress::Direct`] | open egress; no allowlist call exists. Task 14 confirms it |
//!
//! A bridge is only possible where the transport is SSH: it runs over one `ssh -T` channel, so a
//! relay-only provider (E2B, Blaxel, ...) can never be given one. That is why no relay provider in
//! that table is `Bridge`. If a relay provider ever turns out to block the endpoint, the answer is a
//! relay-side bridge, not this one - a new todo, not a change here.
//!
//! `Direct` leaves the `hms` `baseUrl` pointing at the real endpoint: the provider's own egress
//! already reaches it, so nothing on the box has to move. `Bridge` is the stateless model-egress bridge task
//! 1 spiked, shipped instead of re-implemented: [`filter_models`] repoints the `hms` `baseUrl` at
//! `http://127.0.0.1:18802/v1`, the box side (`resources/cloud-host/model-bridge.py`, pushed 0700)
//! accepts plain HTTP there and frames each request, and [`hold`] runs the Mac half over one
//! `ssh -T` channel: this Mac makes the real TLS call and streams the reply back, byte for byte.
//! Nothing about the bridge is stateful - it forwards the headers it is handed, holds no key of its
//! own and reads no credential - and no secret reaches its argv: the `hms` key travels inside the
//! request the engine already makes.
//!
//! A `Bridge` machine needs its dispatch socket open for model calls to work, which is the same
//! command that carries the session, and [`hold`] refuses to publish the socket until the bridge
//! has answered from inside the box. A broken bridge therefore cannot look like a working
//! dispatch.
//!
//! # `KEYROOT_SYNC=no` is provisional, not a design decision
//!
//! Task 1 ran MODE=A with no key-root sync: the box daemon warns `no key record for launch ...
//! continuing unauthenticated (refused from the next release; set SENPI_RPC_KEY_ROOT to the host's
//! agent dir)` and still serves. That holds only for the pinned engine (omo-ai `5.1.79`, engine
//! `2026.11.18-3`): **the next engine release refuses unproven clients**, so the key-root sync path
//! must stay implementable and nothing here may assume the warning stays harmless. The path task 1
//! proved works is MODE A' in `task-1-spike-r2.txt`: copy the box's `~/.omo/agent/rpc/keys/*` to a
//! 0700 directory on this Mac and pass `--rpc-key-root <dir>` on every `host session` verb. When the
//! pin moves past `2026.11.18`, that becomes mandatory and this file's dispatch helpers are where it
//! belongs. `docs/cloud-hosts.md` (todo 15) records the same caveat.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::bootstrap::{self, Pins, Role};
use super::catalog::{Catalog, CatalogRow, CatalogStore};
use super::enroll::{self, AttachProver, EnrollmentCodeIssuer, ProvePolicy};
use super::lifecycle::{self, LifecycleError, ProviderRegistry};
use super::reaper::{HostReaper, ReaperScheduler};
use super::provider::{
    blaxel::BlaxelProvider, codespaces::CodespacesProvider, daytona::DaytonaProvider,
    e2b::E2bProvider, freestyle::FreestyleProvider, instabox::InstaboxProvider,
    opencomputer::OpenComputerProvider, prized::PrizedProvider, sprites::SpritesProvider,
    tensorlake::TensorlakeProvider, vercel::VercelProvider,
};
use super::provider::{CreateSandbox, ExecResult, SandboxProvider, SandboxState, Transport};
use super::slots::{SlotLease, SlotRegistry};

/// Which dispatch form this build ships.
///
/// `A` is task 1's result: the forwarded socket carries the RPC protocol intact and a turn
/// finalizes through it, so the verbs run on this Mac. `B` would run them on the box; nothing
/// else in this file changes when this constant does, which is the point of naming it once.
pub const DISPATCH_MODE: char = 'A';

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;
/// A usage error, or a provider this build does not have - refused before anything is billed.
pub const EXIT_USAGE: i32 = 2;
/// The verb is not available in this dispatch mode.
pub const EXIT_REFUSED: i32 = 3;
/// `NO_RELAY_SLOT`, taken from [`enroll::EXIT_NO_RELAY_SLOT`] so the two cannot drift.
pub const EXIT_NO_RELAY_SLOT: i32 = enroll::EXIT_NO_RELAY_SLOT;

/// How long a created machine gets to reach `ready`. A provider that has not provisioned in five
/// minutes is reported rather than waited on: the machine exists, and `list` will show it.
pub const READY_BUDGET: Duration = Duration::from_secs(300);
const READY_POLL: Duration = Duration::from_secs(5);
/// The bootstrap installs a pinned Node and then a pinned engine over the network.
const BOOTSTRAP_TIMEOUT_SECONDS: u64 = 900;
const EXEC_TIMEOUT_SECONDS: u64 = 600;

/// Where the bootstrap script and its environment land on the box.
const BOX_BOOTSTRAP_SCRIPT: &str = ".ferryx-cloud/bootstrap.sh";
const BOX_BOOTSTRAP_ENV: &str = ".ferryx-cloud/bootstrap.env";
/// The engine launcher the bootstrap writes; `host ensure` and any MODE=B verb run through it so
/// the node that installed the engine is on `PATH`.
const BOX_ENGINE_LAUNCHER: &str = r#""$HOME/.local/share/ferryx-cloud/bin/omo""#;
const BOX_RPC_SOCKET: &str = r#""$HOME/.ferryx-cloud/rpc.sock""#;

/// The agent's own credentials, filtered to the one key a model call needs.
const AUTH_FILE: &str = ".omo/agent/auth.json";
const MODELS_FILE: &str = ".omo/agent/models.json";
const SETTINGS_FILE: &str = ".omo/agent/settings.json";

/// The box-local port the model bridge listens on, and the only address the box's `models.json`
/// is ever pointed at when the provider's own egress is blocked.
///
/// 18802 is the port task 1's spike used, so a box that still carries the spike's bridge behaves
/// the same way here.
pub const MODEL_BRIDGE_PORT: u16 = 18_802;
/// The mode a pushed credential lands with: owner-only, no group, no world.
const SECRET_FILE_MODE: u32 = 0o600;
/// The mode the box-side bridge script lands with: it is a listener, so it is executable and
/// still owner-only - never 0755.
const BRIDGE_FILE_MODE: u32 = 0o700;
/// The box-side half of the bridge, pushed 0700 next to the bootstrap.
const BOX_MODEL_BRIDGE: &str = ".ferryx-cloud/model-bridge.py";
/// The box-side bridge source, embedded so a box never downloads it.
const MODEL_BRIDGE_SCRIPT: &str = include_str!("../../resources/cloud-host/model-bridge.py");

/// How a created machine reaches the model endpoint. Recorded per provider in this module's docs
/// and reported by `cloud list --json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEgress {
    /// The provider's own egress already reaches the endpoint; `models.json` is pushed unchanged.
    Direct,
    /// The box has no route to the endpoint, so `models.json` points at the box-local bridge and
    /// the holder serves it. SSH transport only: the bridge runs over one `ssh -T` channel.
    Bridge,
}

impl ModelEgress {
    pub const fn as_str(self) -> &'static str {
        match self {
            ModelEgress::Direct => "direct",
            ModelEgress::Bridge => "bridge",
        }
    }
}

/// The egress decision for every provider this build ships.
///
/// One table, not a `match` in the middle of `create`: the answer is a recorded decision (see the
/// module docs for the reason per provider), and `every_provider_has_an_egress_decision` fails if
/// a newly added provider is missing from it.
pub const MODEL_EGRESS: &[(&str, ModelEgress)] = &[
    ("blaxel", ModelEgress::Direct),
    ("codespaces", ModelEgress::Direct),
    ("daytona", ModelEgress::Bridge),
    ("e2b", ModelEgress::Direct),
    ("freestyle", ModelEgress::Direct),
    ("instabox", ModelEgress::Direct),
    ("opencomputer", ModelEgress::Direct),
    ("prized", ModelEgress::Direct),
    ("sprites", ModelEgress::Direct),
    ("tensorlake", ModelEgress::Direct),
    ("vercel", ModelEgress::Direct),
];

/// The egress mechanism for `provider`. An unknown name is `Direct`: the registry lookup in
/// `create` refuses it long before this matters, and `list` must render a row from an older build
/// rather than fail.
pub fn model_egress(provider: &str) -> ModelEgress {
    MODEL_EGRESS
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, egress)| *egress)
        .unwrap_or(ModelEgress::Direct)
}

pub const CLOUD_USAGE: &str = "usage: ferryx-cli cloud <verb>\n  \
create --provider <p> --name <n> [--ttl 30m] [--json]\n  \
list [--json]\n  \
stop <name> | resume <name> | delete <name>\n  \
socket <name>            (MODE=A: prints the local socket path)\n  \
mhc <name> -- <args...>  (MODE=B: runs the engine on the box)\n  \
exec <name> -- <cmd...>  (one-shot command on the box, exit code passthrough)\n  \
reconcile                (the reaper entry calls this every 5 minutes)";

/// One parsed `ferryx-cli cloud` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudCliCommand {
    Create { provider: String, name: String, ttl: Option<u64>, json: bool },
    List { json: bool },
    Stop { name: String },
    Resume { name: String },
    Delete { name: String },
    Socket { name: String },
    /// The detached holder `socket` starts; not in the usage text because an operator never runs it.
    Hold { name: String },
    Mhc { name: String, argv: Vec<String> },
    Exec { name: String, argv: Vec<String> },
    /// Hidden: the scheduler entry runs it, and it is the only verb that does not end in a sweep.
    Reconcile,
}

/// Parse `ferryx-cli cloud ...`.
///
/// Every verb is strict about flags: an unknown one is an error rather than something ignored,
/// because `cloud create --provder e2b` must not silently create a machine on the default
/// provider.
pub fn parse_cloud_cli<I, T>(args: I) -> Result<CloudCliCommand, String>
where
    I: IntoIterator<Item = T>,
    T: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|arg| arg.as_ref().to_string()).collect();
    if args.get(1).map(String::as_str) != Some("cloud") {
        return Err(CLOUD_USAGE.into());
    }
    let verb = args.get(2).map(String::as_str).unwrap_or_default();
    // `ferryx-cli cloud` alone is a usage error, not a panic: the verb is missing, so there is
    // nothing after it to slice.
    let rest: &[String] = args.get(3..).unwrap_or(&[]);
    match verb {
        "create" => {
            let mut provider: Option<String> = None;
            let mut name: Option<String> = None;
            let mut ttl: Option<u64> = None;
            let mut json = false;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--provider" if provider.is_none() => {
                        provider = Some(value_after(rest, index, "--provider")?);
                        index += 2;
                    }
                    "--name" if name.is_none() => {
                        name = Some(value_after(rest, index, "--name")?);
                        index += 2;
                    }
                    "--ttl" if ttl.is_none() => {
                        ttl = Some(parse_ttl(&value_after(rest, index, "--ttl")?)?);
                        index += 2;
                    }
                    "--json" => {
                        json = true;
                        index += 1;
                    }
                    other => return Err(format!("unknown cloud create option `{other}`\n{CLOUD_USAGE}")),
                }
            }
            let provider = provider.ok_or_else(|| format!("missing --provider\n{CLOUD_USAGE}"))?;
            let name = name.ok_or_else(|| format!("missing --name\n{CLOUD_USAGE}"))?;
            validate_name(&name)?;
            Ok(CloudCliCommand::Create { provider, name, ttl, json })
        }
        "list" => {
            let mut json = false;
            for arg in rest {
                match arg.as_str() {
                    "--json" => json = true,
                    other => return Err(format!("unknown cloud list option `{other}`\n{CLOUD_USAGE}")),
                }
            }
            Ok(CloudCliCommand::List { json })
        }
        // `socket <name> --hold` is the detached half of `socket`: an operator never types it, so
        // it is parsed here rather than given a usage line of its own.
        "socket" if rest.get(1).map(String::as_str) == Some("--hold") && rest.len() == 2 => {
            validate_name(&rest[0])?;
            Ok(CloudCliCommand::Hold { name: rest[0].clone() })
        }
        "stop" | "resume" | "delete" | "socket" => {
            let name = single_name(rest, verb)?;
            Ok(match verb {
                "stop" => CloudCliCommand::Stop { name },
                "resume" => CloudCliCommand::Resume { name },
                "delete" => CloudCliCommand::Delete { name },
                _ => CloudCliCommand::Socket { name },
            })
        }
        "mhc" | "exec" => {
            // Everything after `--` is the remote argv, passed through untouched: the caller's
            // own flags must not be re-interpreted by this parser.
            let Some(separator) = rest.iter().position(|arg| arg == "--") else {
                return Err(format!("expected `cloud {verb} <name> -- <args...>`\n{CLOUD_USAGE}"));
            };
            let name = single_name(&rest[..separator], verb)?;
            let argv = rest[separator + 1..].to_vec();
            if argv.is_empty() {
                return Err(format!("`cloud {verb}` needs a command after `--`\n{CLOUD_USAGE}"));
            }
            Ok(if verb == "mhc" {
                CloudCliCommand::Mhc { name, argv }
            } else {
                CloudCliCommand::Exec { name, argv }
            })
        }
        "reconcile" if rest.is_empty() => Ok(CloudCliCommand::Reconcile),
        _ => Err(format!("unknown cloud verb `{verb}`\n{CLOUD_USAGE}")),
    }
}

fn value_after(args: &[String], index: usize, flag: &str) -> Result<String, String> {
    args.get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("missing value for {flag}\n{CLOUD_USAGE}"))
}

fn single_name(rest: &[String], verb: &str) -> Result<String, String> {
    match rest {
        [name] if !name.starts_with('-') => {
            validate_name(name)?;
            Ok(name.clone())
        }
        [name, ..] if name.starts_with('-') => {
            Err(format!("unknown cloud {verb} option `{name}`\n{CLOUD_USAGE}"))
        }
        [] => Err(format!("missing machine name\n{CLOUD_USAGE}")),
        _ => Err(format!("expected exactly one machine name\n{CLOUD_USAGE}")),
    }
}

/// A catalog name is also a provider-side name and a socket file stem, so it is a bare token.
fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid machine name `{name}`: letters, digits, '.', '_' and '-' only\n{CLOUD_USAGE}"
        ))
    }
}

/// `30m`, `2h`, `900s` or bare seconds.
fn parse_ttl(value: &str) -> Result<u64, String> {
    let (digits, multiplier) = match value.chars().last() {
        Some('s') => (&value[..value.len() - 1], 1),
        Some('m') => (&value[..value.len() - 1], 60),
        Some('h') => (&value[..value.len() - 1], 3_600),
        _ => (value, 1),
    };
    let amount: u64 = digits
        .parse()
        .map_err(|_| format!("invalid --ttl `{value}`: use 30m, 2h, 900s or seconds"))?;
    if amount == 0 {
        return Err("invalid --ttl: must be at least one second".into());
    }
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| format!("invalid --ttl `{value}`: too large"))
}

/// The provider registry this build ships: every adapter in `provider/`, keyed by its own name.
///
/// Constructing an adapter reads its credential from the environment and never fails - a provider
/// with no key reports a typed `Config` error on its first call, which is what lets `list` show a
/// row whose provider is now unconfigured instead of losing the row.
pub fn registry_from_env() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.insert("blaxel", std::sync::Arc::new(BlaxelProvider::from_env()));
    registry.insert("codespaces", std::sync::Arc::new(CodespacesProvider::from_env()));
    registry.insert("daytona", std::sync::Arc::new(DaytonaProvider::from_env()));
    registry.insert("e2b", std::sync::Arc::new(E2bProvider::from_env()));
    registry.insert("freestyle", std::sync::Arc::new(FreestyleProvider::from_env()));
    registry.insert("instabox", std::sync::Arc::new(InstaboxProvider::new()));
    registry.insert("opencomputer", std::sync::Arc::new(OpenComputerProvider::from_env()));
    registry.insert("prized", std::sync::Arc::new(PrizedProvider::from_env()));
    registry.insert("sprites", std::sync::Arc::new(SpritesProvider::from_env()));
    registry.insert("tensorlake", std::sync::Arc::new(TensorlakeProvider::new()));
    registry.insert("vercel", std::sync::Arc::new(VercelProvider::from_env()));
    registry
}

/// Run one parsed invocation and return the process exit code.
pub fn run_cloud_cli(command: CloudCliCommand) -> i32 {
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime.block_on(run(command)),
        Err(error) => {
            eprintln!("cannot start the cloud runtime: {error}");
            EXIT_FAILURE
        }
    }
}

async fn run(command: CloudCliCommand) -> i32 {
    let registry = registry_from_env();
    match command {
        CloudCliCommand::Create { provider, name, ttl, json } => {
            create(&registry, &provider, &name, ttl, json).await
        }
        CloudCliCommand::List { json } => list(&registry, json).await,
        CloudCliCommand::Stop { name } => stop(&registry, &name).await,
        CloudCliCommand::Resume { name } => resume(&registry, &name).await,
        CloudCliCommand::Delete { name } => delete(&registry, &name).await,
        CloudCliCommand::Socket { name } => socket(&registry, &name).await,
        CloudCliCommand::Hold { name } => hold(&registry, &name).await,
        CloudCliCommand::Mhc { name, argv } => mhc(&registry, &name, &argv).await,
        CloudCliCommand::Exec { name, argv } => exec(&registry, &name, &argv).await,
        CloudCliCommand::Reconcile => reconcile(&registry).await,
    }
}

// ---------------------------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------------------------

/// A failure inside one post-create step: the exit code `create` returns and the line it prints.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StepFailure {    code: i32,
    detail: String,
}

impl StepFailure {
    fn new(detail: impl Into<String>) -> Self {
        Self { code: EXIT_FAILURE, detail: detail.into() }
    }

    fn with_code(code: i32, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

/// Every step after `create`, in the order `create` runs them.
///
/// One list, because the rollback guard ([`rollback_on_failure`]) has to cover *all* of them: a
/// step outside it leaves a billed machine with nothing to attach to (and, on a relay machine, a
/// leased record nothing will ever release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateStep {
    /// The provider reports `ready`.
    WaitReady,
    /// The machine's own route in exists (SSH files, or the file API).
    Connect,
    /// The box's `$HOME`, read rather than assumed.
    Home,
    /// The pinned engine is installed and its launcher runs.
    Bootstrap,
    /// The filtered agent files are on the box at 0600.
    PushAgentFiles,
    /// The box can reach the model endpoint, directly or through the bridge.
    ModelEgress,
    /// A relay machine's pooled record is on the box and proven reachable.
    Enroll,
    /// The host daemon answers, so the first dispatch is not the first failure.
    EnsureHost,
}

impl CreateStep {
    /// The name a failure message carries, so an operator knows which step broke.
    const fn as_str(self) -> &'static str {
        match self {
            CreateStep::WaitReady => "wait_ready",
            CreateStep::Connect => "connect",
            CreateStep::Home => "home",
            CreateStep::Bootstrap => "bootstrap",
            CreateStep::PushAgentFiles => "push_agent_files",
            CreateStep::ModelEgress => "model_egress",
            CreateStep::Enroll => "enroll",
            CreateStep::EnsureHost => "ensure_host",
        }
    }
}

/// The steps one create runs, behind a trait.
///
/// The production implementation ([`LiveSteps`]) is the real work; the trait exists so a test can
/// fail any single step and prove the guard in [`rollback_on_failure`] covers it. Without a seam
/// here the rollback could only be reviewed by reading, which is how three of nine paths escaped
/// it once already.
#[async_trait::async_trait]
trait ProvisionSteps: Send + Sync {
    async fn wait_ready(&self) -> Result<(), StepFailure>;
    async fn connect(&self) -> Result<(), StepFailure>;
    async fn home(&self) -> Result<String, StepFailure>;
    async fn bootstrap(&self, role: Role, home: &str) -> Result<(), StepFailure>;
    async fn push_agent_files(&self, home: &str) -> Result<(), StepFailure>;
    async fn model_egress(&self, home: &str) -> Result<ModelEgress, StepFailure>;
    async fn enroll(&self) -> Result<Option<String>, StepFailure>;
    async fn ensure_host(&self) -> Result<(), StepFailure>;
}

/// Run every step in order, stopping at the first failure.
///
async fn run_steps<S: ProvisionSteps + ?Sized>(
    steps: &S,
    role: Role,
    transport: Transport,
) -> Result<ModelEgress, StepFailure> {
    steps.wait_ready().await?;
    steps.connect().await?;
    let home = steps.home().await?;
    // Push the filtered agent config (auth/models/settings) BEFORE bootstrap starts the `omo`
    // host daemon, so the daemon loads the `hms` provider and stripped settings on its very first
    // launch instead of requiring a restart.
    steps.push_agent_files(&home).await?;
    steps.bootstrap(role, &home).await?;
    let egress = steps.model_egress(&home).await?;
    if transport == Transport::Relay {
        if let Some(detail) = steps.enroll().await? {
            eprintln!("{detail}");
        }
    } else {
        steps.ensure_host().await?;
    }
    Ok(egress)
}

/// What a failed create does to the machine it just made.
#[async_trait::async_trait]
trait Rollbacker: Send + Sync {
    async fn discard(&self, name: &str);
}

/// Everything after the provider's `create`: the steps, inside the one guard, against a machine
/// that already exists.
///
/// This is the seam the rollback tests drive: the real caller passes the real [`LiveSteps`] and a
/// test passes a steps double that fails at one step, and both prove the same thing - the machine
/// is deleted and the relay slot released.
async fn finish_create(
    context: CreateContext<'_>,
    provider: &dyn SandboxProvider,
    row: &CatalogRow,
    role: Role,
    transport: Transport,
    steps: &dyn ProvisionSteps,
) -> Result<ModelEgress, StepFailure> {
    let rollback = MachineRollback { context, provider };
    rollback_on_failure(&rollback, &row.name, run_steps(steps, role, transport)).await
}

/// The production rollback: [`discard`], which deletes the machine and releases its slot.
struct MachineRollback<'a> {
    context: CreateContext<'a>,
    provider: &'a dyn SandboxProvider,
}

#[async_trait::async_trait]
impl Rollbacker for MachineRollback<'_> {
    async fn discard(&self, name: &str) {
        discard(&self.context, self.provider, name).await;
    }
}

/// One guard over every post-create step: on any failure the machine is deleted **and** its relay
/// slot is released, and the caller fails with the step's own code and message.
///
/// Deliberately the only place the create path reaches a rollback. `provision_relay` never releases
/// the lease, and a machine bills from the moment the provider answered, so a step that returns
/// `Err` here must not be able to skip it - which is why the steps are one future with one
/// `Err` arm rather than a `discard` call beside each step.
async fn rollback_on_failure<T>(
    rollback: &dyn Rollbacker,
    name: &str,
    steps: impl std::future::Future<Output = Result<T, StepFailure>>,
) -> Result<T, StepFailure> {
    match steps.await {
        Ok(value) => Ok(value),
        Err(failure) => {
            rollback.discard(name).await;
            Err(failure)
        }
    }
}

/// The production steps: the real bootstrap, credential push and egress setup for one machine.
struct LiveSteps<'a> {
    provider: &'a std::sync::Arc<dyn SandboxProvider>,
    provider_name: &'a str,
    row: &'a CatalogRow,
    lease: Option<&'a SlotLease>,
    /// Filled by [`ProvisionSteps::connect`]; every later step needs it.
    access: tokio::sync::OnceCell<Access>,
}

impl<'a> LiveSteps<'a> {
    fn new(
        provider: &'a std::sync::Arc<dyn SandboxProvider>,
        provider_name: &'a str,
        row: &'a CatalogRow,
        lease: Option<&'a SlotLease>,
    ) -> Self {
        Self { provider, provider_name, row, lease, access: tokio::sync::OnceCell::new() }
    }

    async fn access(&self) -> Result<&Access, StepFailure> {
        self.access
            .get()
            .ok_or_else(|| StepFailure::new("internal: the box access was never established"))
    }
}

#[async_trait::async_trait]
impl ProvisionSteps for LiveSteps<'_> {
    async fn wait_ready(&self) -> Result<(), StepFailure> {
        wait_ready(self.provider.as_ref(), &self.row.sandbox_id)
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::WaitReady.as_str())))
    }

    async fn connect(&self) -> Result<(), StepFailure> {
        let access = open_access(self.provider, self.row)
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::Connect.as_str())))?;
        self.access
            .set(access)
            .map_err(|_| StepFailure::new("internal: the box access was established twice"))
    }

    async fn home(&self) -> Result<String, StepFailure> {
        let access = self.access().await?;
        access
            .home()
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::Home.as_str())))
    }

    async fn bootstrap(&self, role: Role, home: &str) -> Result<(), StepFailure> {
        let access = self.access().await?;
        bootstrap_box(access, role, home)
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::Bootstrap.as_str())))
    }

    async fn push_agent_files(&self, home: &str) -> Result<(), StepFailure> {
        let access = self.access().await?;
        push_agent_files(access, home, model_egress(self.provider_name))
            .await
            .map_err(|error| {
                StepFailure::new(format!("{}: {error}", CreateStep::PushAgentFiles.as_str()))
            })
    }

    async fn model_egress(&self, home: &str) -> Result<ModelEgress, StepFailure> {
        let access = self.access().await?;
        establish_model_egress(access, home, self.provider_name)
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::ModelEgress.as_str())))
    }

    async fn enroll(&self) -> Result<Option<String>, StepFailure> {
        let Some(lease) = self.lease else {
            return Err(StepFailure::with_code(
                EXIT_FAILURE,
                format!(
                    "{}: NO_RELAY_SLOT: a relay machine was created without a leased slot",
                    CreateStep::Enroll.as_str()
                ),
            ));
        };
        let account = enroll::RelayAccount::from_env().map_err(|error| {
            StepFailure::with_code(error.exit_code(), format!("{}: {error}", CreateStep::Enroll.as_str()))
        })?;
        let issuer: &dyn EnrollmentCodeIssuer = &account;
        let prover: &dyn AttachProver = &account;
        let reachability = enroll::provision_and_prove(
            self.provider.as_ref(),
            &self.row.sandbox_id,
            lease,
            issuer,
            prover,
            ProvePolicy::default(),
        )
        .await
        .map_err(|error| {
            StepFailure::with_code(error.exit_code(), format!("{}: {error}", CreateStep::Enroll.as_str()))
        })?;
        Ok(Some(format!(
            "relay record {} is reachable (machineId {})",
            reachability.session_id, reachability.machine_id
        )))
    }

    async fn ensure_host(&self) -> Result<(), StepFailure> {
        let access = self.access().await?;
        ensure_host(access)
            .await
            .map_err(|error| StepFailure::new(format!("{}: {error}", CreateStep::EnsureHost.as_str())))
    }
}

// ---------------------------------------------------------------------------------------------
// Verbs
// ---------------------------------------------------------------------------------------------

async fn create(
    registry: &ProviderRegistry,
    provider_name: &str,
    name: &str,
    ttl: Option<u64>,
    json: bool,
) -> i32 {
    let catalog = match catalog_store() {
        Ok(catalog) => catalog,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    let slots = match SlotRegistry::default_location() {
        Ok(slots) => slots,
        Err(error) => return fail(EXIT_FAILURE, &error.to_string()),
    };
    let scheduler = match HostReaper::from_env() {
        Ok(scheduler) => scheduler,
        Err(error) => return fail(EXIT_FAILURE, &error.to_string()),
    };
    let context = CreateContext { registry, catalog: &catalog, slots: &slots, scheduler: &scheduler };
    create_with(&context, provider_name, name, ttl, json).await
}

/// Everything a create needs besides its arguments: one value so a test can point the whole path
/// at a temp catalog, a temp slot pool and a scheduler that touches nothing.
async fn create_with(
    context: &CreateContext<'_>,
    provider_name: &str,
    name: &str,
    ttl: Option<u64>,
    json: bool,
) -> i32 {
    let CreateContext { registry, catalog, slots, scheduler } = *context;
    // A provider this build does not have is refused before the sweep and before a slot is
    // leased: a misspelt `--provider` must not cost a round trip to every configured provider.
    let Some(provider) = registry.get(provider_name) else {
        return fail(
            EXIT_USAGE,
            &format!(
                "unknown provider: {provider_name} (this build has: {})",
                registry.names().join(", ")
            ),
        );
    };
    // Before anything is created: a row an earlier run left behind is settled first, so this
    // create cannot race a sweep that would stop the machine it is about to bootstrap.
    let _ = sweep(registry, catalog).await;

    let request = CreateSandbox {
        // The key names this attempt, so a retried create after a dropped response returns the
        // same machine instead of a second billable one.
        idempotency_key: format!("fx-{name}-{}", unix_seconds()),
        name: name.to_string(),
        ttl_seconds: ttl,
        // The bootstrap's environment travels separately as a 0600 file: Sprites refuses
        // create-time environment and nothing here may put a value on a command line.
        env: BTreeMap::new(),
    };
    let created = match lifecycle::create_with_scheduler(
        provider.as_ref(),
        provider_name,
        request,
        catalog,
        slots,
        scheduler,
    )
    .await
    {
        Ok(created) => created,
        Err(error) => return failure_from_lifecycle(&error),
    };
    let row = created.row.clone();
    let transport = row.transport;
    let role = match transport {
        Transport::Ssh => Role::Ssh,
        Transport::Relay => Role::Relay,
    };
    eprintln!(
        "created {} on {provider_name} ({:?}, sandbox {})",
        row.name, transport, row.sandbox_id
    );

    // One guard over every remaining step, including the wait for `ready`, the machine's own
    // access, the engine install, the credentials, the model route and the relay enrollment.
    let steps = LiveSteps::new(provider, provider_name, &row, created.slot.as_ref());
    let egress = match finish_create(
        *context,
        provider.as_ref(),
        &row,
        role,
        transport,
        &steps,
    )
    .await
    {
        Ok(egress) => egress,
        Err(failure) => return fail(failure.code, &failure.detail),
    };

    if json {
        let mut value = row_json(&row, Some(SandboxState::Ready));
        if let Some(object) = value.as_object_mut() {
            object.insert("transport".into(), json!(transport_token(transport)));
            object.insert("modelEgress".into(), json!(egress.as_str()));
        }
        println!("{value}");
    } else {
        println!("{}", row.name);
        println!("provider  {provider_name}");
        println!("sandbox   {}", row.sandbox_id);
        println!("transport {}", transport_token(transport));
        println!("egress    {}", egress.as_str());
        println!("socket    ferryx-cli cloud socket {}", row.name);
    }
    let _ = sweep(registry, catalog).await;
    EXIT_OK
}

async fn list(registry: &ProviderRegistry, json: bool) -> i32 {
    let catalog = match catalog_store() {
        Ok(catalog) => catalog,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    list_with(registry, &catalog, json).await
}

/// [`list`] against an explicit catalog: the seam a test uses to prove that reading status never
/// wakes anything.
async fn list_with(registry: &ProviderRegistry, catalog: &CatalogStore, json: bool) -> i32 {
    let rows = match catalog.load().await {
        Ok(catalog) => catalog.rows,
        Err(error) => return fail(EXIT_FAILURE, &format!("cannot read the catalog: {error:?}")),
    };

    // The live state of every row, read with `get` only: a stopped machine stays stopped, and a
    // provider this build cannot reach leaves the row's recorded state in place.
    let mut live: BTreeMap<String, Option<SandboxState>> = BTreeMap::new();
    for row in &rows {
        if row.is_tombstone() {
            live.insert(row.name.clone(), Some(SandboxState::Deleted));
            continue;
        }
        let state = match registry.get(&row.provider) {
            Some(provider) => match provider.get(&row.sandbox_id).await {
                Ok(info) => Some(info.state),
                Err(error) if error.is_not_found() => Some(SandboxState::Deleted),
                Err(_) => None,
            },
            None => None,
        };
        live.insert(row.name.clone(), state);
    }

    if json {
        let array: Vec<Value> = rows
            .iter()
            .map(|row| row_json(row, live.get(&row.name).copied().flatten()))
            .collect();
        println!("{}", Value::Array(array));
    } else if rows.is_empty() {
        println!("no cloud machines");
    } else {
        for row in &rows {
            let state = live
                .get(&row.name)
                .copied()
                .flatten()
                .unwrap_or(row.state);
            println!(
                "{:<24} {:<12} {:<10} {}",
                row.name,
                row.provider,
                state.to_string(),
                row.sandbox_id
            );
        }
    }
    let _ = sweep(registry, catalog).await;
    EXIT_OK
}

async fn stop(registry: &ProviderRegistry, name: &str) -> i32 {
    let (catalog, mut current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    match provider.stop(&row.sandbox_id).await {
        Ok(()) => {}
        Err(error) if error.is_not_found() => {
            return fail(EXIT_FAILURE, &format!("{} is gone at the provider: {error}", row.name))
        }
        Err(error) => return fail(EXIT_FAILURE, &format!("stop failed: {error}")),
    }
    let mut updated = row;
    updated.state = state_after(provider.as_ref(), &updated).await;
    current.upsert(updated.clone());
    if let Err(error) = catalog.save(&current).await {
        return fail(EXIT_FAILURE, &format!("cannot record the stop: {error:?}"));
    }
    println!("{} {}", updated.name, updated.state);
    let _ = sweep(registry, &catalog).await;
    EXIT_OK
}

async fn resume(registry: &ProviderRegistry, name: &str) -> i32 {
    let (catalog, mut current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    let ttl = row.ttl_seconds.max(60);
    match provider.resume(&row.sandbox_id, Some(ttl)).await {
        Ok(()) => {}
        Err(error) => return fail(EXIT_FAILURE, &format!("resume failed: {error}")),
    }
    let mut updated = row;
    updated.state = state_after(provider.as_ref(), &updated).await;
    // Resuming is use: the idle clock starts again from here.
    updated.last_used_at = now_ms();
    current.upsert(updated.clone());
    if let Err(error) = catalog.save(&current).await {
        return fail(EXIT_FAILURE, &format!("cannot record the resume: {error:?}"));
    }
    println!("{} {}", updated.name, updated.state);
    let _ = sweep(registry, &catalog).await;
    EXIT_OK
}

async fn delete(registry: &ProviderRegistry, name: &str) -> i32 {
    let catalog = match catalog_store() {
        Ok(catalog) => catalog,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    let slots = match SlotRegistry::default_location() {
        Ok(slots) => slots,
        Err(error) => return fail(EXIT_FAILURE, &error.to_string()),
    };
    let current = match catalog.load().await {
        Ok(current) => current,
        Err(error) => return fail(EXIT_FAILURE, &format!("cannot read the catalog: {error:?}")),
    };
    let Some(row) = current.row(name).cloned() else {
        return fail(EXIT_USAGE, &format!("no cloud machine named {name} is in the catalog"));
    };
    let Some(provider) = registry.get(&row.provider) else {
        return fail(
            EXIT_USAGE,
            &format!(
                "unknown provider: {} (this build has: {})",
                row.provider,
                registry.names().join(", ")
            ),
        );
    };
    match lifecycle::delete_with_reaper(provider.as_ref(), name, &catalog, &slots).await {
        Ok(deleted) => {
            println!("{} deleted", deleted.row.name);
            if deleted.reaper_removed {
                eprintln!("reaper entry removed: no cloud machine is left");
            }
        }
        Err(error) => return failure_from_lifecycle(&error),
    }
    let _ = sweep(registry, &catalog).await;
    EXIT_OK
}

/// The precondition every dispatch shares: the box's host daemon answers **before** the caller's
/// first verb runs.
///
/// The daemon exits after 15 minutes idle by design, so `ensure_host` is idempotent and cheap when
/// it is up. Doing it here rather than at each verb means a machine this Mac cannot reach fails at
/// the dispatch, with one clear line, instead of inside the caller's first request.
async fn dispatch_ready(access: &Access) -> Result<(), String> {
    ensure_host(access).await
}

/// Wait until `path` answers the RPC probe, or report it after `budget`.
///
/// A socket that exists is not a socket that answers: the forward binds the local end before the
/// far end is reachable, so the only readiness signal is a reply.
async fn wait_for_socket(path: &Path, budget: Duration) -> Result<(), String> {
    let deadline = Instant::now() + budget;
    loop {
        if crate::paired_host::socket_forward::probe(path).await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the dispatch socket {} did not answer within {}ms",
                path.display(),
                budget.as_millis()
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// MODE=A: publish a local socket that answers the box's RPC protocol, print it, and let the
/// holder that serves it outlive this command.
async fn socket(registry: &ProviderRegistry, name: &str) -> i32 {
    if DISPATCH_MODE != 'A' {
        return fail(EXIT_REFUSED, "MODE_B: use ferryx-cli cloud mhc");
    }
    let (catalog, _current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    let access = match open_access(&provider, &row).await {
        Ok(access) => access,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    if let Err(error) = dispatch_ready(&access).await {
        return fail(EXIT_FAILURE, &error);
    }

    let path = match crate::paired_host::socket_forward::socket_path(&row.name) {
        Ok(path) => path,
        Err(_) => return fail(EXIT_FAILURE, "cannot resolve the dispatch socket path"),
    };
    // A previous holder may still be serving it: the path answering is the answer.
    if crate::paired_host::socket_forward::probe(&path).await.is_ok() {
        println!("{}", path.display());
        return EXIT_OK;
    }
    if let Err(error) = spawn_holder(&row.name) {
        return fail(EXIT_FAILURE, &error);
    }
    // The holder owns the forward, so readiness is proven the same way for both transports: the
    // published path must answer one RPC line.
    if let Err(error) = wait_for_socket(&path, Duration::from_secs(30)).await {
        return fail(EXIT_FAILURE, &error);
    }
    println!("{}", path.display());
    let _ = sweep(registry, &catalog).await;
    EXIT_OK
}

/// The detached half of [`socket`]: opens the forward, then serves it until it is killed.
///
/// A forward lives no longer than the process holding it - that is the property that keeps a
/// crashed desktop from leaving an ssh behind - so `socket` cannot both print a path and exit
/// while serving it. This process is the serving one; it writes nothing to stdout because its
/// parent prints the path.
async fn hold(registry: &ProviderRegistry, name: &str) -> i32 {
    let (_catalog, _current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    match row.transport {
        Transport::Ssh => {
            #[cfg(not(unix))]
            {
                let _ = (&provider, &row);
                eprintln!("SSH socket forwarding for {name} is not supported on this platform");
                return EXIT_FAILURE;
            }
            #[cfg(unix)]
            {
                // The same access the verbs use: it resolves the machine's SSH files, and the home
                // directory its rpc socket lives under is read from the box rather than assumed.
                let access = match open_access(&provider, &row).await {
                    Ok(access) => access,
                    Err(error) => {
                        eprintln!("{error}");
                        return EXIT_FAILURE;
                    }
                };
                let AccessKind::Ssh { files, .. } = &access.inner else {
                    eprintln!("{name} is not reachable over SSH");
                    return EXIT_FAILURE;
                };
                let home = match access.home().await {
                    Ok(home) => home,
                    Err(error) => {
                        eprintln!("{error}");
                        return EXIT_FAILURE;
                    }
                };
                let local = match crate::ssh::socket_forward::default_local_socket(&row.name) {
                    Ok(local) => local,
                    Err(error) => {
                        eprintln!("cannot resolve the socket path for {name}: {error}");
                        return EXIT_FAILURE;
                    }
                };
                let spec = match crate::ssh::socket_forward::ForwardSpec::new(local, box_socket(&home)) {
                    Ok(spec) => spec,
                    Err(error) => {
                        eprintln!("bad forward spec for {name}: {error}");
                        return EXIT_FAILURE;
                    }
                };
                let forward = match crate::ssh::socket_forward::start_with_files(files, &spec).await {
                    Ok(forward) => forward,
                    Err(error) => {
                        eprintln!("cannot forward {name}'s socket: {error}");
                        return EXIT_FAILURE;
                    }
                };
                // A machine whose provider blocks the model endpoint serves that endpoint from this
                // Mac over its own `ssh -T` channel. The bridge is proved reachable *before* the
                // socket is published, because a published socket is a promise that a session can run
                // and a session whose first model call fails breaks that promise.
                if model_egress(&row.provider) == ModelEgress::Bridge {
                    let program = match crate::ssh::exec::resolve_ssh_program() {
                        Ok(program) => program,
                        Err(error) => {
                            eprintln!("cannot find ssh: {error}");
                            return EXIT_FAILURE;
                        }
                    };
                    let files = files.clone();
                    let box_home = home.clone();
                    let mut bridge = tokio::spawn(async move {
                        serve_model_bridge(&program, &files, &box_home).await
                    });
                    // The probe is the readiness signal: it answers once the listener is reachable
                    // from inside the box, so this waits for the state rather than for a delay.
                    let mut probe = Box::pin(prove_model_bridge(&access, &home));
                    let ready = tokio::select! {
                        result = &mut probe => Some(result),
                        joined = &mut bridge => {
                            let detail = match joined {
                                Ok(Ok(())) => "the channel closed".to_string(),
                                Ok(Err(error)) => error,
                                Err(error) => error.to_string(),
                            };
                            eprintln!("cannot bridge {name}'s model calls: {detail}");
                            None
                        }
                    };
                    match ready {
                        Some(Ok(())) => {
                            eprintln!("bridging {name}'s model calls over the dispatch channel");
                            hold_open((forward, bridge)).await;
                        }
                        Some(Err(error)) => {
                            eprintln!("cannot bridge {name}'s model calls: {error}");
                            bridge.abort();
                            return EXIT_FAILURE;
                        }
                        None => return EXIT_FAILURE,
                    }
                } else {
                    hold_open(forward).await;
                }
            }
        }
        Transport::Relay => {
            let forward = match relay_forward(&row).await {
                Ok(forward) => forward,
                Err(error) => {
                    eprintln!("cannot forward {name}'s socket: {error}");
                    return EXIT_FAILURE;
                }
            };
            hold_open(forward).await;
        }
    }
    EXIT_OK
}

/// Blocks until the process is killed, keeping `guard` alive. The guard's own `Drop` is what tears
/// the forward down, so it must not be dropped here: holding it in a binding that the loop below
/// never leaves is the whole mechanism.
async fn hold_open<T>(guard: T) {
    let _guard = guard;
    loop {
        tokio::time::sleep(Duration::from_secs(3_600)).await;
    }
}

// ---------------------------------------------------------------------------------------------
// The Mac half of the model bridge
// ---------------------------------------------------------------------------------------------

/// The upstream the box's `hms` calls are relayed to: the `baseUrl` this Mac's own `models.json`
/// records for that provider.
///
/// Read from the local file rather than hard-coded, so changing the endpoint is a `models.json`
/// edit on this Mac and nothing here moves. The value is not a secret (a host and a path prefix),
/// and it is never printed.
#[cfg(any(unix, test))]
fn upstream_endpoint(models: &[u8]) -> Result<(String, String), String> {
    let parsed: Value = serde_json::from_slice(models)
        .map_err(|error| format!("local {MODELS_FILE} is not JSON: {error}"))?;
    let base = parsed
        .get("providers")
        .and_then(|providers| providers.get("hms"))
        .and_then(|hms| hms.get("baseUrl"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("local {MODELS_FILE} has no providers.hms.baseUrl"))?;
    let url = url::Url::parse(base)
        .map_err(|error| format!("local providers.hms.baseUrl is not a URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("local providers.hms.baseUrl must be an absolute http(s) URL".into());
    }
    let mut origin = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        origin.push_str(&format!(":{port}"));
    }
    Ok((origin, url.path().trim_end_matches('/').to_string()))
}

/// The URL one bridged request goes to: the box's path joined to the upstream origin.
///
/// The box's `models.json` points at `http://127.0.0.1:<port>/v1`, so its path already carries the
/// base prefix. A path that does not (a provider client that drops it) still lands correctly.
fn bridged_url(origin: &str, base_path: &str, path: &str) -> String {
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    if base_path.is_empty() || path.starts_with(&format!("{base_path}/")) || path == base_path {
        format!("{origin}{path}")
    } else {
        format!("{origin}{base_path}{path}")
    }
}

/// Serve the box's model bridge over one `ssh -T` channel until it closes.
///
/// Each length-framed request from the box becomes one real HTTP call from this Mac, and the reply
/// is framed back chunk by chunk so server-sent events arrive as they are produced. Nothing is
/// stored: headers and body go out as they came in, and no credential is read here - the `hms` key
/// is already inside the request the engine made.
#[cfg(unix)]
async fn serve_model_bridge(
    program: &str,
    files: &crate::ssh::socket_forward::MachineFiles,
    home: &str,
) -> Result<(), String> {
    let local = local_home()?;
    let models = std::fs::read(local.join(MODELS_FILE))
        .map_err(|error| format!("cannot read {}: {error}", local.join(MODELS_FILE).display()))?;
    let (origin, base_path) = upstream_endpoint(&models)?;

    let script = format!("{}/{BOX_MODEL_BRIDGE}", home.trim_end_matches('/'));
    let mut child = tokio::process::Command::new(program)
        .args([
            "-T",
            "-F",
            &files.conf.to_string_lossy(),
            &files.alias,
            &format!("python3 {} {MODEL_BRIDGE_PORT}", quote_posix(&script)),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("cannot start the model bridge channel: {error}"))?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| "the bridge channel has no stdin".to_string())?;
    let mut output = child
        .stdout
        .take()
        .ok_or_else(|| "the bridge channel has no stdout".to_string())?;

    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(input));
    let client = reqwest::Client::builder()
        .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
        // No overall timeout: a long completion streams for minutes, and the box's own request
        // carries its own bound.
        .build()
        .map_err(|error| format!("cannot build the bridge HTTP client: {error}"))?;

    loop {
        let Some(frame) = read_frame(&mut output).await? else {
            return Ok(());
        };
        if frame.get("t").and_then(Value::as_str) != Some("req") {
            continue;
        }
        let writer = writer.clone();
        let client = client.clone();
        let origin = origin.clone();
        let base_path = base_path.clone();
        tokio::spawn(async move {
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            match forward_one(&client, &origin, &base_path, &frame, &writer, &id).await {
                Ok(()) => {}
                Err(error) => {
                    // The message is a reason, never a header or a body: those carry the key.
                    let _ =
                        write_frame(&writer, &json!({ "t": "err", "id": id, "error": error }))
                            .await;
                }
            }
        });
    }
}

/// One bridged request: the real call from this Mac, then its reply framed back to the box.
async fn forward_one(
    client: &reqwest::Client,
    origin: &str,
    base_path: &str,
    frame: &Value,
    writer: &tokio::sync::Mutex<tokio::process::ChildStdin>,
    id: &Value,
) -> Result<(), String> {
    use base64::Engine as _;
    use futures_util::StreamExt as _;

    let method = frame.get("method").and_then(Value::as_str).unwrap_or("POST");
    let path = frame.get("path").and_then(Value::as_str).unwrap_or("/");
    let url = bridged_url(origin, base_path, path);
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| "the box sent an unusable HTTP method".to_string())?;
    let body = frame
        .get("body")
        .and_then(Value::as_str)
        .map(|encoded| base64::engine::general_purpose::STANDARD.decode(encoded))
        .transpose()
        .map_err(|_| "the box sent an unreadable body".to_string())?
        .unwrap_or_default();

    let mut request = client.request(method, &url);
    if let Some(headers) = frame.get("headers").and_then(Value::as_object) {
        for (name, value) in headers {
            let Some(value) = value.as_str() else { continue };
            // Hop-by-hop and length headers are rewritten by this hop, not copied.
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "host" | "content-length" | "connection" | "accept-encoding" | "transfer-encoding"
            ) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                request = request.header(name, value);
            }
        }
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    let response = request.send().await.map_err(|error| describe(&error))?;
    let status = response.status().as_u16();
    let mut headers = serde_json::Map::new();
    for (name, value) in response.headers() {
        if matches!(
            name.as_str(),
            "connection" | "content-encoding" | "content-length" | "transfer-encoding"
        ) {
            continue;
        }
        if let Ok(value) = value.to_str() {
            headers.insert(name.as_str().to_string(), json!(value));
        }
    }
    write_frame(
        writer,
        &json!({ "t": "res", "id": id, "status": status, "headers": headers }),
    )
    .await?;

    let mut stream = response.bytes_stream();
    while let Some(next) = stream.next().await {
        let chunk = next.map_err(|error| describe(&error))?;
        if chunk.is_empty() {
            continue;
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(&chunk);
        write_frame(writer, &json!({ "t": "chunk", "id": id, "body": encoded })).await?;
    }
    write_frame(writer, &json!({ "t": "end", "id": id })).await?;
    Ok(())
}

/// A transport failure in words. `reqwest`'s own `Display` names the URL and never a header, so
/// nothing a caller logs can carry the key.
fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the upstream model request timed out".to_string()
    } else if error.is_connect() {
        "this Mac could not connect to the model endpoint".to_string()
    } else {
        format!("the model request failed: {error}")
    }
}

/// Write one length-framed JSON object, 4-byte big-endian length first.
async fn write_frame<W>(
    writer: &tokio::sync::Mutex<W>,
    value: &Value,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let payload = serde_json::to_vec(value).map_err(|error| format!("cannot frame: {error}"))?;
    let length = u32::try_from(payload.len()).map_err(|_| "a bridge frame is too large".to_string())?;
    let mut guard = writer.lock().await;
    guard
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|_| "the bridge channel closed".to_string())?;
    guard
        .write_all(&payload)
        .await
        .map_err(|_| "the bridge channel closed".to_string())?;
    guard.flush().await.map_err(|_| "the bridge channel closed".to_string())
}

/// Read one length-framed JSON object. `None` when the channel closed cleanly.
async fn read_frame<R>(reader: &mut R) -> Result<Option<Value>, String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut head = [0u8; 4];
    match reader.read_exact(&mut head).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("cannot read from the bridge channel: {error}")),
    }
    let mut payload = vec![0u8; u32::from_be_bytes(head) as usize];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| format!("cannot read a bridge frame: {error}"))?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|error| format!("a bridge frame is not JSON: {error}"))
}

/// Prove the bridge answers **from inside the box**, so a socket is never published over a model
/// route that does not work.
#[cfg(any(unix, test))]
async fn prove_model_bridge(access: &Access, home: &str) -> Result<(), String> {
    let script = format!("{}/{BOX_MODEL_BRIDGE}", home.trim_end_matches('/'));
    let command = format!(
        "python3 {script} --probe {MODEL_BRIDGE_PORT}",
        script = quote_posix(&script)
    );
    let result = access.run(&command, 60).await?;
    if result.exit_code != Some(0) {
        return Err(format!(
            "the model bridge did not answer from inside the box: {}",
            tail(&result.stderr, 300)
        ));
    }
    Ok(())
}

/// MODE=B only: run the engine on the box and pass its exit code through.
async fn mhc(registry: &ProviderRegistry, name: &str, argv: &[String]) -> i32 {
    if DISPATCH_MODE == 'A' {
        return fail(EXIT_REFUSED, "MODE_A: use ferryx-cli cloud socket");
    }
    let (catalog, _current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    let access = match open_access(&provider, &row).await {
        Ok(access) => access,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    if let Err(error) = dispatch_ready(&access).await {
        return fail(EXIT_FAILURE, &error);
    }
    let mut command = String::from(BOX_ENGINE_LAUNCHER);
    for arg in argv {
        command.push(' ');
        command.push_str(&quote_posix(arg));
    }
    let result = match access.run(&command, EXEC_TIMEOUT_SECONDS).await {
        Ok(result) => result,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    let _ = sweep(registry, &catalog).await;
    result.exit_code.unwrap_or(EXIT_FAILURE)
}

/// One-shot command on the box, exit code passed through unchanged.
async fn exec(registry: &ProviderRegistry, name: &str, argv: &[String]) -> i32 {
    let (catalog, _current, row, provider) = match load_row(registry, name).await {
        Ok(found) => found,
        Err(code) => return code,
    };
    let access = match open_access(&provider, &row).await {
        Ok(access) => access,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    let command = argv.join(" ");
    let result = match access.run(&command, EXEC_TIMEOUT_SECONDS).await {
        Ok(result) => result,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    let _ = sweep(registry, &catalog).await;
    result.exit_code.unwrap_or(EXIT_FAILURE)
}

/// The hidden verb the reaper entry runs. Exit 0 even when a row failed: a scheduler that treats
/// one provider's outage as a job failure retries the whole sweep forever.
async fn reconcile(registry: &ProviderRegistry) -> i32 {
    let catalog = match catalog_store() {
        Ok(catalog) => catalog,
        Err(error) => return fail(EXIT_FAILURE, &error),
    };
    match sweep(registry, &catalog).await {
        Ok(report) => {
            for record in &report.records {
                eprintln!("{} {}: {}", record.provider, record.name, record.action.as_str());
            }
            for failure in &report.failures {
                eprintln!("{} {}: {}", failure.provider, failure.name, failure.detail);
            }
            EXIT_OK
        }
        Err(error) => failure_from_lifecycle(&error),
    }
}

// ---------------------------------------------------------------------------------------------
// The box: bootstrap, credentials, host daemon
// ---------------------------------------------------------------------------------------------

/// How this Mac reaches one machine: over SSH with its own conf, or through the file API.
#[derive(Clone)]
struct Access {
    inner: AccessKind,
}

#[derive(Clone)]
enum AccessKind {
    #[cfg(unix)]
    Ssh {
        program: String,
        files: crate::ssh::socket_forward::MachineFiles,
    },
    Relay {
        provider: std::sync::Arc<dyn SandboxProvider>,
        sandbox_id: String,
    },
}

async fn open_access(
    provider: &std::sync::Arc<dyn SandboxProvider>,
    row: &CatalogRow,
) -> Result<Access, String> {
    match row.transport {
        #[cfg(unix)]
        Transport::Ssh => {
            let target = provider
                .ssh_target(&row.sandbox_id)
                .await
                .map_err(|error| format!("cannot resolve {}'s SSH endpoint: {error}", row.name))?;
            let machine = machine_ssh(&row.name, target);
            let files = crate::ssh::socket_forward::write_machine_files(&machine)
                .map_err(|error| format!("cannot write {}'s SSH files: {error}", row.name))?;
            let program = crate::ssh::exec::resolve_ssh_program()
                .map_err(|error| format!("cannot find ssh: {error}"))?;
            Ok(Access { inner: AccessKind::Ssh { program, files } })
        }
        #[cfg(not(unix))]
        Transport::Ssh => Err(format!(
            "SSH cloud host access for {} is not supported on this platform",
            row.name
        )),
        Transport::Relay => Ok(Access {
            inner: AccessKind::Relay {
                provider: provider.clone(),
                sandbox_id: row.sandbox_id.clone(),
            },
        }),
    }
}

impl Access {
    /// One non-interactive command. Never carries a secret: callers pass paths only.
    async fn run(&self, command: &str, timeout_seconds: u64) -> Result<ExecResult, String> {
        match &self.inner {
            #[cfg(unix)]
            AccessKind::Ssh { program, files } => {
                ssh_exec(program, files, command, timeout_seconds).await
            }
            AccessKind::Relay { provider, sandbox_id } => provider
                .exec(sandbox_id, command, timeout_seconds)
                .await
                .map_err(|error| format!("exec failed: {error}")),
        }
    }

    /// Write one file with `mode`, through the channel that is allowed to carry it.
    ///
    /// `home` is the box's own `$HOME`, read once by the caller, so no image layout is assumed.
    async fn push(
        &self,
        home: &str,
        relative: &str,
        bytes: Vec<u8>,
        mode: u32,
    ) -> Result<(), String> {
        let remote = format!("{}/{relative}", home.trim_end_matches('/'));
        match &self.inner {
            #[cfg(unix)]
            AccessKind::Ssh { program, files } => {
                scp_file(program, files, &remote, bytes, mode).await
            }
            AccessKind::Relay { provider, sandbox_id } => provider
                .write_file(sandbox_id, &remote, bytes, mode)
                .await
                .map_err(|error| format!("cannot write {remote}: {error}")),
        }
    }

    async fn home(&self) -> Result<String, String> {
        let result = self.run("printf %s \"$HOME\"", 30).await?;
        let home = result.stdout.trim().to_string();
        if home.is_empty() {
            return Err("the box did not report a home directory".into());
        }
        Ok(home)
    }
}

/// The bootstrap script and its environment, both pushed as files and then run with `set -a`.
///
/// The environment is delivered this way rather than at create time because Sprites refuses
/// create-time environment and `exec` has no environment parameter: one mechanism that every
/// provider accepts beats two that differ. A value never appears on a command line.
async fn bootstrap_box(access: &Access, role: Role, home: &str) -> Result<(), String> {
    let mut env = bootstrap::env_for(role, Pins::PINNED);
    // A relay role with no pinned client fails inside the script with its own named error; the
    // missing pins are reported here as well so the operator is not left with a box-side message.
    if role == Role::Relay && (Pins::PINNED.ferryx_cli_url.is_empty() || Pins::PINNED.ferryx_cli_sha256.is_empty()) {
        return Err(
            "the relay role needs a pinned ferryx-cli (FX_FERRYX_CLI_URL/FX_FERRYX_CLI_SHA256); this \
             build ships neither, so a relay machine cannot be bootstrapped yet"
                .into(),
        );
    }
    let env_text = env_file_text(&mut env)?;
    access
        .push(home, BOX_BOOTSTRAP_ENV, env_text.into_bytes(), 0o600)
        .await?;
    access
        .push(
            home,
            BOX_BOOTSTRAP_SCRIPT,
            bootstrap::script().as_bytes().to_vec(),
            0o700,
        )
        .await?;
    let command = format!(
        "set -a; . {env}; set +a; sh {script}",
        env = quote_posix(&format!("{home}/{BOX_BOOTSTRAP_ENV}")),
        script = quote_posix(&format!("{home}/{BOX_BOOTSTRAP_SCRIPT}")),
    );
    let result = access.run(&command, BOOTSTRAP_TIMEOUT_SECONDS).await?;
    if result.timed_out {
        // Sprites' exec cannot kill a timed-out remote command, so an immediate retry would race
        // the first bootstrap; nothing here retries at all.
        return Err(format!(
            "the bootstrap on the box timed out after {BOOTSTRAP_TIMEOUT_SECONDS}s"
        ));
    }
    if result.exit_code != Some(0) {
        let detail = tail(&result.stderr, 600);
        return Err(format!(
            "the bootstrap on the box failed with exit {:?}: {detail}",
            result.exit_code
        ));
    }
    Ok(())
}

/// `KEY='value'` lines the script sources. Values are pinned constants, but a stray quote or
/// newline would still break the file, so both are refused.
fn env_file_text(env: &mut BTreeMap<String, String>) -> Result<String, String> {
    let mut text = String::new();
    for (key, value) in env.iter() {
        if value.contains(['\'', '\n', '\r']) {
            return Err(format!("bootstrap environment value for {key} cannot be quoted"));
        }
        text.push_str(key);
        text.push('=');
        text.push('\'');
        text.push_str(value);
        text.push('\'');
        text.push('\n');
    }
    Ok(text)
}

/// The agent's credentials and configuration: the filtered auth file, the filtered model catalog
/// and the settings file, all at 0600.
///
/// Every file is reduced to what the box can use before it is written:
///
/// - `auth.json` keeps its `hms` entry and nothing else - the box needs one model provider, and
///   every other key in that file is a credential it has no use for.
/// - `models.json` keeps the `hms` provider entry only ([`filter_models`]). It carries a literal
///   `apiKey` per provider, so pushing it whole would copy ten other providers' credentials onto a
///   third-party box - the same leak `filter_auth` exists to prevent.
/// - `settings.json` is pushed as it is, unless it holds a key-like field, which is removed
///   ([`filter_settings`]).
///
/// Nothing is printed, and the bytes go straight from the local file to a 0600 remote file.
async fn push_agent_files(access: &Access, home: &str, egress: ModelEgress) -> Result<(), String> {
    let local = local_home()?;
    push_agent_files_from(access, home, &local, egress).await
}

/// [`push_agent_files`] against an explicit agent directory.
///
/// The split exists because the alternative is a test that rewrites `HOME`, which races every
/// other test in this binary: the filtering is worth testing, the environment is not.
async fn push_agent_files_from(
    access: &Access,
    home: &str,
    local: &Path,
    egress: ModelEgress,
) -> Result<(), String> {
    let auth_path = local.join(AUTH_FILE);
    match std::fs::read(&auth_path) {
        Ok(bytes) => {
            let filtered = filter_auth(&bytes)?;
            access.push(home, AUTH_FILE, filtered, SECRET_FILE_MODE).await?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no {}: the box will start without model credentials", auth_path.display());
        }
        Err(error) => return Err(format!("cannot read {}: {error}", auth_path.display())),
    }
    for (relative, keep_catalog) in [(MODELS_FILE, true), (SETTINGS_FILE, false)] {
        let path = local.join(relative);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let filtered = if keep_catalog {
                    filter_models(&bytes, egress)?
                } else {
                    filter_settings(&bytes)?
                };
                access.push(home, relative, filtered, SECRET_FILE_MODE).await?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        }
    }
    Ok(())
}

/// `{"hms": { ... }}` and nothing else. A file without an `hms` key is an error rather than an
/// empty object: pushing `{}` would look like success and break the first model call.
fn filter_auth(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let parsed: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("{AUTH_FILE} is not JSON: {error}"))?;
    let Some(hms) = parsed.get("hms") else {
        return Err(format!("{AUTH_FILE} has no `hms` entry to push"));
    };
    let filtered = json!({ "hms": hms });
    serde_json::to_vec(&filtered).map_err(|error| format!("cannot serialize the credentials: {error}"))
}

/// `models.json` reduced to the `hms` provider entry, with its `baseUrl` repointed when the box has
/// no route to the endpoint of its own.
///
/// The file is `{disabledProviders, providers: {<name>: {..., apiKey, baseUrl, models}}}`. Keeping
/// only `providers.hms` matters twice over: the other entries carry their own literal `apiKey`
/// values, which the box has no use for, and `disabledProviders` names providers that are no longer
/// present. A `models.json` with no `providers.hms` is an error for the same reason `filter_auth`
/// refuses an auth file with no `hms`: an empty catalog looks like success and breaks the first
/// model call.
fn filter_models(bytes: &[u8], egress: ModelEgress) -> Result<Vec<u8>, String> {
    let parsed: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("{MODELS_FILE} is not JSON: {error}"))?;
    let Some(hms) = parsed.get("providers").and_then(|providers| providers.get("hms")) else {
        return Err(format!("{MODELS_FILE} has no providers.hms entry to push"));
    };
    let mut hms = hms.clone();
    if egress == ModelEgress::Bridge {
        // The only edit a bridge machine needs: its engine calls the box-local bridge, which is
        // where the real TLS call is made on this Mac.
        let object = hms.as_object_mut().ok_or_else(|| {
            format!("{MODELS_FILE} providers.hms is not an object")
        })?;
        object.insert(
            "baseUrl".into(),
            json!(format!("http://127.0.0.1:{MODEL_BRIDGE_PORT}/v1")),
        );
    }
    let filtered = json!({ "providers": { "hms": hms } });
    serde_json::to_vec(&filtered).map_err(|error| format!("cannot serialize the model catalog: {error}"))
}

/// `settings.json` without any key-like field.
///
/// The settings file is what makes the engine usable (permission rules, the default model), so it
/// is pushed as it is - but a future version that grew a credential field would otherwise spread it
/// to a third-party box silently. Any field whose name looks like a credential is removed, at every
/// depth, and the rest is untouched.
fn filter_settings(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let parsed: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("{SETTINGS_FILE} is not JSON: {error}"))?;
    let mut filtered = strip_key_like_fields(parsed);
    // Drop local-machine plugin packages and npmCommand (e.g. ["bun"]): on a bare 512 MiB cloud
    // box, auto-installing host plugins on first session open fails with ENOENT or OOM.
    if let Some(object) = filtered.as_object_mut() {
        object.remove("packages");
        object.remove("npmCommand");
    }
    serde_json::to_vec(&filtered)
        .map_err(|error| format!("cannot serialize the settings: {error}"))
}

/// The field names that make a settings value a credential. Matched case-insensitively against the
/// whole field name, so `apiKey`, `api_key` and `APIKEY` all match and a permission rule does not.
const KEY_LIKE_FIELDS: &[&str] = &[
    "apikey",
    "api_key",
    "token",
    "secret",
    "password",
    "credential",
    "credentials",
    "privatekey",
    "private_key",
    "authkey",
    "auth_key",
    "accesstoken",
    "access_token",
    "refreshtoken",
    "refresh_token",
];

fn is_key_like(field: &str) -> bool {
    let normalised = field.trim().to_ascii_lowercase();
    KEY_LIKE_FIELDS.contains(&normalised.as_str())
}

fn strip_key_like_fields(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| !is_key_like(key))
                .map(|(key, value)| (key, strip_key_like_fields(value)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(strip_key_like_fields).collect())
        }
        other => other,
    }
}

/// Make the model endpoint reachable from the box, per the provider's recorded decision
/// ([`model_egress`]).
///
/// `Direct`: nothing to do - the provider's own egress already reaches the endpoint, and
/// `models.json` was pushed unchanged.
///
/// `Bridge`: push the box half of the bridge (0700, never 0755 - it is a listener) and prove it is
/// on the box. The Mac half is started by [`hold`], because it lives exactly as long as the
/// dispatch channel does: a bridge the engine cannot reach is not a working bridge, and the
/// alternative - a detached Mac process per machine - would outlive the command that started it
/// and leave a listener pointing at a machine that may be gone.
async fn establish_model_egress(
    access: &Access,
    home: &str,
    provider_name: &str,
) -> Result<ModelEgress, String> {
    let egress = model_egress(provider_name);
    if egress == ModelEgress::Direct {
        return Ok(egress);
    }
    #[cfg(unix)]
    let is_ssh = matches!(access.inner, AccessKind::Ssh { .. });
    #[cfg(not(unix))]
    let is_ssh = false;
    if !is_ssh {
        // A bridge runs over one `ssh -T` channel, so a relay-only provider cannot be given one.
        // Saying so here beats a machine whose model calls fail with `connection refused`.
        return Err(format!(
            "{provider_name} is recorded as needing the model bridge, but its transport is the relay; \
             the bridge is SSH-only and this machine has no route to the endpoint"
        ));
    }
    access
        .push(
            home,
            BOX_MODEL_BRIDGE,
            MODEL_BRIDGE_SCRIPT.as_bytes().to_vec(),
            BRIDGE_FILE_MODE,
        )
        .await?;
    // The bridge is only useful if it is where the engine looks for it. Proving the file is there
    // (and the interpreter it needs) turns a missing python into a create-time error instead of a
    // model call that fails on the box minutes later.
    let command = format!(
        "test -x {script} && command -v python3 >/dev/null",
        script = quote_posix(&format!("{home}/{BOX_MODEL_BRIDGE}")),
    );
    let result = access.run(&command, 60).await?;
    if result.exit_code != Some(0) {
        return Err(format!(
            "the box cannot run the model bridge (python3 missing or the script is not executable): {}",
            tail(&result.stderr, 300)
        ));
    }
    Ok(egress)
}

/// `omo host ensure`, run before any dispatch: the host daemon exits after 15 minutes idle.
async fn ensure_host(access: &Access) -> Result<(), String> {
    let command = format!("{BOX_ENGINE_LAUNCHER} host ensure --socket {BOX_RPC_SOCKET}");
    let result = access.run(&command, 120).await?;
    if result.exit_code != Some(0) {
        return Err(format!(
            "the box could not ensure its host daemon (exit {:?}): {}",
            result.exit_code,
            tail(&result.stderr, 400)
        ));
    }
    Ok(())
}

/// The box's own rpc socket, which the forward ends at.
fn box_socket(home: &str) -> String {
    format!("{}/.ferryx-cloud/rpc.sock", home.trim_end_matches('/'))
}

/// A T1 machine's SSH files, from the provider's own endpoint.
#[cfg(unix)]
fn machine_ssh(
    name: &str,
    target: super::provider::SshTarget,
) -> crate::ssh::socket_forward::MachineSsh {
    let (identity_file, certificate_file) = match target.identity {
        super::provider::SshIdentity::KeyFile(path) => {
            (Some(path.to_string_lossy().into_owned()), target.certificate_file)
        }
        super::provider::SshIdentity::Agent => (None, target.certificate_file),
    };
    crate::ssh::socket_forward::MachineSsh {
        name: name.to_string(),
        hostname: target.host,
        username: Some(target.user),
        port: Some(target.port),
        identity_file,
        pinned_host_key: target.known_hosts_line,
        proxy_command: target.proxy_command,
        certificate_file,
    }
}

/// The relay transport's local listener, over the paired-host route the desktop already has.
#[cfg(unix)]
async fn relay_forward(
    row: &CatalogRow,
) -> Result<crate::paired_host::socket_forward::SocketForward, String> {
    let dir = crate::remote::auth::canonical_identity_dir()
        .map_err(|error| format!("cannot resolve the identity directory: {error}"))?;
    // The record the box enrolled as, so the relay's machineId - not the box's own name - is what
    // the local inventory is searched by.
    let machine_id = match &row.slot {
        Some(_) => {
            let slots = SlotRegistry::default_location().map_err(|error| error.to_string())?;
            slots
                .lease_of(&row.name)
                .await
                .map_err(|error| format!("cannot read the slot lease: {error}"))?
                .map(|lease| lease.machine_id().to_string())
        }
        None => None,
    };
    let service = crate::paired_host::service::PairedHostService::open(PathBuf::from(&dir));
    let find_host = |dir: &Path| -> Result<Option<crate::paired_host::inventory::HostView>, String> {
        let inventory = crate::paired_host::inventory::Inventory::open(dir)
            .map_err(|error| format!("cannot read the paired-host inventory: {error}"))?;
        Ok(inventory
            .list()
            .into_iter()
            .find(|host| machine_id.as_ref() == Some(&host.machine_id)))
    };
    let host = match find_host(Path::new(&dir))? {
        Some(host) => host,
        None => {
            // Freshly enrolled cloud slots may not be paired in this Mac's local inventory yet:
            // adopt the local account session once so `cloud socket` works with no manual step.
            if let Ok(home) = local_home() {
                let token_path = enroll::account_session_token_path(&home);
                if let Ok(token) = std::fs::read_to_string(&token_path) {
                    let trimmed = token.trim();
                    if !trimmed.is_empty() {
                        let _ = service
                            .adopt_account_session(
                                enroll::RELAY_ORIGIN.to_string(),
                                Some(trimmed.to_string()),
                            )
                            .await;
                    }
                }
            }
            find_host(Path::new(&dir))?.ok_or_else(|| {
                format!(
                    "{} has no paired host for its relay record yet; run the account reconcile first",
                    row.name
                )
            })?
        }
    };
    let client = crate::paired_host::client::MachineClient::new();
    crate::paired_host::socket_forward::listen(
        client,
        service,
        host.host_id,
        host.generation,
        &row.name,
    )
    .await
    .map_err(|error| format!("cannot listen for {}: {}", row.name, error.code))
}

#[cfg(not(unix))]
async fn relay_forward(
    _row: &CatalogRow,
) -> Result<crate::paired_host::socket_forward::SocketForward, String> {
    Err("the relay socket forward is unix-only".into())
}

/// Start the detached holder that serves one socket, and return once it exists.
fn spawn_holder(name: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot find this executable: {error}"))?;
        let mut command = std::process::Command::new(exe);
        command
            .args(["cloud", "socket", name, "--hold"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: `setsid` is async-signal-safe, takes no arguments, touches no shared state, and
        // runs between fork and exec where only such calls are permitted. It detaches the holder
        // from this process's session so it survives this command exiting.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        command
            .spawn()
            .map(|_child| ())
            .map_err(|error| format!("cannot start the socket holder: {error}"))
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        Err("the dispatch socket is unix-only".into())
    }
}

// ---------------------------------------------------------------------------------------------
// SSH plumbing (T1): a command and a file push over the per-machine conf
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
async fn ssh_exec(
    program: &str,
    files: &crate::ssh::socket_forward::MachineFiles,
    command: &str,
    timeout_seconds: u64,
) -> Result<ExecResult, String> {
    // `-T` because nothing interactive is ever wanted here, and the conf already pins
    // BatchMode, the per-machine known_hosts and (where the provider has one) ProxyCommand.
    let argv = [
        program.to_string(),
        "-T".to_string(),
        "-F".to_string(),
        files.conf.to_string_lossy().into_owned(),
        files.alias.clone(),
        command.to_string(),
    ];
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_seconds.saturating_add(30)),
        tokio::process::Command::new(&argv[0]).args(&argv[1..]).output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => Ok(ExecResult {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: false,
        }),
        Ok(Err(error)) => Err(format!("cannot run ssh: {error}")),
        Err(_) => Ok(ExecResult {
            exit_code: None,
            stdout: String::new(),
            stderr: format!("ssh did not finish within {timeout_seconds}s"),
            timed_out: true,
        }),
    }
}

/// Format a remote path for an `ssh` shell command: an absolute path is POSIX-quoted as-is, while
/// a home-relative path is anchored under `"$HOME/"`.
#[cfg(any(unix, test))]
fn remote_shell_path(path: &str) -> String {
    if path.starts_with('/') {
        quote_posix(path)
    } else {
        format!("\"$HOME/\"{}", quote_posix(path))
    }
}

/// Push one file onto the box with `scp`, then enforce its mode.
///
/// `scp` reads the same per-machine conf as `ssh`, so it inherits the pinned host key and the
/// `ProxyCommand` a CodeSpace needs. The bytes come from a 0600 temporary file that is removed
/// before this returns. `remote_path` may be either absolute (as passed by `Access::push`) or
/// home-relative.
#[cfg(unix)]
async fn scp_file(
    program: &str,
    files: &crate::ssh::socket_forward::MachineFiles,
    remote_path: &str,
    bytes: Vec<u8>,
    mode: u32,
) -> Result<(), String> {
    let dir = tempfile::Builder::new()
        .prefix("fx-cloud-push")
        .tempdir()
        .map_err(|error| format!("cannot create a temporary directory: {error}"))?;
    let local = dir.path().join("payload");
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&local)
            .map_err(|error| format!("cannot create the staging file: {error}"))?;
        file.write_all(&bytes)
            .map_err(|error| format!("cannot stage the file: {error}"))?;
    }
    // The remote directory is created first: `scp` does not make intermediate directories, and a
    // fresh box has no `~/.ferryx-cloud` or `~/.omo/agent`.
    if let Some(parent) = Path::new(remote_path).parent() {
        let parent = parent.to_string_lossy();
        if !parent.is_empty() && parent != "." && parent != "/" {
            let parent_expr = remote_shell_path(&parent);
            let command = format!("mkdir -p {parent_expr} && chmod 700 {parent_expr}");
            let result = ssh_exec(program, files, &command, 30).await?;
            if result.exit_code != Some(0) {
                return Err(format!(
                    "cannot create {parent} on the box: {}",
                    tail(&result.stderr, 300)
                ));
            }
        }
    }
    let scp_program = scp_program(program);
    let argv = [
        scp_program,
        "-q".to_string(),
        "-F".to_string(),
        files.conf.to_string_lossy().into_owned(),
        local.to_string_lossy().into_owned(),
        format!("{}:{}", files.alias, remote_path),
    ];
    let output = tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|error| format!("cannot run scp: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "scp of {remote_path} failed: {}",
            tail(&String::from_utf8_lossy(&output.stderr), 300)
        ));
    }
    // `scp` creates the file with the staging mode masked by the box's umask; the mode is only
    // certain once it is set explicitly.
    let target_expr = remote_shell_path(remote_path);
    let command = format!("chmod {mode:o} {target_expr}");
    let result = ssh_exec(program, files, &command, 30).await?;
    if result.exit_code != Some(0) {
        return Err(format!(
            "cannot set the mode of {remote_path} on the box: {}",
            tail(&result.stderr, 300)
        ));
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn scp_program(ssh_program: &str) -> String {
    match Path::new(ssh_program).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.join("scp").to_string_lossy().into_owned()
        }
        _ => "scp".to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

async fn load_row(
    registry: &ProviderRegistry,
    name: &str,
) -> Result<(CatalogStore, Catalog, CatalogRow, std::sync::Arc<dyn SandboxProvider>), i32> {
    let catalog = match catalog_store() {
        Ok(catalog) => catalog,
        Err(error) => return Err(fail(EXIT_FAILURE, &error)),
    };
    let current = match catalog.load().await {
        Ok(current) => current,
        Err(error) => {
            return Err(fail(
                EXIT_FAILURE,
                &format!("cannot read the catalog: {error:?}"),
            ))
        }
    };
    let Some(row) = current.row(name).cloned() else {
        return Err(fail(
            EXIT_USAGE,
            &format!("no cloud machine named {name} is in the catalog"),
        ));
    };
    let Some(provider) = registry.get(&row.provider) else {
        return Err(fail(
            EXIT_USAGE,
            &format!(
                "unknown provider: {} (this build has: {})",
                row.provider,
                registry.names().join(", ")
            ),
        ));
    };
    Ok((catalog, current, row, provider.clone()))
}

fn catalog_store() -> Result<CatalogStore, String> {
    CatalogStore::default_location().map_err(|error| format!("cannot locate the catalog: {error:?}"))
}

fn local_home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "cannot resolve the home directory".to_string())
}

async fn wait_ready(provider: &dyn SandboxProvider, sandbox_id: &str) -> Result<(), String> {
    let deadline = Instant::now() + READY_BUDGET;
    loop {
        match provider.get(sandbox_id).await {
            Ok(info) => match info.state {
                SandboxState::Ready => return Ok(()),
                SandboxState::Error => {
                    return Err(format!(
                        "the provider reports {sandbox_id} in error: {}",
                        info.error.unwrap_or_else(|| "no detail".into())
                    ))
                }
                SandboxState::Deleted => {
                    return Err(format!("{sandbox_id} was deleted while it was provisioning"))
                }
                _ => {}
            },
            Err(error) if error.is_not_found() => {
                return Err(format!("{sandbox_id} is not known to the provider: {error}"))
            }
            // A transient read failure is not a failed create; the deadline decides.
            Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{sandbox_id} did not become ready within {}s",
                READY_BUDGET.as_secs()
            ));
        }
        tokio::time::sleep(READY_POLL).await;
    }
}

/// Everything a create or a delete needs besides its arguments, bundled so a test can point the
/// whole path at a temp catalog, a temp slot pool and a scheduler that touches nothing.
#[derive(Clone, Copy)]
struct CreateContext<'a> {
    registry: &'a ProviderRegistry,
    catalog: &'a CatalogStore,
    slots: &'a SlotRegistry,
    scheduler: &'a dyn ReaperScheduler,
}

/// Delete a machine whose bootstrap or enrollment failed: `provision_relay` never releases the
/// lease, and a half-provisioned box has nothing that can attach to it.
async fn discard(context: &CreateContext<'_>, provider: &dyn SandboxProvider, name: &str) {
    match lifecycle::delete_with_scheduler(
        provider,
        name,
        context.catalog,
        context.slots,
        context.scheduler,
    )
    .await
    {
        Ok(_) => eprintln!("rolled back {name}: machine deleted and its relay slot released"),
        Err(error) => eprintln!(
            "ROLLBACK FAILED for {name}: {error} - delete it with `ferryx-cli cloud delete {name}`"
        ),
    }
}

/// The sweep every verb ends with, at the wall clock.
async fn sweep(
    registry: &ProviderRegistry,
    catalog: &CatalogStore,
) -> Result<lifecycle::ReconcileReport, LifecycleError> {
    let report = lifecycle::reconcile(catalog, registry).await?;
    for failure in &report.failures {
        eprintln!("reconcile: {} {}: {}", failure.provider, failure.name, failure.detail);
    }
    Ok(report)
}

async fn state_after(provider: &dyn SandboxProvider, row: &CatalogRow) -> SandboxState {
    match provider.get(&row.sandbox_id).await {
        Ok(info) => info.state,
        Err(error) if error.is_not_found() => SandboxState::Deleted,
        // The call that just succeeded is the fact; a read failure must not overwrite it.
        Err(_) => row.state,
    }
}

fn row_json(row: &CatalogRow, live: Option<SandboxState>) -> Value {
    json!({
        "name": row.name,
        "provider": row.provider,
        "sandboxId": row.sandbox_id,
        "transport": transport_token(row.transport),
        "state": live.unwrap_or(row.state).to_string(),
        "createdAt": row.created_at,
        "lastUsedAt": row.last_used_at,
        "ttlSeconds": row.ttl_seconds,
        "idleDeadline": row.idle_deadline(),
        "hardDeadline": row.hard_deadline,
        "socketPath": row.socket_path,
        "slot": row.slot,
    })
}

fn transport_token(transport: Transport) -> &'static str {
    match transport {
        Transport::Ssh => "ssh",
        Transport::Relay => "relay",
    }
}

fn failure_from_lifecycle(error: &LifecycleError) -> i32 {
    let code = match error {
        LifecycleError::UnknownProvider(_)
        | LifecycleError::NotInCatalog(_)
        | LifecycleError::AlreadyDeleted(_)
        | LifecycleError::ProviderNotConfigured { .. } => EXIT_USAGE,
        LifecycleError::Slot(super::slots::SlotError::NoFreeSlot) => EXIT_NO_RELAY_SLOT,
        _ => EXIT_FAILURE,
    };
    fail(code, &format!("{}: {error}", error.code()))
}

fn fail(code: i32, message: &str) -> i32 {
    eprintln!("{message}");
    code
}

/// The last `limit` characters, so a provider's multi-line output cannot flood a terminal.
fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().rev().nth(limit) {
        Some((index, _)) => format!("…{}", &trimmed[index + 1..]),
        None => trimmed.to_string(),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> i64 {
    i64::try_from(unix_seconds().saturating_mul(1_000)).unwrap_or(i64::MAX)
}

fn quote_posix(value: &str) -> String {
    crate::ssh::direct::quote_posix(value)
}

/// The Machines list's cloud rows: the catalog as it was last recorded.
///
/// Recorded state only - no `provider.get` per row. The settings list is drawn on every visit, and
/// fanning out to eleven providers there would turn opening a pane into eleven network calls, some
/// of which wake nothing but still cost a round trip. `ferryx-cli cloud list --json` is the view
/// that joins live state, and it is the one an operator asks for deliberately.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_cloud_host_list() -> Result<Vec<Value>, String> {
    let store = catalog_store()?;
    let catalog = store
        .load()
        .await
        .map_err(|error| format!("cannot read the cloud catalog: {error:?}"))?;
    Ok(catalog
        .rows
        .iter()
        .filter(|row| !row.is_tombstone())
        .map(|row| {
            json!({
                "name": row.name,
                "provider": row.provider,
                "sandboxId": row.sandbox_id,
                "state": row.state.to_string(),
                "transport": transport_token(row.transport),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(words: &[&str]) -> Result<CloudCliCommand, String> {
        parse_cloud_cli(words)
    }

    #[test]
    fn create_takes_a_provider_a_name_and_an_optional_ttl() {
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "create", "--provider", "e2b", "--name", "box-1"]),
            Ok(CloudCliCommand::Create {
                provider: "e2b".into(),
                name: "box-1".into(),
                ttl: None,
                json: false,
            })
        );
        assert_eq!(
            parse(&[
                "ferryx-cli", "cloud", "create", "--name", "box-1", "--provider", "e2b", "--ttl",
                "30m", "--json",
            ]),
            Ok(CloudCliCommand::Create {
                provider: "e2b".into(),
                name: "box-1".into(),
                ttl: Some(1_800),
                json: true,
            })
        );
    }

    #[test]
    fn an_unknown_flag_is_an_error_not_something_ignored() {
        // `--provder` must not fall back to a default provider: a typo would otherwise create a
        // billable machine on a provider the operator never named.
        let error = parse(&[
            "ferryx-cli",
            "cloud",
            "create",
            "--provder",
            "e2b",
            "--name",
            "box-1",
        ])
        .expect_err("a misspelt flag is refused");
        assert!(error.contains("unknown cloud create option `--provder`"), "{error}");
        assert!(parse(&["ferryx-cli", "cloud", "list", "--jsn"]).is_err());
        assert!(parse(&["ferryx-cli", "cloud", "stop", "--force"]).is_err());
    }

    #[test]
    fn a_missing_name_or_provider_is_rejected() {
        for words in [
            vec!["ferryx-cli", "cloud", "create", "--provider", "e2b"],
            vec!["ferryx-cli", "cloud", "create", "--name", "box-1"],
            vec!["ferryx-cli", "cloud", "stop"],
            vec!["ferryx-cli", "cloud", "resume"],
            vec!["ferryx-cli", "cloud", "delete"],
            vec!["ferryx-cli", "cloud", "socket"],
        ] {
            let error = parse(&words).expect_err("a missing name is a usage error");
            assert!(
                error.starts_with("missing --provider")
                    || error.starts_with("missing --name")
                    || error.starts_with("missing machine name"),
                "{words:?} -> {error}"
            );
        }
    }

    #[test]
    fn a_name_that_could_leave_the_socket_directory_is_refused() {
        for name in ["../escape", "a/b", "", "-dash", ".hidden", "a b"] {
            assert!(
                validate_name(name).is_err(),
                "{name:?} must not become a path segment"
            );
        }
        assert!(validate_name("box-1.2_a").is_ok());
    }

    #[test]
    fn mhc_and_exec_pass_everything_after_the_separator_through() {
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "mhc", "box-1", "--", "host", "session", "--json"]),
            Ok(CloudCliCommand::Mhc {
                name: "box-1".into(),
                argv: vec!["host".into(), "session".into(), "--json".into()],
            })
        );
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "exec", "box-1", "--", "uname", "-a"]),
            Ok(CloudCliCommand::Exec {
                name: "box-1".into(),
                argv: vec!["uname".into(), "-a".into()],
            })
        );
        // Without `--` there is no way to tell the remote argv from this parser's own flags.
        assert!(parse(&["ferryx-cli", "cloud", "mhc", "box-1", "host"]).is_err());
        assert!(parse(&["ferryx-cli", "cloud", "exec", "box-1", "--"]).is_err());
    }

    #[test]
    fn the_hidden_hold_form_is_the_only_socket_invocation_with_two_words() {
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "socket", "box-1", "--hold"]),
            Ok(CloudCliCommand::Hold { name: "box-1".into() })
        );
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "socket", "box-1"]),
            Ok(CloudCliCommand::Socket { name: "box-1".into() })
        );
    }

    #[test]
    fn list_and_reconcile_take_no_operand() {
        assert_eq!(parse(&["ferryx-cli", "cloud", "list"]), Ok(CloudCliCommand::List { json: false }));
        assert_eq!(
            parse(&["ferryx-cli", "cloud", "list", "--json"]),
            Ok(CloudCliCommand::List { json: true })
        );
        assert_eq!(parse(&["ferryx-cli", "cloud", "reconcile"]), Ok(CloudCliCommand::Reconcile));
        assert!(parse(&["ferryx-cli", "cloud", "reconcile", "box-1"]).is_err());
        assert!(parse(&["ferryx-cli", "cloud"]).is_err());
        assert!(parse(&["ferryx-cli", "cloud", "frobnicate"]).is_err());
        // A different group is not this parser's business.
        assert!(parse(&["ferryx-cli", "account", "enroll"]).is_err());
    }

    #[test]
    fn a_ttl_is_seconds_or_a_suffixed_amount() {
        assert_eq!(parse_ttl("900"), Ok(900));
        assert_eq!(parse_ttl("900s"), Ok(900));
        assert_eq!(parse_ttl("30m"), Ok(1_800));
        assert_eq!(parse_ttl("2h"), Ok(7_200));
        for bad in ["0", "0m", "soon", "1d", ""] {
            assert!(parse_ttl(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn the_pushed_credentials_are_only_the_hms_entry() {
        let source = br#"{"hms":{"token":"v1"},"other":{"token":"keep-local"}}"#;
        let filtered = filter_auth(source).expect("filtered");
        let value: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(value, json!({"hms": {"token": "v1"}}));
        assert!(!String::from_utf8(filtered).unwrap().contains("keep-local"));

        // No `hms` key: an error, never an empty object that would look like a success.
        assert!(filter_auth(br#"{"other":{}}"#).is_err());
        assert!(filter_auth(b"not json").is_err());
    }

    #[test]
    fn the_bootstrap_environment_file_is_shell_safe() {
        let mut env = BTreeMap::new();
        env.insert("FX_ROLE".to_string(), "ssh".to_string());
        assert_eq!(env_file_text(&mut env).unwrap(), "FX_ROLE='ssh'\n");

        // A value carrying a quote or a newline would break out of the line it is sourced from.
        let mut hostile = BTreeMap::new();
        hostile.insert("FX_ROLE".to_string(), "ssh';rm -rf ~;'".to_string());
        assert!(env_file_text(&mut hostile).is_err());
        let mut newline = BTreeMap::new();
        newline.insert("FX_ROLE".to_string(), "ssh\nrm -rf ~".to_string());
        assert!(env_file_text(&mut newline).is_err());
    }

    #[test]
    fn the_box_socket_is_under_whatever_home_the_box_reported() {
        assert_eq!(box_socket("/home/daytona"), "/home/daytona/.ferryx-cloud/rpc.sock");
        assert_eq!(box_socket("/home/daytona/"), "/home/daytona/.ferryx-cloud/rpc.sock");
        assert_eq!(box_socket("/root"), "/root/.ferryx-cloud/rpc.sock");
    }

    #[test]
    fn an_empty_catalog_prints_an_empty_json_array() {
        // The acceptance case: `cloud list --json` on a machine that never created one.
        let store = CatalogStore::at(
            std::env::temp_dir().join(format!("fx-cli-empty-{}.json", std::process::id())),
        );
        let _ = std::fs::remove_file(store.path());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let catalog = runtime.block_on(store.load()).expect("a missing catalog is empty");
        assert!(catalog.rows.is_empty());
        let rendered = Value::Array(
            catalog
                .rows
                .iter()
                .map(|row| row_json(row, None))
                .collect(),
        );
        assert_eq!(rendered.to_string(), "[]");
    }

    #[test]
    fn a_row_reports_the_live_state_and_never_the_stale_one() {
        let row = CatalogRow {
            name: "box-1".into(),
            provider: "e2b".into(),
            sandbox_id: "sbx-1".into(),
            transport: Transport::Relay,
            created_at: 1_000,
            last_used_at: 2_000,
            ttl_seconds: 1_800,
            hard_deadline: 9_000,
            state: SandboxState::Ready,
            socket_path: None,
            slot: Some("record-1".into()),
        };
        let live = row_json(&row, Some(SandboxState::Stopped));
        assert_eq!(live["state"], "stopped");
        assert_eq!(live["transport"], "relay");
        // `last_used_at` is unix milliseconds and the deadline is written in the same unit.
        assert_eq!(live["idleDeadline"], 2_000 + 1_800_000);
        // No live reading (provider unreachable): the recorded state stands rather than a guess.
        assert_eq!(row_json(&row, None)["state"], "ready");
    }

    #[test]
    fn a_lifecycle_failure_maps_onto_the_documented_exit_codes() {
        assert_eq!(
            failure_from_lifecycle(&LifecycleError::UnknownProvider("nope".into())),
            EXIT_USAGE
        );
        assert_eq!(
            failure_from_lifecycle(&LifecycleError::NotInCatalog("nope".into())),
            EXIT_USAGE
        );
        assert_eq!(
            failure_from_lifecycle(&LifecycleError::Slot(super::super::slots::SlotError::NoFreeSlot)),
            EXIT_NO_RELAY_SLOT
        );
        assert_eq!(EXIT_NO_RELAY_SLOT, 4);
    }

    #[test]
    fn the_registry_names_every_provider_this_build_ships() {
        let registry = registry_from_env();
        assert_eq!(
            registry.names(),
            vec![
                "blaxel",
                "codespaces",
                "daytona",
                "e2b",
                "freestyle",
                "instabox",
                "opencomputer",
                "prized",
                "sprites",
                "tensorlake",
                "vercel",
            ]
        );
        assert!(registry.get("nope").is_none());
    }
}

#[cfg(test)]
mod housekeeping_tests {
    use super::*;

    // -----------------------------------------------------------------------------------------
    // Model egress (D1)
    // -----------------------------------------------------------------------------------------

    #[test]
    fn every_provider_this_build_ships_has_an_egress_decision() {
        // A provider added to the registry without a row here would silently fall back to
        // `Direct`, which is precisely the D1 failure: a box whose first model call dies.
        let registry = registry_from_env();
        for name in registry.names() {
            assert!(
                MODEL_EGRESS.iter().any(|(provider, _)| *provider == name),
                "{name} has no egress decision in MODEL_EGRESS"
            );
        }
        assert_eq!(MODEL_EGRESS.len(), registry.names().len());
        // Daytona is the one provider the spike proved blocked, and the only bridge candidate.
        assert_eq!(model_egress("daytona"), ModelEgress::Bridge);
        assert_eq!(model_egress("e2b"), ModelEgress::Direct);
        // A row from an older build, or a provider this build dropped, renders as Direct.
        assert_eq!(model_egress("a-provider-this-build-lacks"), ModelEgress::Direct);
    }

    #[test]
    fn every_model_egress_mechanism_has_its_own_name() {
        assert_eq!(ModelEgress::Direct.as_str(), "direct");
        assert_eq!(ModelEgress::Bridge.as_str(), "bridge");
        assert_ne!(ModelEgress::Direct.as_str(), ModelEgress::Bridge.as_str());
    }

    #[test]
    fn the_default_ready_budget_waits_long_enough_to_be_worth_polling() {
        // A budget collapsed to zero would turn every create into an immediate `wait_ready`
        // failure whose rollback deletes a machine that was about to be ready.
        assert!(
            READY_BUDGET >= Duration::from_secs(120),
            "READY_BUDGET is only {}s",
            READY_BUDGET.as_secs()
        );
        assert!(READY_POLL >= Duration::from_millis(100));
        assert!(READY_POLL < READY_BUDGET);
    }

    #[test]
    fn the_bridge_upstream_is_read_from_the_local_model_catalog() {
        let models = br#"{"providers":{"hms":{"baseUrl":"https://endpoint.invalid/v1"}}}"#;
        let (origin, base) = upstream_endpoint(models).expect("endpoint");
        assert_eq!(origin, "https://endpoint.invalid");
        assert_eq!(base, "/v1");
        // A port and a deeper prefix are carried, not dropped.
        let models = br#"{"providers":{"hms":{"baseUrl":"http://127.0.0.1:9000/a/b/"}}}"#;
        let (origin, base) = upstream_endpoint(models).expect("endpoint");
        assert_eq!(origin, "http://127.0.0.1:9000");
        assert_eq!(base, "/a/b");
        assert!(upstream_endpoint(br#"{"providers":{}}"#).is_err());
        assert!(upstream_endpoint(br#"{"providers":{"hms":{"baseUrl":"ftp://x"}}}"#).is_err());
    }

    #[test]
    fn a_bridged_request_keeps_the_base_prefix_exactly_once() {
        assert_eq!(
            bridged_url("https://h.invalid", "/v1", "/v1/chat/completions"),
            "https://h.invalid/v1/chat/completions"
        );
        // A client that drops the prefix still lands on the right path.
        assert_eq!(bridged_url("https://h.invalid", "/v1", "/models"), "https://h.invalid/v1/models");
        assert_eq!(bridged_url("https://h.invalid", "/v1", "/v1"), "https://h.invalid/v1");
        assert_eq!(bridged_url("https://h.invalid", "", "/m"), "https://h.invalid/m");
    }

    #[tokio::test]
    async fn a_bridge_frame_round_trips_and_a_closed_channel_is_a_clean_end() {
        let (writer_side, mut reader_side) = tokio::io::duplex(4096);
        let writer = tokio::sync::Mutex::new(writer_side);
        let payload = json!({"t": "res", "id": 7, "status": 200, "headers": {"a": "b"}});
        write_frame(&writer, &payload).await.expect("written");
        let read = read_frame(&mut reader_side).await.expect("read").expect("a frame");
        assert_eq!(read, payload);
        drop(writer);
        assert_eq!(read_frame(&mut reader_side).await.expect("clean end"), None);
    }

    // -----------------------------------------------------------------------------------------
    // What reaches the box (D3)
    // -----------------------------------------------------------------------------------------

    #[test]
    fn a_bridged_box_points_its_model_catalog_at_the_box_local_bridge() {
        let source = br#"{
            "disabledProviders": ["other"],
            "providers": {
                "hms": {"apiKey": "hms-key", "baseUrl": "https://endpoint.invalid/v1", "models": ["m"]},
                "other": {"apiKey": "other-key", "baseUrl": "https://other.invalid/v1"}
            }
        }"#;
        let filtered = filter_models(source, ModelEgress::Bridge).expect("filtered");
        let text = String::from_utf8(filtered.clone()).expect("utf-8");
        let value: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(
            value["providers"]["hms"]["baseUrl"],
            json!(format!("http://127.0.0.1:{MODEL_BRIDGE_PORT}/v1"))
        );
        // The one key the box needs is kept; nothing else from the file is.
        assert_eq!(value["providers"]["hms"]["apiKey"], json!("hms-key"));
        assert_eq!(value["providers"]["hms"]["models"], json!(["m"]));
        assert!(value.get("disabledProviders").is_none());
        assert!(value["providers"].get("other").is_none());
        assert!(!text.contains("other-key"), "another provider's key reached the pushed bytes");
        assert!(!text.contains("other.invalid"), "another provider's endpoint reached the box");
        assert!(
            !text.contains("endpoint.invalid"),
            "the bridged catalog still names the endpoint the box cannot reach"
        );
    }

    #[test]
    fn a_direct_box_keeps_its_endpoint_and_drops_every_other_provider() {
        let source = br#"{"providers":{"hms":{"apiKey":"hms-key","baseUrl":"https://endpoint.invalid/v1"},"other":{"apiKey":"other-key"}}}"#;
        let filtered = filter_models(source, ModelEgress::Direct).expect("filtered");
        let value: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(value["providers"]["hms"]["baseUrl"], json!("https://endpoint.invalid/v1"));
        assert_eq!(value.as_object().expect("object").len(), 1, "only `providers` survives");
        assert!(value["providers"].get("other").is_none());
        // No `providers.hms` is an error, never an empty catalog that looks like a success.
        assert!(filter_models(br#"{"providers":{"other":{}}}"#, ModelEgress::Direct).is_err());
        assert!(filter_models(b"not json", ModelEgress::Direct).is_err());
    }

    #[test]
    fn a_settings_file_without_a_credential_is_pushed_unchanged() {
        let source = br#"{"defaultProvider":"mahoquot","lookAt":{"models":["a"]},"permissions":{"allow":["read"]}}"#;
        let filtered = filter_settings(source).expect("filtered");
        let before: Value = serde_json::from_slice(source).unwrap();
        let after: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(before, after, "a settings file with no key field must not be rewritten");
    }

    #[test]
    fn a_key_like_settings_field_is_removed_at_every_depth() {
        let source = br#"{
            "apiKey": "sk-top",
            "nested": {"token": "tok-inner", "keep": 1, "deeper": [{"secret": "sec-inner"}, {"password": "pw-inner"}]},
            "list": [{"api_key": "key-inner", "name": "n"}]
        }"#;
        let filtered = filter_settings(source).expect("filtered");
        let text = String::from_utf8(filtered.clone()).unwrap();
        for secret in ["sk-top", "tok-inner", "sec-inner", "pw-inner", "key-inner"] {
            assert!(!text.contains(secret), "a credential survived the filter: {secret}");
        }
        let value: Value = serde_json::from_slice(&filtered).unwrap();
        assert_eq!(value["nested"]["keep"], json!(1));
        assert_eq!(value["list"][0]["name"], json!("n"));
    }

    // -----------------------------------------------------------------------------------------
    // Rollback (D2)
    // -----------------------------------------------------------------------------------------

    /// A scheduler that is always loaded and does nothing: the reaper is not this file's subject.
    struct NoopScheduler;

    #[async_trait::async_trait]
    impl ReaperScheduler for NoopScheduler {
        fn platform(&self) -> &'static str {
            "noop"
        }

        async fn is_loaded(&self) -> Result<bool, crate::cloud_host::reaper::ReaperError> {
            Ok(true)
        }

        async fn install(&self) -> Result<(), crate::cloud_host::reaper::ReaperError> {
            Ok(())
        }

        async fn remove(&self) -> Result<(), crate::cloud_host::reaper::ReaperError> {
            Ok(())
        }
    }

    /// A temp catalog and a temp slot pool with one seeded record, plus the scheduler.
    struct Fixture {
        /// Held so the temp directory outlives the stores that point into it.
        _dir: tempfile::TempDir,
        catalog: CatalogStore,
        slots: SlotRegistry,
        scheduler: NoopScheduler,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let catalog = CatalogStore::at(dir.path().join("catalog.json"));
            let slots = SlotRegistry::at(crate::cloud_host::slots::location_in(dir.path()));
            let record = slots.root().join("rec-1");
            std::fs::create_dir_all(&record).expect("slot dir");
            std::fs::write(
                record.join(crate::cloud_host::slots::IDENTITY_FILE),
                r#"{"machineId":"m-1","displayName":null,"publicKey":"cHVi","privateKey":"cHJpdg=="}"#,
            )
            .expect("identity");
            Self { _dir: dir, catalog, slots, scheduler: NoopScheduler }
        }

        fn context<'a>(&'a self, registry: &'a ProviderRegistry) -> CreateContext<'a> {
            CreateContext {
                registry,
                catalog: &self.catalog,
                slots: &self.slots,
                scheduler: &self.scheduler,
            }
        }
    }

    /// The steps double: one knob to fail a step, and a record of which steps ran.
    #[derive(Default)]
    struct FaultySteps {
        fail_at: Option<CreateStep>,
        reached: parking_lot::Mutex<Vec<CreateStep>>,
    }

    impl FaultySteps {
        fn failing_at(step: CreateStep) -> Self {
            Self { fail_at: Some(step), reached: parking_lot::Mutex::new(Vec::new()) }
        }

        fn reached(&self) -> Vec<CreateStep> {
            self.reached.lock().clone()
        }

        fn step(&self, step: CreateStep) -> Result<(), StepFailure> {
            self.reached.lock().push(step);
            if self.fail_at == Some(step) {
                return Err(StepFailure::new(format!("{}: injected", step.as_str())));
            }
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ProvisionSteps for FaultySteps {
        async fn wait_ready(&self) -> Result<(), StepFailure> {
            self.step(CreateStep::WaitReady)
        }

        async fn connect(&self) -> Result<(), StepFailure> {
            self.step(CreateStep::Connect)
        }

        async fn home(&self) -> Result<String, StepFailure> {
            self.step(CreateStep::Home)?;
            Ok("/home/tester".to_string())
        }

        async fn bootstrap(&self, _role: Role, _home: &str) -> Result<(), StepFailure> {
            self.step(CreateStep::Bootstrap)
        }

        async fn push_agent_files(&self, _home: &str) -> Result<(), StepFailure> {
            self.step(CreateStep::PushAgentFiles)
        }

        async fn model_egress(&self, _home: &str) -> Result<ModelEgress, StepFailure> {
            self.step(CreateStep::ModelEgress)?;
            Ok(ModelEgress::Direct)
        }

        async fn enroll(&self) -> Result<Option<String>, StepFailure> {
            self.step(CreateStep::Enroll)?;
            Ok(None)
        }

        async fn ensure_host(&self) -> Result<(), StepFailure> {
            self.step(CreateStep::EnsureHost)
        }
    }

    /// Every step a relay create runs, in order, so a step added to `run_steps` without a case
    /// here shows up as a mismatch rather than as an untested path.
    const RELAY_STEPS: &[CreateStep] = &[
        CreateStep::WaitReady,
        CreateStep::Connect,
        CreateStep::Home,
        CreateStep::PushAgentFiles,
        CreateStep::Bootstrap,
        CreateStep::ModelEgress,
        CreateStep::Enroll,
    ];

    /// Create a machine the way `create` does, then fail one step: the machine must be deleted and
    /// its relay slot released. Every step, because D2 is exactly the claim that no step escapes.
    #[tokio::test]
    async fn a_failure_at_any_relay_step_deletes_the_machine_and_releases_the_slot() {
        for (index, step) in RELAY_STEPS.iter().enumerate() {
            let fixture = Fixture::new();
            let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
            let registry = {
                let mut registry = ProviderRegistry::new();
                registry.insert("fake", provider.clone());
                registry
            };
            let context = fixture.context(&registry);
            let created = lifecycle::create_with_scheduler(
                provider.as_ref(),
                "fake",
                CreateSandbox {
                    idempotency_key: format!("fx-box-1-{index}"),
                    name: "box-1".to_string(),
                    ttl_seconds: None,
                    env: BTreeMap::new(),
                },
                &fixture.catalog,
                &fixture.slots,
                &fixture.scheduler,
            )
            .await
            .expect("created");
            assert!(created.slot.is_some(), "a relay create leases a slot");
            let row = created.row.clone();

            let steps = FaultySteps::failing_at(*step);
            let failure = finish_create(
                context,
                provider.as_ref(),
                &row,
                Role::Relay,
                Transport::Relay,
                &steps,
            )
            .await
            .expect_err("the injected step failed the create");
            assert!(
                failure.detail.starts_with(step.as_str()),
                "the message names the step that broke: {}",
                failure.detail
            );

            // 1. Nothing bills: the machine is gone at the provider.
            assert!(
                provider.live_ids().is_empty(),
                "a failure at {} left {} machine(s) billing",
                step.as_str(),
                provider.live_ids().len()
            );
            // 2. The relay record is back in the pool: `provision_relay` never returns it.
            assert_eq!(
                fixture.slots.lease_of("box-1").await.expect("pool readable"),
                None,
                "a failure at {} kept the relay slot leased",
                step.as_str()
            );
            // 3. Nothing after the failing step ran.
            assert_eq!(steps.reached(), RELAY_STEPS[..=index].to_vec());
            // 4. The catalog records a tombstone, not a row that claims a live machine.
            let stored = fixture.catalog.load().await.expect("catalog readable");
            assert_eq!(stored.row("box-1").map(|row| row.state), Some(SandboxState::Deleted));
        }
    }

    /// The SSH path's last step (`ensure_host`) is inside the same guard, and an SSH machine has no
    /// slot: the rollback still has to delete it.
    #[tokio::test]
    async fn a_failure_at_ensure_host_deletes_an_ssh_machine() {
        let fixture = Fixture::new();
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::ssh());
        let registry = {
            let mut registry = ProviderRegistry::new();
            registry.insert("fake", provider.clone());
            registry
        };
        let context = fixture.context(&registry);
        let created = lifecycle::create_with_scheduler(
            provider.as_ref(),
            "fake",
            CreateSandbox {
                idempotency_key: "fx-box-1-ssh".to_string(),
                name: "box-1".to_string(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            },
            &fixture.catalog,
            &fixture.slots,
            &fixture.scheduler,
        )
        .await
        .expect("created");
        assert!(created.slot.is_none(), "an SSH machine takes no relay slot");
        let row = created.row.clone();

        let steps = FaultySteps::failing_at(CreateStep::EnsureHost);
        let failure =
            finish_create(context, provider.as_ref(), &row, Role::Ssh, Transport::Ssh, &steps)
                .await
                .expect_err("ensure_host failed");
        assert!(failure.detail.starts_with("ensure_host"), "{}", failure.detail);
        assert!(provider.live_ids().is_empty(), "the SSH machine is still billing");
        assert_eq!(steps.reached().last().copied(), Some(CreateStep::EnsureHost));
        assert!(!steps.reached().contains(&CreateStep::Enroll), "an SSH create enrolls nothing");
    }

    #[tokio::test]
    async fn a_rollback_twice_is_a_no_op_not_a_second_delete() {
        // `discard` runs once per failed create, but the delete it calls must stay idempotent: a
        // reaper, an operator retry and this guard can all reach the same tombstone.
        let fixture = Fixture::new();
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let registry = {
            let mut registry = ProviderRegistry::new();
            registry.insert("fake", provider.clone());
            registry
        };
        let context = fixture.context(&registry);
        let created = lifecycle::create_with_scheduler(
            provider.as_ref(),
            "fake",
            CreateSandbox {
                idempotency_key: "fx-box-1-twice".to_string(),
                name: "box-1".to_string(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            },
            &fixture.catalog,
            &fixture.slots,
            &fixture.scheduler,
        )
        .await
        .expect("created");
        let row = created.row.clone();
        let steps = FaultySteps::failing_at(CreateStep::Home);
        finish_create(context, provider.as_ref(), &row, Role::Relay, Transport::Relay, &steps)
            .await
            .expect_err("home failed");

        let deletes_before = provider
            .call_methods()
            .iter()
            .filter(|method| **method == "delete")
            .count();
        discard(&context, provider.as_ref(), "box-1").await;
        let deletes_after = provider
            .call_methods()
            .iter()
            .filter(|method| **method == "delete")
            .count();
        assert_eq!(deletes_after, deletes_before, "a second rollback deleted again");
        assert!(provider.live_ids().is_empty());
        let stored = fixture.catalog.load().await.expect("catalog readable");
        assert_eq!(stored.row("box-1").map(|row| row.state), Some(SandboxState::Deleted));
        assert_eq!(fixture.slots.lease_of("box-1").await.expect("pool"), None);
    }

    /// The seam itself: a future that fails is rolled back, one that succeeds is not.
    #[tokio::test]
    async fn the_guard_rolls_back_only_on_failure() {
        let fixture = Fixture::new();
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let registry = {
            let mut registry = ProviderRegistry::new();
            registry.insert("fake", provider.clone());
            registry
        };
        let context = fixture.context(&registry);
        let created = lifecycle::create_with_scheduler(
            provider.as_ref(),
            "fake",
            CreateSandbox {
                idempotency_key: "fx-box-1-guard".to_string(),
                name: "box-1".to_string(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            },
            &fixture.catalog,
            &fixture.slots,
            &fixture.scheduler,
        )
        .await
        .expect("created");
        let row = created.row.clone();
        let rollback = MachineRollback { context, provider: provider.as_ref() };

        let kept: Result<u8, StepFailure> = Ok(7);
        assert_eq!(rollback_on_failure(&rollback, "box-1", async { kept }).await, Ok(7));
        assert_eq!(provider.live_ids().len(), 1, "a successful create was rolled back");
        assert!(fixture.slots.lease_of("box-1").await.expect("pool").is_some());

        let failed: Result<u8, StepFailure> = Err(StepFailure::with_code(EXIT_FAILURE, "boom"));
        let outcome = rollback_on_failure(&rollback, "box-1", async { failed }).await;
        assert_eq!(outcome, Err(StepFailure::with_code(EXIT_FAILURE, "boom")));
        assert!(provider.live_ids().is_empty());
        assert_eq!(fixture.slots.lease_of("box-1").await.expect("pool"), None);
        let _ = row;
    }

    #[tokio::test]
    async fn an_unknown_provider_is_refused_before_any_provider_call() {
        let fixture = Fixture::new();
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let registry = {
            let mut registry = ProviderRegistry::new();
            registry.insert("fake", provider.clone());
            registry
        };
        let context = fixture.context(&registry);

        assert_eq!(create_with(&context, "nope", "box-1", None, false).await, EXIT_USAGE);
        // The pre-create sweep lists every provider, so a check moved after it shows up here.
        assert!(
            provider.calls().is_empty(),
            "an unknown provider reached a provider call: {:?}",
            provider.call_methods()
        );
        assert_eq!(fixture.slots.lease_of("box-1").await.expect("pool"), None);
        assert!(fixture.catalog.load().await.expect("catalog").rows.is_empty());
    }

    // -----------------------------------------------------------------------------------------
    // list must not wake anything
    // -----------------------------------------------------------------------------------------

    #[tokio::test]
    async fn list_reads_status_and_never_wakes_or_creates() {
        let fixture = Fixture::new();
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let sandbox_id = provider
            .create(CreateSandbox {
                idempotency_key: "fx-box-1-1".into(),
                name: "box-1".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("created");
        let _ = provider.get(&sandbox_id).await.expect("ready");
        provider.stop(&sandbox_id).await.expect("stopped");
        let _ = provider.get(&sandbox_id).await.expect("stopped");

        let mut catalog = Catalog::default();
        catalog.upsert(CatalogRow {
            name: "box-1".into(),
            provider: "fake".into(),
            sandbox_id: sandbox_id.clone(),
            transport: Transport::Relay,
            created_at: now_ms(),
            last_used_at: now_ms(),
            ttl_seconds: 1_800,
            hard_deadline: now_ms() + 6 * 60 * 60 * 1_000,
            state: SandboxState::Stopped,
            socket_path: None,
            slot: None,
        });
        fixture.catalog.save(&catalog).await.expect("saved");

        let registry = {
            let mut registry = ProviderRegistry::new();
            registry.insert("fake", provider.clone());
            registry
        };
        let before = provider.call_methods().len();
        assert_eq!(list_with(&registry, &fixture.catalog, true).await, EXIT_OK);
        // Only the calls `list` itself made: the setup above created and stopped the machine.
        let during: Vec<&'static str> = provider.call_methods()[before..].to_vec();
        assert!(!during.contains(&"resume"), "list woke a machine: {during:?}");
        assert!(!during.contains(&"create"), "list created something: {during:?}");
        assert!(!during.contains(&"exec"), "list ran a command on the box: {during:?}");
        // The status it printed came from the provider, not from the stored row.
        assert!(during.contains(&"get"), "list trusted the stored state: {during:?}");
    }

    // -----------------------------------------------------------------------------------------
    // Dispatch preconditions
    // -----------------------------------------------------------------------------------------

    /// An `Access` over a real provider, so the exec and write paths run for real.
    fn relay_access(provider: std::sync::Arc<dyn SandboxProvider>, sandbox_id: &str) -> Access {
        Access {
            inner: AccessKind::Relay { provider, sandbox_id: sandbox_id.to_string() },
        }
    }

    async fn ready_machine(provider: &crate::cloud_host::provider::fake::FakeProvider) -> String {
        let id = provider
            .create(CreateSandbox {
                idempotency_key: format!("fx-{}-1", unix_seconds()),
                name: "box-1".into(),
                ttl_seconds: None,
                env: BTreeMap::new(),
            })
            .await
            .expect("created");
        let _ = provider.get(&id).await.expect("ready");
        id
    }

    #[tokio::test]
    async fn a_dispatch_makes_sure_the_host_daemon_answers_first() {
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&provider).await;
        let access = relay_access(provider.clone(), &id);
        dispatch_ready(&access).await.expect("the daemon answered");
        let execs = provider.execs_of(&id);
        assert_eq!(execs.len(), 1, "a dispatch runs exactly one ensure: {execs:?}");
        assert!(
            execs[0].contains("host ensure --socket"),
            "the dispatch precondition is not `host ensure`: {}",
            execs[0]
        );
        // The socket path is resolved on the box, never built from this Mac's home.
        assert!(execs[0].contains("$HOME/.ferryx-cloud/rpc.sock"), "{}", execs[0]);
    }

    #[tokio::test]
    async fn a_dispatch_whose_host_daemon_fails_reports_the_failure() {
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&provider).await;
        provider.fail_next(crate::cloud_host::provider::ErrorKind::Retryable);
        let access = relay_access(provider.clone(), &id);
        assert!(dispatch_ready(&access).await.is_err(), "a failed ensure must not look ready");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_socket_that_answers_is_ready_and_a_silent_one_is_reported() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let dir = tempfile::tempdir().expect("temp dir");
        // (a) A listener that answers one RPC line is ready.
        let answering = dir.path().join("answering.sock");
        let listener = tokio::net::UnixListener::bind(&answering).expect("bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = [0u8; 128];
            let _ = stream.read(&mut buffer).await;
            let _ = stream.write_all(b"{\"type\":\"protocol_info\"}\n").await;
        });
        wait_for_socket(&answering, Duration::from_secs(5))
            .await
            .expect("an answering socket is ready");
        // Bounded: if the readiness check stops connecting, this test must fail rather than hang
        // on a listener that never accepted.
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("the readiness check never reached the listener")
            .expect("server task");

        // (b) A bound socket nobody answers is not ready, and the wait ends on its budget.
        let silent = dir.path().join("silent.sock");
        let listener = tokio::net::UnixListener::bind(&silent).expect("bind");
        let hold = tokio::spawn(async move {
            let _held = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let outcome = wait_for_socket(&silent, Duration::from_millis(200)).await;
        let error = outcome.expect_err("a socket that never answers must not report ready");
        assert!(error.contains("did not answer"), "the message does not say what timed out: {error}");
        assert!(error.contains("silent.sock"), "the message does not name the socket: {error}");
        hold.abort();
    }

    #[tokio::test]
    async fn the_box_bridge_is_proved_from_inside_the_box() {
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&provider).await;
        let access = relay_access(provider.clone(), &id);
        prove_model_bridge(&access, "/home/tester").await.expect("the probe ran");
        let execs = provider.execs_of(&id);
        assert_eq!(execs.len(), 1, "{execs:?}");
        assert!(execs[0].contains("--probe"), "the probe is not the probe: {}", execs[0]);
        assert!(
            execs[0].contains(&MODEL_BRIDGE_PORT.to_string()),
            "the probe does not name the bridge port: {}",
            execs[0]
        );
        assert!(execs[0].contains(BOX_MODEL_BRIDGE), "{}", execs[0]);
    }

    #[tokio::test]
    async fn a_bridge_cannot_be_asked_of_a_relay_only_machine() {
        let provider = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&provider).await;
        let access = relay_access(provider.clone(), &id);
        let error = establish_model_egress(&access, "/home/tester", "daytona")
            .await
            .expect_err("a relay machine cannot be bridged");
        assert!(error.contains("SSH-only"), "{error}");
        // The refusal is before any work: nothing ran and nothing was pushed.
        assert!(provider.execs_of(&id).is_empty());
        // A Direct provider needs no bridge at all.
        assert_eq!(
            establish_model_egress(&access, "/home/tester", "e2b").await.expect("direct"),
            ModelEgress::Direct
        );
    }

    // -----------------------------------------------------------------------------------------
    // Nothing secret on a command line
    // -----------------------------------------------------------------------------------------

    /// A provider that records what `write_file` was handed. `FakeProvider` deliberately leaves
    /// that at the trait default, so a test that asserts on pushed bytes needs its own double.
    struct RecordingProvider {
        inner: std::sync::Arc<crate::cloud_host::provider::fake::FakeProvider>,
        writes: parking_lot::Mutex<Vec<(String, u32, Vec<u8>)>>,
    }

    impl RecordingProvider {
        fn new(inner: std::sync::Arc<crate::cloud_host::provider::fake::FakeProvider>) -> Self {
            Self { inner, writes: parking_lot::Mutex::new(Vec::new()) }
        }

        fn writes(&self) -> Vec<(String, u32, Vec<u8>)> {
            self.writes.lock().clone()
        }
    }

    #[async_trait::async_trait]
    impl SandboxProvider for RecordingProvider {
        fn capabilities(&self) -> crate::cloud_host::provider::Capabilities {
            self.inner.capabilities()
        }

        async fn create(
            &self,
            request: CreateSandbox,
        ) -> Result<String, crate::cloud_host::provider::SandboxError> {
            self.inner.create(request).await
        }

        async fn get(
            &self,
            sandbox_id: &str,
        ) -> Result<crate::cloud_host::provider::SandboxInfo, crate::cloud_host::provider::SandboxError> {
            self.inner.get(sandbox_id).await
        }

        async fn stop(&self, sandbox_id: &str) -> Result<(), crate::cloud_host::provider::SandboxError> {
            self.inner.stop(sandbox_id).await
        }

        async fn resume(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), crate::cloud_host::provider::SandboxError> {
            self.inner.resume(sandbox_id, ttl_seconds).await
        }

        async fn delete(&self, sandbox_id: &str) -> Result<(), crate::cloud_host::provider::SandboxError> {
            self.inner.delete(sandbox_id).await
        }

        async fn extend_ttl(
            &self,
            sandbox_id: &str,
            ttl_seconds: Option<u64>,
        ) -> Result<(), crate::cloud_host::provider::SandboxError> {
            self.inner.extend_ttl(sandbox_id, ttl_seconds).await
        }

        async fn exec(
            &self,
            sandbox_id: &str,
            command: &str,
            timeout_seconds: u64,
        ) -> Result<ExecResult, crate::cloud_host::provider::SandboxError> {
            self.inner.exec(sandbox_id, command, timeout_seconds).await
        }

        async fn write_file(
            &self,
            sandbox_id: &str,
            path: &str,
            bytes: Vec<u8>,
            mode: u32,
        ) -> Result<(), crate::cloud_host::provider::SandboxError> {
            let _ = sandbox_id;
            self.writes.lock().push((path.to_string(), mode, bytes));
            Ok(())
        }

        async fn list(&self) -> Result<Vec<crate::cloud_host::provider::ListedSandbox>, crate::cloud_host::provider::SandboxError> {
            self.inner.list().await
        }
    }

    fn agent_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".omo/agent")).expect("agent dir");
        std::fs::write(
            dir.path().join(AUTH_FILE),
            r#"{"hms":{"token":"hms-token"},"other":{"token":"other-token"}}"#,
        )
        .expect("auth");
        std::fs::write(
            dir.path().join(MODELS_FILE),
            r#"{"providers":{"hms":{"apiKey":"hms-key","baseUrl":"https://endpoint.invalid/v1"},"other":{"apiKey":"sk-other-1234","baseUrl":"https://other.invalid/v1"}}}"#,
        )
        .expect("models");
        std::fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"defaultProvider":"mahoquot","nested":{"apiKey":"sk-settings-5678"},"permissions":{"allow":["read"]}}"#,
        )
        .expect("settings");
        dir
    }

    #[tokio::test]
    async fn the_pushed_files_are_filtered_owner_only_and_never_on_an_argv() {
        let fake = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&fake).await;
        let provider = std::sync::Arc::new(RecordingProvider::new(fake.clone()));
        let access = relay_access(provider.clone(), &id);
        let local = agent_dir();

        push_agent_files_from(&access, "/home/tester", local.path(), ModelEgress::Direct)
            .await
            .expect("pushed");

        let writes = provider.writes();
        assert_eq!(writes.len(), 3, "auth, models and settings: {writes:?}");
        for (path, mode, _) in &writes {
            assert!(path.starts_with("/home/tester/"), "{path}");
            // The literal 0600, not the constant: a test that compares against the constant
            // passes whatever the constant becomes, which is exactly the D3 defect.
            assert_eq!(
                format!("{mode:o}"),
                "600",
                "{path} was pushed at {mode:o}, not 0600"
            );
            assert_eq!(*mode, 0o600, "{path} was pushed at {mode:o}, not 0600");
        }
        let joined: String = writes
            .iter()
            .map(|(_, _, bytes)| String::from_utf8_lossy(bytes).into_owned())
            .collect();
        for secret in ["other-token", "sk-other-1234", "other.invalid", "sk-settings-5678"] {
            assert!(!joined.contains(secret), "{secret} reached the box");
        }
        // What the box needs is still there.
        assert!(joined.contains("hms-token"), "the hms credentials were dropped");
        assert!(joined.contains("hms-key"));
        assert!(joined.contains("mahoquot"), "the settings the engine needs were dropped");
        // Nothing about a file went through `exec`, so no byte of one can be on a command line.
        assert!(
            fake.execs_of(&id).is_empty(),
            "a push ran a command: {:?}",
            fake.execs_of(&id)
        );
    }

    #[tokio::test]
    async fn a_bridged_box_is_pushed_a_repointed_catalog() {
        let fake = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&fake).await;
        let provider = std::sync::Arc::new(RecordingProvider::new(fake.clone()));
        let access = relay_access(provider.clone(), &id);
        let local = agent_dir();

        push_agent_files_from(&access, "/home/tester", local.path(), ModelEgress::Bridge)
            .await
            .expect("pushed");
        let models = provider
            .writes()
            .into_iter()
            .find(|(path, _, _)| path.ends_with(MODELS_FILE))
            .expect("models.json was pushed");
        let value: Value = serde_json::from_slice(&models.2).unwrap();
        assert_eq!(
            value["providers"]["hms"]["baseUrl"],
            json!(format!("http://127.0.0.1:{MODEL_BRIDGE_PORT}/v1"))
        );
    }

    #[tokio::test]
    async fn a_missing_agent_file_is_not_invented_on_the_box() {
        let fake = std::sync::Arc::new(crate::cloud_host::provider::fake::FakeProvider::new());
        let id = ready_machine(&fake).await;
        let provider = std::sync::Arc::new(RecordingProvider::new(fake.clone()));
        let access = relay_access(provider.clone(), &id);
        let local = tempfile::tempdir().expect("temp dir");

        push_agent_files_from(&access, "/home/tester", local.path(), ModelEgress::Direct)
            .await
            .expect("nothing to push is not a failure");
        assert!(provider.writes().is_empty(), "{:?}", provider.writes());
    }

    #[test]
    fn the_box_side_bridge_is_the_shipped_script() {
        // The resource is embedded, so a box never fetches it; a build that lost the file would
        // otherwise fail only on a fresh machine.
        assert!(MODEL_BRIDGE_SCRIPT.contains("127.0.0.1"), "the bridge does not bind to loopback");
        assert!(MODEL_BRIDGE_SCRIPT.contains("--probe"), "the bridge has no readiness probe");
        assert!(MODEL_BRIDGE_SCRIPT.contains("!I"), "the bridge has no length framing");
        assert_eq!(BOX_MODEL_BRIDGE, ".ferryx-cloud/model-bridge.py");
        assert_eq!(BRIDGE_FILE_MODE, 0o700, "a listener must not be pushed 0755");
        assert_ne!(BRIDGE_FILE_MODE, SECRET_FILE_MODE);
    }

    #[test]
    fn the_pushed_modes_are_the_literals_that_were_reviewed() {
        // Stated as numbers so a drive-by edit to a constant cannot move a credential's mode
        // without a failing test in the same commit.
        assert_eq!(SECRET_FILE_MODE, 0o600);
        assert_eq!(BRIDGE_FILE_MODE, 0o700);
    }

    #[test]
    fn a_path_with_a_space_is_quoted_before_it_reaches_a_command() {
        // The bridge's script path is built from the box's own `$HOME`, so the quoting is what
        // keeps a hostile home directory out of the command.
        assert_eq!(quote_posix("/home/a b/model-bridge.py"), "'/home/a b/model-bridge.py'");
        assert_eq!(quote_posix("/home/it's/x"), "'/home/it'\\''s/x'");
    }
}
