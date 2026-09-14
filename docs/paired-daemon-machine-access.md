# Paired daemon machine access (opt-in, incomplete)

Machine access is not phone mirroring. A machine control grant permits browsing raw filesystem paths and executing programs as the daemon's OS user. Only pair machines and desktops you control. Mirror grants retain active-desktop-session restrictions and path redaction.

## Current availability

Settings > Remote Access > Paired machines (native desktop only) provides pairing, inventory refresh, capability checking, re-pairing, and confirmed credential forgetting. There is no separate opt-in rollout gate: `remoteHostStore.machineFeaturesEnabled` follows native inventory readiness alone. When the local daemon cannot answer, the adapter fails closed, marks retained rows offline, and preserves credentials, host selection, projects, and layout references.

The terminal proxy currently advertises false. Enabling the preference cannot provide live remote terminals. Inventory or successful pairing alone is not evidence of terminal support. The settings Add Project button remains disabled without its workspace navigation callback; use the workspace Add Project dialog. This packet does not wire that dialog or claim the full workflow is complete.

## Linux operator procedure (human fixture QA required)

Use a clean, dedicated Linux service account or disposable container with no existing Ferryx daemon, projects, or sessions. Do not run these commands against a developer's existing daemon. Install a compatible `ferryx-cli` separately; remote installation is not part of this feature.

Set `FERRYX_RELAY_URL` to your compatible HTTPS relay origin in both shells. In the dedicated fixture account:

```sh
export FERRYX_RELAY_URL=https://your-relay.example
ferryx-cli --daemon
```

Leave that fixture daemon running. In a second shell under the same fixture account and environment:

```sh
export FERRYX_RELAY_URL=https://your-relay.example
ferryx-cli pair generate --access machine
```

The running daemon owns the relay identity and issues the PIN. Do not launch a second standalone pairing server. An old daemon that refuses machine pairing must be upgraded in the fixture; do not substitute a mirror PIN. The PIN is short-lived; machine PINs stay valid for ten minutes (single-use), mirror PINs for one minute. Do not store it in evidence or logs. Permanent bearer credentials must never appear in URLs or renderer logs.

In the macOS native desktop, open Settings > Remote Access > Paired machines. The pairing form asks for the PIN only: the built-in relay (`https://relay.checka.cc`) and a default machine label are applied automatically; enter a custom relay origin or label only through the collapsed Advanced controls. Choose Pair machine. The separate QR-code section is for phone mirrors, not machine authorization. Refresh machines and Check capabilities. Resolve the displayed version/scope error rather than selecting a Local/SSH fallback.

Once compatible proxy and project capabilities are shipped, enable Paired daemon projects, open workspace Add Project > Paired Daemon, select this host and a permitted folder, and verify the returned canonical remote identity. Verify remote `pwd`, split panes, and original session reattachment after reconnection. These are required human acceptance checks, not capabilities proven by this settings packet.

## Troubleshooting and lifecycle

- `NATIVE_CONTEXT_REQUIRED`: use native desktop and a compatible local daemon; browser mirror is not a machine client.
- `UNSUPPORTED_CAPABILITY`: relay, remote daemon, or local daemon lacks required support. Upgrade the incompatible fixture component. Inventory alone does not imply a terminal proxy.
- `MACHINE_GRANT_REQUIRED`: mirror-only or revoked credentials cannot operate machine projects. Generate a new machine PIN as the daemon owner and Re-pair.
- `STALE_HOST_GENERATION`: refresh inventory after credential change and retry capability negotiation. Do not reuse an old request.
- Offline: retain saved projects/layouts; reconnect to the owning machine. An incomplete inventory is not empty authority.

Re-pair pre-fills the relay and label, but requires a fresh PIN. Forget credentials requires explicit confirmation and is distinct from unregistering a remote project, deleting files, closing remote sessions, or revoking other devices. Cancel changes nothing. Late results cannot authorize a different credential generation.

For UI rollback, turn off Paired daemon projects. Do not delete layouts or credential stores and do not restore the hijacking desktop mirror branch. Keep newer storage intact; binary downgrade involving v3 persistence requires a separate stopped-writer/v2-backup rehearsal.

For fixture cleanup, forget only the disposable host's credentials, close only terminals you explicitly created for QA, and stop only the fixture daemon through its owning foreground shell/service. Delete the disposable account/container only after confirming it contains no unrelated work. Forget alone does not stop remote processes.

## Architecture and path exception

The desktop keeps ordinary native panes and project ownership; it does not embed a remote website. Renderer operations carry an opaque host ID and generation to the native paired-host adapter. Native code owns credentials, relay transport, scoped requests, and terminal proxying. Machine traffic stays on the paired relay origin; automatic direct-path upgrade is disabled and there is no SSH fallback.

Raw remote paths are permitted only in authenticated machine-scoped operations and machine events. This exception must not widen legacy mirror/browser payloads, expose permanent credentials, or cause desktop filesystem probes of remote paths. Unsupported actions must remain visibly unavailable rather than execute locally.
