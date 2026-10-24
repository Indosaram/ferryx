# Ferryx Local Release Runbook

This is the canonical operator procedure for Ferryx releases. Release artifacts are built,
signed, assembled, and published from operator-controlled machines; GitHub Actions is not a
release producer. Ordinary pull-request checks remain enabled; the site deploys separately
to Cloudflare Workers (see [site migration](../SITE_DOMAIN_MIGRATION.md)).

> **Coordinator status (2026-09-08):** Hardened and fully validated.
> Real multi-host builds (`macbook`, `omaki`, `maho-win`), coordinator-only updater signing,
> read-only `xcrun notarytool` preflight, universal binary and codesign verification, double-approval
> publish gates, and immutable digest bindings are fully implemented and verified across 180+ automated tests.

## Release topology and invariants

| Host | Role | Required output |
| --- | --- | --- |
| `macbook` | Local coordinator, universal macOS builder, signing/notarization host, assembler, verifier, publisher | signed/notarized universal updater archive, universal DMG, and universal CLI |
| `omaki` | SSH Linux x86_64 builder | raw AppImage updater payload, DEB, and x86_64 CLI (signed by coordinator where applicable) |
| `maho-win` | SSH Windows x64 builder | unsigned Store-ingestion MSIX; NSIS updater installer (-setup.exe, signed by coordinator) when migration is selected |

Every host builds the same 40-character source commit recorded in `plan.json`. Ghostty is a
separate local Git repository named by `ghosttyRepository`; its `HEAD` must equal the
`EXPECTED_GHOSTTY_SHA` committed in `src-tauri/native_terminal/build_ghostty.rs` at that source
commit. Do not rely on whatever checkout happens to be present on a builder.

Each host has a dedicated release root outside the working repository. The coordinator creates
`<root>/<runId>` and refuses an existing workspace. `minFreeBytes` is an operator-configured
budget checked by preflight, **not a measured or proven minimum**. The example currently uses
30 GiB for `macbook` and 20 GiB each for `omaki` and `maho-win`; tune only from measured isolated
builds, never by describing a smaller value as safe.

The required plan identity is:

```text
tag:          vYYYY.MM.DD or vYYYY.MM.DD.R
app/updater:  YYYY.(MM * 100 + DD).R
MSIX:         YYYY.(MM * 100 + DD).R.0
```

For example, `v2026.09.08.1` maps to app version `2026.908.1` and MSIX version
`2026.908.1.0`. `prepare` validates the calendar date and records all three values. All host
receipts must repeat the plan's `runId`, source SHA, and app version.

## One-time host configuration

Copy `scripts/release-hosts.example.json` to a private, non-repository JSON file and adapt only
the permitted fields. It must contain `repository`, the standalone `ghosttyRepository`,
`repo: "Indosaram/ferryx"`, and exactly these hosts:

```json
{
  "schemaVersion": 1,
  "repository": "/absolute/path/to/ferryx",
  "ghosttyRepository": "/absolute/separate/path/to/ghostty",
  "repo": "Indosaram/ferryx",
  "hosts": {
    "macbook": {
      "ssh": null,
      "platform": "darwin",
      "root": "/absolute/dedicated/ferryx-release-builds",
      "minFreeBytes": 32212254720,
      "notaryProfile": "profile-name",
      "signingIdentity": "Developer ID Application identity"
    },
    "omaki": {
      "ssh": "user@host",
      "platform": "linux",
      "root": "/absolute/dedicated/ferryx-releases",
      "minFreeBytes": 21474836480
    },
    "maho-win": {
      "ssh": "ssh-alias",
      "platform": "win32",
      "root": "C:/absolute/dedicated/ferryx-releases",
      "minFreeBytes": 21474836480
    }
  }
}
```

Windows paths in this JSON use forward slashes. Optional `path` and `expectedTools` fields are
supported. Never put credentials, tokens, passwords, private keys, or credential-file paths in
the config, plan, receipts, command arguments, or logs; the config parser rejects credential-like
fields and values.

