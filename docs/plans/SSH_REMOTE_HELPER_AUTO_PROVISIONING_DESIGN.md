# Ferryx SSH Remote Helper Auto-Provisioning

## UI/UX-first architecture and implementation specification

**Status:** Proposed; this document does not implement provisioning.  
**Design date:** 2026-09-10  
**Scope:** Ferryx desktop, SSH host settings, remote terminal startup, standalone helper distribution, installation, upgrades, and recovery.  
**Initial remote targets:** Linux x86_64, Linux arm64, and Windows x64.  
**Primary outcome:** Opening a remote terminal should lead to a working shell or a precise recovery action, never an unexplained `helper_missing` error or an indefinitely blank pane.

---

## 1. Executive decision

Adopt **just-in-time provisioning on the first remote terminal open, with proactive preparation as an explicit opt-in when adding a host**. Opening a project directory, saving/importing a host, and testing connectivity remain distinct from installing software.

A successful first-use journey is:

```text
Save SSH host -> Open remote project -> Open terminal
    -> Establish trusted SSH connection
    -> Identify remote OS, CPU, and runtime requirements
    -> Check existing helper and installation policy
    -> Transfer the verified bundled artifact when necessary
    -> Verify and install without replacing a running executable
    -> Start or select a compatible detached daemon
    -> Validate both bridge connections
    -> Create the requested PTY exactly once -> Show terminal
```

The visible experience belongs in the pane, not in a blocking application-wide wizard. Settings, the pane, and the workspace badge observe the same backend operation. Several panes opening simultaneously must not launch competing installers.

### 1.1 Non-negotiable product contracts

1. **No surprise remote mutation.** New host forms disclose automatic first-use installation. Existing hosts without an installation policy receive a one-time inline choice before the first write. “Test connection” is read-only.
2. **No dependency on remote Internet access.** All three initial remote artifacts ship in every desktop bundle. Automatic setup uses the desktop's SSH transport, not remote `curl`, a package manager, or a compiler.
3. **No destructive repair or upgrade.** Installation, cancellation, retries, and application upgrades never implicitly terminate existing remote PTYs. Reconnection never silently creates a replacement session.
4. **No counterfeit progress.** Percentages describe measured transfer bytes, not guessed total setup completion. “Ready” follows a real protocol/identity handshake; terminal readiness additionally requires PTY creation or attachment.
5. **One authoritative orchestrator.** A service in the local Ferryx daemon owns provisioning, cancellation, locking, snapshots, and deadlines. React only expresses intent and renders state.
6. **Fail closed on trust and integrity.** A host-key mismatch, invalid signature, checksum mismatch, or unsafe installation path cannot trigger an unsigned download, a permission bypass, or automatic host-key acceptance.

### 1.2 Alternatives and trade-offs

| Approach | Benefit | Cost | Decision |
| --- | --- | --- | --- |
| Provision every saved/imported host | Early readiness feedback | Unwanted writes, bulk network activity, inaccessible hosts, slower settings | Reject as the default |
| Provision only on terminal open | Work happens when needed | First terminal takes longer | Default, with a clear pane placeholder |
| Offer “Prepare now” during host addition | Users can validate end-to-end readiness early | Requires host-scoped orchestration without a workspace | Include as an unchecked opt-in |
| Download the helper on the remote host | Smaller desktop bundle | Breaks offline hosts and adds remote TLS/tooling dependencies | Manual recovery only |
| Download helper assets on the desktop by default | Smaller bundle | First use depends on release service availability | Optional signed-update path, not the baseline |
| Replace one fixed executable and restart | Simple paths | Windows file-lock problems and loss of detached sessions | Reject; use immutable installations and pinned runtimes |

**Non-goals:** unattended SSH trust enrollment, password collection, `sudo`, installing Git or an SSH server, supporting every POSIX platform, migrating live PTYs between daemon processes, and redesigning Ferryx's web-remote transport. An ordinary external SSH terminal may be offered for diagnostics, but it must not be represented as Ferryx's durable terminal backend.

## 2. Repository baseline and integration constraints

The following is observed code, not a statement that the proposed feature already exists. References use workspace-relative paths and named symbols so they remain useful after line numbers move.

| ID | Inspected implementation | Consequence for this design |
| --- | --- | --- |
| R1 | `src-tauri/src/ipc/ssh.rs`: `SshHostStore`, `cmd_ssh_update_host`, `cmd_ssh_test_connection`, `cmd_ssh_install_project_helper` | Hosts persist in `ssh_hosts.json`; test calls `runtime::detect`; explicit install requires a workspace and local binary. Add host-scoped preparation without manufacturing a workspace. |
| R2 | `src-tauri/src/ssh/helper_setup.rs`: `default_location`, `install`, `ensure_started`, `parse_ready_output`, `map_ensure_started_error` | Default executable is under `~/.ferryx/bin`; runtime root includes a hash-derived host slug. Installation reads the entire local file into memory and sends it through SSH stdin. The existing path is not a progress-reporting SFTP implementation. |
| R3 | `src-tauri/src/ssh/runtime.rs`: `RemoteEnvironment`, `detect`, `RemoteExecutor::command`, `RemotePlatform::validate_path` | Detection distinguishes POSIX/Windows and executor, home, temp, Git, and a version field; it does not supply a trustworthy CPU/libc target. POSIX detection also accepts Darwin/BSD, which does not imply helper artifact support. |
| R4 | `src-tauri/src/ssh/direct.rs`: `ssh_plan`, `quote_posix` | Preserve argument validation, non-PTY execution, `BatchMode=yes`, `StrictHostKeyChecking=yes`, disabled forwarding/control reuse, and bounded SSH execution. |
| R5 | `src-tauri/src/ipc/terminal.rs`: `cmd_terminal_spawn` | The IPC entry routes remote requests through `DaemonClient` and `TerminalStartup::RemoteSsh`. Provisioning cannot exist only as a React preflight. |
| R6 | `src-tauri/src/terminal/remote.rs`: `RemoteRuntime::create`, `restore`; `src-tauri/src/ssh/bridge.rs`: `SshBridgeClient::connect_with_target` | Create uses a stable request ID; restore reattaches and must never spawn. The bridge calls `ensure_started` and validates protocol/capabilities, host/owner/epoch, and parity across independent control/read connections. |
| R7 | `remote-helper/Cargo.toml`; `src-tauri/src/ferryx_scope/ssh/standalone.rs`, `helper.rs`, `process.rs` | A standalone crate already points at the shared helper implementation. Handshake currently reports protocol `1`, `sshHelperV1`, identity, OS, and architecture, but not a helper build version. Process code already uses an OS runtime lock and private endpoint validation. |
| R8 | `ui/src/components/settings/SshSection.tsx`; `ui/src/lib/sshHosts.ts` | Host save, connection test, and integration preparation are existing surfaces. Prepared/test state is invalidated after connection edits. Agent-extension preparation is not remote-helper installation. |
| R9 | `ui/src/components/TerminalPane.tsx`, `NativeTerminalPane.tsx`; `ui/src/state/workspaceStore.ts` | There are reconnect/disconnected/expired/legacy-lost overlays and stable spawn retry IDs. New setup state must not be misclassified as an exited session. Native surfaces require explicit visibility/input gating. |
| R10 | `src-tauri/tauri.conf.json`; `src-tauri/tests/clean_dev_resource_contract.rs` | The inspected resource map includes `../ui/dist -> ui/dist`, not helper artifacts. Preserve the existing mapping and clean-development behavior. |
| R11 | `scripts/release-local.mjs`; `scripts/build-latest-json.mjs` | Release preparation/assembly uses immutable plans, artifact hashes, receipts, signature verification, and strict inventories. Mutating release stages explicitly reject CI/GitHub Actions execution. Extend this local-release architecture, not an assumed cloud publishing workflow. |

**Important correction to the simplified problem description:** in the inspected code, `cmd_terminal_spawn` does not directly install or start the helper. Startup reaches `helper_setup::ensure_started` through the local daemon's remote runtime and bridge. The current install transport buffers bytes and invokes SSH with stdin; SFTP would be an additional adapter, not an already implemented streaming capability. [R2, R5, R6]

The existing fixed executable and per-host root remain migration inputs. New managed installations need versioned executable paths and runtime references; merely wrapping `install()` with a spinner would not solve concurrency, version negotiation, or session safety.

## 3. User journeys

### 3.1 Add a host: save first, prepare optionally

Retain existing host/authentication fields. Add a “Remote terminal setup” section below authentication:

```text
Remote terminal setup
[x] Install the Ferryx helper automatically when first needed
    Copies a verified helper from this app into your remote account's
    .ferryx directory. No administrator access or remote Internet needed.

[ ] Prepare this host now after saving
    Tests SSH and prepares the helper without opening a terminal.

                                      [Cancel] [Save host]
```

The first checkbox is checked for newly created hosts and is saved explicitly, including a consent schema version. “Prepare now” is unchecked. It implies authorization for this immediate preparation even when future automatic installation is disabled; the copy must explain that distinction.

Save validates and persists the host first. Only after a successful save does an opted-in preparation start. A preparation failure must not undo the saved host or re-open an invalid host form. The host row displays “Saved · Setup needs attention” with “Retry setup” and “Details.” Closing Settings unsubscribes its view but does not cancel a preparation the user explicitly requested.

Imported hosts are not bulk-provisioned. Hosts loaded from older stores without helper policy default to **ask before first installation**, not silently to consent. Disabled hosts cannot prepare. Editing hostname, user, port, key, jump host, effective SSH configuration, or helper policy invalidates affected snapshots and prevents an old operation from activating against the new revision.

### 3.2 Test connection is not installation

Keep **Test connection** read-only: verify SSH access and gather environment/target diagnostics. It must not create `.ferryx`, chmod anything, install extensions, upload a helper, or start a daemon. A result can say:

