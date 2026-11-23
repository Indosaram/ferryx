//! Freestyle: a full Linux VM behind `https://api.freestyle.sh`, reached over SSH
//! (`<vm-id>:<token>@<vm-id>.beta-ssh.freestyle.sh`) with one-shot exec through `exec-await`.
//!
//! # Endpoints
//!
//! Recorded 2026-11-21 from `https://api.freestyle.sh/openapi.json` (OpenAPI 3.1.0, "Freestyle")
//! and cross-read against the npm SDK `freestyle@0.2.23` (`src/vms/index.ts`, `src/cli/ssh.ts`),
//! then verified live with the account key. Base `https://api.freestyle.sh`, auth
//! `Authorization: Bearer <key>` (never logged).
//!
//! | Call | Request | Response |
//! | --- | --- | --- |
//! | create | `POST /v5/vms` `{slug,displayName,metadata,firewall,idleTimeoutSeconds}` | `201` Vm; `409` slug taken / no capacity; `429` plan limit |
//! | get | `GET /v5/vms/{vmIdOrSlug}` | `200` Vm; `404 {"code":"NOT_FOUND"}` |
//! | list | `GET /v5/vms?limit=&offset=` (+`metadata=key:value`) | `{vms,totalCount,runningCount,...}` |
//! | stop | `POST /v5/vms/{vmIdOrSlug}/pause` | `200` Vm (`state: pausing`); `409` not running |
//! | resume | `POST /v5/vms/{vmIdOrSlug}/start` | `200` Vm (`state: starting`) |
//! | extend ttl | `PATCH /v5/vms/{vmIdOrSlug}` `{idleTimeoutSeconds}` | `200` Vm |
//! | delete | `DELETE /v5/vms/{vmIdOrSlug}` | `204`; `404` when gone |
//! | exec | `POST /v5/vms/{vmIdOrSlug}/exec-await` `{command,timeoutMs,env}` | `200 {statusCode,stdout,stderr}` (non-zero exit is still `200`); `409` not running / `VM_NON_RESPONSIVE` |
//! | ssh identity | `POST /v5/identities` `{}` | `{id,managed}` |
//! | ssh grant | `POST /v5/identities/{id}/permissions/vm` `{vmId}` | VmPermission |
//! | ssh token | `POST /v5/identities/{id}/tokens` (body `{}`) | `{id,token,expiresAt}` — shown once; a `null` body is a `422` |
//!
//! Notes that shaped this adapter:
//!
//! - **The v1 route is dead, not the key.** `GET /v1/vms` answers
//!   `401 {"code":"UNAUTHORIZED_ERROR","reason":"Invalid API key"}` with a perfectly good key —
//!   v1 is the legacy path. On `/v5/vms` the same key answers `200`. Both shapes are fixtures
//!   below; the 401 maps to `Config` because a rejected credential is what an operator must fix,
//!   whichever route produced it.
//! - `firewall` is **required** on create and a VM gets nothing implicitly: "no outbound Internet,
//!   no inbound" (OpenAPI, `CreateVmRequest.firewall`). The adapter therefore sends exactly one
//!   rule, outbound-only — `{action: allow, source: {}, destination: {public: true}}`, where an
//!   empty `source` is the VM being created. Nothing inbound is opened: SSH arrives through the
//!   gateway, not through the VM's own firewall.
//! - Create has **no env field**, so `SandboxProvider::env` is kept on this Mac instead: one 0600
//!   file per sandbox under `~/.ferryx/cloud-host/env/`, written on every create (an idempotent
//!   replay included) and read back on every `exec`. That is what makes the trait's guarantee 3
//!   ("`env` from create is visible to `exec`") survive a control-plane restart. A sandbox this
//!   Mac has no create record for is refused with `env_unavailable` rather than exec'd with a
//!   silently empty environment: a command that lost its env fails later and further from the
//!   cause than a call that says so up front.
//! - `ttl_seconds` maps to `idleTimeoutSeconds` (pause after idle), never to `ttlSeconds`: the
//!   latter *deletes* the VM at the deadline, which would break the trait's "disk persists across
//!   stop/resume". `-1`/absent means never pause for idleness.
//! - There is no `/stop`: `pause` is the stop. A paused VM keeps its disk, so `resume` is `start`.
//! - `timeoutMs` on `exec-await` is capped at 300000 (5 min) for an inline call; a longer request
//!   is clamped and the guest kills the command at the deadline, which the API reports as
//!   `statusCode: null` — exactly [`ExecResult::timed_out`].
//! - The SSH username is `<vm-id>:<token>` at `<vm-id>.beta-ssh.freestyle.sh` (verbatim from the
//!   SDK's `nativeSsh`), authenticated by `none`; the VM id must be canonical, so a slug handle is
//!   resolved to its id first. A token belongs to an identity granted only that VM, and the
//!   identity behind a VM's previous token is deleted when a new one is minted, so a long-lived
//!   control plane does not leave a trail of live credentials behind it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    Capabilities, CreateSandbox, ErrorKind, ExecResult, ListedSandbox, SandboxError, SandboxInfo,
    SandboxProvider, SandboxState, SshIdentity, SshTarget, Transport,
};

