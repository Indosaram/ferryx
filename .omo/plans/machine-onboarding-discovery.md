# Plan: machine onboarding (SSH auto-install, install key, approval) + in-machine workspace discovery

Status: decision-complete. Source research: docs/research/machine-onboarding-workspace-discovery-2026-10-23.md.
Branch: feat/machine-onboarding-20261023, worktree /Volumes/T9-Mac/project/ferryx-wt/onb-20261023 (base origin/main ba14711b).

## Security model (binding for every lane)
- An install key is a 32-byte random token with the prefix `fxk_` (`fxk_<64 hex>`). It is shown exactly once and stored only as sha256 in `enrollment_codes` with `kind = 'install_key'`.
- An install key is bound to the issuing user and to the account origin.
- Install key TTL: default 3600 s; the request may ask for any value from 600 to 86400 s, and values outside that range are clamped.
- Install keys are single-use: a successful enroll sets `consumed_at` and `consumed_machine_id` and keeps the row for audit.
- Refusals:
  - reuse: 409 `INSTALL_KEY_USED`
  - expired: 401 `INSTALL_KEY_EXPIRED` (the row is deleted)
  - unknown key: 401 `ENROLL_CODE_INVALID` (unchanged)
  - plan-limit refusal: does NOT consume the key (same as today's code)
- Each user may hold at most 10 outstanding unconsumed keys: 429 `INSTALL_KEY_LIMIT`. When billing is on, an early `may_enroll_new` check refuses issuing with 402 `PLAN_LIMIT_REACHED`.
- Cross-account: a key always enrolls into its issuer's account. If the machine_id is already owned by another user, enroll returns 409 `ACCOUNT_MACHINE_CLAIMED` (unchanged path).
- Secrets never go into argv:
  - CLI: `--key-stdin` or `FERRYX_INSTALL_KEY`.
  - install.sh: `--key` is read by the shell script and passed to the CLI via env, never as an argument.
  - SSH path: the key goes over ssh stdin.
- Approval requests (RFC 8628 shape):
  - The CLI posts its machine identity (machineId, publicKey, displayName, platform, appVersion, optional `email` hint). The server stores `requester_ip` (first `cf-connecting-ip` or `x-forwarded-for` value, else "unknown") and returns `deviceCode` (secret, stored hashed), `userCode` (8 chars from `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`, `XXXX-XXXX`), `verificationUri = {origin}/approve`, `verificationUriComplete = {origin}/approve?code=XXXX-XXXX`, `expiresIn 900` and `interval 3`.
  - The owner binds by userCode (lookup), sees name, OS, IP, appVersion and created time, then approves or denies. The decline action is the default and visually primary.
  - If the request carried an email hint, only that user may look it up or approve it; anyone else gets 403 `MACHINE_REQUEST_FOREIGN_ACCOUNT`.
  - Once a request is decided, approve or deny returns 409 `MACHINE_REQUEST_DECIDED`.
  - If the machine_id is owned by another user, approve returns 409 `ACCOUNT_MACHINE_CLAIMED`.
  - Approval mints an enrollment code bound to (user, machine_id, public_key) and stores it hashed. Enroll with a bound code refuses any other machine_id or key with 401 `ENROLL_CODE_MACHINE_MISMATCH`.
  - Poll is delivered once: the plaintext code is returned once and then cleared.
- Rate limits:
  - machine-requests create: 10/min per IP and 1000 pending globally (429 `RATE_LIMITED`)
  - poll: faster than `interval` returns 400 `slow_down`
  - lookup: 20/min per user (429)
- The email magic-link device flow (`account login --email`) stays unchanged as the fallback.

## Wire contract (account service, JSON camelCase, errors are the existing `{code,message,details}`)
| Method | Path | Auth | Body → Response |
|---|---|---|---|
| POST | /api/account/v1/install-keys | bearer | {label?, ttlSecs?} → {keyId, key, expiresAt, installCommand:{sh, ps1}} |
| GET | /api/account/v1/install-keys | bearer | → [{keyId, label, createdAt, expiresAt, consumedAt?, consumedMachineId?}] (never the key) |
| POST | /api/account/v1/install-keys/{keyId}/revoke | bearer | → 204 |
| POST | /api/account/v1/machine-requests | none | {machineId, publicKey, displayName, platform, appVersion, email?} → {deviceCode, userCode, verificationUri, verificationUriComplete, expiresIn, interval} |
| POST | /api/account/v1/machine-requests/poll | none | {deviceCode} → {status:"pending"} \| {status:"approved", enrollmentCode} \| {status:"denied"}; expired → 400 MACHINE_REQUEST_EXPIRED |
| GET | /api/account/v1/machine-requests | bearer | → pending requests whose email hint is this user's email (for the app card) |
| POST | /api/account/v1/machine-requests/lookup | bearer | {userCode} → {requestId, machineId, displayName, platform, appVersion, requesterIp, createdAt, expiresAt} |
| POST | /api/account/v1/machine-requests/{requestId}/approve | bearer | {userCode} → {machineId} |
| POST | /api/account/v1/machine-requests/{requestId}/deny | bearer | {userCode} → 204 |

`installCommand.sh` = `curl -fsSL {origin}/install.sh | sh -s -- --key {key}`. `installCommand.ps1` = `& ([scriptblock]::Create((irm {origin}/install.ps1))) -Key {key}`.
The enroll endpoint is unchanged on the wire: the CLI sends the install key or approved code as `enrollmentCode`. The kind is looked up from the stored row.

## Daemon discovery contract
- Capability `repoDiscoveryV1`, under the same Machine+Control condition as `directoryBrowseV1` (server.rs ~3859).
- `GET /api/v1/fs/repositories?path=<abs|~>&depth=<1..4, default 3>&includeHidden=<bool>` returns `{path, repos:[{repoRoot, isLinkedWorktree, gitCommonDir, branch, registeredWorkspaceId}], truncated, elapsedMs}`.
- Walk rules:
  - breadth-first from the resolved root (`filesystem::resolve_directory`)
  - stop descending at any dir that has `.git` (a dir or a file)
  - never follow symlinks
  - skip `node_modules`, `target`, `.cache`, `vendor`, `dist`, `build`, `Library`, `.Trash`, and hidden dirs unless includeHidden
- Caps: 3 s budget, 20 000 entries, 200 repos, depth ≤ 4. Exceeding any cap sets `truncated:true`; nothing errors.
- branch comes from `HEAD` parsing only (no git subprocess per repo). A linked worktree is a `.git` file whose `gitdir:` points into `<common>/worktrees/`.
- `registeredWorkspaceId` is joined against the machine catalog by canonical root.
- The walk runs in `run_blocking`. It shares the `fs/directories` rate limiter and revocation select.
- Relay `validate_http_query` allows path, depth and includeHidden for this route.
- The desktop gets `paired_host` Operation `DiscoverRepos{path,depth,includeHidden}` plus a map_result arm. The UI gets `discoverRepos` in `pairedDaemonProject.ts` and a decoder in `pairedDaemonContracts.ts`.
- UI: `PairedDaemonProjectForm` shows "Discovered repositories" above the folder picker when the capability is present.
  - Rows show repoRoot, branch and a linked-worktree badge.
  - "Open" on a registered row calls `onRegistered` with the existing project. "Add & open" calls `registerProject` and then `onRegistered`.

## CLI and service contract
- `ferryx-cli up [--key-stdin | --key-env] [--email <hint>] [--origin URL] [--name NAME] [--no-service] [--json]`, in this order:
  1. Load or create the identity.
  2. If already enrolled to this origin, skip enrolling.
  3. Otherwise, with a key, enroll.
  4. Otherwise create a machine request, print the approval URL and userCode (stderr: `Approve this machine: <verificationUriComplete>` and `Code: XXXX-XXXX`), poll until approved, denied or expired, then enroll.
  5. Unless --no-service, install the service.
  6. Print a JSON summary when --json: {accountId, machineRecordId, relayOrigin, service:{backend, supervised, installed, linger?, pid?, reason?}}.
- Exit codes: 0 ok, 2 usage, 3 key used or expired or invalid, 4 denied, 5 approval expired, 6 plan limit, 1 other.
- `ferryx-cli service install|status|uninstall [--json]`:
  - install calls `daemon::supervisor::ensure()` when `supervisor::supported()`.
  - On Linux it also tries `loginctl enable-linger $USER` and reports `linger`.
  - When unsupported (no systemd user manager, or PID 1 not systemd), it falls back to spawning a detached `ferryx-cli --daemon` (setsid, nohup, log at `<data dir>/daemon.log`). It skips the spawn when the daemon socket already answers, and reports `supervised:false, reason`.
  - It never kills a running daemon.
- install.sh and install.ps1 (in `scripts/`, which the relay embeds; `site/public/` copies kept byte-identical):
  - accept `--key K`, `--email E`, `--no-up`, and env `FERRYX_INSTALL_KEY`
  - after installing the binary, run `FERRYX_INSTALL_KEY=K ferryx-cli up --key-env [--email E] --origin $FERRYX_ORIGIN`
  - with no key, still run `ferryx-cli up` (approval flow) unless `--no-up`, or when stdin is not a TTY and no key or email is given; then print the next step
  - print typed failures and do not delete the installed binary

## SSH auto-install contract
- New `#[tauri::command] cmd_ssh_install_ferryx_node(app, host_id, account_origin, install_key) -> SshNodeInstallReport{cliPath, version, enrolled:{accountId, machineRecordId}?, service:{supervised, backend, reason?}}` in `ipc/ssh.rs`, registered in `lib.rs`.
  - It takes stored host_id only.
  - It runs over `direct::ssh_plan` + `bounded_output_with_stdin` with a 180 s timeout.
  - Remote script: `curl -fsSL {origin}/install.sh | FERRYX_ORIGIN={origin} sh -s -- --key-stdin --json` with the key on stdin. install.sh reads one stdin line when `--key-stdin` is given.
  - It parses the last JSON line of stdout.
  - Errors are typed IpcError codes `SSH_NODE_INSTALL_FAILED` with details {stage, exitCode, stderrTail}.
- UI AddMachineModal SSH tab: after save, if signed in, offer "Install Ferryx and add to account". It issues a key (POST install-keys), invokes the command, shows the stages, then polls `listMachines` until the machine is online (60 s).

## UI contract
- `ui/src/remote/accountSession.ts` gets `issueInstallKey`, `listInstallKeys`, `revokeInstallKey`, `lookupMachineRequest`, `approveMachineRequest`, `denyMachineRequest` and `listPendingMachineRequests`, using the existing fetch/error helpers.
- `AddMachineModal` METHODS becomes `install` (default for signed-in users: one-liner with copy buttons for sh/ps1 plus a TTL note), `approve` (enter code → approval card), `ssh` and `import`. The `enrollment` code method stays reachable as "Enrollment code (manual)".
- `MachineApprovalCard.tsx` (new) renders name, OS, IP, app version and expiry, with Deny (default focus) and Approve buttons. Typed errors map to messages by code.
- `RemoteSection`:
  - while the modal is open or a request is pending, polls `listMachines` every 5 s plus `listPendingMachineRequests` (bounded, stops on close)
  - shows pending requests with an email hint as approval cards
- Web approval page: RemoteApp route `/approve` (reads `?code=`), signs in with the existing AccountLoginPage when no token, then shows the same `MachineApprovalCard`.

## Waves and disjoint write scopes
- **W1** (parallel):
  - **L-key**: `src-tauri/src/account/{service.rs, store.rs, store_sqlite/*}` and new `src-tauri/src/account/install_keys.rs`, `machine_requests.rs` and their `*_tests.rs`; router entries in service.rs.
  - **L-disc**: new `src-tauri/src/remote/repo_discovery.rs`, plus `remote/server.rs` (route and capability only), `remote/relay_server.rs` (`validate_http_query` only), `remote/machine_protocol.rs`, `paired_host/client.rs`, `ui/src/lib/pairedDaemonProject.ts`, `ui/src/lib/pairedDaemonContracts.ts`, `ui/src/components/PairedDaemonProjectForm.tsx` and tests.
  - **L-cli**: `src-tauri/src/cli.rs` (up/service parse+run), `src-tauri/src/bin/cli.rs` dispatch, `src-tauri/src/account/enroll_client.rs` (machine-request client fns), `scripts/install.sh`, `scripts/install.ps1`, `site/public/install.{sh,ps1}`.
- **W2** (after W1 merges into the branch):
  - **L-ssh**: `src-tauri/src/ipc/ssh.rs`, `src-tauri/src/lib.rs` (one line), `ui/src/lib/sshHosts.ts`.
  - **L-ui**: `ui/src/remote/accountSession.ts`, `ui/src/components/settings/AddMachineModal.tsx`, `ui/src/components/settings/RemoteSection.tsx`, new `ui/src/components/settings/remote/MachineApprovalCard.tsx`, `ui/src/remote/RemoteApp.tsx` (approve route; hooks above the auth early return), a devtools QA page `ui/onboarding-approval-qa.html` + `ui/src/devtools/approvalQaMain.tsx` for screenshots, and tests.
- **W3**: QA on noye (`ferryxqa@100.107.15.119`, Debian 13, no systemd) against an isolated relay+account instance from this branch, deploy, merge, cleanup.

## Tests (each lane owns its own; workers do not run cargo/bun; the orchestrator runs the gates)
- **L-key:**
  - key issue → enroll ok
  - reuse → 409 INSTALL_KEY_USED
  - expired → 401 INSTALL_KEY_EXPIRED
  - key of A → machine owned by A; machine owned by B → 409 ACCOUNT_MACHINE_CLAIMED
  - outstanding limit 429
  - list never returns the key
  - request → lookup by code → approve → poll returns code once → enroll ok
  - foreign approver with email hint → 403
  - decided → 409
  - bound code with another machine_id → 401 ENROLL_CODE_MACHINE_MISMATCH
  - deny → poll denied
  - slow_down
  - mutation: removing the consumed check makes the reuse test RED
- **L-disc:**
  - a fixture tree with nested repos, a linked worktree, a symlink loop, node_modules containing a repo, a hidden repo, and depth 5
  - asserts depth cap, ignore rules, symlink not followed, linked worktree flagged, truncated on an entry cap, branch parsed
  - capability listed only under Machine+Control
  - relay query allowlist
  - vitest for the form list (Open vs Add & open, capability absent hides it)
- **L-cli:** parse tests for up/service/key-stdin/exit-code mapping; install.sh tested by an existing or new `scripts/*.test.mjs` harness if present, else a shell syntax check (`sh -n`).
- **L-ssh:** remote script builder unit test (key never in argv, JSON line parsing, typed errors).
- **L-ui:** vitest for accountSession calls, the modal install/approve tabs, the approval card (deny default focus, error codes), and the RemoteApp /approve route; screenshots at 1280 and 390 from the QA page.

## Verification gates (orchestrator, this Mac)
- `cargo test --lib account:: remote::repo_discovery remote::relay_server:: paired_host:: cli:: ipc::ssh`
- `cargo clippy --all-targets -- -D warnings`
- `bun run --cwd ui test <touched>`
- `bunx tsc -p ui --noEmit`
- `bun run --cwd site build` (site/public touched)

## QA (noye, isolated)
- Build a Linux `ferryx-cli` / `ferryx-relay` for glibc ≤ 2.41 on omarchy with `cargo zigbuild` or a Debian 13 docker image, since omarchy's native build needs GLIBC_2.43+ and noye has 2.41.
- Run a QA relay+account from this branch on omarchy (separate data dir, port 18787, loopback-only, reached through an SSH tunnel), with `FERRYX_DOWNLOADS_DIR` serving the QA CLI.
- On noye as ferryxqa:
  - install key: one-liner → enrolled + daemon running (supervised:false) → reuse refused → expired refused
  - approval: `ferryx-cli up` with no key prints URL/code → owner approves via API → enrolled → foreign approver refused
  - SSH path: the desktop IPC script run against `ferryxqa@noye`
  - discovery: fixture tree under ~ferryxqa → `fs/repositories` via the QA relay → expected rows
- Each step's evidence goes in the notepad.

## Deploy / merge / cleanup
- Merge the branch into main in a dedicated merge worktree, then push. The main checkout has foreign dirty files and is never reset.
- Deploy the relay binary built from main to omarchy `/home/indo/bin/ferryx-relay` (backup first), then `systemctl restart ferryx-relay`.
- curl checks: health 200; `POST install-keys`, `GET machine-requests` and `POST machine-requests/lookup` without a token return 401 (not 404); `POST machine-requests` without a body returns 400.
- Never touch any `ferryx --daemon`.
- Remove the worktrees, branch, cargo targets, RUN_TMP, the noye ferryxqa user and the omarchy QA relay data, with receipts.