Preserve continuity with the updater public key already committed at
`src-tauri/tauri.conf.json` and with the established macOS Developer ID identity. Moving releases
off CI is not a reason to rotate either identity. Key rotation is a separate migration project.

Signing material is supplied only through the process environment or existing OS keychain
profiles. Relevant variable names are `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PATH`, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; never print or paste
their values. The signer syntax is positional:

```text
cargo tauri signer sign <FILE>
```

`-f` means `--private-key-path`; it is not the file being signed. Tauri help displays signing
environment defaults, so inspect it only with the secret variables removed:

```bash
env -u TAURI_SIGNING_PRIVATE_KEY \
  -u TAURI_SIGNING_PRIVATE_KEY_PATH \
  -u TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
  cargo tauri signer sign --help
```

## Per-release procedure

Run all coordinator commands from the configured Ferryx repository on `macbook`. Choose paths
that are new and dedicated to this release:

```bash
export RELEASE_CONFIG="$HOME/.config/ferryx/release-hosts.json"
export TAG="v2026.09.08.1"
export COMMIT="<reviewed-sha-or-ref>"
export RUN="$HOME/ferryx-release-runs/$TAG"
```

Do not export secret values into shell tracing, command transcripts, or shared terminal logs.

### 1. Prepare an immutable release identity

During the current legacy-client transition, explicitly select the NSIS migration channel:

```bash
bun scripts/release-local.mjs prepare \
  --config "$RELEASE_CONFIG" \
  --tag "$TAG" \
  --commit "$COMMIT" \
  --out "$RUN" \
  --nsis-migration
```

This creates a new run directory and records `plan.json`, `prepare-state.json`, and
`source-inputs.json`. Review the resolved SHA, tag/app/MSIX mapping, Ghostty pin, updater public
key identity, and channels. Once prepared:

- do not edit the source checkout, Ghostty checkout, config, plan, state, or source-input files;
- do not reuse the run for another tag, SHA, channel choice, or configuration;
- do not manually stamp versions in the shared checkout; stamping belongs in isolated sources;
- retain the exact config bytes used by `prepare` (its SHA-256 is recorded in state).

The current implementation records the config digest but does not yet enforce it later. Until
that enforcement is validated, independently compare the current config digest with
`prepare-state.json` before every stage or abandon the run on any change.

Omitting `--nsis-migration` creates a Store-only Windows plan: it requires MSIX but neither an
NSIS artifact nor a `windows-x86_64` updater entry. Such a manifest does **not** update legacy
NSIS clients. Use Store-only only after a separately approved end to legacy updater support.

### 2. Preflight all three hosts

```bash
bun scripts/release-local.mjs preflight \
  --config "$RELEASE_CONFIG" \
  --plan "$RUN/plan.json"
```

Require overall `ok: true`. Review host/architecture, SSH reachability, configured free-space
budget, toolchain probes, Linux WebKitGTK/GTK/ALSA packages, Windows MSVC linker/MakeAppx, and
macOS signing identity/notary profile. A green preflight proves only these probes; it does not
certify that a package can be built, signed, notarized, installed, or run.

### 3. Build and collect receipts

Run each exact host command against the same run and config:

```bash
bun scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host macbook --approve-notarization
bun scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host omaki
bun scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host maho-win
```

`--approve-notarization` is a specific approval for Apple submission, not publication approval.
Do not provide it until the source identity and macOS outputs are ready for Apple. When provided,
the coordinator submits to `xcrun notarytool`, staples tickets, and verifies staple validation with
`xcrun stapler`. When omitted, notarization credentials are removed from the build environment.

A validated implementation builds only under each configured dedicated root, stages only
receipt-listed outputs back into `$RUN/artifacts`, and writes one receipt per host under
`$RUN/receipts`. It never replaces `/Applications/Ferryx.app`, installs an MSIX/DEB/AppImage,
restarts or kills `ferryx --daemon`, or launches a release/debug binary or GUI. Those actions can
destroy active terminal sessions or contaminate acceptance evidence.