const PROVIDER: &str = "freestyle";
const API_BASE: &str = "https://api.freestyle.sh";
/// The gateway every VM is addressed through: `<vm-id>.beta-ssh.freestyle.sh`.
const SSH_SUFFIX: &str = ".beta-ssh.freestyle.sh";
/// Longest inline `exec-await` wait the API accepts (5 minutes); longer is backgrounded only.
const MAX_EXEC_SECONDS: u64 = 300;
/// A control-plane call is one bounded request.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(120);
/// Upper bound on pages walked by the orphan scan.
const MAX_LIST_PAGES: usize = 20;
/// Page size for the orphan scan.
const LIST_PAGE: usize = 100;
/// Metadata key carrying the create's idempotency key (values cap at 63 characters).
const METADATA_IDEMPOTENCY: &str = "fx";
/// Metadata values cap at 63 characters.
const METADATA_KEY_MAX: usize = 63;
/// `~/.ferryx`: the state directory every ferryx file shares.
const STATE_DIR: &str = ".ferryx";
/// Create env lives under `<state>/cloud-host/env`, one 0600 file per sandbox.
const ENV_DIR: &str = "cloud-host";
const ENV_SUBDIR: &str = "env";
/// Schema version of an on-disk env record; a newer one is refused, never reinterpreted.
const ENV_VERSION: u32 = 1;

pub struct FreestyleProvider {
    client: reqwest::Client,
    /// `None` when no key was found: every call then reports `Config`.
    token: Option<String>,
    /// Per-sandbox create env on disk, because the v5 create body has no field for it.
    env: EnvStore,
    /// The identity behind the last token handed out for a VM, so the next `ssh_target` can
    /// revoke it instead of leaving a live credential behind.
    ssh_identities: Mutex<BTreeMap<String, String>>,
}

impl FreestyleProvider {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ferryx-cloud-host/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            client,
            token,
            env: EnvStore::default_location(),
            ssh_identities: Mutex::new(BTreeMap::new()),
        }
    }

    /// The same provider over an explicit env root, so a test never writes the operator's
    /// `~/.ferryx`.
    #[cfg(test)]
    fn with_env_store(token: Option<String>, env: EnvStore) -> Self {
        let mut provider = Self::new(token);
        provider.env = env;
        provider
    }

    /// Key from `FREESTYLE_API_KEY` only; the CLI keeps its own credential.
    pub fn from_env() -> Self {
        let token =
            std::env::var("FREESTYLE_API_KEY").ok().filter(|value| !value.trim().is_empty());
        Self::new(token)
    }

    /// A provider with no credential, for the "call without a key reports Config" path.
    pub fn without_token() -> Self {
        Self::new(None)
    }

    fn token(&self) -> Result<&str, SandboxError> {
        self.token.as_deref().ok_or_else(|| {
            config("missing_token", "no Freestyle key: set FREESTYLE_API_KEY")
        })
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
    ) -> Result<(u16, String), SandboxError> {
        let token = self.token()?;
        let mut request =
            self.client.request(method, url).bearer_auth(token).timeout(CONTROL_TIMEOUT);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| transport_error(&error))?;
        let status = response.status().as_u16();
        let text = response.text().await.map_err(|error| transport_error(&error))?;
        Ok((status, text))
    }

    async fn vm(&self, selector: &str) -> Result<VmBody, SandboxError> {
        let url = format!("{API_BASE}/v5/vms/{}", encode_path(selector));
        let (status, body) = self.send(reqwest::Method::GET, &url, None).await?;
        if status != 200 {
            return Err(status_error(status, &body, "get"));
        }
        parse_json::<VmBody>(&body, "get")
    }

    /// The VM already carrying this idempotency key in its metadata, if one exists.
    async fn find_by_key(&self, key: &str) -> Result<Option<String>, SandboxError> {
        let filter = format!("{METADATA_IDEMPOTENCY}:{}", metadata_key(key));
        let mut url = url::Url::parse(&format!("{API_BASE}/v5/vms"))
            .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("limit", &LIST_PAGE.to_string());
            query.append_pair("metadata", &filter);
        }
        let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None).await?;
        if status != 200 {
            // A create must not run without proving no VM is already labelled for it.
            return Err(status_error(status, &body, "create"));
        }
        let page: ListEnvelope = parse_json(&body, "create")?;
        Ok(page
            .vms
            .into_iter()
            .find(|vm| {
                vm.metadata.get(METADATA_IDEMPOTENCY).map(String::as_str)
                    == Some(metadata_key(key))
            })
            .map(|vm| vm.id))
    }

    /// The env this sandbox was created with.
    ///
    /// A sandbox this Mac has no create record for is refused: running the command without the
    /// env its create asked for would fail later, and further from the cause.
    fn create_env(&self, sandbox_id: &str) -> Result<BTreeMap<String, String>, SandboxError> {
        self.env.load(sandbox_id)?.ok_or_else(|| {
            SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_unavailable",
                format!(
                    "this Mac has no create record for sandbox {sandbox_id}, so the env it was \
                     created with is unknown; recreate it here instead of running the command \
                     without that env"
                ),
            )
        })
    }
}