> SSH connection successful · Linux arm64 detected. Remote terminal helper has not been prepared.

Provide a separate **Prepare remote terminal** action after a successful test. This uses the same provisioning service as first terminal open. Existing “integration preparation” must be labeled as agent integration and must not set the helper-ready badge. [R1, R8]

A test may fail because a host is reachable but its supported command executor cannot run. Report connection and environment results separately; do not show a green “Terminal ready” badge based on reachability alone.

### 3.3 First remote workspace/tab open

Opening a remote directory or Git worktree without requesting a terminal remains possible through existing direct SSH operations. When an action requires a terminal, create a **provisional logical pane** immediately, before awaiting network activity. It has the chosen host/workspace, intended CWD, logical action ID, consumer ID, and setup state, but no backend session ID and no native terminal attachment.

For an explicitly enabled first-use policy, setup begins automatically. Otherwise show an inline authorization card:

> **Prepare remote terminal on build-server?**  
> Ferryx needs its helper to keep terminal processes running after SSH disconnects. It will install a verified binary in your account's `.ferryx` directory. It will not use administrator privileges.

Actions: **Install and continue**, **Manual setup**, **Cancel**. Include an unchecked “Allow future helper installations on this host” checkbox; a one-time acceptance does not silently become permanent consent.

Once the helper is ready, the existing logical terminal request proceeds exactly once. Do not require a second click, reopen the tab, or present a toast that the user must chase. Keep the same intended CWD, worktree identity, rows/columns, and `clientRequestId` through setup and transport retries.

### 3.4 Multiple panes and multiple windows

All views targeting the same host revision and compatible artifact policy join one operation. Each receives its own consumer lease. The first pane may say “Preparing remote terminal”; subsequent panes say “Waiting for setup already running on this host” and show the shared progress.

Cancel in one pane releases that pane's consumer, not everybody else's operation. Closing a pane before PTY creation cannot leave a future ghost terminal. Explicit host-wide cancellation in Settings requires a confirmation when other panes are waiting. UI navigation does not own the worker's lifetime.

Different configured aliases can refer to one account. Do not merge their daemon identities merely because hostname strings look similar: helper handshakes are bound to host IDs. Installation locking is account-wide on the remote filesystem, while runtime identity remains host-scoped. [R2, R6]

### 3.5 Existing helper, application updates, and reconnect

For a compatible installed helper, show only a brief “Checking remote terminal…” state and go straight to the shell. Do not force a minimum spinner duration.

For an optional update, keep existing compatible sessions usable. Display “Ready · Helper update pending” when activation is deferred. For a required protocol change, install and start a new runtime alongside the old one; new terminals use it, while old sessions remain pinned to their original runtime. Never imply that a daemon restart preserves live PTYs.

For a restored session, the pane says **Reconnecting to existing terminal**, not **Installing and creating terminal**. Repairing a missing executable can restore bridge access, but a missing runtime/changed epoch means the original PTY may be gone. Offer **Open new terminal** explicitly and preserve the lost session's identity and diagnostic details. [R6, R9]

## 4. Terminal pane and visual state machine

### 4.1 Primary setup surface

Use the existing terminal-pane background and Ferryx design tokens. Center a card with a maximum width of 480 px, 24 px internal padding, and an 8 px spacing rhythm. It should fit a 280 px-wide split; below that, switch to a compact title, active step, and expandable details. Do not impose a desktop-wide modal or fixed-height content that clips a short pane.

```text
┌ SSH · build-server / ~/projects/api ───────────────────────┐
│                                                          │
│        Preparing remote terminal                         │
│        build-server · user@host                          │
│                                                          │
│        ✓ Connected securely                              │
│        ✓ Detected Linux x86_64                           │
│        ✓ Selected verified helper                        │
│        ◌ Transferring helper                             │
│          [█████████───────────] 45%                       │
│          4.5 MiB / 10.0 MiB · 1.2 MiB/s                   │
│        ○ Verify and install                              │
│        ○ Start helper and validate connection            │
│        ○ Open terminal                                   │
│                                                          │
│        You can use other workspaces during setup.        │
│        [Cancel]                      [Show details ▾]    │
└──────────────────────────────────────────────────────────┘
```

Numbers above are illustrative, not measured package sizes or performance promises. The byte denominator comes from the selected manifest entry. Upload reaching 100% changes the caption to “Transfer sent · Verifying remote file”; it does not mark setup complete.

Use a spinner only for an active stage, a check for completion, a hollow mark for pending stages, and an error icon plus explanatory text on failure. Avoid color-only status. Show the host label first; allow full connection/path details to be expanded without leaking them into global notifications.

### 4.2 Visual state mapping

| Backend phase or condition | Pane title/status | Progress treatment | Available actions |
| --- | --- | --- | --- |
| `queued`, `connecting` | Connecting to host… | Indeterminate; reveal checklist if setup lasts beyond 150 ms | Cancel, details |
| `awaiting_consent` | Prepare remote terminal? | No spinner; explain remote changes | Install and continue, manual setup, cancel |
| `detecting`, `probing` | Checking remote environment… | Indeterminate; show verified OS/CPU when known | Cancel, details |
| `resolving`, `preflighting` | Preparing installation… | Show source, version, destination | Cancel, details |
| `transferring` | Transferring helper… | Measured transfer percentage and speed | Cancel, details |
| `verifying` | Verifying remote helper… | Transfer row checked only after remote byte verification | Cancel, details |
| `waiting_lock` | Another Ferryx installation is finishing… | Indeterminate with elapsed wait, not a frozen upload bar | Cancel, details |
| `installing`, `starting`, `handshaking`, `activating` | Installing / Starting / Checking helper… | Named steps; no invented percentage | Cancel where safe, details |
| Helper `ready`, pane awaiting spawn | Opening terminal… | Final pane-specific step | Cancel; setup details remain available |
| Terminal attached | Working terminal | Remove setup surface | Normal terminal controls |
| `failed` | Could not prepare remote terminal | Failed step remains visible; completed steps retained | Recovery actions determined by error |
| `cancelled` | Setup cancelled | Neutral, not an error toast | Resume setup, close pane |
| Policy/version conflict | Action required | No infinite spinner | Review settings, manual setup, cancel |

`awaiting_consent` is a paused state, not a failed attempt. A failed operation remains inspectable until retried or dismissed. Retry increments its attempt identity and revalidates current remote state; retained checklist history is labeled “Previous attempt” rather than shown as fresh verification.

### 4.3 Failure card and progressive disclosure

```text
Could not install the remote helper
Ferryx cannot write to /home/user/.ferryx/bin.
No existing terminal sessions were stopped.

[Retry after fixing permissions] [Manual setup]
[Open logs]                     [Host settings]

Details ▾
Stage: helper_permissions
Code: SSH_HELPER_PERMISSION_DENIED
Operation: …    Attempt: 2
```

Use the “No existing sessions were stopped” sentence only for operations that did not execute an explicit destructive user action. An SSH drop can leave remote cleanup unconfirmed; say “Connection lost; existing sessions were not intentionally stopped” rather than claiming an observed remote state.

Logs open a structured drawer with stage timestamps, byte counts, sanitized command exit information, operation ID, and recovery guidance. Keep diagnostics separate from terminal output: installation logs must not enter the shell's PTY stream, history, or 512 KiB replay buffer. Allow “Copy diagnostic summary,” with a preview and redaction enabled by default.

### 4.4 Native terminal composition and readiness

Do not mount/attach `NativeTerminalPane` to a provisional backend ID. Gate native surface visibility, focus, and keyboard forwarding until a valid backend session exists. A CSS z-index alone is not the contract for native surfaces. On reconnect, retain existing session/output state and coordinate the setup/reconnect overlay with native visibility rather than destroying its session. [R9]

Surface precedence is: explicit trust/consent action, setup failure/progress, existing reconnect/lost-session affordance, normal terminal. Exactly one primary recovery surface is visible. A provisioning failure before first spawn must not fall into the current disconnected/exited-session overlay merely because `backendSessionId` is null.

Helper readiness means both bridge connections have validated the expected protocol and matching host/owner/epoch. Terminal readiness means creation/attachment has succeeded and renderer attachment is acknowledged; **do not wait for first terminal output**, because a valid shell may be silent. Restore keyboard focus only when this pane is still active; an inactive pane finishing setup must not steal focus.

### 4.5 Accessibility and interaction requirements

Use `role="status"` and polite live announcements for phase changes, not every progress tick. Transfer progress exposes a labeled progressbar with byte-based value; omit a numeric value for indeterminate phases. Announce a failure once and move focus only when necessary to expose the primary recovery action. Respect reduced motion and high-contrast themes.

All actions must be keyboard accessible with visible focus and at least 32 px interactive height in the compact desktop layout. Target 4.5:1 text contrast. Escape closes a details/manual modal, not the installation itself; cancellation requires an explicit action. Do not automatically focus a Copy button or announce raw stderr. Store message keys and interpolation data so labels, time units, errors, and copy feedback can be localized.

## 5. SSH host settings and preferences

Add a **Remote helper** card per host, using the existing Settings primitives rather than a second settings framework. [R8]

```text
Remote helper                                     Ready
Installed: 2026.908.1     Protocol: 1
Target: Linux arm64      Source: Bundled with Ferryx

Automatically install when needed                 [On]
Automatically update compatible helpers           [On]
Release channel                                   [Stable ▾]
Helper executable override                        [           ]
                                                  [Validate]

[Prepare / Check now] [View logs] [Manual setup]
```

