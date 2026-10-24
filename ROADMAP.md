# Ferryx Roadmap

What the maintainer is working on, what comes after it, and what is wanted but not scheduled.
It reflects the owner decisions of 2026-10-24 and the open items of the 2026-10-24 non-adoption
review. Dates are not promised; items move when they ship or when a decision changes. Shipped
work is recorded in the [changelog](CHANGELOG.md).

## Now

- **Honest public claims.** Site, README, and docs match shipped code: supported agents come from
  the detection manifests, Gemini CLI is not supported (Antigravity `agy` is), QR pairing is not
  offered, and copy describes Ferryx as source-available under the [Ferryx License](LICENSE).
- **Secure defaults.** Relay web sessions end-to-end encrypted through the existing encrypted
  tunnel, agent control off for new installs with a visible toggle, per-source persisted PIN
  failure budgets, and a least-privilege release CSP and CORS allowlist.
- **Session-survival proof.** Automated gates for GUI quit, kill, bundle swap, chained and
  cross-version upgrades, and reboot on every release, with published results.
- **Easier installation.** A Linux CLI without the desktop stack, a clear refusal on unsupported
  ARM installs, a DOM terminal fallback on GPU-less machines, and macOS permissions asked at first
  use.
- **Repository-friendly worktrees.** `.ferryx-worktrees` and `ferryx/` branches for new repos
  (configurable), excluded from `git status`; existing `.orca-worktrees` and `orca/*` keep working
  and are never moved. A terminal-first shortcut preset on Windows and Linux.
- **Release hygiene.** The `vYYYY.MMDD.N` tag scheme, a beta update channel next to the default
  stable channel, checksums and an SBOM per release, and a release-profile production relay.

## Next

- **Plaintext relay removal.** Old clients keep the plaintext relay path for exactly one release
  after the end-to-end release ships; it is removed in the release after that.
- **Relay redundancy.** A second relay host and a public status page for `relay.ferryx.dev`.
- **Signed Windows builds** through the Microsoft Store.
- **Footprint numbers.** Memory and per-session cost measured on every release and published.
- **Comparisons and positioning.** Published comparisons with cmux, Orca, and herdr, and a "when
  not to use Ferryx" page.

## Later

- **Windows and Linux ARM64 binaries**, once ARM build hosts exist. Until then ARM installs build
  from source.
- **Shared dependencies across worktrees** (`node_modules`, `target`) through a
  package-manager-aware design.
- **Paid plans.** The Free plan covers 3 machines; paid checkout stays closed until the owner opens
  it.
- **More maintainers.** Ferryx has one maintainer today. Contributions are welcome under the
  [contributing guide](CONTRIBUTING.md) and the [CLA](CLA.md).

## Not planned

- Building, signing, or publishing releases in hosted CI. Releases are built on maintainer-controlled
  machines and published with checksums.
- Homebrew tap or winget-pkgs publication. Windows package-manager installs go through the
  Microsoft Store.
- Cloud workspaces, team multiplayer, or SSO.