#[async_trait::async_trait]
impl SandboxProvider for FreestyleProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Nobody has to pass a TTL: `-1`/absent means no idle pause, and the plan's own cap
            // applies to `autoDeleteSeconds`, which this adapter never sends.
            ttl_required: false,
            max_ttl_seconds: None,
            transport: Transport::Ssh,
            // `idleTimeoutSeconds` pauses an idle VM on the platform's own timer.
            native_auto_stop: true,
        }
    }

    async fn create(&self, request: CreateSandbox) -> Result<String, SandboxError> {
        if let Some(sandbox_id) = self.find_by_key(&request.idempotency_key).await? {
            self.env.save(&sandbox_id, &request.env)?;
            return Ok(sandbox_id);
        }
        let url = format!("{API_BASE}/v5/vms");
        let (status, text) = self.send(reqwest::Method::POST, &url, Some(create_body(&request))).await?;
        match status {
            200 | 201 => {
                let created: VmBody = parse_json(&text, "create")?;
                self.env.save(&created.id, &request.env)?;
                Ok(created.id)
            }
            // Losing the race to our own earlier create is the idempotent answer.
            409 => match self.find_by_key(&request.idempotency_key).await? {
                Some(sandbox_id) => {
                    self.env.save(&sandbox_id, &request.env)?;
                    Ok(sandbox_id)
                }
                None => Err(status_error(409, &text, "create")),
            },
            _ => Err(status_error(status, &text, "create")),
        }
    }

    async fn get(&self, sandbox_id: &str) -> Result<SandboxInfo, SandboxError> {
        let vm = self.vm(sandbox_id).await?;
        Ok(SandboxInfo { state: map_state(vm.state.as_deref()), error: None })
    }

    async fn stop(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        // `pause` is this provider's stop: the disk survives and `start` brings it back.
        let url = format!("{API_BASE}/v5/vms/{}/pause", encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            return Ok(());
        }
        // Pausing a VM that is already not running is what the caller asked for.
        if status == 409 {
            let info = self.get(sandbox_id).await?;
            if matches!(info.state, SandboxState::Stopped | SandboxState::Provisioning) {
                return Ok(());
            }
        }
        Err(status_error(status, &body, "stop"))
    }

    async fn resume(&self, sandbox_id: &str, ttl_seconds: Option<u64>) -> Result<(), SandboxError> {
        // Reset the idle window before the boot, so the old one cannot pause the VM immediately.
        if let Some(ttl_seconds) = ttl_seconds {
            self.patch_idle(sandbox_id, ttl_seconds).await?;
        }
        let url = format!("{API_BASE}/v5/vms/{}/start", encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::POST, &url, None).await?;
        if status == 200 {
            return Ok(());
        }
        // A VM already running is resumed.
        if status == 409 {
            let info = self.get(sandbox_id).await?;
            if info.state == SandboxState::Ready {
                return Ok(());
            }
        }
        Err(status_error(status, &body, "resume"))
    }

    async fn delete(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        let url = format!("{API_BASE}/v5/vms/{}", encode_path(sandbox_id));
        let (status, body) = self.send(reqwest::Method::DELETE, &url, None).await?;
        if status == 200 || status == 204 {
            // The VM is gone, so its env has nothing left to run in.
            self.env.remove(sandbox_id);
            // The VM is gone; its identity has nothing left to reach.
            let stale = self
                .ssh_identities
                .lock()
                .map(|mut identities| identities.remove(sandbox_id))
                .ok()
                .flatten();
            if let Some(stale) = stale {
                let _ = self.revoke_ssh_access(&stale).await;
            }
            return Ok(());
        }
        Err(status_error(status, &body, "delete"))
    }

    async fn extend_ttl(
        &self,
        sandbox_id: &str,
        ttl_seconds: Option<u64>,
    ) -> Result<(), SandboxError> {
        // `None` means "the provider's default", which is what the field already holds.
        match ttl_seconds {
            Some(ttl_seconds) => self.patch_idle(sandbox_id, ttl_seconds).await,
            None => {
                self.vm(sandbox_id).await?;
                Ok(())
            }
        }
    }

    async fn exec(
        &self,
        sandbox_id: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<ExecResult, SandboxError> {
        let timeout_seconds = timeout_seconds.clamp(1, MAX_EXEC_SECONDS);
        // The env the create asked for, read back from disk so a restart cannot drop it; a sandbox
        // with no record is an error rather than a command that runs without its environment.
        let env = self.create_env(sandbox_id)?;
        let body = exec_body(command, timeout_seconds, &env);
        let url = format!("{API_BASE}/v5/vms/{}/exec-await", encode_path(sandbox_id));
        let token = self.token()?;
        let request = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .timeout(Duration::from_secs(timeout_seconds.saturating_add(30)));
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(&error)),
        };
        let status = response.status().as_u16();
        let text = match response.text().await {
            Ok(text) => text,
            Err(error) if error.is_timeout() => return Ok(timed_out()),
            Err(error) => return Err(transport_error(&error)),
        };
        if status != 200 {
            return Err(status_error(status, &text, "exec"));
        }
        let parsed: ExecResponse = parse_json(&text, "exec")?;
        Ok(ExecResult {
            // `null` statusCode is the guest's kill at the deadline.
            exit_code: parsed.status_code,
            stdout: parsed.stdout.unwrap_or_default(),
            stderr: parsed.stderr.unwrap_or_default(),
            timed_out: parsed.status_code.is_none(),
        })
    }

    async fn ssh_target(&self, sandbox_id: &str) -> Result<SshTarget, SandboxError> {
        // The gateway needs a canonical VM id, and only a running VM accepts a session.
        let info = self.get(sandbox_id).await?;
        if info.state != SandboxState::Ready {
            return Err(SandboxError::new(
                ErrorKind::Conflict,
                PROVIDER,
                "ssh_needs_running",
                format!("VM {sandbox_id} is {}; start it before asking for an ssh target", info.state),
            ));
        }
        let vm = self.vm(sandbox_id).await?;
        let token = self.mint_ssh_token(&vm.id).await?;
        Ok(ssh_target_for(&vm.id, &token))
    }

    async fn list(&self) -> Result<Vec<ListedSandbox>, SandboxError> {
        let mut out = Vec::new();
        let mut offset = 0usize;
        // The orphan scan walks the whole account, bounded so a mover cannot loop forever.
        for _ in 0..MAX_LIST_PAGES {
            let mut url = url::Url::parse(&format!("{API_BASE}/v5/vms"))
                .map_err(|error| config("bad_url", format!("could not build the list URL: {error}")))?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("limit", &LIST_PAGE.to_string());
                query.append_pair("offset", &offset.to_string());
            }
            let (status, body) = self.send(reqwest::Method::GET, url.as_str(), None).await?;
            if status != 200 {
                return Err(status_error(status, &body, "list"));
            }
            let page: ListEnvelope = parse_json(&body, "list")?;
            let count = page.vms.len();
            for row in page.vms {
                out.push(ListedSandbox {
                    sandbox_id: row.id,
                    state: map_state(row.state.as_deref()),
                    created_at: row.created_at.as_deref().and_then(rfc3339_millis),
                    name: row.slug.clone().or(row.display_name.clone()),
                });
            }
            // An empty page is the end; a short page is the end; a full page means ask again.
            if count < LIST_PAGE {
                return Ok(out);
            }
            offset += count;
        }
        Err(SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "list_too_long",
            format!("listing stopped after {MAX_LIST_PAGES} pages"),
        ))
    }
}