| Setting | Default and semantics |
| --- | --- |
| Automatic installation | Explicitly on for new hosts with disclosure; missing legacy setting means ask. Off allows probing/using an existing helper but never an automatic write. |
| Auto-update helper | On for managed installations. Governs replacement selection, not first-install consent. Off leaves compatible versions running; an incompatible version requires a clear manual approval. |
| Release channel | Stable by default. Beta is opt-in and visibly badged. Channel selects signed, desktop-compatible artifacts; it never means an unchecked mutable “latest” binary. |
| Custom helper path override | Empty by default. An **absolute remote executable path**, not a local file, URL, shell command, or string of arguments. Show “Externally managed”; disable managed install/auto-update controls while set. |
| Check now | Re-probes and offers the appropriate update/prepare action; it does not bypass consent, restart daemons, or downgrade a newer compatible helper. |
| Restore managed helper | Explicitly clears an override for future sessions; never deletes the custom file or rewrites existing session descriptors. |

The custom-path field is validated according to the detected remote filesystem. Disallow control characters, relative paths, executable arguments, and unexpected symlinks/reparse points. A custom executable is executable code chosen by the user: require explicit acknowledgement before first execution, validate its bootstrap/protocol response, and do not silently fall back to another binary if it fails. A custom compatible helper on an unbundled target is an expert-managed exception, not a newly supported packaged target.

Persist the policy as an optional versioned `helper` object on each host, with serde/TypeScript migration coverage. Suggested fields: `policyVersion`, `installMode: "automatic" | "ask" | "manual"`, `autoUpdate`, `channel: "stable" | "beta"`, `remoteExecutableOverride`, and consent provenance. Keep last observations and transfer progress out of `ssh_hosts.json`; they belong in a separate cache/snapshot store. Preserve tombstones and unrelated host fields when migrating. [R1]

Distinguish connectivity from preparation in every badge: **Not checked**, **Connected · Not prepared**, **Preparing**, **Ready**, **Ready · Update pending**, **Needs attention**, **Externally managed**. Display observation age; a cached Ready label is not proof that the remote is currently reachable.

Beta availability depends on signed inventory. When its artifact is not bundled/cached, ask before retrieving it on the desktop, or explain that it is unavailable offline. No remote downloads or background polling of every saved host are required. Host deletion removes local preferences and stops new operations but does not delete remote binaries or kill remote sessions. Remote uninstall is a separate, explicitly confirmed future action.

## 6. Backend architecture and ownership

### 6.1 Components

```text
React: SSH settings / provisional pane / workspace status
                 | short commands + snapshots
                 v
Tauri IPC facade + targeted event relay + resource resolver
                 | authenticated local daemon protocol
                 v
Local Ferryx daemon
  HelperProvisioningService
    - host/policy revision validation and consent
    - operation registry, consumer leases, cancellation, deadlines
    - target and artifact selection
    - SSH streaming, remote verification, installation, readiness
    - snapshot persistence and sanitized event journal
                 |
                 | existing validated OpenSSH transport; no PTY
                 v
Remote account
  private staging -> verified installer -> immutable helper build
                                         -> detached runtime(s)
                                         -> framed stdio bridges
```

The local daemon is the owner because the existing terminal path already traverses it, and it can outlive a renderer/window. Tauri's process resolves resources, then registers a validated immutable asset bundle with the daemon. Do not let React pass arbitrary local helper paths into the automatic path. [R5, R6]

The daemon must not depend on having `AppHandle`. Introduce a `HelperAssetProvider` interface and a `VerifiedAssetBundle` descriptor containing a manifest digest, app version/channel, and a backend-controlled cache root. Tauri copies verified resources into an account-private, content-addressed local cache before registration. The daemon independently verifies the signed manifest and hashes when opening assets. This also prevents an application updater from removing an in-use resource halfway through transfer.

Add explicit local-daemon protocol capability negotiation for `sshHelperProvisioningV1`. An old local daemon without the capability produces an actionable “Restart Ferryx to enable remote setup” result; it cannot silently acknowledge new commands. App/daemon cache registration is authenticated, revision-bound, and never accepted from a web-remote client. Inventory and policy travel through the existing daemon request/event architecture, not a second unauthenticated port.

### 6.2 Service interfaces and terminal integration

Proposed Rust service responsibilities:

```rust
// Interface sketch, not code currently present in the repository.
ensure_helper_ready(intent, consumer, policy_revision) -> OperationSnapshot
get_operation(operation_id) -> OperationSnapshot
release_consumer(operation_id, consumer_id) -> CancelOutcome
retry_operation(operation_id, expected_attempt, consumer) -> OperationSnapshot
wait_ready(operation_id, cancellation) -> ReadyHelperRef
```

A `ReadyHelperRef` binds host configuration revision, trusted remote account identity, target triple, verified artifact digest, exact executable, runtime root, protocol, owner/epoch, and a readiness lease. It is never just `true` or an untrusted path string. Creation rechecks readiness/identity when opening bridges; a prior success event is not authority to skip a handshake.

Route first-use `RemoteRuntime::create` and the existing bridge startup through this service, passing a provisioning context rather than letting each bridge initiate its own install. Keep restore intent separate: select the executable/root pinned in the saved descriptor, repair only the needed artifact if policy permits, and validate the original target. Preserve `clientRequestId` across all retries. The service never calls `pty.spawn` itself. [R6]

Migrate `cmd_ssh_install_project_helper(workspace_id, local_binary)` into an explicit advanced-import adapter using the same verifier/installer. Resolve workspace to host and preserve its compatibility signature during migration, but do not make it the automatic renderer API. Arbitrary imported binaries require an explicit user trust path; official artifacts must match signed inventory.

### 6.3 Operation identity and concurrency

Use separate identities for different concerns:

| Identity | Purpose |
| --- | --- |
| `hostId` + `hostRevision` | Logical host configuration and policy; revision includes effective SSH connection settings |
| `operationId` | One ensure/provision job, reused by its consumers |
| `producerEpoch` + `attempt` + `sequence` | Event freshness across daemon restart, retry, and reordering |
| `consumerId` | A pane/settings action's cancellation interest |
| `clientRequestId` | Exactly one logical PTY creation; not an installation attempt ID |
| `artifactDigest` | Immutable bytes and deduplicated local/remote installation |
| Remote host/owner/epoch + runtime root | Existing daemon/session identity, preserved across reconnection |

Before detection, deduplicate by host ID/revision and compatible requested policy. After discovery, serialize installations by verified account home/root and target; remote filesystem locking remains authoritative across aliases, other desktop installations, and other computers. Never share daemons across differing host IDs just because binaries are shared.

Use short registry mutex sections; do not hold a global lock while doing SSH I/O. Two consumers asking for incompatible channels/required protocols may share the verified artifact cache, but they do not inherit each other's chosen version. Host or policy edits mark an old operation superseded; old events and activation attempts are rejected by revision checks.

## 7. Provisioning state machine, safety, and recovery

### 7.1 State transitions

```text
queued -> connecting -> detecting -> probing
                                  | existing compatible runtime
                                  +---------------------> handshaking -> ready
                                  | mutation needs authorization
                                  +-> awaiting_consent --approval--+
                                  |                               |
                                  +-> resolving -> preflighting <-+
                                         -> transferring -> verifying
                                         -> waiting_lock -> installing
                                         -> starting -> handshaking
                                         -> activating -> ready

Any active state -> failed(reason, recovery actions)
Any cancellable state -> cancelling -> cancelled
Retry -> new attempt -> connecting/probing (never blindly repeat activation)
Host revision change -> cancelled(reason: superseded)
```

A known compatible installed binary can skip transfer/install and proceed to start/handshake. A staged valid artifact can skip retransfer after a reconnect, but only after fresh size/hash/path checks. Optional upgrade deferral produces Ready with a warning, not a permanent blocked spinner.

Installation and activation are different: installation publishes immutable verified bytes; activation selects a successfully handshaken runtime for future sessions. A failure between them leaves the old selection intact. A failed PTY create after helper readiness is a terminal failure, not a request to reinstall the helper.

### 7.2 Target probing before artifact selection

Extend environment detection with structured fields: `os`, normalized `arch`, `pointerWidth`, `endianness`, kernel/Windows build, libc family/version when applicable, home, executor, and PTY capability prerequisites. Keep executor version separate from OS version; the current Windows `version` is a PowerShell version. [R3]

For POSIX, use bounded, nonce-framed probes for `uname -s`, `uname -m`, and available libc information such as `getconf GNU_LIBC_VERSION`; absence is unknown, not proof of glibc. Do not execute an arbitrary existing remote binary simply to discover libc. Accept only tested normalizations: `x86_64`/`amd64` to x86_64, `aarch64`/`arm64` to aarch64. Confirm a compatible 64-bit userland and loader; CPU alone is insufficient for a 32-bit or unusual container image.

For Windows, derive OS architecture rather than assuming the architecture of a PowerShell process. Check OS build and ConPTY availability; distinguish native Windows from WSL by the environment actually reached through SSH. Windows ARM64 emulation is not silently included in x64 support. No compilation or dependency installation is performed to satisfy missing prerequisites.

Use bounded response sizes and the existing quoting/encoded-command helpers. MOTD/login noise may precede a nonce-delimited probe, but malformed or oversized responses must fail clearly. Resolve home on the remote host, not from the desktop environment. Refresh observations after SSH/configuration changes and always validate helper identity at bridge creation.

### 7.3 Verified staging and installation

The transaction is deliberately more than the existing `cat > tmp; mv` sequence. [R2]

