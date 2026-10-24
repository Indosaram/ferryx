# Security Policy

## Reporting a vulnerability

Report security issues privately through GitHub Security Advisories:
<https://github.com/Indosaram/ferryx/security/advisories/new>.

Please do not open a public issue, pull request, or Discord thread for a suspected
vulnerability. Include the affected version (release tag), platform, and the steps or
proof of concept needed to reproduce the problem. Reports are handled by the maintainer on a
best-effort basis; there is no guaranteed response or fix timeline.

The user-facing security model (relay encryption, agent control, machine grants, the shared
trust root, data retention, the updater trust chain) and the limits of the hosted relay are
published at <https://ferryx.dev/docs/security/>.

## Supported versions

Ferryx is calendar-versioned (`vYYYY.MM.DD.N`). Only the latest release on the
[releases page](https://github.com/Indosaram/ferryx/releases/latest) receives security
fixes. Fixes ship in a new release; older releases are not patched. See
[CHANGELOG.md](CHANGELOG.md) for what changed between releases.

## Threat model summary

- **Local-first data.** Sessions, workspaces, and preferences stay on your machines; the
  desktop app sends no telemetry. What the remote gateway, pairing tokens, and website
  analytics handle is documented in the
  [privacy declaration](site/src/content/docs/privacy.md).
- **The relay is part of the trust root for direct peers.** In production the account
  service runs inside `ferryx-relay`, so whoever controls the relay host's account signing
  key can attest a direct-path key for any machine of the account owner. Pin keys manually
  (`ferryx direct-trust`, manual pins win) or set `FERRYX_DIRECT_AUTO_TRUST=0` if that is
  not acceptable. See the shared trust root caveat in
  [docs/direct-peer.md](docs/direct-peer.md).
- **A machine grant is code execution.** Pairing a machine is not phone mirroring: a machine
  control grant permits browsing raw filesystem paths and executing programs as the daemon's
  OS user. Only pair machines and desktops you control. See
  [docs/paired-daemon-machine-access.md](docs/paired-daemon-machine-access.md).
- **Self-hosting.** Running your own relay moves the relay and account trust onto
  infrastructure you operate; see the
  [self-hosted relay guide](https://ferryx.dev/docs/self-hosted-relay/).
- **Hosted relay availability.** `relay.ferryx.dev` runs on a primary host with a read-only
  warm standby behind a health-checked failover; there is no SLA. Measured uptime of both
  hosts is public at [status.ferryx.dev](https://status.ferryx.dev/), and the operations
  runbook is [docs/relay-failover.md](docs/relay-failover.md).

In scope: the desktop app, the headless daemon and `ferryx-cli`, the remote gateway, the
mobile web client, `ferryx-relay` and its account service, and the install scripts published
in this repository.

## Release provenance

Ferryx releases are built on operator-controlled machines, not on hosted CI. Every build uses one
40-character source commit, recorded in the release's `provenance.json`:

| Host | Builds |
| --- | --- |
| `macbook` (macOS, release coordinator) | universal updater archive, `Ferryx_universal.dmg`, `ferryx-cli-darwin-universal`; also assembles, signs and publishes the release |
| `omaki` (Linux x86_64) | `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `ferryx-cli-linux-amd64` |
| `maho-win` (Windows x64) | `Ferryx_x64.msix`, and the NSIS installer when it is shipped |

Each GitHub release carries:

- `SHA256SUMS`: the SHA-256 of every other asset.
- `SHA256SUMS.minisig`: a minisign signature of `SHA256SUMS`, made with the same key that signs app
  updates (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`).
- `ferryx-src-tauri.cdx.json` and `ferryx-ui.cdx.json`: CycloneDX SBOMs for the Rust app and the UI.
- `provenance.json`: the commit, tag, Ghostty pin, `sourceDateEpoch`, and per host the toolchain
  versions it reported (node, bun, zig, rust) and the assets it built.

The reproducible build inputs are that commit, the committed `src-tauri/Cargo.lock` and
`ui/bun.lock`, the Ghostty pin and the recorded toolchain versions.

To verify a download, put it in one directory with `SHA256SUMS` and `SHA256SUMS.minisig`, then run:

```bash
minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm
shasum -a 256 -c SHA256SUMS --ignore-missing
```

On Linux, `sha256sum -c SHA256SUMS --ignore-missing` works the same way. Both commands must exit 0.
If either one fails, do not run the file. The full operator procedure is in
[docs/releases/LOCAL_RELEASE_RUNBOOK.md](docs/releases/LOCAL_RELEASE_RUNBOOK.md#release-provenance).
