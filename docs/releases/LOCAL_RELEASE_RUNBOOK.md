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
tag:          vYYYY.MMDD.N
app/updater:  YYYY.(MM * 100 + DD).N
MSIX:         YYYY.(MM * 100 + DD).N.0
```

For example, `v2026.1024.1` maps to app version `2026.1024.1` and MSIX version
`2026.1024.1.0`, and `v2026.0908.1` maps to `2026.908.1`. `prepare` validates the calendar date
and records all three values. All host receipts must repeat the plan's `runId`, source SHA, and
app version.

### Tag scheme

Every new release tag is `vYYYY.MMDD.N`: a four-digit year (2026 or later), the zero-padded month
and day of a real calendar date, and a revision `N` of 1..65535 without leading zeros, counting
releases of that day. `scripts/release-tag-policy.mjs` enforces it, and `release-local.mjs prepare`
refuses any other tag before it reads the config, touches git, or builds. Check a tag by hand with:

```bash
bun scripts/release-tag-policy.mjs "$TAG"   # exit 0 = conforming, 1 = rejected
```

Tags made before this policy (`0.1.0`, `v0.1.0`, and the `vYYYY.MM.DD.N` series such as
`v2026.08.26.1`-`.11`) are published and stay in place: never delete, move, or re-point them.
They are listed as legacy in `CHANGELOG.md`, and the version tools still parse the
`vYYYY.MM.DD.N` form so plans and receipts of those releases keep validating. The app versions
both forms produce are the same, so the change does not reorder updates.

### Update channels

The in-app updater has two channels, chosen in Settings > General > Software Update > "Update
channel". Stable is the default.

| Channel | Manifest (asset of the latest GitHub release) |
| --- | --- |
| Stable | `latest.json` (the `plugins.updater.endpoints` entry in `src-tauri/tauri.conf.json`) |
| Beta | `latest-beta.json`, then `latest.json` when no beta manifest is published |

Endpoints are fixed in `src-tauri/src/ipc/updater.rs` (`STABLE_UPDATE_ENDPOINT`,
`BETA_UPDATE_ENDPOINT`); the UI sends only the channel name. To ship a beta, assemble the release
as usual, copy its signed `latest.json` to `latest-beta.json` in `$RUN/publish` before publishing,
and keep the stable `latest.json` pointing at the last stable build. Both manifests are signed with
the same updater key, so a beta never bypasses signature verification.

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
export TAG="v2026.1024.1"
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

### Remote desktop: the LGPL FFmpeg payload

The remote desktop engine (`crates/ferryx-rd`, cargo feature `remote-desktop`, part of the
default features in `src-tauri/Cargo.toml`) links FFmpeg 9 dynamically, so every desktop release
ships FFmpeg's shared libraries. There is no opt-in: each `build` stage builds FFmpeg from the
pinned source on its host, links against that prefix (`FFMPEG_DIR`, which overrides the dev SDK
path in `.cargo/config.toml`), and bundles it. The headless `ferryx-cli`, `ferryx-account` and
`ferryx-relay` binaries are built with `--no-default-features` and link no FFmpeg.

Each host stages `src-tauri/resources/ffmpeg/payload/<triple>/` (gitignored) through
`scripts/stage-ffmpeg.mjs` before building and runs `stage-ffmpeg.mjs --check` again right before
`tauri build`. The build stops if the payload or its `build-receipt.txt` is missing, if the
receipt does not name the pinned source tarball (`src-tauri/resources/ffmpeg/pin.json`), if the
configuration contains `--enable-gpl`, `--enable-nonfree`, `--enable-libx264`, `--enable-libx265`,
`--enable-libxvid`, `--enable-libfdk-aac` or `--enable-libopenh264`, or if any staged file differs
from `stage.json`. No release payload contains OpenH264 on any OS.

| Host | FFmpeg source | Where it lands | Load path |
| --- | --- | --- | --- |
| `macbook` | `scripts/build-ffmpeg-lgpl.sh` per slice (`FERRYX_FFMPEG_ARCH=arm64` / `x86_64`), lipo'd to universal; cached under `<root>/ffmpeg-cache/lgpl-<script sha>-<arch>`; the payload's `sdk/` (arm64 headers + universal dylibs) is `FFMPEG_DIR` for both slices | `Ferryx.app/Contents/Frameworks/*.dylib` (`bundle.macOS.frameworks` overlay), legal files in `Contents/Resources/ffmpeg-legal/` | install names `@rpath/...`; `src-tauri/build.rs` adds `@executable_path/../Frameworks` (and the daemon-helper equivalents) when `CARGO_FEATURE_REMOTE_DESKTOP` is set |
| `omaki` | `scripts/build-ffmpeg-lgpl.sh` with `FERRYX_FFMPEG_OPENH264=0`; cached under `<root>/ffmpeg-cache`; the prefix is `FFMPEG_DIR` | deb/AppImage `usr/lib/Ferryx/ffmpeg/*.so.N`, legal in `usr/lib/Ferryx/ffmpeg-legal/`; deb gains `libva2`, `libva-drm2` | `patchelf --set-rpath '$ORIGIN'` on each library; binary rpath `$ORIGIN/../lib/Ferryx/ffmpeg` |
| `maho-win` | `scripts/build-ffmpeg-lgpl.sh` run by MSYS2 bash with `--toolchain=msvc` (NVENC, AMF, D3D11VA, Media Foundation `h264_mf`/`hevc_mf`; no OpenH264); cached under `<root>\ffmpeg-cache\lgpl-<script sha>-win64` | DLLs beside `ferryx.exe` in NSIS (resource overlay) and MSIX (`build-msix.ps1 -FfmpegPayloadDir`), legal in `ffmpeg-legal\`; headers + import libs in the payload's `sdk\` become `FFMPEG_DIR` for the link | Windows DLL search order (application dir) |

The macOS FFmpeg dylibs are loose Mach-O files, so `finalizeMacosBundle` signs them with the
hardened runtime before the outer app, and they are inside the notarized app and DMG with no
extra steps. After bundling, `verifyMacosFfmpegBundle` checks the Frameworks dylibs, their
`@rpath` install names, the binary's rpath and that the binary links FFmpeg only through
`@rpath`. The Linux build checks with `dpkg-deb -c` that the libraries and receipt are in the deb.

Prerequisites: macOS needs Xcode command line tools and `nasm` (for the x86_64 slice); Linux
needs `nasm`, `pkg-config`, `libva-dev`, `patchelf`, a C toolchain, `make`, `clang`/`libclang-dev`
(bindgen) and the capture stack headers listed in the README's Linux prerequisites. The first
release on a host compiles FFmpeg (tens of minutes per slice); later builds reuse the cache until
`build-ffmpeg-lgpl.sh` changes.

Windows (`maho-win`) prerequisites, one-time:

- Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC x64 and a
  Windows SDK, which provides the Media Foundation and D3D11 headers). The build finds it with
  `vswhere` and imports `vcvars64.bat` before calling MSYS2, so `cl.exe` and `link.exe` are on
  the inherited `PATH`.
- MSYS2 at `C:\msys64` (or set `FERRYX_MSYS2_ROOT`), with
  `pacman -S --needed make nasm pkgconf diffutils tar xz curl`. Only its shell and Unix tools are
  used; the compiler is MSVC (`--toolchain=msvc`), so the DLLs and `.lib` import libraries match
  Rust's `x86_64-pc-windows-msvc`. Make sure MSYS2's `link.exe` (coreutils) does not shadow MSVC's:
  the inherited `PATH` puts the Visual Studio directories first.
- No GPU SDKs: NVENC and AMF are loaded at runtime from the display driver, and the build only
  needs the pinned header packages that `build-ffmpeg-lgpl.sh` downloads.
- `ffmpeg-sys-next` needs `libclang` for bindgen: set `LIBCLANG_PATH` to the LLVM install's
  `bin` directory (on `maho-win`, `C:\Users\sook\ferryx-wt\.llvm\bin`) in the host config `path`
  or the user environment.

To stage by hand (for example to inspect a payload):

```bash
bash scripts/build-ffmpeg-lgpl.sh "$PWD/.ffmpeg" "$PWD/.ffmpeg-build"     # Linux: FERRYX_FFMPEG_OPENH264=0; Windows: MSYS2 shell with vcvars64 env
bun scripts/stage-ffmpeg.mjs --target x86_64-unknown-linux-gnu --prefix "$PWD/.ffmpeg"
bun scripts/stage-ffmpeg.mjs --target <triple> --check
```

**LGPL obligations per release.** Ship `THIRD_PARTY_NOTICES.md` with the release notes, and
attach the corresponding FFmpeg source to the GitHub release: the tarball named in `pin.json`
`source.url` (sha256 `source.sha256`); it is the source of all three platforms. Every bundle
already carries `ffmpeg-legal/build-receipt.txt` (exact configure flags) and `SOURCE.txt` (source
location and written offer). Never enable `--enable-gpl`, `--enable-nonfree` or
`--enable-libopenh264`: the stage step refuses it. To move FFmpeg, update `ffmpeg_version` and
`ffmpeg_sha256` in `build-ffmpeg-lgpl.sh` and `source` in `pin.json` together; the build caches
are keyed on the script's hash, so every host rebuilds.

**Developer SDKs.** Dev, test and CI builds do not compile FFmpeg: `scripts/fetch-ffmpeg.mjs`
downloads a prebuilt LGPL SDK (`pin.json` `sdk`) from a dedicated, non-latest prerelease of this
repository (`ffmpeg-sdk-<version>-<n>`). Those archives are `build-ffmpeg-lgpl.sh` output
(Linux without OpenH264, macOS lipo'd universal, Windows MSVC) plus `build-receipt.txt`, which the
fetch checks against the pinned source and the LGPL flag rules. When FFmpeg moves, build the
three archives the same way, publish them under a new `ffmpeg-sdk-*` prerelease tag (never mark
it latest), and update `sdk.tag` and every `sdk.assets[*].sha256` in `pin.json`.

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
| `ferryx.rb`, `ferryx-cli.rb` | Homebrew cask (`Ferryx_universal.dmg`) and linux x64 formula (`ferryx-cli-linux-amd64`), rendered by `scripts/release-packaging.mjs` from `SHA256SUMS.txt` with the tag's version and hashes |

Both SBOMs are generated from a clean `git archive` of `plan.json.commitSha`, never from the working
checkout. Which host builds which asset:

| Host | Assets |
| --- | --- |
| `macbook` | `Ferryx.app.tar.gz` (+`.sig`), `Ferryx_universal.app.tar.gz` (+`.sig`), `Ferryx_universal.dmg`, `ferryx-cli-darwin-universal` |
| `omaki` | `Ferryx_amd64.AppImage` (+`.sig`), `Ferryx_amd64.deb`, `ferryx-cli-linux-amd64` |
| `maho-win` | `Ferryx_x64.msix`, and `Ferryx_x64-setup.exe` (+`.sig`) when NSIS migration is selected |
| `macbook` (coordinator) | `latest.json`, `SHA256SUMS.txt`, `ferryx.rb`, `ferryx-cli.rb`, the SBOMs, `provenance.json`, `SHA256SUMS`, `SHA256SUMS.minisig` |

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

### 4b. Session survival gate

A release is not drafted until session survival is proven on all three platforms for the exact
release commit. Run the daemon-lifetime QA harness from a worktree checked out at
`plan.json.commitSha` on each QA host. It runs against isolated QA OS jobs and a QA bundle only;
never point it at the live daemon:

```bash
# mac (isolated QA jobs on this Mac), linux (omarchy), windows (maho-win)
node scripts/qa/daemon-lifetime/run.mjs --platform mac     --only Q01,Q02,Q03,Q03b,Q07,Q08,Q08b,Q11,Q14
node scripts/qa/daemon-lifetime/run.mjs --platform linux   --only Q01,Q02,Q03,Q03b,Q07,Q08,Q08b,Q11,Q14
node scripts/qa/daemon-lifetime/run.mjs --platform windows --only Q01,Q02,Q03,Q07,Q08b,Q11,Q14
```

Windows skips the ids that `scripts/qa/daemon-lifetime/lib/summary.mjs` does not list for it
(`Q03b`, `Q08`). Copy each run's `summary.json` (its path is the last line the harness logs) to
`$RUN/survival/<platform>/summary.json`, and the footprint benchmark result from
`bench/footprint/run.mjs` (todo 17, IS-8 per release) to `$RUN/survival/footprint.json`. Then:

```bash
bun scripts/release-local.mjs survival-gate --run "$RUN"
```

The gate (`scripts/lib/release-survival.mjs`) writes `$RUN/survival/survival-report.md`: a
scenario x platform table of PASS/FAIL with kept/before live session counts. It refuses with a
typed code when:

| Code | Cause |
| --- | --- |
| `SURVIVAL_SUMMARY_MISSING` | a platform has no `survival/<platform>/summary.json` |
| `SURVIVAL_SUMMARY_INVALID` | the file is not a daemon-lifetime summary for that platform |
| `SURVIVAL_STALE_COMMIT` | the summary `commit` field is not `plan.json.commitSha` |
| `SURVIVAL_FAIL` | any gate scenario is FAIL on any platform |
| `SURVIVAL_SCENARIO_NOT_PASSED` | a gate scenario is BLOCKED, NOT_RUN or absent |
| `SURVIVAL_FOOTPRINT_MISSING` | `survival/footprint.json` is missing or not JSON |

Any FAIL blocks the release. `publish` runs the same gate before its first network call, so a
failing gate never creates a draft. Owner override: when a QA host is unreachable, the owner may
pass `--survival-waiver "<reason>"` to both `survival-gate` and `publish`. A waiver covers only
evidence that could not be produced (missing summary, a not-run scenario, missing footprint);
it never covers a FAIL, a stale commit or an invalid summary. The waiver and its reason are
printed in `survival-report.md`, and `publish` appends the report to `RELEASE_NOTES.md`, so both
appear in the GitHub release body. Until todo 17 lands `footprint.json`, use
`--survival-waiver "footprint pending todo 17"`.

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
  bun scripts/release-local.mjs publish --run "$RUN" --approve-publish [--survival-waiver "<reason>"]
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

Homebrew: the release carries the rendered `ferryx.rb` (cask) and `ferryx-cli.rb` (formula);
the templates live in `packaging/homebrew/`. No tap is published yet. Once the
`Indosaram/homebrew-ferryx` tap exists, copy the two files from the public release into its
`Casks/` and `Formula/` directories. Windows package-manager installs go through the Store
listing (`winget install --source msstore 9NLHQL5JNLM4`); no winget-pkgs manifest is authored.

Microsoft Partner Center submission is manual and separately approved. The local release may
produce an unsigned `Ferryx_x64.msix` for Store ingestion, but attaching it to GitHub or making a
GitHub Release public does not submit it to the Store. Verify the reserved Store identity and
MSIX version, then obtain Store-submission approval before uploading in Partner Center.

### 6. Store submission

The Microsoft Store is the signed Windows channel: the Store signs the MSIX it publishes, so Store
installs show no SmartScreen warning. No code-signing certificate is bought and SignPath is not
used. Every release with a Windows build must therefore produce `Ferryx_x64.msix` at the plan's
canonical quad version (`plan.json.msixVersion`, e.g. `2026.1024.1.0`).

`assemble` enforces this. When a `maho-win` receipt or Windows artifacts exist, it reads the
receipt's MSIX, opens its `AppxManifest.xml`, and refuses to assemble with a typed error:
`STORE_MSIX_MISSING` (no MSIX), `STORE_MSIX_INVALID` (no readable manifest), or
`STORE_MSIX_VERSION_MISMATCH` (Identity version is not the canonical quad). On success,
`assemble` and `publish` both print:

```text
Store submission pending: <msixVersion>
```

That line is a to-do, not a status. After publication, submit the release's `Ferryx_x64.msix` to
the existing product `9NLHQL5JNLM4` by following
[MS_STORE_RESUBMISSION_RECOVERY.md](MS_STORE_RESUBMISSION_RECOVERY.md), either through the
Partner Center UI or with `msstore submission`. Executors use the `ferryx-store-resubmission`
skill (Maho/Aside browser, per the owner's rules). Submission acceptance is not certification,
and certification is not public availability. Check the public listing with:

```bash
bun scripts/store-listing-status.mjs
```

It reports `live` only when `https://apps.microsoft.com/detail/9NLHQL5JNLM4` serves the Ferryx
product page. Until it reports `live`, the site and READMEs keep the `.exe` row labeled "not
code-signed; Store version pending certification" and do not link the Store. Once it is live, make
the Store the first Windows download in `site/src/lib/downloads.ts`, `README.md`, and
`README.zh-CN.md`, keep the `.exe` as a secondary unsigned installer, and document
`winget install --source msstore 9NLHQL5JNLM4`.

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
- Confirm the handover moved every session: the predecessor logs
  `Transferred sessions to the successor over the handover socket` with `requested=<N>
  transferred=<M>` (`transfer_sessions_reply` in `src-tauri/src/daemon/server.rs`) in
  `~/.ferryx/logs/daemon.log`. Require `requested == transferred`. A shortfall (for example
  `requested=20 transferred=18`) means sessions were lost in the handover: stop and record it
  before launching the GUI.
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

## Mobile (Android/iOS)

The mobile companion (`mobile/`) follows the same local-only rule: AABs and IPAs are built,
signed, and uploaded from the owner's machine. No GitHub Actions workflow may build, sign, or
upload them, and no Play/App Store credential (upload keystore, `keystore.properties`, App Store
Connect API key, `APPLE_*` signing secrets) may appear in `.github/workflows`. `bun
scripts/release-workflow-policy.mjs` (covered by `scripts/release-workflow.test.mjs`) must still
pass after any workflow change.

Mobile is versioned separately from the three-host desktop plan: store version `1.0.0` (semver)
plus a strictly increasing Android `versionCode` (`FERRYX_ANDROID_VERSION_CODE`) and iOS build
number (`--build-number`). It is not part of `plan.json`, `release-local.mjs`, receipts, or the
updater manifest; stores update the app.

1. Start from a reviewed, clean commit. Run `bun run --cwd mobile test` and
   `bun run --cwd ui build:mobile`.
2. Android: with JDK 17, SDK 36, and `NDK_HOME` set to NDK `28.2.13676358`, and with the upload
   keystore described by `mobile/keystore.properties` or `FERRYX_ANDROID_KEYSTORE_PROPERTIES`, run
   `bun run android:init` and
   `bun run android:build -- --aab --target aarch64 --target armv7 --target x86_64` from `mobile/`.
   The Gradle log must not show `WARNING: Ferryx release signing disabled`; `jarsigner -verify`
   and `zipalign -c -P 16 -v 4` (16 KB alignment) must pass.
3. iOS: with `APPLE_DEVELOPMENT_TEAM` set, run `bun run ios:init` and
   `bun run ios:build -- --export-method app-store-connect --build-number <n>`, then validate and
   upload the IPA with Transporter or `xcrun altool`.
4. Store submission (Play closed testing/production, TestFlight/App Review) is a separate,
   owner-approved step, the same as Partner Center for MSIX. The full command reference and the
   owner checklist are in [mobile/README.md](../../mobile/README.md#release-build-owner-machine-only).

## Browser engine (Chromium) release

The Chromium engine that browser tabs use (`crates/ferryx-browser-engine`) is not inside
Ferryx: the app downloads it on demand from Settings > Browser > Chromium Engine. It ships as
its own GitHub release, `browser-engine-<ENGINE_VERSION>`, separate from the app tags (it does
not use the `vYYYY.MMDD.N` scheme and never becomes "latest"). Like every release it is built,
signed and published locally with `scripts/browser-engine/release-engine.mjs`; no hosted
workflow may run it.

What the app checks before it installs an engine (`src-tauri/src/browser/engine_install.rs`):

- `manifest.json.sig` is a detached ed25519 signature (base64) over the exact
  `manifest.json` bytes, made by a key listed in `TRUSTED_MANIFEST_KEYS`. A missing or
  wrong signature refuses the install (`unsigned` / `badSignature`).
- `manifest.json` `version` equals the app's `ENGINE_VERSION` (`versionMismatch`).
- The archive's size and SHA-256 equal the manifest entry (`partial`, `oversized`,
  `checksumMismatch`); a full disk is reported as `diskFull`.
- The new engine goes into `<data>/browser-engine/<version>_<sha12>`; the `active` pointer
  switches only after it unpacked completely, the engine it replaced is kept for rollback
  (Settings shows "Use <version>"), and older ones are removed.

### One-time: the manifest signing key

```bash
bun scripts/browser-engine/release-engine.mjs keygen --out "$HOME/.config/ferryx/engine-key"
```

This writes `engine-manifest.key` (PKCS#8 PEM, mode 0600) outside the repository and prints the
public key. Put the printed line into `TRUSTED_MANIFEST_KEYS` in `engine_install.rs`, and ship
an app release with it before publishing an engine signed by that key: an app that does not
list the key refuses the engine. Back the private key up the same way as the updater key; never
commit it, never pass it on argv in a shared terminal log. To rotate, add the new public key,
release the app, sign engines with the new key, and drop the old key one release later.

Debug builds (only) also trust a key from `FERRYX_BROWSER_ENGINE_PUBLIC_KEY`, so QA can install
an engine signed with a throwaway key from a test server set by
`FERRYX_BROWSER_ENGINE_MANIFEST_URL`.

### Per engine release

Bump `ENGINE_VERSION` (and `MANIFEST_URL`) in `engine_install.rs` and the engine crate version
together when the engine changes, then on each OS:

```bash
export ENGINE_OUT="$HOME/ferryx-releases/engine-$(date +%Y%m%d)"   # one fresh dir per release

# macOS (maho-mac or this Mac): Developer ID, hardened runtime, then notarize and staple.
bun scripts/browser-engine/release-engine.mjs build --out "$ENGINE_OUT" \
  --identity "Developer ID Application: Indo Yoon (5DUM8WPB4C)" \
  --notarize --notary-profile FerryxNotary

# Windows (maho-win), with an Authenticode certificate in the user store:
bun scripts/browser-engine/release-engine.mjs build --out "$ENGINE_OUT" --signtool-cert <thumbprint>
# Without --signtool-cert the engine is built unsigned and recorded as such
# ("codeSigned": false in engine-windows-x86_64.json; `manifest` warns about it).
```

`build` runs `bundle-engine --release`, signs inside out (framework libraries, the framework,
each helper app with `entitlements/helper.plist`, the outer app with `entitlements/engine.plist`:
allow-jit and unsigned executable memory for V8, camera and microphone for pages), checks
`codesign --verify --deep --strict` and records `spctl`, zips with `ditto` (symlinks and modes
kept), and writes `engine-<platform>.zip` plus `engine-<platform>.json`. Copy the Windows pair
into the same `$ENGINE_OUT` on the Mac, then sign the manifest and check it as the app will:

```bash
bun scripts/browser-engine/release-engine.mjs manifest --dir "$ENGINE_OUT" \
  --key "$HOME/.config/ferryx/engine-key/engine-manifest.key"
bun scripts/browser-engine/release-engine.mjs verify --dir "$ENGINE_OUT" \
  --public-key "$(cat "$HOME/.config/ferryx/engine-key/engine-manifest.pub")"
# the app's own verifier, same check in Rust:
cargo run --manifest-path src-tauri/Cargo.toml --example verify_engine_manifest -- \
  "$ENGINE_OUT" "$(cat "$HOME/.config/ferryx/engine-key/engine-manifest.pub")"
```

Publishing is a separate, owner-approved step. Without `--publish` the command only prints the
`gh` commands; with it, it creates the release as a draft, uploads `manifest.json`,
`manifest.json.sig` and the archives, downloads the draft and compares every byte, and only then
makes it public (`--latest=false`, so the app's updater `latest.json` is untouched). It refuses
to run under CI and fails if the release already exists (no `--clobber`).

```bash
bun scripts/browser-engine/release-engine.mjs publish --dir "$ENGINE_OUT" \
  --public-key "$(cat "$HOME/.config/ferryx/engine-key/engine-manifest.pub")" --publish
```

Then install from a released app build on each OS (Settings > Browser > Chromium Engine >
Download) and open a browser tab. If a published engine is bad, publish a fixed one under a new
`ENGINE_VERSION`; users can switch back with "Use <previous>" meanwhile. Never replace assets of
a published engine release.

## Checks, cleanup, and recovery

### Completion checklist

- [ ] Pull-request checks are green; no hosted workflow builds/signs/publishes releases.
- [ ] Config is credential-free and unchanged; all roots are dedicated and meet configured budgets.
- [ ] Plan tag, app version, MSIX version, source SHA, Ghostty pin, channels, and updater key are reviewed.
- [ ] The tag passes `bun scripts/release-tag-policy.mjs "$TAG"`; no published tag was deleted or moved.
- [ ] All three receipts match the plan and list only the expected artifacts.
- [ ] Developer ID signing, notarization/stapling, updater signatures, package metadata, and hashes pass.
- [ ] The macOS post-build release check passes on `Ferryx.app` and `Contents/Helpers/Ferryx Daemon.app`.
- [ ] `assemble` and `verify` pass against the production updater public key.
- [ ] The section 4b session survival gate passed for `plan.json.commitSha` on mac, linux and windows with zero FAIL and `footprint.json` present, or any `--survival-waiver` reason is printed in `survival-report.md` and the release notes.
- [ ] `SHA256SUMS`, `SHA256SUMS.minisig`, both `*.cdx.json` SBOMs and `provenance.json` are in `$RUN/publish`, and `minisign -Vm SHA256SUMS -P <pubkey>` plus `shasum -a 256 -c SHA256SUMS` pass there.
- [ ] Draft bytes and inventory are reviewed before the separate publication approval.
- [ ] `verify-remote` passes after publication.
- [ ] `assemble` printed `Store submission pending: <msixVersion>`; the MSIX was submitted per step 6 with its own approval and Partner Center result, and `bun scripts/store-listing-status.mjs` was recorded.
- [ ] Mobile store builds, if any, were built and signed locally and versionCode/build number increased.

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

## Production relay build

The production relay (`relay.ferryx.dev`) runs the standalone `ferryx-relay` binary. Build it with
the release profile:

```bash
cd src-tauri && cargo build --release --no-default-features --bin ferryx-relay
# output: $CARGO_TARGET_DIR/release/ferryx-relay (default src-tauri/target/release/ferryx-relay)
```

`--no-default-features` leaves out the desktop GUI and remote desktop stacks (WebKitGTK, FFmpeg,
PipeWire), which the relay never uses.

This replaces the earlier dev-profile build with optimization overrides
(`CARGO_PROFILE_DEV_OPT_LEVEL=3` and related settings) used for the 2026-10-22 deploy. The release
profile runs the bundled-helper freshness gate in `src-tauri/build.rs`, so build from a checkout
whose `src-tauri/resources/helpers/manifest.json` matches the staged helpers; if the gate fails,
fix the staging rather than falling back to the dev profile. Build on the Linux x86_64 relay host
or an equivalent builder, never in hosted CI.

Redeploying is an owner action. On the relay host, back up the running binary, install the new one,
and restart the unit; keep the existing `FERRYX_DEPLOYMENT_MODE` drop-in, without which the relay
refuses to start:

```bash
cp ~/bin/ferryx-relay ~/bin/ferryx-relay.bak-$(date +%Y%m%d-%H%M%S)
install -m 0755 "$CARGO_TARGET_DIR/release/ferryx-relay" ~/bin/ferryx-relay
sudo systemctl restart ferryx-relay
```

Verify against `relay.ferryx.dev` only: the health endpoint returns 200, and the entitlement
endpoint returns 401 without credentials. Paired daemons reconnect within their 30 s retry after
the restart.

## Relay end-to-end encryption rollout (todo 7)

Order matters. Ship the daemons and clients first, and deploy the relay last. Updated clients ask
the relay for an encrypted session (`?e2e=1`), so the relay has to be the final piece to change.

- Deploying the production relay is an owner action. Do not do it from a release run.
- Leave `FERRYX_RELAY_REQUIRE_E2E` unset for this release. Plaintext keeps working for old clients
  and PIN-paired devices. Todo 11 sets it after release R plus one cycle.
- Watch the `ferryx::relay::e2e` deprecation warnings. The `client_build` field shows which
  clients still use plaintext.
- Rollback is redeploying the previous relay build.