1. Resolve a manifest entry for the detected target and desktop protocol/capability requirements. Verify the manifest signature, byte length, and local SHA-256 before any remote transfer or execution. Treat artifacts as data on the desktop; do not attempt to execute a Linux helper on macOS to learn its target.
2. Check the remote managed root and its ancestors for allowed ownership, private access, symlink/reparse-point hazards, writability, and available storage. Require space for the new staging file plus the installed copy and reserve; account for quotas and inode exhaustion. Estimates are advisory, so write failures still need typed handling.
3. Create an unpredictable, exclusive staging file under a validated account-private `.ferryx/staging` directory. Never stage under a repository or a world-writable predictable path. Linux creation starts with `umask 077`; Windows creates a private directory/file with current-user SID-based ACLs. Preserve justified SYSTEM/Administrators access; do not grant broad Users/Everyone access.
4. Stream a bounded local file reader to the existing non-PTY SSH stdin transport. Use a bounded buffer, backpressure, concurrent bounded stdout/stderr drains, cancellation, and an idle timer. Do not load the entire helper into a `Vec<u8>` or send binary content through React/Tauri events. SFTP can later implement the same adapter interface, but is not required for the first release.
5. Close/flush input and require remote length and SHA-256 confirmation. Linux verification can use an available trusted `sha256sum`, `shasum`, or OpenSSL command; Windows can use `Get-FileHash`/a .NET hash stream. When no remote hash tool is available, hash a bounded streaming readback over SSH or report `SSH_HELPER_VERIFIER_UNAVAILABLE`; do not execute the unverified candidate to verify itself.
6. Only after verification, invoke the candidate's proposed `install-local --no-start` command. This new helper subcommand acquires an OS-held account-wide install lock, revalidates paths/hash, copies itself into an immutable version directory, syncs files, and publishes an installation receipt. It reuses the helper's existing portable locking approach, not a lease inferred from a stale PID file. This is a new implementation requirement, not an existing CLI. [R7]
7. Under that lock, reuse an already installed identical artifact; reject a differing file at a digest-qualified destination. Never truncate an installed executable, follow a substituted symlink, or overwrite a running Windows `.exe`. Linux files/directories receive mode 700 where appropriate; private metadata uses 600. Windows uses explicit ACL validation rather than POSIX chmod or account-name interpolation.
8. Release the installation lock, start the selected exact build at its selected runtime root, and validate both bridge connections. Publish the new active selection only after readiness. A proposed helper `activate` command performs an OS-locked, compare-and-swap metadata update; it cannot select an unverified/unhealthy candidate merely because a frontend supplied a path.
9. Remove this operation's staging file after the installer exits. Keep a bounded receipt/journal sufficient to reconcile a lost connection. Cleanup never deletes installed builds or another operation's files.

The install lock and daemon runtime lock have different purposes. An OS lock is released when its holder exits; a persistent lock filename alone is not “busy.” If another installer holds the lock, emit `waiting_lock`, then re-probe/reuse its result. Do not automatically steal a lock by age or delete it based on a PID alone.

### 7.4 Timeouts, retries, and cancellation

These are initial configurable engineering budgets, to be tuned with fault-injection evidence rather than presented as predicted completion times:

| Operation | Initial bound | Recovery policy |
| --- | --- | --- |
| SSH connection establishment | Preserve the current 5 s connection timeout per attempt | Retry only recognized transient failures |
| Environment detection | Preserve the current 12 s overall probe budget initially | Separate unsupported executor from connection failure |
| Metadata/version/hash-tool probe | 10 s; bounded output | Fail with the specific failed probe |
| Remote lock wait | 30 s | Re-probe after owner finishes; offer retry on timeout |
| Transfer | 30 s no-progress timeout; 5 min total | At most two automatic transport retries with jittered 1 s/3 s backoff |
| Verification/install | 60 s each, excluding lock wait | Do not retry integrity/path failures automatically |
| Daemon start | 15 s | Check existing endpoint before attempting another start |
| Each bridge handshake | 10 s | Bounded connection cleanup on failure |
| Whole automated attempt | 10 min, excluding explicit consent waiting | Cancel child processes and return actionable timeout |

The whole-attempt deadline dominates retries. Increase transfer limits through a bounded advanced policy for slow links, not an infinite timeout. Do not retry password/authentication failures, changed host keys, unsupported targets, permissions, full disks, or checksum failures on a timer. Expose “Retry” only when it has a useful meaning.

For SSH interruption, kill/reap owned local SSH children, reconcile remote staging/install receipts on reconnect, and restart transfer from byte zero unless a later resumable protocol proves an offset and prefix hash. The first release does not claim resumable uploads. Killing an SSH installer must not kill an independently running helper daemon.

Cancellation releases a consumer lease. When the last lease disappears, cancel owned work, except a user-authorized proactive preparation retains its operation lease until explicitly cancelled. If cancellation arrives during a short atomic commit, finish that commit and report `cancelled` with `installedButNotActivated` metadata. A started but unused helper may remain idle; do not kill it if another client could have attached. Cleanup failures are recorded as warnings with safe next-attempt cleanup, not concealed as successful remote deletion.

Daemon restart invalidates producer epoch and returns saved in-flight snapshots as interrupted, followed by remote reconciliation. UI remount alone does not restart the job. Application exit/restart may stop local transport; the remote transaction must remain idempotent, and existing remote PTYs remain independent.

## 8. Binary bundling and release packaging

### 8.1 Initial support matrix

| Remote target | Bundled filename | Requirements and exclusions |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `ferryx-remote-helper` | 64-bit Linux/glibc userland, compatible loader/kernel, writable executable account directory |
| `aarch64-unknown-linux-gnu` | `ferryx-remote-helper` | Little-endian 64-bit Linux/glibc userland; independently verified arm64 build |
| `x86_64-pc-windows-msvc` | `ferryx-remote-helper.exe` | Native Windows x64, usable PowerShell executor, ConPTY, compatible runtime dependencies |

Choose MSVC for the initial Windows artifact, not an ambiguous MSVC-or-GNU runtime choice. Pin its toolchain and either statically link the permitted CRT dependencies or document and probe every required redistributable. Prove operation on a clean Windows host without developer tools.

The **proposed product baseline** is Linux glibc 2.28/kernel 4.18 or newer, subject to release qualification of the actual artifact. Record effective minimum versions and required shared libraries in the manifest; a target triple alone is not a compatibility guarantee. Rust's platform-support documentation is a reference for target identities, not proof of this project's dependency floor. [E4]

Windows ConPTY documents Windows 10 version 1809 and Windows Server 2019 as its API minimums; Ferryx's qualified support matrix may be narrower. Check the real API and artifact dependencies rather than assuming every Windows SSH server supports it. [E5]

FreeBSD, OpenBSD, NetBSD, macOS remotes, MIPS, 32-bit targets, big-endian arm64, Linux musl/Alpine, and Windows ARM64 have no automatic artifact in this initial matrix. Show the detected target and supported options before uploading. Do not substitute the desktop's architecture or a “near enough” Linux binary.

### 8.2 Resource layout and resolution

Proposed build staging layout:

```text
src-tauri/resources/helpers/
  manifest.json
  manifest.json.minisig
  x86_64-unknown-linux-gnu/ferryx-remote-helper
  aarch64-unknown-linux-gnu/ferryx-remote-helper
  x86_64-pc-windows-msvc/ferryx-remote-helper.exe
```

Each desktop package, including macOS, carries **all three remote targets**. These are remote payloads, not desktop sidecars. Tauri's `externalBin` convention is for target-specific binaries used with the application; resource files are the correct packaging mechanism here. [E1, E2]

Extend the existing resource map without removing frontend resources:

```json
{
  "bundle": {
    "resources": {
      "../ui/dist": "ui/dist",
      "resources/helpers/": "helpers/"
    }
  }
}
```

Use a directory mapping, not an object-map glob that could flatten identically named binaries from different target directories. Tauri documents directory structure preservation and backend resolution via `app.path().resolve(..., BaseDirectory::Resource)`. Resolve `helpers/manifest.json` and manifest-selected relative artifact paths with that API; never construct paths from the current working directory, an assumed `.app` layout, or a renderer-supplied target string. [E1]

Validate paths against an allowlisted target table, canonical containment, ordinary file type, and expected length/hash. Runtime-selected resources are read-only. Signed/cached copies go to app cache, not back into the installed bundle. Test merged platform configs and final installed package layouts, including MSIX/AppImage resource access.

Release packaging fails if any required target or its metadata is missing. Development startup must not download/build helpers implicitly: use an explicitly prepared fixture/asset directory and a debug-only configuration override, or show “Helper assets unavailable in this development build.” A release build must reject unsigned development inventories and debug override sources. Preserve the existing clean-dev resource contract. [R10]

### 8.3 Signed manifest contract

Use a helper manifest separate from desktop `latest.json`. Illustrative schema (all placeholders must be generated before release):

```json
{
  "schemaVersion": 1,
  "releaseSequence": 123,
  "helperVersion": "2026.908.1",
  "sourceCommit": "<full-commit-sha>",
  "channel": "stable",
  "bootstrapSchema": 1,
  "supportedProtocols": [1],
  "capabilities": ["sshHelperV1", "helperBuildInfoV1", "managedInstallV1"],
  "artifacts": [
    {
      "target": "x86_64-unknown-linux-gnu",
      "path": "x86_64-unknown-linux-gnu/ferryx-remote-helper",
      "byteLength": 0,
      "sha256": "<64-lowercase-hex-digits>",
      "minimumRuntime": { "libc": "glibc", "glibc": "2.28", "kernel": "4.18" },
      "requiredLibraries": [],
      "downloadUrl": "<optional-immutable-https-artifact-url>"
    }
  ]
}
```

The example shows one entry to illustrate the schema; a shipping manifest requires exactly the supported inventory, positive byte lengths, real dependency metadata, and matching files for all three targets. Do not interpret `requiredLibraries: []` as evidence of static linkage. Windows entries carry a Windows minimum-build/dependency record instead of libc fields.

Sign the exact manifest bytes with a dedicated pinned helper-release key. Hashes alone detect accidental changes but do not authenticate a replaced manifest. App signing/updater signatures remain separate; existing updater artifact signing does not automatically verify an independently downloaded helper manifest. Key rotation requires a release-approved trust transition; an unknown key is not accepted from the same download it signs. Signed `releaseSequence`, channel, compatibility, and revocation metadata prevent automatic rollback to an older disallowed build.