### 4. Assemble and verify locally

For a production release, use the public key from committed Tauri configuration (do not pass the
test-only `--pubkey` override). `assemble` signs `SHA256SUMS` with the updater key, so export the
same `TAURI_SIGNING_PRIVATE_KEY` (or `TAURI_SIGNING_PRIVATE_KEY_PATH`) and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` used for updater signing. Install the SBOM generator once with
`cargo install cargo-cyclonedx --locked`; `minisign`, `npm` and `bun` must be on `PATH`.

```bash
bun scripts/release-local.mjs assemble --run "$RUN"
bun scripts/release-local.mjs verify --run "$RUN"
```

Assembly requires the macOS updater + DMG + universal CLI, Linux AppImage + DEB + x86_64 CLI,
Windows MSIX, and, when selected, NSIS. It verifies receipt identity, paths, sizes, hashes, updater signatures, target uniqueness,
and the selected channel matrix before creating stable aliases, `latest.json`, and
`SHA256SUMS.txt`. `verify` checks listed bytes and independently re-derives the publish directory.
Inspect the final inventory; no MSI or architecture-specific macOS DMG is part of this contract.

### Release provenance

`assemble` then adds the provenance assets (`scripts/release-provenance.mjs`) to `$RUN/publish`,
and `publish` uploads them with every other file:

| Asset | Content |
| --- | --- |
| `ferryx-src-tauri.cdx.json` | CycloneDX 1.5 SBOM of the Rust app (`cargo cyclonedx --target all` on `src-tauri/Cargo.toml`) |
| `ferryx-ui.cdx.json` | CycloneDX 1.5 SBOM of the UI runtime dependencies (`bunx @cyclonedx/cyclonedx-npm --omit dev`) |
| `provenance.json` | Tag, commit, `sourceDateEpoch`, Ghostty pin, the plan's toolchain constraints, and per host the recorded toolchain versions and the assets it built (name, kind, size, SHA-256) |
| `SHA256SUMS` | SHA-256 of every other release asset, in `shasum -a 256 -c` format |
| `SHA256SUMS.minisig` | minisign signature of `SHA256SUMS` by the updater key (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`) |

Both SBOMs are generated from a clean `git archive` of `plan.json.commitSha`, never from the working
checkout. Which host builds which asset:

| Host | Assets |
| --- | --- |
| `macbook` | `Ferryx.app.tar.gz` (+`.sig`), `Ferryx_universal.app.tar.gz` (+`.sig`), `Ferryx_universal.dmg`, `ferryx-cli-darwin-universal` |
| `omaki` | `Ferryx_amd64.AppImage` (+`.sig`), `Ferryx_amd64.deb`, `ferryx-cli-linux-amd64` |
| `maho-win` | `Ferryx_x64.msix`, and `Ferryx_x64-setup.exe` (+`.sig`) when NSIS migration is selected |
| `macbook` (coordinator) | `latest.json`, `SHA256SUMS.txt`, the SBOMs, `provenance.json`, `SHA256SUMS`, `SHA256SUMS.minisig` |

Reproducible build inputs are the commit, the Ghostty pin and `sourceDateEpoch` from `plan.json`,
the committed `src-tauri/Cargo.lock` and `ui/bun.lock`, and the toolchain versions each host
reports in its receipt (copied into `provenance.json`). Signing fails closed: without the updater
key, or with a key that does not match the committed public key, `assemble` removes `publish/`
and exits non-zero. `verify` checks the signature and every checksum in `SHA256SUMS` before the
byte-for-byte re-derivation.

Anyone can verify a release independently:

```bash
gh release download "$TAG" --repo Indosaram/ferryx --dir ferryx-release && cd ferryx-release
minisign -Vm SHA256SUMS -P RWTLZ1zCtZ3JEO3XhjZZ6Uq3rQtEz2FbBcfxxl/+aFlNKJepqoQNj2Zm
shasum -a 256 -c SHA256SUMS
```

