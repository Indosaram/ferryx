# Cloud Hosts (`ferryx-cli cloud`)

Disposable cloud sandboxes managed from `ferryx-cli` and exposed as local RPC sockets so existing `omo host session` / `mhc host session` commands dispatch work to them unchanged.

## What it does

- Provisions, lists, stops, resumes, and deletes sandboxes across six providers through one `SandboxProvider` trait (`src-tauri/src/cloud_host/provider/`).
- Bootstraps each Linux x86_64 box with a checksummed, pinned runtime (`omo-ai@5.1.79` / engine `2026.11.18-3`, pinned Node `v24.22.3` when image Node < 24) behind an install lock and `.install-complete` marker (`src-tauri/resources/cloud-host/bootstrap.sh`).
- Pushes only filtered `hms` credentials (`~/.omo/agent/auth.json`, `models.json`, `settings.json`, all mode `0600`) — never OAuth tokens or other providers' keys.
- Connects over two transports with no inbound port opened on T2 boxes:
  - **T1 (`ssh`)**: per-machine SSH config (`-F`, never touching `~/.ssh/config`) with `ssh -N -L <local-sock>:<remote-sock>` stream-local forwarding (`src-tauri/src/ssh/socket_forward.rs`).
  - **T2 (`relay`)**: outbound `ferryx-cli --daemon` enrolled into a leased slot in `~/.ferryx/cloud-slots/` and bridged over the authenticated `/api/v1/cloud/socket` relay stream (`src-tauri/src/remote/cloud_socket.rs`, `src-tauri/src/paired_host/socket_forward.rs`).
- Surfaces catalog rows read-only in the desktop Settings -> Machines list with a `cloud:<provider>` badge.

## Dispatch mode (Task 1 spike result)

From the Wave 1 dispatch-mode spike evidence (`cloud-host` plan item 1, attempt 2):

- `ENGINE=omo` (the legacy `mhc` daemon accepted RPC calls but never finalized turns; `omo` finishes turns cleanly through the forwarded socket).
- `FINAL_MODE=A` — `ferryx-cli cloud socket <name>` publishes a local `0600` Unix socket at `$TMPDIR/ferryx-cloud-<uid>/<name>.sock`, and `omo host session open/wait/read/close --socket "$S"` runs on this Mac against it.
- `ferryx-cli cloud mhc <name> -- <args...>` (MODE=B) exits `3` with `MODE_A: use ferryx-cli cloud socket` in this build.
- **`KEYROOT_SYNC=no` is provisional**: the pinned engine (`omo-ai@5.1.79` / `2026.11.18-3`) logs a warning when no RPC key record is present and still serves, **but the next engine release refuses unproven clients**. Keep the MODE A' key-root sync path (`~/.omo/agent/rpc/keys/*` copied into a `0700` local dir and passed via `--rpc-key-root <dir>`) implementable whenever the engine pin moves past `2026.11.18`.

## Providers

| Provider | Transport | Model Egress | Native Auto-Stop | Provider Max TTL | Free Allocation (no card) | Notes / Limits |
| --- | --- | --- | --- | --- | --- | --- |
| `daytona` | T1 (`ssh`) | `Bridge` (`model-bridge.py`) | Yes | Account default 30m (Ferryx cap 6h) | $200 one-time credit | Direct egress to `hms` endpoint blocked (`000`); uses stateless `ssh -T` HTTP bridge on `127.0.0.1:18802` |
| `freestyle` | T1 (`ssh`) | `Direct` (firewall allowlist) | Yes | None (Ferryx cap 6h; exec cap 300s) | Monthly: 200 vCPU-h, 400 GiB-h RAM, 60,000 GiB-h disk, 50 GB transfer | Create-time env persisted locally in `0600` records under `~/.ferryx/cloud-host/env/` |
| `codespaces` | T1 (`ssh`, `gh` CLI) | `Direct` | Yes | 4h (`240m`) | Monthly Personal Free: 120 core-h, 15 GB-month storage | Needs `FERRYX_CODESPACE_REPO=owner/repo`; uses `ProxyCommand` (`cs.<name>.main` has no public DNS); stops at free limit without payment method |
| `instabox` | T1 (`ssh`, CLI) | `Direct` | Yes | 24h (Ferryx cap 6h) | 1 always-on slot (4 vCPU / 8 GB / 20 GB) | **1-slot limit** (`bong-i-1` in use); requires `CertificateFile`; refuses `ssh_target` on suspended machines instead of waking |
| `e2b` | T2 (`relay`, REST) | `Direct` | Yes | **1h** (`3600s`) | $100 one-time credit (10 concurrent) | Hard **1-hour** sandbox lifetime; exec timeout enforced in-sandbox |
| `blaxel` | T2 (`relay`, REST) | `Direct` | Yes | 24h (Ferryx cap 6h) | Up to $200 one-time credit | Uses `BL_WORKSPACE` + `BL_API_KEY`; explicit `chmod` applied after multipart uploads |

*(Dropped low-quota/unavailable providers: Fly Sprites `$1`, Vercel Sandbox `5h CPU/mo`, OpenComputer `10 credits`, Tensorlake unapproved, Prized `$30`.)*

Provider API keys are read by variable name from `~/.config/compute-secrets.env` (with optional `export ` prefix stripped). Secret values never appear in argv, stdout, logs, catalog rows, or evidence files.

### Codespaces prerequisites