Bundled bytes are the default. An explicit desktop download may use only signed, compatible inventory over HTTPS, enforce response-size/redirect/time bounds, verify before cache publication, and stay inside approved distribution origins. Expired or unverifiable online metadata does not invalidate a still-approved bundled artifact, but a known revoked artifact cannot be silently reused. No shell executes release metadata or a remote URL.

### 8.4 Local release pipeline integration

Extend the existing immutable release plan with helper version, source commit, three target triples, toolchain/sysroot identifiers, signing-key ID, and helper inventory digest. The dependency order is essential:

```text
prepare immutable release plan
  -> build or fetch all helper targets
  -> verify native target smoke evidence + sign helper inventory
  -> distribute the identical verified helper inventory to desktop builders
  -> build/sign each desktop bundle with those resources
  -> validate build receipts and embedded inventory
  -> assemble desktop latest.json + separate helper assets/manifest
  -> publish immutable assets, then publish discoverability metadata
```

Build the existing standalone crate rather than the full Tauri application:

```text
cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target x86_64-unknown-linux-gnu
cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target aarch64-unknown-linux-gnu
cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target x86_64-pc-windows-msvc
```

These are target-specific builder commands, not a claim that a macOS machine can run all three successfully without the corresponding toolchains. Linux builds need pinned compatible sysroots/linkers or native builders. Windows MSVC should use a qualified Windows builder. Cross-compilation success is not runtime evidence; use native remote test hosts for final PTY/disconnect validation. The full source snapshot must include the shared `src-tauri/src/ferryx_scope/ssh` sources referenced by the standalone manifest. [R7]

A prebuilt artifact is acceptable only when its signed receipt binds target, source commit, toolchain, helper version, hash, and smoke evidence to the immutable plan. Never fetch a floating release by name. Cache build output outside checked-in source; stage verified resources into the isolated release checkout.

Concrete integration work:

- Add helper build/fetch/inventory preparation to `scripts/release-local.mjs` before desktop build dispatch. Preserve its prohibition on mutating CI release stages. Extend `scripts/lib/release-contract.mjs` and `scripts/lib/release-platforms.mjs` rather than bypassing their inventories. [R11]
- Extend receipts with helper inventory digest and separate helper artifact kinds or a separately validated helper-receipt set. Update strict expected-file validation deliberately; do not drop a new receipt into a directory that currently rejects it.
- Extend `scripts/build-latest-json.mjs` to validate/package helper assets and signed manifest separately. Keep desktop updater platform keys and signature semantics unchanged. Assembly must prove all desktop bundles embed the same planned helper inventory.
- Synchronize the standalone crate version with app release metadata, or introduce an explicit independently versioned helper field. Version equality is convenient for attribution, not the protocol compatibility rule. Sign Windows artifacts before computing their final hashes.

## 9. Version contract and non-destructive rolling upgrades

### 9.1 Distinguish four versions

The desktop app version, helper build version, bootstrap metadata schema, and PTY wire protocol are different contracts. Do not compare date-based version strings lexicographically or require exact app/helper version equality. Use parsed release versions plus signed release ordering, and explicit supported protocol/capability sets.

Add a bounded, side-effect-free `--version --json` command to the standalone helper. Proposed output includes `bootstrapSchema`, `helperVersion`, `sourceCommit`, compiled target triple, `supportedProtocols`, and capabilities. It must not start a daemon or open a PTY. Existing helpers do not expose this contract; a failed version probe should fall back once to the existing trusted protocol-1 handshake, marking build identity as legacy/unknown rather than declaring the executable absent. [R7]

Add helper build metadata to the runtime handshake. Preserve existing `protocol`, `capabilities`, `hostId`, `ownerId`, `epoch`, OS, and architecture fields. Protocol negotiation chooses a mutually supported protocol with all required capabilities. Both independent bridge connections must select the same runtime identity and negotiated protocol. Static binary metadata is not proof that the currently running daemon is that binary. [R6]

Keep bootstrap metadata schema 1 backward-readable while introducing future wire protocols. The current startup parser and helper liveness probe are hardcoded to protocol 1; update them and their tests together, rather than only extending frontend types. Unknown additive JSON fields are tolerated; unsupported protocol major changes are explicit errors. [R2, R7]

### 9.2 Managed layout and legacy migration

```text
~/.ferryx/
  bin/ferryx-remote-helper[.exe]              # existing compatibility location
  bin/helpers/<target>/<version>-<digest>/
      ferryx-remote-helper[.exe]             # immutable managed executable
  staging/<operation-nonce>.partial
  install.lock                              # OS-held lock, not a lease marker
  helper/<host-slug>/                        # existing legacy runtime root
  helper/<host-slug>/runtimes/<build-id>/     # new independently pinned runtime
  helper/<host-slug>/selection-p<protocol>.json
```

Windows uses the remote account's equivalent paths and private ACLs. Build IDs/path components are generated from validated manifest data and bounded to keep filesystem/socket paths practical; overlong runtime socket paths produce a specific diagnostic, not an unrelated startup timeout.

Keep the well-known executable for compatibility with existing installations. On a fresh managed install it may be populated when absent; thereafter **it is not the authority for new managed activation**. Do not auto-overwrite it while legacy users or a running Windows executable may depend on it. New clients resolve the versioned path through verified selection metadata; old persisted descriptors continue to use the legacy path/root. The UI's details show the actual active path, not an assumption that the fixed path always contains the newest build.

This is an intentional evolution of `default_location`, not an invisible filesystem change. Add `resolve_managed_location` and retain a legacy fallback. New descriptors persist build ID, exact executable/root, negotiated protocol, and identity; missing fields deserialize as legacy. Never rewrite a restored descriptor to the newest root simply because an update exists.

### 9.3 Selection policy

| Observed state | Automatic action | User-visible result |
| --- | --- | --- |
| Missing helper, automatic install allowed | Install selected bundled/cached artifact | Setup progress then ready |
| Exact/compatible approved build already available | Reuse after real handshake | Ready; no transfer |
| Older compatible runtime with live sessions | Keep old sessions; optionally stage/start newer build for new sessions | Ready; old sessions preserved |
| Newer compatible helper than bundled | Reuse if allowed by signed policy; do not downgrade | Ready |
| Incompatible helper, auto-update allowed | Install/start compatible build side by side | New terminals use new runtime |
| Incompatible helper, updates disabled | Do not write until explicit approval | Update required; approve or manual setup |
| Externally managed override | Probe only; never replace automatically | Ready or custom-helper action required |
| Revoked/known unsafe build | Block new use; show explicit remediation | No silent fallback to revoked artifact |
| Old restored runtime unsupported by this desktop | Leave it running; preserve descriptor | Explain compatibility limit and recovery choices |

“Rolling upgrade” here means routing future sessions to a new daemon while existing sessions drain naturally. It does **not** mean migrating PTY handles, process ownership, or ring-buffer state between processes. Preserve the existing 512 KiB sequence/cursor replay contract on each original runtime.

Retain at least the active build, a last-known-good build, and every build referenced by any live/detached session or retained descriptor. Reference uncertainty means retain, not garbage-collect. A managed daemon can later support an authenticated drain/admission barrier: reject new spawns, then exit only after all sessions and retention leases are gone. `pty.list` returning zero is not by itself safe authorization to stop a daemon because another client can race a new spawn. Legacy helpers without the barrier are never automatically stopped for cleanup.

Support the previous wire protocol in a new desktop release whenever still-supported sessions may use it. Removal requires an explicit compatibility/deprecation release policy and user notice. Multiple desktop versions must not ping-pong a global current binary; selections are protocol-aware, monotonic for automatic upgrades, and descriptors are immutable.

### 9.4 Atomic activation and rollback

Publish active selection metadata only after both bridge handshakes succeed. The activation command locks, rechecks host/policy/selection generation, and atomically replaces a small metadata file. Use an appropriate Windows atomic replacement API for existing metadata, not “delete then rename”; creation uses exclusive create. Linux uses same-filesystem staging, file sync, rename, and parent-directory sync where supported.

If upload/install/start/handshake fails, leave the previous selection untouched. A rejected candidate can remain quarantined with a diagnostic until safe cleanup. If activation succeeds but the desktop misses its acknowledgement, re-read the receipt and selection instead of starting a second runtime blindly. A rollback may select a previously approved compatible build for **future sessions**; it must not destroy the failing runtime if sessions have already been created there. Security revocations override last-known-good fallback.

## 10. Tauri IPC, events, and frontend state

### 10.1 Proposed public commands

All commands below are proposed additions except the existing install adapter. Names are stable design targets, not currently available endpoints.

| Command | Input | Result/semantics |
| --- | --- | --- |
| `cmd_ssh_ensure_helper` | `hostId`, `expectedHostRevision`, `consumerId`, `intent`, `requestId` | Validates host and policy; returns an operation snapshot promptly. `intent` is prepare/create/restore; restore is bound to an existing descriptor by the backend. |
| `cmd_ssh_get_helper_status` | `hostId`, expected revision | Cached helper observation plus current operation; read-only |
| `cmd_ssh_get_helper_operation` | `operationId` | Authoritative current snapshot for race/restart recovery |
| `cmd_ssh_approve_helper_action` | operation/attempt, scoped consent choice | Authorizes the displayed mutation for the current revision only |
| `cmd_ssh_retry_helper` | operation ID, expected attempt, consumer ID | Re-probes in a new attempt; rejects stale actions |
| `cmd_ssh_cancel_helper` | operation ID, consumer ID | Releases caller's consumer and returns remaining-consumer/cleanup state |
| `cmd_ssh_get_helper_logs` | operation ID, bounded cursor/page size | Sanitized bounded diagnostic pages |
| `cmd_ssh_get_helper_manual_guide` | host/revision and selected artifact identity | Safe rendered instructions, hashes, prerequisites, or explicit unavailability |