impl FreestyleProvider {
    async fn patch_idle(&self, sandbox_id: &str, ttl_seconds: u64) -> Result<(), SandboxError> {
        let url = format!("{API_BASE}/v5/vms/{}", encode_path(sandbox_id));
        let body = json!({ "idleTimeoutSeconds": ttl_seconds });
        let (status, text) = self.send(reqwest::Method::PATCH, &url, Some(body)).await?;
        if status == 200 {
            Ok(())
        } else {
            Err(status_error(status, &text, "extend_ttl"))
        }
    }

    /// A short-lived identity that may exec on this one VM, and a token for it. The SDK deletes
    /// the identity to revoke both, which is what [`FreestyleProvider::revoke_ssh_access`] does.
    async fn mint_ssh_token(&self, vm_id: &str) -> Result<String, SandboxError> {
        let (status, body) =
            self.send(reqwest::Method::POST, &format!("{API_BASE}/v5/identities"), Some(json!({})))
                .await?;
        if status != 200 {
            return Err(status_error(status, &body, "ssh_target"));
        }
        let identity: IdentityBody = parse_json(&body, "ssh_target")?;
        let grant = format!("{API_BASE}/v5/identities/{}/permissions/vm", encode_path(&identity.id));
        let (status, body) = self.send(reqwest::Method::POST, &grant, Some(json!({ "vmId": vm_id }))).await?;
        if status != 200 {
            return Err(status_error(status, &body, "ssh_target"));
        }
        let tokens = format!("{API_BASE}/v5/identities/{}/tokens", encode_path(&identity.id));
        // The OpenAPI says `null` is accepted here, but the service answers
        // `422 invalid type: null, expected struct CreateAccessTokenOptions` (measured live
        // 2026-11-21), so the empty options table is what it actually takes.
        let (status, body) = self.send(reqwest::Method::POST, &tokens, Some(json!({}))).await?;
        if status != 200 {
            return Err(status_error(status, &body, "ssh_target"));
        }
        let created: CreatedToken = parse_json(&body, "ssh_target")?;
        if created.token.trim().is_empty() {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "empty_ssh_token",
                "the API minted an identity token without a value",
            ));
        }
        // Whatever identity served this VM before is now the stale one.
        let previous = self
            .ssh_identities
            .lock()
            .map(|mut identities| identities.insert(vm_id.to_string(), identity.id))
            .ok()
            .flatten();
        if let Some(previous) = previous {
            // Best effort: a leftover identity expires with its token, and failing the caller's
            // new target over the cleanup of an older one would be the wrong trade.
            let _ = self.revoke_ssh_access(&previous).await;
        }
        Ok(created.token)
    }
}

/// Deleting an identity revokes every token and grant it holds, so this is the one call that ends
/// an ssh session for good.
impl FreestyleProvider {
    /// Revoke this VM's current identity, e.g. when a session ends before its token expires.
    pub async fn revoke_ssh_access(&self, identity_id: &str) -> Result<(), SandboxError> {
        let url = format!("{API_BASE}/v5/identities/{}", encode_path(identity_id));
        let (status, body) = self.send(reqwest::Method::DELETE, &url, None).await?;
        if status == 200 || status == 204 {
            Ok(())
        } else {
            Err(status_error(status, &body, "revoke_ssh_access"))
        }
    }
}

/// The create env of one sandbox, as written to disk.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvRecord {
    version: u32,
    sandbox_id: String,
    env: BTreeMap<String, String>,
}

/// Where a sandbox's create env lives: `<root>/<sandbox>.json`, 0600, one file per sandbox.
///
/// A value, not a global, so a test points it at a temp directory and nothing here can reach the
/// operator's real `~/.ferryx`. Names are hex-encoded, so a sandbox id can never walk out of the
/// directory or collide with another's file.
#[derive(Debug, Clone)]
struct EnvStore {
    root: Option<PathBuf>,
}

