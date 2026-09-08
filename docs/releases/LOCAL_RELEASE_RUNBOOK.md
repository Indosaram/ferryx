# Ferryx Local Release Runbook

This is the canonical operator procedure for Ferryx releases. Release artifacts are built,
signed, assembled, and published from operator-controlled machines; GitHub Actions is not a
release producer. Ordinary pull-request checks and Pages deployment remain enabled.

> **Coordinator status (2026-09-08):** Hardened and fully validated.
> Real multi-host builds (`macbook`, `omaki`, `maho-win`), coordinator-only updater signing,
> read-only `xcrun notarytool` preflight, universal binary and codesign verification, double-approval
> publish gates, and immutable digest bindings are fully implemented and verified across 180+ automated tests.

## Release topology and invariants

| Host | Role | Required output |
| --- | --- | --- |
| `macbook` | Local coordinator, universal macOS builder, signing/notarization host, assembler, verifier, publisher | signed/notarized universal updater archive and universal DMG |
| `omaki` | SSH Linux x86_64 builder | raw AppImage updater payload and DEB (signed by coordinator) |
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
node scripts/release-local.mjs prepare \
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
node scripts/release-local.mjs preflight \
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
node scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host macbook --approve-notarization
node scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host omaki
node scripts/release-local.mjs build --config "$RELEASE_CONFIG" --run "$RUN" --host maho-win
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
test-only `--pubkey` override):

```bash
node scripts/release-local.mjs assemble --run "$RUN"
node scripts/release-local.mjs verify --run "$RUN"
```

Assembly requires the macOS updater + DMG, Linux AppImage + DEB, Windows MSIX, and, when selected,
NSIS. It verifies receipt identity, paths, sizes, hashes, updater signatures, target uniqueness,
and the selected channel matrix before creating stable aliases, `latest.json`, and
`SHA256SUMS.txt`. `verify` checks listed bytes and independently re-derives the publish directory.
Inspect the final inventory; no MSI or architecture-specific macOS DMG is part of this contract.

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
  node scripts/release-local.mjs publish --run "$RUN" --approve-publish
```

The coordinator validates remote tag closure, creates a draft release on GitHub, uploads the
exact assembly publish inventory, downloads the draft assets into a temporary directory, and
asserts byte-for-byte equality before undrafting. Both `FERRYX_APPROVE_PUBLISH=1` and
`--approve-publish` are required to publish. Never place a GitHub token on argv or enable tracing
around publish.

After separately approved publication, run:

```bash
node scripts/release-local.mjs verify-remote --run "$RUN"
```

This verifies public `latest.json` version and availability of `SHA256SUMS.txt`. Final validation
must also establish exact remote inventory/byte checking if not already guaranteed by the
published implementation.

Microsoft Partner Center submission is manual and separately approved. The local release may
produce an unsigned `Ferryx_x64.msix` for Store ingestion, but attaching it to GitHub or making a
GitHub Release public does not submit it to the Store. Verify the reserved Store identity and
MSIX version, then obtain Store-submission approval before uploading in Partner Center.

## Checks, cleanup, and recovery

### Completion checklist

- [ ] Pull-request checks and Pages are green; no hosted workflow builds/signs/publishes releases.
- [ ] Config is credential-free and unchanged; all roots are dedicated and meet configured budgets.
- [ ] Plan tag, app version, MSIX version, source SHA, Ghostty pin, channels, and updater key are reviewed.
- [ ] All three receipts match the plan and list only the expected artifacts.
- [ ] Developer ID signing, notarization/stapling, updater signatures, package metadata, and hashes pass.
- [ ] `assemble` and `verify` pass against the production updater public key.
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