Expose these only to authorized desktop windows; restrict the backend command registry/capabilities and validate inputs even when the renderer is trusted. Do not expose helper bytes, arbitrary local paths, private keys, SSH agent sockets, or remote endpoint tokens. Expensive network/filesystem work must not execute on the Tauri main/UI thread.

### 10.2 Progress event contract

Event name: **`ssh_helper_provision_progress`**. Use the same schema for snapshots and progress. Proposed TypeScript contract:

```ts
type HelperProvisionPhase =
  | "queued" | "connecting" | "detecting" | "probing"
  | "awaiting_consent" | "resolving" | "preflighting"
  | "transferring" | "verifying" | "waiting_lock" | "installing"
  | "starting" | "handshaking" | "activating"
  | "ready" | "failed" | "cancelling" | "cancelled";

type RecoveryAction =
  | "retry" | "open_logs" | "manual_install" | "edit_host"
  | "review_trust" | "approve_update" | "choose_custom_path";

interface HelperProvisionError {
  code: string;
  stage: string;
  messageKey: string;
  retryable: boolean;
  recoveryActions: RecoveryAction[];
  details: {
    target?: string;
    remotePath?: string;
    requiredBytes?: number;
    availableBytes?: number;
    exitCode?: number;
    diagnosticId: string;
  };
}

interface HelperProvisionSnapshot {
  schemaVersion: 1;
  operationId: string;
  producerEpoch: string;
  hostId: string;
  hostRevision: string;
  attempt: number;
  sequence: number; // monotonically increasing safe integer within producer epoch
  phase: HelperProvisionPhase;
  messageKey: string;
  completedSteps: string[];
  target: string | null;
  helperVersion: string | null;
  artifactDigest: string | null;
  source: "bundled" | "verified_cache" | "download" | "custom" | null;
  bytesSent: number;
  bytesTotal: number | null;
  percentage: number | null; // transfer percentage only, not total setup completion
  bytesPerSecond: number | null;
  elapsedMs: number;
  remainingConsumers: number;
  cancellable: boolean;
  updatedAt: string;
  warnings: string[];
  error: HelperProvisionError | null;
}
```

`bytesSent` measures bytes accepted into the local SSH transfer stream, not a guarantee of remote durable writes. Never label it “remote bytes verified.” The verifier's successful result is a separate completed step. Use an exponentially smoothed transfer rate after enough samples; before that, show no speed. Reset transfer progress for a new attempt and label the retry. Optional ETA is omitted until stable and never drives readiness.

Emit state transitions immediately and throttle transfer updates to at most four per second per operation. Coalesce intermediate updates for slow consumers; terminal snapshots are retained. Keep event payloads small, with no binary data or raw command output.

Tauri documents that events are not a high-throughput stream and do not supply fine-grained event-data capability isolation. Use low-rate events for sanitized progress, emit only to subscribed authorized desktop windows, and never use broad global events for sensitive logs. The logs command is bounded; a future live-log channel must be separately authorized. Channels are an option for a dedicated ordered stream, not a replacement for snapshots or durable operation ownership. [E3]

### 10.3 Listener race, reordering, and remount protocol

Register the app-level event listener before calling ensure. A new view then calls ensure/get-status and merges the returned snapshot with any buffered events. Accept only the current host revision/operation/producer epoch and the newest `(attempt, sequence)`. Duplicate and stale events are ignored.

When the daemon epoch changes, discard ordering assumptions and fetch a snapshot. On reconnect/window remount, fetch status before rendering cached success as current. If no progress/heartbeat arrives for 5 seconds, query the snapshot; do not infer failure from a dropped event. Terminal completion is both in the operation registry and in a final event, so losing the final event cannot strand a spinner.

The service persists sanitized stage transitions and final results, not every byte tick. Bound active operations, event journals, and log pages. A reasonable initial policy is 256 recent stage events per operation, 2 MiB of sanitized logs, and seven days of completed-operation retention, subject to local privacy settings. Enforce caps in the backend, not only in the view.

### 10.4 Frontend file structure and spawn race prevention

Proposed additions:

```text
ui/src/lib/sshHelper.ts                        # typed commands/event adapter
ui/src/state/sshHelperStore.ts                 # per-operation snapshot reducer
ui/src/components/ssh/HelperProvisionOverlay.tsx
ui/src/components/ssh/HelperProgressChecklist.tsx
ui/src/components/ssh/HelperErrorCard.tsx
ui/src/components/ssh/HelperLogsDrawer.tsx
ui/src/components/ssh/ManualHelperInstallDialog.tsx
```

Integrate with `SshSection`, `SshWorkspaceStatus`, `TerminalPane`, `NativeTerminalPane`, and `workspaceStore`. Create explicit pending-pane state rather than overloading `remoteConnectionState="disconnected"`. Keep setup-operation identity separate from a session's remote generation/epoch. [R8, R9]

After helper readiness, only the owner of the original logical action may issue the terminal spawn. Repeated ready events, Strict Mode remounts, Retry clicks, and late operation responses cannot spawn twice. Before applying a late spawn result, confirm that the pane/consumer still exists; if a create succeeded after user cancellation, reconcile the exact newly created session under the existing cancellation contract, never close an unrelated restored session.

All spawn entry points, including batched spawn and app-level direct calls, must use backend readiness gating. UI preflight improves visibility but is not a correctness boundary. Provisioning retries never mint a new `clientRequestId` for an ambiguous existing create. [R5, R6, R9]

## 11. Error taxonomy and actionable recovery

Return structured errors through `IpcError` and the local-daemon failure path without flattening them into strings. Preserve code, stage, retryability, operation/attempt correlation, and allowlisted details. The UI renders localized copy from codes/message keys; it must not search English stderr for “Permission denied.” Use OS error values, known bootstrap markers, SSH exit/diagnostic classification, and conservative unknown-error fallback.

Map legacy `CLI_EXECUTABLE_NOT_FOUND` plus `stage: helper_missing` to provisioning-required only when it is specifically the helper lookup, not when local `ssh` or another executable is missing. Preserve `stage: helper_permissions` for permission failures even if the operation phase was preflight/install/start. A bridge error must retain this context through `RemoteFailure`. [R2, R4, R6]

| Code / diagnostic stage | User-facing explanation | Recovery and retry rule |
| --- | --- | --- |
| `SSH_HELPER_TARGET_UNSUPPORTED` / `target` | “Automatic setup is not available for FreeBSD/MIPS/this target.” | Show detected target and initial matrix; manual/custom expert option only when suitable. Do not repeat an identical upload. |
| `SSH_HELPER_RUNTIME_UNSUPPORTED` / `runtime` | “This Linux runtime/Windows build cannot run the bundled helper.” | Show detected and required libc/loader/ConPTY values; upgrade remote environment or use qualified custom build. |
| `SSH_HELPER_DISK_FULL` / `helper_storage` | “There is not enough space to install the helper.” | Show required/available bytes when reliable; explain quota/inodes may also be exhausted. Retry after freeing space; never delete projects or live builds. |
| `SSH_HELPER_READ_ONLY` / `helper_storage` | “The installation filesystem is read-only.” | Host settings/custom executable on an allowed volume; no automatic remount. |
| `SSH_HELPER_PERMISSION_DENIED` / `helper_permissions` | “Ferryx cannot write or execute in this account's helper directory.” | Display exact owned path; manual guide and settings. Do not suggest `sudo chmod -R 777` or recursive ownership changes. |
| `SSH_HELPER_NOEXEC` / `helper_permissions` | “This filesystem does not allow programs to run here.” | Explain that chmod may not help; manually place a compatible helper on an executable private volume and configure override. |
| `SSH_HELPER_TRANSFER_INTERRUPTED` / `transfer` | “SSH disconnected while transferring the helper.” | Bounded transport retry, then Retry/Open logs; old installation remains valid. |
| `SSH_HELPER_INTEGRITY_FAILED` / `verify` | “The helper did not match its verified release.” | Do not execute; quarantine/remove only the candidate, show logs and trusted-source recovery. No unsigned fallback or automatic loop. |
| `SSH_HELPER_ASSET_UNAVAILABLE` / `resolve` | “This app installation is missing the required helper asset.” | Repair/update desktop, import verified artifact, or approved desktop retrieval; no guessed URL. |
| `SSH_HELPER_VERIFIER_UNAVAILABLE` / `verify` | “The remote file could not be independently verified.” | Use streaming readback or manual verified installation; never run unverified code. |
| `SSH_HELPER_INSTALL_BUSY` / `install_lock` | “Another installation is using this helper directory.” | Wait/re-probe within bounds; Retry later. Do not delete a live lock. |
| `SSH_HELPER_START_FAILED` / `startup` | “The helper was installed but did not start.” | Show sanitized loader/ACL/endpoint diagnostics; retain previous selection, allow re-probe. |
| `SSH_HELPER_PROTOCOL_MISMATCH` / `handshake` | “This helper is incompatible with this Ferryx version.” | Approved side-by-side update or custom-helper correction; preserve running sessions. |
| `SSH_HELPER_UPDATE_REQUIRED` / `policy` | “A compatible helper is required; automatic updates are off.” | Approve this update, change preference, manual setup, cancel. |
| `SSH_HELPER_UNSAFE_PATH` / `path_validation` | “The helper path has unsafe ownership or links.” | Explicit manual repair/custom path; never follow or chmod the suspicious target. |
| `SSH_HOST_KEY_UNTRUSTED` / `ssh_trust` | “Verify this SSH host before Ferryx connects.” | Open SSH trust instructions; no automatic acceptance. |
| `SSH_HOST_KEY_CHANGED` / `ssh_trust` | “The host's SSH identity changed.” | Security warning and administrator verification; never auto-remove `known_hosts` entries. |
| `SSH_AUTH_FAILED` / `ssh_auth` | “SSH authentication failed.” | Edit identity/jump host or unlock SSH agent; no helper install attempt. |
| `SSH_HELPER_TIMEOUT` / actual phase | “Setup stopped because this step did not finish.” | Show timed-out step, elapsed time, operation ID, and phase-specific retry. |
| `SSH_HELPER_STATE_CHANGED` / `revision` | “Host settings changed during setup.” | Refresh and retry with the new settings; no stale activation. |