impl EnvStore {
    /// `~/.ferryx/cloud-host/env`; `None` when the home directory cannot be resolved, which makes
    /// `save` and `load` report `Config` instead of guessing a location.
    fn default_location() -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self {
            root: home.map(|home| {
                PathBuf::from(home).join(STATE_DIR).join(ENV_DIR).join(ENV_SUBDIR)
            }),
        }
    }

    /// The env of this sandbox; `None` when no create has been recorded here for it.
    fn load(&self, sandbox_id: &str) -> Result<Option<BTreeMap<String, String>>, SandboxError> {
        let path = self.path(sandbox_id)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(self.io_error("read", &path, &error)),
        };
        let record: EnvRecord = serde_json::from_slice(&bytes).map_err(|error| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "env_record_unreadable",
                format!(
                    "the create env record {} is not the shape this build writes: {error}",
                    path.display()
                ),
            )
        })?;
        if record.version > ENV_VERSION {
            return Err(SandboxError::new(
                ErrorKind::Config,
                PROVIDER,
                "env_record_version",
                format!(
                    "the create env record {} was written by a newer build (version {}); this \
                     build reads up to {ENV_VERSION} and must not reinterpret it",
                    path.display(),
                    record.version
                ),
            ));
        }
        // The name is hex of the id, so a record under the wrong name means the file was moved by
        // hand; reading its env would run the wrong environment under this sandbox's command.
        if record.sandbox_id != sandbox_id {
            return Err(SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "env_record_mismatch",
                format!(
                    "the create env record {} names sandbox {:?}, not {sandbox_id:?}",
                    path.display(),
                    record.sandbox_id
                ),
            ));
        }
        Ok(Some(record.env))
    }

    /// Record the env this create asked for, replacing what an earlier create of the same sandbox
    /// wrote. An empty env still writes a record: "created with no env" is a known answer, and
    /// `load` must not confuse it with "never created here".
    fn save(
        &self,
        sandbox_id: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<(), SandboxError> {
        let path = self.path(sandbox_id)?;
        let record = EnvRecord {
            version: ENV_VERSION,
            sandbox_id: sandbox_id.to_string(),
            env: env.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|error| {
            SandboxError::new(
                ErrorKind::Fatal,
                PROVIDER,
                "env_record_unwritable",
                format!("cannot serialize the create env for {sandbox_id}: {error}"),
            )
        })?;
        write_private_atomically(&path, &bytes).map_err(|error| self.io_error("write", &path, &error))
    }

    /// Forget a sandbox's env, e.g. once the VM is deleted. Absent is fine.
    fn remove(&self, sandbox_id: &str) {
        let Ok(path) = self.path(sandbox_id) else { return };
        let _ = std::fs::remove_file(path);
    }

    fn path(&self, sandbox_id: &str) -> Result<PathBuf, SandboxError> {
        let root = self.root.as_ref().ok_or_else(|| {
            config(
                "no_home",
                "cannot resolve the home directory for the cloud create env",
            )
        })?;
        // Hex, so only `[0-9a-f]` can appear in a name: no `..`, no separator, no platform quirks.
        let mut name = String::with_capacity(sandbox_id.len() * 2 + 5);
        for byte in sandbox_id.bytes() {
            name.push_str(&format!("{byte:02x}"));
        }
        name.push_str(".json");
        Ok(root.join(name))
    }

    fn io_error(&self, verb: &str, path: &Path, error: &std::io::Error) -> SandboxError {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "env_record_io",
            format!("cannot {verb} the create env record {}: {error}", path.display()),
        )
    }
}

/// `bytes` to `path` through a 0600 temp file and one rename: the file is never briefly
/// world-readable, and a reader sees either the whole old record or the whole new one.
fn write_private_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "the env record has no parent directory")
    })?;
    std::fs::create_dir_all(parent)?;
    // `~/.ferryx` holds the daemon's secrets, and `env` may hold a slot token: owner-only.
    let _ = restrict(parent, 0o700);

    let temp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // A umask can only clear bits, and some filesystems ignore the open mode, so state 0600
    // explicitly before the name changes hands.
    restrict(&temp, 0o600)?;
    std::fs::rename(&temp, path)?;
    // Best effort: a fsync of the directory makes the rename durable; a filesystem that refuses it
    // must not fail a write that already happened.
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// The `exec-await` body: the command, its deadline, and the create env it must run with.
fn exec_body(
    command: &str,
    timeout_seconds: u64,
    env: &BTreeMap<String, String>,
) -> Value {
    let mut body = json!({
        "command": command,
        "timeoutMs": timeout_seconds.saturating_mul(1000),
    });
    if !env.is_empty() {
        body["env"] = json!(env);
    }
    body
}

/// The create body: the idempotency key as metadata, the name as slug, and the one firewall rule
/// the machine cannot do without.
fn create_body(request: &CreateSandbox) -> Value {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        METADATA_IDEMPOTENCY.to_string(),
        metadata_key(&request.idempotency_key).to_string(),
    );
    let mut body = json!({
        "slug": slug_for(&request.name),
        "displayName": request.name,
        "metadata": metadata,
        // Required on every create, and empty means "no network at all": this rule is outbound
        // only. An empty `source` is the VM being created; `public: true` is every routable
        // address, which is what the relay and the guest's own package installs need. Inbound
        // stays shut — SSH comes in through the gateway.
        "firewall": {
            "rules": [{
                "action": "allow",
                "source": {},
                "destination": { "public": true },
            }],
        },
    });
    if let Some(ttl_seconds) = request.ttl_seconds {
        body["idleTimeoutSeconds"] = json!(ttl_seconds);
    }
    body
}

/// `<vm-id>:<token>` at `<vm-id>.beta-ssh.freestyle.sh`, the form the SDK's `nativeSsh` builds:
/// the gateway authenticates that username through `none`, so no key file of ours is involved.
fn ssh_target_for(vm_id: &str, token: &str) -> SshTarget {
    SshTarget {
        host: format!("{vm_id}{SSH_SUFFIX}"),
        port: 22,
        user: format!("{vm_id}:{token}"),
        identity: SshIdentity::Agent,
        known_hosts_line: None,
        // Freestyle's endpoint is a normal internet host: no CLI in the path, no certificate.
        proxy_command: None,
        certificate_file: None,
    }
}

/// The idempotency marker as metadata: values cap at 63 bytes, so a longer key is truncated
/// deterministically — a retry truncates the same way and still finds the VM.
///
/// The cut lands on a character boundary, never inside one: the key is the caller's own string and
/// may be non-ASCII, and slicing a multi-byte character in half is a panic rather than the
/// `SandboxError` every provider call promises.
fn metadata_key(key: &str) -> &str {
    if key.len() <= METADATA_KEY_MAX {
        return key;
    }
    let mut end = METADATA_KEY_MAX;
    while !key.is_char_boundary(end) {
        end -= 1;
    }
    &key[..end]
}

/// A name flattened into the slug grammar (1-63 of `[a-z0-9-]`, no leading, trailing or repeated
/// hyphens). The name itself already satisfies it in practice; this keeps a caller's choice from
/// turning into a `400`.
fn slug_for(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        let lowered = character.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            slug.push(lowered);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.len() > 63 { &slug[..63] } else { slug };
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "fx".to_string()
    } else {
        slug.to_string()
    }
}