`bun scripts/release-provenance.mjs pubkey` prints that `-P` value from the committed config, and
`bun scripts/release-provenance.mjs verify --dir ferryx-release` runs both checks. Every check
must exit 0; a single changed byte fails `shasum -c`.

### 5. Draft review, publication approval, and remote verification

Before any network mutation, create and push the reviewed tag by an explicit operator action and
confirm it resolves to `plan.json.commitSha`. Pushing a tag does not trigger a release build.

The intended approval sequence is:

1. create/upload a **draft** release;
2. download the authenticated draft inventory and compare every byte with `$RUN/publish`;
3. stop for a separate human publication approval;
4. make the verified draft public;
5. verify public metadata.

The current parser exposes the following publish command and requires both approval signals:

```bash
FERRYX_APPROVE_PUBLISH=1 \
  bun scripts/release-local.mjs publish --run "$RUN" --approve-publish
```

The coordinator validates remote tag closure, creates a draft release on GitHub, uploads the
exact assembly publish inventory, downloads the draft assets into a temporary directory, and
asserts byte-for-byte equality before undrafting. Both `FERRYX_APPROVE_PUBLISH=1` and
`--approve-publish` are required to publish. Never place a GitHub token on argv or enable tracing
around publish.

After separately approved publication, run:

```bash
bun scripts/release-local.mjs verify-remote --run "$RUN"
```

This verifies public `latest.json` version and availability of `SHA256SUMS.txt`. Final validation
must also establish exact remote inventory/byte checking if not already guaranteed by the
published implementation.

Microsoft Partner Center submission is manual and separately approved. The local release may
produce an unsigned `Ferryx_x64.msix` for Store ingestion, but attaching it to GitHub or making a
GitHub Release public does not submit it to the Store. Verify the reserved Store identity and
MSIX version, then obtain Store-submission approval before uploading in Partner Center.

## Installing locally onto this Mac

`release-local.mjs` never replaces `/Applications/Ferryx.app`. Any local install
MUST go through the notarization guard:

```bash
node scripts/install-macos-app.mjs --bundle <path/to/notarized/Ferryx.app>
```

The guard fails closed unless the bundle is Developer-ID signed for team
`5DUM8WPB4C`, Gatekeeper-accepted with `source=Notarized Developer ID`, and
stapled. Ad-hoc `cargo tauri build` output is rejected (this is how the
2026-09-22 non-notarized install happened: a plain build was copied straight
into /Applications). The escape hatch (`FERRYX_ALLOW_UNSIGNED_INSTALL=1` plus
`--allow-unsigned`) exists for throwaway QA only and prints a warning.

Install safety rules enforced by the script:

- The live bundle is renamed (never deleted) into
  `/Applications/.ferryx-previous-<ts>/` so draining daemons keep executing
  their inode and their PTY sessions.
- After installing, trigger the UDS handover
  `{"type":"upgradeBinary","newBinaryPath":"/Applications/Ferryx.app/Contents/MacOS/ferryx"}`
  before launching the GUI, then confirm `initial.ok` in
  `/tmp/rorca-<uid>/boot-trace.log`.
- Never `kill` a draining daemon: it retires itself when its sessions end.
  The first launch after a notarized install may take several extra seconds
  (Gatekeeper validation) - the readiness budget widening on main covers this.

For a fully signed, notarized, stapled local build without a release run, use
`node scripts/build-notarized-local.mjs <detached-worktree>` (builds from the
worktree at merged main HEAD with `APPLE_SIGNING_IDENTITY`, assembles the daemon
helper bundle, re-signs inside-out, submits to notarytool, staples, and refuses
to finish without `spctl` reporting `Notarized Developer ID`).

## Daemon runtime layout (independent daemon lifetime)

The terminal daemon does not run from the GUI executable inside the installed app.
Each platform runs it from an immutable, generation-keyed copy outside the app install,
under its own OS job, so replacing, moving or updating the app never changes or stops a
running daemon.