Do not promise unsupported manual binaries exist. For an unsupported target, the guide may show detection results and custom-build requirements but must disable “Copy install command” when no validated matching artifact is available.

## 12. Manual installation, offline fallback, and trust

### 12.1 Manual guide modal

The modal has **Install from a verified release**, **Use a local artifact**, and **Diagnose access** sections. It displays the host/remote account, detected target, helper version, SHA-256, exact destination, and whether remote Internet access is needed. The user sees where to run a command: “Run in a shell on the remote host,” not ambiguously on the desktop.

The app generates commands only from verified immutable manifest data and safely quotes interpolated values. Show “Copy command,” a copy confirmation, “Verify installation,” and a return-to-pane action. Copying is never execution. Verify re-runs backend checks and resumes the original waiting terminal request only after readiness.

No `curl | sh`, `Invoke-Expression`, disabled TLS verification, automatic privilege escalation, automatic host-key acceptance, or unreviewed script execution is part of the guide. The bootstrap executable is run only after comparison against the digest from metadata authenticated by the desktop.

### 12.2 Linux command template

The following is a **rendering template**, not a claim that these release URLs or the new `install-local` command exist today. Phase 2 supplies that CLI; Phase 4 renders real signed metadata and tests the scripts. Shipping UI must never enable Copy with unresolved `{{...}}` values. The rendered command requires `curl` and `sha256sum`; other verifier choices need their own tested template.

```sh
set -eu
umask 077
work="$(mktemp -d "${TMPDIR:-/tmp}/ferryx-helper.XXXXXX")"
trap 'rm -f "$work/helper"; rmdir "$work" 2>/dev/null || true' EXIT HUP INT TERM
curl --fail --location --proto '=https' --proto-redir '=https' \
  --tlsv1.2 --connect-timeout 15 --max-time 300 \
  --output "$work/helper" '{{artifactUrl}}'
printf '%s  %s\n' '{{sha256}}' "$work/helper" | sha256sum --check --status -
chmod 700 "$work/helper"
"$work/helper" install-local \
  --managed-root "$HOME/.ferryx" \
  --expected-sha256 '{{sha256}}' --no-start
printf 'Helper installed. Return to Ferryx and choose Verify installation.\n'
```

The verified installer enforces the same immutable destinations, owner checks, locking, and no-replacement-of-running-binaries policy as automatic setup. The temporary directory must permit execution; on a noexec temp filesystem the guide offers a validated private executable staging location, not instructions to remount it. The manual installer does not start a shell, select a runtime for an unknown host ID, or terminate old sessions.

### 12.3 Windows PowerShell command template

This is likewise generated only for a matching signed Windows artifact and a helper implementing `install-local`. Use a tested HTTPS distribution route; the checksum remains pinned even when the download endpoint redirects.

```powershell
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ('ferryx-helper-' + [Guid]::NewGuid().ToString('N'))
$file = Join-Path $work 'ferryx-remote-helper.exe'
[IO.Directory]::CreateDirectory($work) | Out-Null
try {
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    & icacls $work /inheritance:r /grant:r "*${sid}:(OI)(CI)F" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot secure the temporary helper directory.' }
    Invoke-WebRequest -UseBasicParsing -Uri '{{artifactUrl}}' -OutFile $file -MaximumRedirection 5
    $actual = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne '{{sha256}}') { throw 'Helper checksum mismatch. Nothing was installed.' }
    & $file install-local --managed-root (Join-Path $HOME '.ferryx') --expected-sha256 '{{sha256}}' --no-start
    if ($LASTEXITCODE -ne 0) { throw 'Helper installation failed; review the diagnostic above.' }
    Write-Output 'Helper installed. Return to Ferryx and choose Verify installation.'
} finally {
    if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force }
    if (Test-Path -LiteralPath $work) { [IO.Directory]::Delete($work, $false) }
}
```

The installer copies verified bytes into a new version path; it does not move or overwrite its running `.exe`. Do not recommend changing global PowerShell execution policy, turning off security software, or weakening the account directory ACL to make setup succeed.

### 12.4 Offline and locked-down hosts

Preferred fallback remains desktop-to-remote SSH upload using bundled or verified cached assets. Remote `curl`/PowerShell download is unnecessary for normal setup. “Use a local artifact” opens an explicit desktop file picker and verifies the chosen file against a signed manifest for the detected target before routing it through the same installer.

For a fully offline transfer outside Ferryx, let the user export the selected helper plus signed metadata and instructions; the remote script uses the already copied file instead of HTTP. Enterprise/custom artifacts use the externally managed path and explicit trust acknowledgement rather than pretending to have an official signature. A read-only/noexec home can use a manually installed compatible executable on a suitable private volume; do not automatically move installation into `/tmp` or a shared repository.

Unknown SSH host trust is resolved outside automatic provisioning through explicit verification and the user's normal SSH workflow. Preserve OpenSSH's strict known-host behavior and do not quietly enable password prompting/agent forwarding to make setup succeed. [R4, E6]

## 13. Implementation phasing and work breakdown

The phase boundaries below are delivery slices, not permission to ship an unsafe intermediate automatic installer. Agree on UI copy/state contracts first; develop the shared event/error fixtures alongside backend work. Automatic setup remains feature-gated until the end-to-end release criteria pass.

### Phase 1: Bundling and asset resolution in the Tauri backend

| Work item | Concrete files/modules | Acceptance evidence |
| --- | --- | --- |
| P1.1 Define inventory schema, trust root, version/protocol requirements, target table | New `src-tauri/src/ssh/helper_assets.rs`; extend standalone metadata in `src-tauri/src/ferryx_scope/ssh/process.rs`/`helper.rs` | Valid/invalid signatures, paths, lengths, target mismatches, unknown schema, old handshake fixtures |
| P1.2 Build all initial helpers independently of Tauri | `remote-helper/Cargo.toml`, shared standalone sources; new `scripts/build-remote-helpers.mjs` | Three target receipts, reproducible source/toolchain attribution, native version/PTY smoke results |
| P1.3 Stage resources and resolve backend paths | `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`, new asset provider; `clean_dev_resource_contract.rs` | All three artifacts resolvable from each installed desktop package; missing/corrupt asset fails closed; clean dev remains usable |
| P1.4 Extend local release plan/receipts/assembly | `scripts/release-local.mjs`, `scripts/build-latest-json.mjs`, `scripts/lib/release-contract.mjs`, `scripts/lib/release-platforms.mjs` | Packaging fails on missing/extra/mismatched helpers; identical manifest digest across desktop receipts; desktop updater schema unchanged |

**Exit:** verified assets can be selected by a remote target without network downloads and without executing remote binaries locally. No automatic remote mutation is enabled yet.

### Phase 2: Auto-provisioning backend state machine and event stream

| Work item | Concrete files/modules | Acceptance evidence |
| --- | --- | --- |
| P2.1 Rich target probe and policy migration | `src-tauri/src/ssh/runtime.rs`, `src-tauri/src/ssh/mod.rs`, `src-tauri/src/ipc/ssh.rs` | Linux x64/arm64/Windows detection fixtures; musl/BSD/MIPS rejection; legacy policy defaults to ask |
| P2.2 Daemon-owned operation service and IPC relay | New `src-tauri/src/ssh/helper_provision.rs`; `src-tauri/src/daemon/protocol.rs`, `client.rs`, `server.rs`; `src-tauri/src/ipc/ssh.rs`, `lib.rs` | Single-flight, consumer cancellation, revision fencing, snapshots, event ordering, old-daemon capability fallback |
| P2.3 Bounded transfer and verified remote installer | `src-tauri/src/ssh/direct.rs`, `helper_setup.rs`; new `src-tauri/src/ferryx_scope/ssh/install.rs` and CLI dispatch | Real byte progress, bounded memory, remote hash/readback, OS lock contention, crash consistency, private paths/ACLs, no running executable overwrite |
| P2.4 Readiness gate and versioned runtime routing | `src-tauri/src/terminal/remote.rs`, `src-tauri/src/ssh/bridge.rs`, helper process/handshake code; IPC terminal routing | Create waits for ready; restore never spawns; dual connection identity retained; immutable descriptor migration |
| P2.5 Upgrade/activation safety | Provisioner and installer; remote descriptor serialization | Side-by-side upgrade with live detached sessions; rollback/CAS; mixed desktop versions; last-known-good preservation |

**Exit:** a headless test consumer can prepare a fresh supported host, observe all phases, cancel/retry safely, and obtain readiness without creating a PTY. Current application functionality remains behind the existing path until UI integration is complete.

### Phase 3: Frontend components, progress overlays, and settings

| Work item | Concrete files/modules | Acceptance evidence |
| --- | --- | --- |
| P3.1 Typed IPC and app-level snapshot store | New `ui/src/lib/sshHelper.ts`, `ui/src/state/sshHelperStore.ts` | Listener-before-ensure race, out-of-order events, epoch change, terminal event loss, remount recovery |
| P3.2 Provisional panes and native-surface gating | `ui/src/state/workspaceStore.ts`, `ui/src/App.tsx`, `TerminalPane.tsx`, `NativeTerminalPane.tsx` | Immediate placeholder, no premature backend attach, no focus theft, exactly-once spawn under Strict Mode/retry/close races |
| P3.3 Shared checklist/error/log components | New `ui/src/components/ssh/*`; integrate `SshWorkspaceStatus.tsx` | Every phase/error fixture, narrow splits, keyboard actions, reduced motion, accessibility checks |
| P3.4 Host-add preparation and preferences | `SshSection.tsx`, `ui/src/lib/sshHosts.ts` and associated tests | Save/test never installs; opt-in preparation survives Settings close; settings migration/invalidation; custom-path mode |