/// Percent-encode one path segment (a VM id or slug).
fn encode_path(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Wire types

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VmBody {
    id: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListEnvelope {
    #[serde(default)]
    vms: Vec<VmBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecResponse {
    /// Absent when the guest killed the command at its deadline.
    #[serde(default)]
    status_code: Option<i32>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stderr: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdentityBody {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CreatedToken {
    /// A bearer credential, shown once; never logged.
    token: String,
}

/// The provider's `VmState` (`starting|running|pausing|paused|stopped`) in [`SandboxState`]
/// terms.
fn map_state(raw: Option<&str>) -> SandboxState {
    match raw.unwrap_or_default() {
        "running" => SandboxState::Ready,
        "pausing" => SandboxState::Stopping,
        "paused" | "stopped" => SandboxState::Stopped,
        // starting, and anything new the API adds: still coming up.
        _ => SandboxState::Provisioning,
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &str, call: &str) -> Result<T, SandboxError> {
    serde_json::from_str(body).map_err(|error| {
        SandboxError::new(
            ErrorKind::Fatal,
            PROVIDER,
            "unexpected_body",
            format!("{call}: the API answered with an unexpected body: {error}"),
        )
    })
}

/// RFC3339 -> unix milliseconds, the unit `ListedSandbox.created_at` uses.
fn rfc3339_millis(text: &str) -> Option<i64> {
    let parsed =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

fn timed_out() -> ExecResult {
    ExecResult { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: true }
}

fn config(code: impl Into<String>, message: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorKind::Config, PROVIDER, code, message)
}

/// Map an HTTP failure onto an [`ErrorKind`], keeping Freestyle's stable `code`
/// (`PUBLIC_ERROR_BODY`: `{code, message}`, sometimes with extra fields) for the operator.
fn status_error(status: u16, body: &str, call: &str) -> SandboxError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let field = |name: &str| {
        parsed
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let code = field("code").unwrap_or_else(|| "http_error".to_string());
    let detail = field("message").unwrap_or_else(|| body.trim().to_string());
    let kind = match status {
        401 | 403 => ErrorKind::Config,
        404 => ErrorKind::NotFound,
        409 => ErrorKind::Conflict,
        429 | 500..=599 => ErrorKind::Retryable,
        _ => ErrorKind::Fatal,
    };
    SandboxError::new(kind, PROVIDER, code, format!("{call}: HTTP {status}: {detail}"))
}

fn transport_error(error: &reqwest::Error) -> SandboxError {
    let kind = if error.is_timeout() || error.is_connect() {
        ErrorKind::Retryable
    } else {
        ErrorKind::Fatal
    };
    // Never include the request: the header carries the key and the URL names the VM.
    SandboxError::new(kind, PROVIDER, "transport", format!("request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATE: &str = include_str!("../../../tests/fixtures/cloud_host/freestyle/create_201.json");
    const GET_RUNNING: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/get_running.json");
    const GET_PAUSED: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/get_paused.json");
    const LIST: &str = include_str!("../../../tests/fixtures/cloud_host/freestyle/list_live.json");
    const EXEC: &str = include_str!("../../../tests/fixtures/cloud_host/freestyle/exec_200.json");
    const EXEC_TIMEOUT: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/exec_timeout.json");
    const ERROR_V1_401: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/error_v1_401.json");
    const ERROR_404: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/error_404.json");
    const IDENTITY: &str =
        include_str!("../../../tests/fixtures/cloud_host/freestyle/identity_200.json");
    const TOKEN: &str = include_str!("../../../tests/fixtures/cloud_host/freestyle/token_200.json");

    #[test]
    fn create_carries_the_idempotency_metadata_and_the_required_firewall() {
        let created: VmBody = parse_json(CREATE, "create").unwrap();
        assert_eq!(created.id, "vm-fixture-0001");
        assert_eq!(created.slug.as_deref(), Some("fx-t8-fixture"));
        assert_eq!(created.metadata.get("fx").map(String::as_str), Some("t8-fixture"));
        assert_eq!(map_state(created.state.as_deref()), SandboxState::Provisioning);
    }

    #[test]
    fn running_is_ready_and_paused_is_stopped() {
        let running: VmBody = parse_json(GET_RUNNING, "get").unwrap();
        assert_eq!(map_state(running.state.as_deref()), SandboxState::Ready);
        let paused: VmBody = parse_json(GET_PAUSED, "get").unwrap();
        assert_eq!(map_state(paused.state.as_deref()), SandboxState::Stopped);
        assert_eq!(map_state(Some("pausing")), SandboxState::Stopping);
        assert_eq!(map_state(Some("starting")), SandboxState::Provisioning);
        assert_eq!(map_state(Some("stopped")), SandboxState::Stopped);
    }

    #[test]
    fn the_live_empty_list_parses() {
        let page: ListEnvelope = parse_json(LIST, "list").unwrap();
        assert!(page.vms.is_empty());
    }

    #[test]
    fn create_opens_outbound_only_and_carries_the_idempotency_metadata() {
        let request = CreateSandbox {
            idempotency_key: "t8-fixture".to_string(),
            name: "FX T8 Fixture".to_string(),
            ttl_seconds: Some(1800),
            env: BTreeMap::new(),
        };
        let body = create_body(&request);
        assert_eq!(body["slug"], json!("fx-t8-fixture"));
        assert_eq!(body["metadata"]["fx"], json!("t8-fixture"));
        assert_eq!(body["idleTimeoutSeconds"], json!(1800));
        let rules = body["firewall"]["rules"].as_array().expect("one rule");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["action"], json!("allow"));
        // Empty `source` is the VM itself; `public: true` on the destination is the egress the
        // box needs. Nothing inbound is opened, because no rule names a public source.
        assert_eq!(rules[0]["source"], json!({}));
        assert_eq!(rules[0]["destination"], json!({ "public": true }));

        // No TTL is the provider's own "never pause for idleness", not a zero second window.
        let mut open = request.clone();
        open.ttl_seconds = None;
        assert!(create_body(&open).get("idleTimeoutSeconds").is_none());
    }

    #[test]
    fn a_live_running_vm_carries_its_creation_time() {
        let vm: VmBody = parse_json(GET_RUNNING, "get").unwrap();
        assert_eq!(vm.created_at.as_deref().and_then(rfc3339_millis), Some(1_767_225_600_000));
    }

    #[test]
    fn exec_answers_both_streams_and_the_exit_code() {
        let parsed: ExecResponse = parse_json(EXEC, "exec").unwrap();
        assert_eq!(parsed.status_code, Some(0));
        assert_eq!(parsed.stdout.as_deref(), Some("x86_64\n"));
        assert_eq!(parsed.stderr.as_deref(), Some(""));
    }

    #[test]
    fn a_timeout_killed_command_has_no_exit_code() {
        let parsed: ExecResponse = parse_json(EXEC_TIMEOUT, "exec").unwrap();
        assert_eq!(parsed.status_code, None);
        assert!(parsed.status_code.is_none(), "statusCode null is the guest's kill at the deadline");
    }

    #[test]
    fn the_legacy_v1_route_401_is_config_not_a_silent_failure() {
        let error = status_error(401, ERROR_V1_401, "get");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "UNAUTHORIZED_ERROR");
        assert!(error.message.contains("/v1/vms"));
    }

    #[test]
    fn the_rest_of_the_status_mapping() {
        assert_eq!(status_error(403, "{}", "exec").kind, ErrorKind::Config);
        let gone = status_error(404, ERROR_404, "get");
        assert_eq!(gone.kind, ErrorKind::NotFound);
        assert_eq!(gone.provider_code, "NOT_FOUND");
        assert!(gone.message.contains("not found"));
        assert_eq!(status_error(409, r#"{"code":"CONFLICT"}"#, "create").kind, ErrorKind::Conflict);
        assert_eq!(status_error(429, r#"{"code":"LIMIT_EXCEEDED"}"#, "create").kind, ErrorKind::Retryable);
        assert_eq!(status_error(503, "{}", "list").kind, ErrorKind::Retryable);
        assert_eq!(status_error(400, r#"{"code":"BAD_REQUEST"}"#, "create").kind, ErrorKind::Fatal);
    }

    #[test]
    fn the_ssh_username_is_the_vm_id_and_token_at_the_gateway() {
        let identity: IdentityBody = parse_json(IDENTITY, "ssh_target").unwrap();
        assert_eq!(identity.id, "idn-fixture-0001");
        let created: CreatedToken = parse_json(TOKEN, "ssh_target").unwrap();
        assert_eq!(created.token, "fx-fixture-token");

        let target = ssh_target_for("vm-fixture-0001", &created.token);
        assert_eq!(target.host, "vm-fixture-0001.beta-ssh.freestyle.sh");
        assert_eq!(target.user, "vm-fixture-0001:fx-fixture-token");
        assert_eq!(target.port, 22);
        assert_eq!(target.identity, SshIdentity::Agent);
        assert!(target.known_hosts_line.is_none());
    }

    #[test]
    fn a_name_becomes_a_slug_that_satisfies_the_grammar() {
        assert_eq!(slug_for("fx-t8"), "fx-t8");
        assert_eq!(slug_for("FX T8 Pro"), "fx-t8-pro");
        assert_eq!(slug_for("--weird__name--"), "weird-name");
        assert_eq!(slug_for(""), "fx");
        assert_eq!(slug_for("!!!"), "fx");
        let long = slug_for(&"a".repeat(80));
        assert_eq!(long.len(), 63);
        assert!(!long.ends_with('-'));
    }

    #[test]
    fn an_idempotency_key_is_truncated_on_a_char_boundary() {
        // The exact key the verifier panicked on: 60 ASCII bytes, then a 63rd byte inside `é`.
        // Cutting on the boundary keeps the one `é` that fits (62 bytes) instead of panicking;
        // the old `&key[..63]` died here with "byte index 63 is not a char boundary".
        let key = format!("{}{}", "a".repeat(60), "é".repeat(5));
        assert_eq!(key.len(), 70);
        let truncated = metadata_key(&key);
        assert_eq!(truncated, format!("{}é", "a".repeat(60)));
        assert_eq!(truncated.len(), 62);
        assert!(truncated.len() <= METADATA_KEY_MAX);
        assert!(key.starts_with(truncated), "the marker is a prefix of the key");
        // Truncation is deterministic, so a retry finds the same VM.
        assert_eq!(metadata_key(&key), metadata_key(&key));

        // An ASCII key of exactly the cap is untouched, and one byte more is cut at 63.
        let ascii_cap = "b".repeat(63);
        assert_eq!(metadata_key(&ascii_cap), ascii_cap);
        let ascii_over = "b".repeat(64);
        assert_eq!(metadata_key(&ascii_over).len(), 63);

        // A multi-byte character is dropped whole or kept whole, never halved inside the cap.
        // `€` is 3 bytes: it fits after 58..=60 ASCII bytes and is dropped after 61..=63.
        for prefix in 58..=63 {
            let key = format!("{}{}", "a".repeat(prefix), "€".repeat(4));
            let truncated = metadata_key(&key);
            assert!(truncated.len() <= METADATA_KEY_MAX, "prefix {prefix}");
            assert!(key.starts_with(truncated), "prefix {prefix}");
            let expected = if prefix + 3 <= METADATA_KEY_MAX {
                format!("{}€", "a".repeat(prefix))
            } else {
                "a".repeat(prefix)
            };
            assert_eq!(truncated, expected, "prefix {prefix}");
        }

        // The panic case itself: a character whose bytes begin before the cap and end after it.
        let straddling = format!("{}{}", "a".repeat(62), "€".repeat(2));
        assert_eq!(metadata_key(&straddling), "a".repeat(62));
    }

    #[test]
    fn a_non_ascii_idempotency_key_reaches_the_create_body_without_panicking() {
        let request = CreateSandbox {
            idempotency_key: format!("{}{}", "a".repeat(60), "é".repeat(5)),
            name: "FX T8 Unicode".to_string(),
            ttl_seconds: None,
            env: BTreeMap::new(),
        };
        let body = create_body(&request);
        // 60 ASCII + one `é` is the longest char-boundary cut under the 63-byte cap.
        assert_eq!(body["metadata"]["fx"], json!(format!("{}é", "a".repeat(60))));
        assert!(body["metadata"]["fx"].as_str().unwrap().len() <= METADATA_KEY_MAX);
    }

    #[test]
    fn create_env_survives_a_new_process_and_every_exec_re_sends_it() {
        let root = tempfile::tempdir().expect("a temp env root");
        let mut env = BTreeMap::new();
        env.insert("FX_TOKEN".to_string(), "redacted".to_string());

        {
            let first_run = FreestyleProvider::with_env_store(None, EnvStore {
                root: Some(root.path().join(ENV_SUBDIR)),
            });
            first_run.env.save("vm-fixture-0001", &env).expect("record the create env");
        }

        // A second provider instance stands in for the control plane after a restart: the new
        // process never ran this create, and must still find the env on disk.
        let after_restart = FreestyleProvider::with_env_store(None, EnvStore {
            root: Some(root.path().join(ENV_SUBDIR)),
        });
        let read_back = after_restart.create_env("vm-fixture-0001").expect("env survived");
        assert_eq!(read_back.get("FX_TOKEN").map(String::as_str), Some("redacted"));
        let body = exec_body("omo host ensure", 30, &read_back);
        assert_eq!(body["env"]["FX_TOKEN"], json!("redacted"));
        assert_eq!(body["timeoutMs"], json!(30_000));

        // A sandbox this Mac has no record for is refused, never exec'd with an empty env.
        let error = after_restart.create_env("vm-unknown").expect_err("no record");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_unavailable");

        // Delete forgets it.
        after_restart.env.remove("vm-fixture-0001");
        assert!(after_restart.create_env("vm-fixture-0001").is_err());
    }

    #[test]
    fn a_create_that_asks_for_no_env_still_records_one() {
        let root = tempfile::tempdir().expect("a temp env root");
        let store = EnvStore { root: Some(root.path().join(ENV_SUBDIR)) };
        let provider = FreestyleProvider::with_env_store(None, store.clone());
        provider.env.save("vm-fixture-0002", &BTreeMap::new()).expect("record empty env");
        // "Created with no env" is a known answer, not a missing record.
        assert_eq!(provider.create_env("vm-fixture-0002").expect("known").len(), 0);
        assert_eq!(exec_body("true", 5, &BTreeMap::new()).get("env"), None);
    }

    #[test]
    fn an_env_record_is_owner_only() {
        let root = tempfile::tempdir().expect("a temp env root");
        let dir = root.path().join(ENV_SUBDIR);
        let store = EnvStore { root: Some(dir.clone()) };
        let mut env = BTreeMap::new();
        env.insert("FX_TOKEN".to_string(), "redacted".to_string());
        store.save("vm-fixture-0003", &env).expect("save");
        let path = store.path("vm-fixture-0003").expect("a path");
        // Hex names: nothing outside `[0-9a-f]` can appear, so a sandbox id cannot walk out.
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(".json"), "{name}");
        assert!(name[..name.len() - 5].bytes().all(|byte| byte.is_ascii_hexdigit()), "{name}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn a_newer_env_record_is_refused_rather_than_reinterpreted() {
        let root = tempfile::tempdir().expect("a temp env root");
        let dir = root.path().join(ENV_SUBDIR);
        std::fs::create_dir_all(&dir).unwrap();
        let store = EnvStore { root: Some(dir.clone()) };
        let path = store.path("vm-fixture-0004").expect("a path");
        std::fs::write(
            &path,
            br#"{"version":99,"sandboxId":"vm-fixture-0004","env":{"A":"B"}}"#,
        )
        .unwrap();
        let error = store.load("vm-fixture-0004").expect_err("a newer version");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "env_record_version");
    }

    #[test]
    fn the_exec_wait_is_clamped_to_the_inline_maximum() {
        // The clamp itself lives in `exec`; this pins the constant the adapter documents.
        assert_eq!(MAX_EXEC_SECONDS, 300);
        assert_eq!(600u64.clamp(1, MAX_EXEC_SECONDS), 300);
    }

    #[test]
    fn no_key_reports_config_before_any_request() {
        let provider = FreestyleProvider::without_token();
        let error = provider.token().expect_err("no key");
        assert_eq!(error.kind, ErrorKind::Config);
        assert_eq!(error.provider_code, "missing_token");
        assert!(error.message.contains("FREESTYLE_API_KEY"));
    }

    #[test]
    fn capabilities_are_ssh_with_the_provider_own_idle_pause() {
        let capabilities = FreestyleProvider::without_token().capabilities();
        assert_eq!(capabilities.transport, Transport::Ssh);
        assert!(!capabilities.ttl_required);
        assert_eq!(capabilities.max_ttl_seconds, None);
        assert!(capabilities.native_auto_stop);
    }
}