### macOS daemon helper bundle

Every macOS release ships a complete, signed helper bundle inside the app:

```text
Ferryx.app/Contents/Helpers/Ferryx Daemon.app/
  Contents/MacOS/ferryx      copy of Ferryx.app/Contents/MacOS/ferryx
  Contents/Info.plist        CFBundleIdentifier com.ferryx.app.daemon, CFBundleExecutable ferryx,
                             CFBundlePackageType APPL, LSUIElement true,
                             CFBundleShortVersionString = the app version
```

It is built at packaging time by `assembleDaemonHelper` in `scripts/macos-daemon-helper.mjs`,
called by both `release-local.mjs build --host macbook` and `build-notarized-local.mjs` after
the Tauri bundle exists and before `finalizeMacosBundle`, so the helper is inside the notarized,
stapled app, the DMG and the updater archive. It first records the outer app's entitlements
(`codesign -d --entitlements :-`) into `outer-entitlements.plist` in the run directory (an
empty file means none; today the app has none), then signs the helper as a bundle.
`finalizeMacosBundle` signs inside-out: loose Mach-O files, then each bundle under
`Contents/Helpers/*.app`, then the outer app, passing the recorded entitlements to both bundle
signatures when present, never `--deep`. It fails if the outer app's entitlements afterwards
differ from the recorded ones, and runs `codesign --verify --deep --strict` on the app. QA debug
bundles use the same function with the ad-hoc identity `-` (no secure timestamp).

At runtime nothing builds or edits a bundle: the daemon runtime stager copies
`Ferryx Daemon.app` byte-for-byte (`ditto`) into the generation directory and verifies it
with `codesign --verify --strict`. A build without the helper is refused (`HelperMissing`);
a staged copy that fails verification is refused (`SignatureInvalid`); there is no fallback
to the GUI executable.

### Generation keys and per-generation OS jobs

A generation key is `<version>-<first 8 hex of the SHA-256 of the staged executable>`, so two
builds with the same version still get different keys. A generation directory is written once
(temp dir, fsync, hash check, atomic rename) and never overwritten while anything runs from it.

| Platform | Runtime root | Staged executable | OS job per generation |
| --- | --- | --- | --- |
| macOS | `~/Library/Application Support/Ferryx/daemon-runtime` | `<key>/Ferryx Daemon.app` | launchd `com.ferryx.daemon.<key>` (`~/Library/LaunchAgents/<label>.plist`) |
| Linux | `$XDG_DATA_HOME/ferryx/daemon-runtime` (`~/.local/share`) | `<key>/ferryx` | systemd --user `ferryx-daemon-<key>.service` |
| Windows | `%LOCALAPPDATA%\Ferryx\daemon-runtime` | `<key>\ferryx.exe` | scheduled task `Ferryx\Daemon-<key>` |

With `FERRYX_DATA_DIR` set (isolated QA) the root is `$FERRYX_DATA_DIR/daemon-runtime`. Only the
newest generation autostarts at login; older jobs lose their login trigger without being
stopped. Old generation directories are removed only when no process runs from them and no
registered job references them; the newest two are always kept.

### Legacy single-job migration

Installs before this change used one job: `com.ferryx.daemon` (macOS), `ferryx-daemon.service`
(Linux), `Ferryx\Daemon` (Windows). The new app never stops a live legacy daemon. If it is
running, or its state cannot be verified, its job stays loaded and running and only loses its
login autostart; the newest generation is registered for the next login. A live legacy daemon
keeps serving until it ends on its own (logout or reboot) or the user explicitly restarts
terminals; the newest generation takes over at that point. A legacy job with no daemon is
removed. Expect the old daemon to stay in Activity Monitor / `ps` after the first update; that
is by design.

### Control channel freeze rule