`gh codespace create` is repository-scoped, so `codespaces` needs `FERRYX_CODESPACE_REPO=owner/repo` naming a repository the signed-in `gh` account can create codespaces in; the adapter passes it as `-R`. Because `gh codespace create` fails in non-TTY subprocesses with `error getting machine: no terminal` when no machine type is specified, the adapter passes `-m basicLinux32gb` by default (overrideable via `FERRYX_CODESPACE_MACHINE`), alongside `--display-name`, `--idle-timeout`, and `--default-permissions`.

GitHub must also be able to bill the owner for the machine:

- **Personal account**: a payment method with a Codespaces spending limit above `$0`, or free monthly allowance left (120 core-hours, 15 GB-month storage on the Free plan).
- **Organization repository**: Codespaces enabled for the repository (org policy plus repository setting) and the member permitted to use it; the org pays.
- Either way the idle timeout is fixed at create (`240m` maximum) and cannot be extended afterwards.

A refused create reports **every** meaningful line of `gh`'s output, because `gh` prints the billing assurance `Codespaces usage for this repository is paid for by <owner>` *before* the real error. A spending-limit, payment-required or Codespaces-not-enabled refusal is a `config` error (fix the account; a retry cannot help); an account already at its codespace maximum is a `conflict` error.

## CLI usage

```bash
# Create and bootstrap a cloud machine (default idle TTL 30m, hard cap min(provider max, 6h))
ferryx-cli cloud create --provider <provider> --name <name> [--ttl 30m] [--json]

# List catalog rows joined with live provider state (never wakes stopped machines)
ferryx-cli cloud list [--json]

# Open local dispatch socket (MODE=A); prints $TMPDIR/ferryx-cloud-<uid>/<name>.sock
S=$(ferryx-cli cloud socket <name>)
omo host session open --socket "$S" --cwd /tmp --name e2e --prompt "Reply with exactly: <name>-ok" --json
omo host session wait <session-id> --socket "$S" --until done --json
omo host session read <session-id> --socket "$S" --messages --json
omo host session close <session-id> --socket "$S" --json

# One-shot command execution (exit code passthrough)
ferryx-cli cloud exec <name> -- <cmd...>

# Lifecycle management
ferryx-cli cloud stop <name>
ferryx-cli cloud delete <name>

# Run idle-stop & orphan sweep (called every 5 minutes by the user scheduler)
ferryx-cli cloud reconcile
```

## Live verification status (2026-11-24)

- **End-to-end verified (`PASS`):** `daytona` (T1 SSH + `model-bridge.py`, `daytona-ok`), `e2b` (T2 relay + auto slot enroll/re-pair, `e2b-ok`), and three-host parallel fan-out with the local Mac host (`local-ok`).
- **Second-round live smoke on the other 3 active providers (commit `5b28975d`, all rolled back/deleted to 0 leftovers):**
  - `freestyle`: `create` + SSH bootstrap + `ensure_host` **PASS** (171s, Ubuntu glibc 2.39); `cloud socket` forward timed out at 30s (needs stream-local forward investigation).
  - `codespaces`: `gh codespace create -m basicLinux32gb` **PASS**; next step `ssh -T` `$HOME` probe returned empty output and rolled back cleanly.
  - `blaxel`: create + BusyBox `flock -n` bootstrap (`omo` + `ferryx-cli` download/sha256) **PASS** (76s); `enroll` exited 127 (`not found`) because Blaxel runs musl/BusyBox while the pinned `ferryx-cli` artifact is dynamically linked against glibc (`ld-linux-x86-64.so.2`).
- **Skipped:** `instabox` (CLI session expired; `bong-i-1` untouched).
- **Dropped (5 low-quota/unavailable providers removed in `5b28975d`):** `sprites`, `vercel`, `opencomputer`, `tensorlake`, `prized`.

### Exit codes

- `0`: success
- `1`: runtime/provider failure
- `2`: usage error, invalid machine name, unknown provider, or unknown machine
- `3`: verb refused in the active dispatch mode (`MODE_A: use ferryx-cli cloud socket`)
- `4`: `NO_RELAY_SLOT` — all relay machine slots in `~/.ferryx/cloud-slots/` are leased (refused before any billable `create` call)

## Cleanup and safety guarantees

1. **Two-layer automatic shutdown**:
   - Every provider adapter configures provider-native auto-stop at create time.
   - The first `cloud create` installs a user-level 5-minute reaper (`~/Library/LaunchAgents/dev.ferryx.cloud-reaper.plist` on macOS, `systemd --user` timer on Linux, Task Scheduler on Windows) running `ferryx-cli cloud reconcile`. As soon as every catalog row is `deleted`, the reaper removes its scheduler entry.
2. **Single rollback guard on create**: any failure across the post-create steps (`WaitReady`, `Connect`, `Home`, `Bootstrap`, `PushAgentFiles`, `ModelEgress`, `Enroll`, `EnsureHost`) deletes the provider machine and releases any leased relay slot before returning.
3. **Status never wakes stopped machines**: `cloud list` calls `provider.get` only; only `cloud resume` wakes a stopped machine.
4. **Catalog & slot pool integrity**:
   - Catalog lives at `~/.ferryx/cloud-hosts.json` (mode `0600`), keeping confirmed-deleted machines as tombstones while tracking active machines until provider deletion is confirmed.
   - Relay slot pool lives at `~/.ferryx/cloud-slots/` (`0700` dirs, `0600` lease files, `flock`-serialized) so reused machine IDs keep their pinned Ed25519 keypairs without hitting the 15-machine account limit; a slot is released only after the provider confirms deletion (or rollback).
5. **No leftover local listeners or processes**: `cloud socket` cleans up its forwarded socket only when its `(dev, ino)` still matches (never unlinking a newer forward for the same name), and `model-bridge.py` shuts down its loopback listener immediately when its SSH channel closes.