**Exit:** first terminal open and proactive preparation share one visible backend operation, with production copy and no UI-only correctness dependency.

### Phase 4: Verification, error recovery, and manual fallback

| Work item | Concrete files/modules | Acceptance evidence |
| --- | --- | --- |
| P4.1 Generated guides and official-artifact import | Manual guide backend, `ManualHelperInstallDialog.tsx`, existing explicit-install adapter | Real rendered Linux/PowerShell commands executed on disposable hosts; checksums/quoting/placeholders tested; offline import parity |
| P4.2 Backend/SSH integration regression suite | Extend `helper_setup_tests.rs`, `bridge_tests.rs`, `src-tauri/src/daemon/ssh_survival_tests.rs`; new provisioning tests | Interrupted transfer, full disk, read-only/noexec, lock contention, stale revision, active-session upgrades |
| P4.3 Frontend and native renderer regression suite | Extend SSH settings/reconnect/exit-attach tests; new helper store/overlay tests | Recovery button correctness, shared consumer semantics, native occlusion/input, multi-window and batch-open races |
| P4.4 Installed-package release qualification | Existing release test infrastructure plus helper inventory tests | All remote targets from each desktop family; clean hosts, offline remote, update/rollback, support-bundle redaction |
| P4.5 Controlled rollout and support readiness | Feature flag, local diagnostics, release checklist, user documentation | Opt-in qualification -> beta -> stable; rollback disables future automatic mutations without killing running helpers |

**Exit:** all release gates in Section 14 pass, generated manual commands reference actual immutable assets, and support can diagnose failures from a redacted operation summary.

## 14. Verification matrix and release gates

### 14.1 Required scenario coverage

| Area | Minimum scenarios and assertions |
| --- | --- |
| Fresh setup | Empty `.ferryx` on Linux x64, Linux arm64, Windows x64; exactly one install and one requested PTY; no remote Internet/compiler required |
| Warm setup | Compatible helper already running; no upload, no daemon restart, real handshake; silent shell still becomes usable |
| Unsupported target | BSD/MIPS/musl/32-bit/Windows ARM64; diagnostic before transfer and no fabricated manual command |
| Storage/access | ENOSPC, quota exhaustion, inode exhaustion, read-only mount, noexec, non-executable file, ownership mismatch, Windows ACL denial/file lock; old selection intact |
| Transfer integrity | Drop SSH at several byte offsets, truncate/corrupt candidate, wrong local target, changed manifest, oversized input; candidate never executed before verification |
| Filesystem attacks | Symlink/reparse/hard-link hazards, substituted staging path, unsafe ancestor, traversal in manifest, spaces/quotes/Unicode in HOME/path; no writes outside allowed roots |
| Concurrency | Ten panes, two windows, two desktop processes, two computers, and alias hosts sharing an account; lock correctness and no cross-host runtime identity leakage |
| Cancellation | One consumer cancels, all cancel, proactive settings view closes, cancel during commit, pane closes during spawn; no unrelated session is stopped |
| Crash/restart | Desktop/daemon exits during transfer, install, start, and activation; reconcile immutable receipts, no false Ready from persisted cache |
| Event delivery | Missing initial/final event, duplicate/reordered events, old attempt after retry, epoch reset, host edit/delete; store converges from snapshot |
| Versions | Legacy protocol-1 helper without version CLI; newer compatible helper; required protocol upgrade; updates off; custom override; beta offline; revoked artifact; no downgrade loop |
| Session survival | Keep a command writing sequenced output while SSH disconnects and another client upgrades; reattach old target and verify cursor behavior, gaps, and 512 KiB buffer semantics |
| Restore safety | Missing executable versus missing runtime versus changed epoch; repairing transport never calls create/`pty.spawn` for a restored target |
| UI/native | Small split, inactive pane completion, keyboard-only flow, high contrast/reduced motion, Settings navigation, native overlay input suppression; no dead blank pane |
| Packaging | Each desktop family includes all three targets; paths work after installation/update; no dependence on developer checkout/CWD; signatures/hashes verified after packaging |
| Manual/offline | Rendered scripts run on supported remote shells, fail before execution on wrong digest, avoid replacing a running `.exe`, and resume original pane only after Verify succeeds |

Use deterministic fake transports and clocks for most tests, then disposable SSH VMs/hosts for OS behavior. Permission/noexec/disk-full tests must not modify production accounts. Record clean Windows ConPTY/ACL evidence and real Linux arm64 PTY/disconnect evidence; QEMU or a cross-compiler alone is not final qualification.

### 14.2 Regression commands and evidence ownership

During implementation, use the repository's established runners and focused tests, including `cargo test --manifest-path remote-helper/Cargo.toml`, scoped backend/helper tests, existing SSH survival tests, UI Vitest tests, and release-script tests. Run formatting/type checks for changed Rust/TypeScript at each phase. Choose exact installed-package and live-host commands from the existing harness rather than inventing an environment-independent live test command.

For this design-only change, verification is limited to document completeness, references, examples, formatting, and scoped Git inspection. Writing this specification does not constitute a successful three-platform build, live SSH test, or implemented UI.

### 14.3 Measurable quality gates

Treat these as acceptance targets to measure, not established performance:

- The provisional pane appears on the next UI render; a long network operation never blocks the UI thread. Phase changes reach a healthy subscribed UI within 250 ms in local integration tests.
- Transfer memory is bounded independently of artifact size; initial per-operation buffer budget is at most 1 MiB, excluding explicitly bounded logs/protocol frames. Update rate is at most four progress events per second.
- Opening ten simultaneous panes produces one logical provisioning operation per compatible host revision; all intended creates retain distinct stable request IDs and none are duplicated.
- Automated fault tests show zero unverified candidate executions, zero implicit stops of existing PTYs, and zero remote writes from Save/Test connection.
- Warm-start and fresh-start latency distributions are recorded by phase and target, with regressions compared against the established test baseline. Do not promise one universal setup duration across SSH links.

A feature flag may disable future automatic provisioning while leaving probes, explicit manual setup, and existing sessions available. Rollout proceeds through disposable-host qualification, opt-in users, beta, then stable. No rollout control forcibly uninstalls helpers or terminates sessions.

## 15. Security, operations, and remaining qualification decisions

The threat model covers compromised downloads/manifests, untrusted renderer input, unrelated remote users, unsafe writable directories, stale operations, and accidental cross-host execution. A fully compromised SSH account or root on the remote host can lie about execution and modify that account's files; checksum verification is not remote attestation. State that boundary rather than claiming cryptographic proof of a trustworthy remote machine.

Keep diagnostics local by default. Redact secrets, SSH key paths where unnecessary, endpoint tokens, usernames/hostnames/home paths in exported summaries, and raw environment data. Do not collect PTY content for provisioning telemetry. An opt-in aggregate report can include phase, target family, error code, source, duration, and retry count. Keep signed release metadata and local installation receipts available for incident diagnosis without exposing authentication material.

Before enabling stable automatic setup, resolve and record these qualification choices:

| Decision | Default in this design | Required evidence/owner |
| --- | --- | --- |
| Linux runtime floor and shared libraries | Proposed glibc 2.28/kernel 4.18 | Release engineering proves actual linked requirements on oldest supported x64/arm64 images |
| Windows runtime distribution | MSVC artifact with qualified dependency strategy | Windows clean-host ConPTY, ACL, CRT, and executable-lock tests |
| Helper signing and rotation | Dedicated pinned key; exact-byte signed manifest | Security/release ownership, offline signing/rotation procedure, rejection tests |
| Immutable helper download URLs | Bundled default; optional signed immutable endpoints | Release engineering publishes real assets before enabling generated download commands |
| Local daemon protocol migration | Explicit `sshHelperProvisioningV1` capability | Backend integration and app/daemon version-skew tests |
| Runtime drain/cleanup policy | Retain referenced or uncertain runtimes; no legacy auto-stop | Backend proves admission-barrier safety before enabling automatic retirement |
| Store-distributed desktop packages | Resources remain remote payloads; no assumed exemption | Package/signing qualification and applicable distribution review before release |

These are implementation release gates, not blockers to publishing this specification. The safe baseline remains usable without optional online updating or automatic garbage collection: bundled artifacts, explicit consent policy, immutable installations, backend snapshots, and pinned session identities.

## 16. Reference notes

Repository references R1–R11 are defined in Section 2 and identify the inspected implementation baseline. Proposed modules, commands, paths, and capabilities elsewhere are explicitly implementation work rather than claims about existing APIs.

External primary references, consulted 2026-09-10:

- **E1 — Tauri v2, Embedding Additional Files.** Resource directory mappings, preservation of hierarchy, and `BaseDirectory::Resource` resolution. <https://v2.tauri.app/develop/resources/>
- **E2 — Tauri v2, Embedding External Binaries.** Desktop sidecar/external-binary conventions, contrasted with remote resource payloads. <https://v2.tauri.app/develop/sidecar/>
- **E3 — Tauri v2, Calling the Frontend from Rust.** Event throughput/type/capability limitations and channels. <https://v2.tauri.app/develop/calling-frontend/>
- **E4 — The rustc book, Platform Support.** Rust target identities and platform qualification context. <https://doc.rust-lang.org/rustc/platform-support.html>
- **E5 — Microsoft Learn, CreatePseudoConsole.** ConPTY API minimum client/server requirements. <https://learn.microsoft.com/en-us/windows/console/createpseudoconsole>
- **E6 — OpenBSD/OpenSSH, ssh_config(5).** Host-key checking, batch authentication, forwarding, and connection configuration semantics. <https://man.openbsd.org/ssh_config>