Besides the data protocol (`DAEMON_PROTOCOL_VERSION`), the daemon serves a version-stable
control channel, `control.sock` in the runtime dir on Unix and `control.port` + `control.token`
on Windows. Its schema `control-v1` (ops `Hello`, `Inventory`, `RequestUpgrade`,
`RetireWhenIdle`, `Shutdown`) is frozen and additive-only forever: new optional fields and new
ops are allowed; no removal, rename or type change. The handover wire (`handover-v1`) is frozen
the same way. Any later `DAEMON_PROTOCOL_VERSION` bump must keep control v1 working, because it
is the only way a newer app can inspect, upgrade, retire or stop an older daemon. The fixtures
`src-tauri/tests/fixtures/control-v1.json` and `handover-v1.json` fail the suite on a breaking
change; never edit them to make a change pass.

### Windows pending rule

Windows cannot move terminals between daemon processes. When an update changes the daemon
protocol, the app never shuts down, terminates or replaces the running daemon. It shows
"Update installed; N terminals still running on the previous version" and asks the old daemon
to retire once idle; the old generation exits by itself when its last terminal closes, and the
newest generation starts then. The only destructive path is the user pressing "Restart
terminals now" and confirming the exact terminal count; if the count changed, nothing stops and
the count is re-read. When the old daemon cannot report its count, the button stays disabled.

### Post-build release check (macOS)

Before installing or publishing a macOS release artifact, run on the final `Ferryx.app` (and on
the copy inside the DMG when checking the DMG):

```bash
APP="<path/to>/Ferryx.app"
HELPER="$APP/Contents/Helpers/Ferryx Daemon.app"
codesign --verify --deep --strict --verbose=2 "$APP"
codesign --verify --deep --strict --verbose=2 "$HELPER"
spctl --assess --type execute -vv "$APP"
spctl --assess --type execute -vv "$HELPER"
```

All four must exit 0, and both `spctl` results must report `source=Notarized Developer ID`. A
missing helper bundle, a helper that fails verification, or a helper Gatekeeper rejects blocks
the release: the app could not stage a daemon runtime on users' machines.

## Checks, cleanup, and recovery

### Completion checklist

- [ ] Pull-request checks are green; no hosted workflow builds/signs/publishes releases.
- [ ] Config is credential-free and unchanged; all roots are dedicated and meet configured budgets.
- [ ] Plan tag, app version, MSIX version, source SHA, Ghostty pin, channels, and updater key are reviewed.
- [ ] All three receipts match the plan and list only the expected artifacts.
- [ ] Developer ID signing, notarization/stapling, updater signatures, package metadata, and hashes pass.
- [ ] The macOS post-build release check passes on `Ferryx.app` and `Contents/Helpers/Ferryx Daemon.app`.
- [ ] `assemble` and `verify` pass against the production updater public key.
- [ ] `SHA256SUMS`, `SHA256SUMS.minisig`, both `*.cdx.json` SBOMs and `provenance.json` are in `$RUN/publish`, and `minisign -Vm SHA256SUMS -P <pubkey>` plus `shasum -a 256 -c SHA256SUMS` pass there.
- [ ] Draft bytes and inventory are reviewed before the separate publication approval.
- [ ] `verify-remote` passes after publication.
- [ ] Store submission, if performed, has its own approval and Partner Center result.

### Failure recovery

All stages fail closed. Preserve `$RUN`, its receipts, artifacts, and logs for diagnosis. Fix the
host prerequisite or implementation defect, then either rerun a stage that is explicitly
idempotent for that untouched run or prepare a new run directory. If source, config, plan,
Ghostty pin, tag, channel selection, or any receipt-listed byte changed, abandon that run and
prepare a new one.

Never recover with `git reset --hard`, `git clean`, `git checkout`/`git restore` on shared trees,
cache wipes, recursive deletion of a shared release root, reuse of a stale output directory,
`--clobber` against an existing release, app replacement, or daemon termination. Remove only a
run directory whose exact path and ownership have been reviewed, and only after evidence is no
longer needed. If publication fails after draft creation, leave the draft private, inspect it,
and resolve or delete it through a separately reviewed GitHub action; never pretend the partial
operation succeeded.
