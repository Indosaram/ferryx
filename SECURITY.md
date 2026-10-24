# Security Policy

## Reporting a vulnerability

Report security issues privately through GitHub Security Advisories:
<https://github.com/Indosaram/ferryx/security/advisories/new>.

Please do not open a public issue, pull request, or Discord thread for a suspected
vulnerability. Include the affected version (release tag), platform, and the steps or
proof of concept needed to reproduce the problem. Reports are handled by the maintainer on a
best-effort basis; there is no guaranteed response or fix timeline.

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

In scope: the desktop app, the headless daemon and `ferryx-cli`, the remote gateway, the
mobile web client, `ferryx-relay` and its account service, and the install scripts published
in this repository.
