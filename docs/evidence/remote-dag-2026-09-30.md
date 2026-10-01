# Remote DAG root fix - evidence record (2026-09-30)

Scope: the remote DAG root fix as it stands after the capability-gate correction and the bundled
helper artifact install. Every claim below is marked with its source: `[parent]` = reported by the
parent run and recorded as explicit scope/outcome pairs (not re-measured here), `[measured]` = verified
in this session with read-only tools, `[source]` = read from the repository.

Status: **partially green, nothing claimed beyond what ran.** The artifact/plumbing chain is proved,
Unix first spawn is GREEN on both the earlier run and the current service artifact (2 passed / 0 failed),
and the Windows unit cases are green at revision `8b47`. The overall goal is **not** complete: still open
are the live install and its GUI deployment permission, every live-GUI step, the old-agent rendezvous,
and home paths beyond the documented ceiling - all listed in section 5. Earlier first-spawn findings are
superseded - see section 5. Superseded wording is marked inline rather than deleted.

## 1. Capability gate correction (JS, uncommitted) `[source]`

`scripts/build-remote-helpers.mjs`

- The byte-substring capability oracle (`verifyHelperArtifactCapabilities`) is deleted. It was a
  proven false negative: the compiler legitimately splits or merges string constants, so a missing
  contiguous token is not evidence of a missing capability.
- `stageHelpers` now requires measured evidence per receipt: `helperVersion` (must equal the version
  read from `remote-helper/Cargo.toml`) and `capabilities` (array of non-empty strings; must contain
  every entry of `REQUIRED_HELPER_CAPABILITIES`). Optional `runtimeCapabilities` must be a subset.
  The manifest records the **measured** capability list per artifact, not the required constant.
- `validateBundledHelpers` enforces the canonical `REQUIRED_HELPER_CAPABILITIES` unconditionally -
  a manifest that declares an emptied `requiredCapabilities` cannot relax it - keeps the existing
  helperVersion / sha256 / byteLength / ELF-PE-Mach-O magic+arch checks, and after the artifact loop
  enforces the shipped target matrix by sorted `deepEqual` against `requiredTargets` (default
  `TARGETS`, five targets), which also rejects duplicate target entries.
- `--check-bundled <repoRoot>` is unchanged; the production path in `src-tauri/build.rs` therefore
  gets the five-target default. Note: the JS CLI exits 0 with "skipped development build gate" when no
  manifest is found - only the Rust gate hard-errors on a missing manifest, so the CLI must always be
  given the real repository root. Two independent encounters of that branch are on record: this
  session's first invocation passed a literal-quoted path, which `resolve()` turned into a
  non-existent directory and the CLI reported as "skipped" with exit 0; the parent's latest Windows
  resource check first skipped because the parent directory was missing, and after the path was fixed
  it verified the fingerprint and exited 0 `[parent]`. A "skipped" line is a path/config signal, never
  a pass of the bundle itself.

`scripts/build-remote-helpers.test.mjs` - 22 tests `[measured: inventory]`, fixtures carry measured
evidence, negatives are deterministic and structured (`ERR_ASSERTION` with `actual`/`expected`):
missing capability evidence, missing `agentStateV1`/`dagSubscribeV1`, missing measured version, wrong
measured version, bundle artifact missing a recorded capability, bundle artifact without recorded
evidence, emptied `requiredCapabilities` (with and without artifact evidence), incomplete shipped
matrix, duplicated shipped target. No prose pinning in the new tests.

Measured basis for deleting the byte scan `[measured]` - fresh, same-source `--release` builds,
byte-exact counts (python read+count):

| anchor | aarch64-darwin | x86_64-darwin | windows.exe |
|---|---|---|---|
| `sshHelperV1` / `capabilities` / `helperVersion` | 1 | 0 | 0 |
| `project.register` | 0 | 1 | 1 |
| `SNAPSHOT_TOO_LARGE` | 1 | 1 | 1 |
| `INVALID_CHECKPOINT_JSON` | 1 | 0 | 1 |

Two literals from the same function split per target, and the live release handshake passed with all
four capabilities while zero contiguous capability tokens were present `[parent]`. Runtime truth stays
the handshake; the CLI compile surface (`--capabilities`, plus the shared constant) is owned by
worker 29d and was not modified here.

## 2. Five-target build matrix `[parent]`

Five remote release builds, each artifact's `--version` and `--capabilities` executed on a host that
can run it, bound to the artifact hash:

| target | execution used |
|---|---|
| darwin aarch64 (Mac ARM) | native |
| darwin x86_64 (Mac Intel) | native / Rosetta |
| linux x86_64 | native |
| linux aarch64 | qemu (glibc 2.17 floor) |
| windows x86_64 | native |

All four capabilities observed on every artifact; common source fingerprint
`4c74e5fd038615c26296c86a1b4df09f3d4faef4ff3245115ab795e8cdf8c315`. This is the **rebuilt** matrix,
built after the frozen root-scoped-unit source change moved the helper source closure; the earlier build
(fingerprint `49ebe4f262443df8898f2b9821507a969daf23d76394e52f100f21fb19b6bb12`) is **historical**, and the
install made from it is superseded by section 3.

## 3. Bundled artifact install `[measured]`

Preconditions before any copy: `src-tauri/resources/helpers` held exactly the previous own set
(manifest sha256 `e5c37016432b2817a475262c559a618da8fb864aeaf46432feaa30e2f84834d5`, fingerprint
`49ebe4f2...`, the five recorded binary hashes) with no foreign change anywhere in the resources tree,
and the live repository fingerprint equalled the new tar manifest's `sourceFingerprint`
(`4c74e5fd038615c26296c86a1b4df09f3d4faef4ff3245115ab795e8cdf8c315`). The staged copy passed
`validateBundledHelpers({ repoRoot, resourcesDir: <staged> })` before the move.

Installed manifest: `helperVersion` 2026.930.1, `schemaVersion` 1, `protocolVersion` 1,
`sourceFingerprint` `4c74e5fd038615c26296c86a1b4df09f3d4faef4ff3245115ab795e8cdf8c315`,
`requiredCapabilities` = sshHelperV1, dagSubscribeV1, agentStateV1; file sha256
`c3f5eb12ec960fb48c7c60dce3f750d15167232c66b220564632f302fd186c90`.

| target | sha256 | byteLength | recorded capabilities |
|---|---|---|---|
| x86_64-unknown-linux-gnu | `feeaa891216fdfcc4e0b2dc54594053d29d2c900a3502d338d98737390a52173` | 1435272 | all four |
| aarch64-unknown-linux-gnu | `fae9210568b0e50029d48ae63a2384356d6e8a2a4e6801c631e22f743903a186` | 1294704 | all four |
| aarch64-apple-darwin | `6de27da1baea821ce43b7274a2b37b0ebe4606b6f146e85f51512e6de24c9b7c` | 1474368 | all four |
| x86_64-apple-darwin | `5978a9dccb7a4e513e2087a93d016438b462482c56054bbd0df241687ab4a2fc` | 1540968 | all four |
| x86_64-pc-windows-msvc | `57f8131d50ff38d363351d6539501c6bd7406ffddde3bfcaad5254d5ebc761bf` | 1267200 | all four |

("all four" = sshHelperV1, dagStreamingV1, dagSubscribeV1, agentStateV1 - the measured surface carries
`dagStreamingV1` beyond the required three. The two darwin artifacts are byte-identical to the historical
set; the linux and windows artifacts are the rebuilt ones.)

Post-install verification: `validateBundledHelpers({ repoRoot })` on the installed path passes, and
the production gate `bun scripts/build-remote-helpers.mjs --check-bundled /Volumes/T9-Mac/project/ferryx`
prints "Bundled helper assets verified match source fingerprint 4c74e5fd..." with exit 0. The installed
tree is exactly five target directories plus `manifest.json`, no stray or AppleDouble entries, and the
installed checksums match the manifest. The repository source fingerprint was unchanged by the install
(resources are not part of the closure). The five binaries were `chmod 755`-ed to match the previously
committed mode; content was not modified.

Rollback material: the previous own set (fingerprint `49ebe4f2...`, manifest sha256 `e5c37016...`) was
preserved - not erased - at `/tmp/ferryx-dag-old-bundled-helpers-49ebe4-1790782736855`, and the earlier
pre-fix set (helperVersion 2026.917.1) remains at
`/tmp/ferryx-dag-old-bundled-helpers-1790780783026-i6d52b`. Source tar
`/tmp/ferryx-dag-helpers-verified-service.tar`, sha256
`378d48ff3ac71a619837cf73c96f951269dbe09c760fd9900e4ef5bf5aa7cf6c` (12 entries under
`helpers-verified-service/`, no absolute or parent-relative paths); the earlier tar
`/tmp/ferryx-dag-helpers-verified.tar` (sha256 `1a5d10eb...`) stays as history.

## 4. Parent-run verification outcomes `[parent]`

Recorded as scope -> outcome, in the parent's corrected reading of its own shorthand. No counts are
glossed into prose and no figure is carried over from an earlier, ambiguous phrasing.

| scope | outcome |
|---|---|
| UI tests | 61 **tests** across 4 UI files, all passed; 0 failures |
| CLI tests | 2 **passed**, 0 failures; no CLI errors |
| JS tests | 37 **passed**, 0 failures |
| Windows backend, `cargo check` | exit code **0** (2m16s), with warnings present - this is not a zero-warning run |
| Remote release builds | five targets (Mac ARM, Mac Intel, Linux x86_64, Linux aarch64 with glibc 2.17 floor, Windows x86_64); `--version` / `--capabilities` executed on each (native, Rosetta, qemu); all four capabilities; artifact hashes bound to the receipts; common source fingerprint `4c74e5fd...` (rebuilt matrix; the earlier `49ebe4f2...` build is historical) |
| Staging | five-target stage validated at fingerprint `4c74e5fd...`; shipped resources replaced with the rebuilt set (manifest sha `c3f5eb12...`); the previous own set preserved at `/tmp/ferryx-dag-old-bundled-helpers-49ebe4-1790782736855` |
| Unix paired | 3 **passed**, 0 failures, under the isolated foreign-compat overlay |
| Unix first spawn | NOT GREEN at the time of that run; **superseded** by the green first-spawn run in section 5 |
| Scope of the 61 UI tests | hook / store / component level; not a live GUI run |
| Windows unit name | 2 passed, 0 failures |
| Windows root, 37-byte boundary | final revision `8b47`: 2 passed, exit 0 |
| Final UI build | exit 0, 33.11 s |
| Windows resource gate (current) | actual fingerprint `4c74e5fd...` verified, exit 0; source verified `[parent]` |
| Unix first spawn, service artifact (current) | 2 passed, 0 failed, 17.00 s; sentinel `DAG_UNIX_FIRST_SPAWN_EXIT0`, monitor `bash_321`; compiled from the current process `83b6` against test-artifact closure `4c74...` |
| Remote Linux, owned two-root coexistence | PASS, 0 failures: two owned roots at once with zero PTY; systemd units for root A (pid 3129795) and root B (pid 3129805), unit-root digests `1658...` / `1ceb...`; root A idempotent (same pid and epoch on re-run); cleanup **observed**: process A absent, process B in `Z` with no child and its socket closed, 0 sessions. Only the owned PIDs were signalled - no `systemctl stop`, units themselves not touched - and unit state was **not** checked after cleanup. The exact exe/root/host were checked before TERM |

Diagnostics state `[parent]`, worker-verified as an intermittent flap with no edits involved:
**TS2339 matcher type diagnostics** appeared both in an edited test and in an untouched test - the
ownership test went **9 -> 0** and the in-repo integration test went **4 -> 0**, both without any edit -
so these diagnostics are intermittent rather than caused by a change. The observation resolves the flap
but does **not** conclusively prove a non-source-code-defect attribution: attribution is **unverified**,
and no definite cause is claimed. The test files are excluded from
the build's type coverage (tsconfig test exclude), which means the build passing **does not prove the
types of test files**. No new repository-wide type gate is in scope for this record. UI Vitest: **61
passed**; earlier UI build: **0 failures** `[parent]`; final UI build: **exit 0** (33.11 s) `[parent]`. Rust LSP request timeouts
also occurred during this session's tooling `[measured]`, with no attribution claimed.

## 5. Green now, superseded findings, and what stays open

**Green now - Unix first spawn (real ownership path)** `[parent]`: 2 pass / 0 fail. The test binary was
the owned private copy under staged helpers, driven over a real loopback SSHD with authentication, sha
qualified: absent before, present after, actual connect then close, with supervision through the
existing ignored entry executed as a subprocess.

The same case is now green on the **current service artifact**: 2 passed, 0 failed in 17.00 s, exit
sentinel `DAG_UNIX_FIRST_SPAWN_EXIT0` (monitor `bash_321`), compiled from the current process `83b6`
against test-artifact closure `4c74...`. No pending tests remain.

**Superseded findings (kept for provenance).** The earlier failures were, in order: (a) `SUN_LEN` on
the fixture path, then (b) the supervision harness mode being absent. Both are superseded by the run
above; neither is the state of the tree any more. The route they surfaced is real and visible in
source: `FERRYX_SSH_SUPERVISOR_LIBTEST` is the child-only route
(`src-tauri/src/ssh/transport_unix.rs:63`), the libtest entry is `#[ignore]`-marked with the reason
"invoked as a subprocess by Owner::prepare through FERRYX_SSH_SUPERVISOR_LIBTEST"
(`src-tauri/src/ssh/bridge_tests.rs:1775`), and the subprocess is launched with that env set
(`src-tauri/src/daemon/remote_ssh_tests.rs:996`) `[source]`. No mocks were used for the fix `[parent]`.
This green does not support an all-feature live claim.

**Compatibility overlay** `[parent]`: the overlay was **applied in an isolated snapshot only**; the
shared tree and the foreign tree's files were left untouched. Its outcomes, stated exactly: 6 errors -
shell HRTB 4, pty 2; handshake 31 missing-field failures; Debug 5 error sites, derive 1.

**Two independent layers, not one fix.**

- *Layer A - socket path length.* The compact client-side runtime root is
  `{home}/.ferryx/r/<version>/<32-hex host digest>` while the executable stays under
  `{home}/.ferryx/versions/<version>/bin/...`; existing roots are never migrated, so old and new
  locations coexist and no running PTY is signalled `[source]` (`src-tauri/src/ssh/helper_setup.rs:735-758`,
  tests `src-tauri/src/ssh/helper_setup_tests.rs:893-948`; the parent cites current file hash `d9b2...`
  for the client-only implementation `[parent]`). Its effect is limited to: a *new* helper bind no
  longer fails on socket-path length. Documented bound: macOS rejects at 104 bytes, ceiling about 37
  UTF-8 bytes of home path. The Unix side of that boundary is final: the Unix root test reports 2
  pass / 0 fail including the 37-byte home case (current test file hash `8b47...`) `[parent]`. **Still
  unresolved: a home path longer than that ceiling** - there is no length preflight, so it stays
  documented rather than assumed safe.
- *Layer B - provider rendezvous (independent of Layer A).* An agent already running without the
  agent-state env cannot have its provider id associated retrospectively; the association must be owned
  by a fresh agent session. The compact `r/` root is **not** a mechanism for this layer, and no
  "newest agent" heuristic may be used `[parent]`.

**Windows** `[parent]`: green on the final revision - revision `8b47` reports 2 passed with exit 0,
including the 37-byte boundary, and the unit-name change reports 2 passed, 0 failures. The earlier
`491b` run's 2 pass / 0 fail covered the same two cases and is superseded by this final revision.

**Packaging - systemd unit scope** `[parent]` + `[source]`: the helper unit was host-scoped, so two owned
roots could not coexist - while the live old helper kept running, starting the new root waited **a
single 5-second timeout** (not five attempts) and gave up, so the new root was not launched; the parent
confirmed the active branch in source. Worker 29d was authorized for the minimal **root-scoped unit
name** plus a deterministic identity test, and the remote Linux probe now passes: two owned roots at
once with zero PTY, units for root A and root B (`1658...` / `1ceb...` root digests), root A idempotent
on re-run (same pid and epoch), and cleanup **observed**: process A absent, process B in `Z` with no
child and its socket closed, 0 sessions. Only the owned PIDs were signalled - no `systemctl stop`, the
units themselves were not touched - and unit state was not checked after cleanup. The exact exe/root/host
were checked before TERM. User-scoped `systemd status` reads 0 / available.

**Badge evidence scope** `[parent]`: the 61 UI tests are hook / store / component level, not a live
GUI run; no live GUI claim is made here.

**Installed application still ships the old helper set** `[measured]`. `/Applications/Ferryx.app`
reports `CFBundleShortVersionString` 2026.930.1, but its bundled
`Contents/Resources/helpers/manifest.json` (sha256
`72b85fa7e54f16bcd11830ea06af90ff8c6e798d753db1fc13b76179a4f5f830`) declares helperVersion
**2026.917.1** with no `sourceFingerprint` and five artifacts. So the running/installed app still
carries the pre-fix helper bundle while the repository now holds the rebuilt set
(`c3f5eb12...`, fingerprint `4c74e5fd...`), whose predecessor (`e5c37016...` / `49ebe4f2...`) is kept as
historical. No deployment was performed and no GUI deployment has been approved.

**Feature state vs live state** `[parent]`: the root ingress managed-worktrees provider-id sink is
already **wired in code** (implemented and tested); what remains is live-deployment verification, not
implementation. The agent-rendezvous item is Layer B above, and it is independent of the path-length
work.

**No live GUI permissions run yet** `[parent]`: asked and awaiting answer; no deployment performed.

**Uncommitted** `[measured]`: the two gate scripts, `src-tauri/build.rs`, and the new
`src-tauri/resources/helpers` set are dirty in the shared working tree, as are the compact-root files
(`src-tauri/src/ssh/helper_setup.rs`, `src-tauri/src/ssh/helper_setup_tests.rs` and their fixture -
`src-tauri/src/ipc/ssh.rs` is not part of the compact root; it was already changed by the earlier
qualification work). The commit is the parent's call.

**Open items (all of them).**

1. **Installed-app GUI deployment permission**: `/Applications/Ferryx.app` is still v2026.930.1 carrying
   helper manifest 2026.917.1; no deployment of the rebuilt bundle has been requested or approved, so no
   live-GUI or installed-app claim is available yet.
2. **Old-agent missing rendezvous** (Layer B): a provider id cannot be attached to an already-running
   agent that lacks the agent-state env; a fresh agent session must own the rendezvous, and no "newest
   agent" guess is allowed. Independent of the socket-path work.
3. **Home path beyond the documented ceiling**: the ~37-byte macOS home bound is covered by tests, but
   there is no length preflight for longer homes, so that case stays unresolved.

No pending tests remain: the Unix service-artifact first-spawn rerun passed (2 passed, 0 failed,
17.00 s, sentinel `DAG_UNIX_FIRST_SPAWN_EXIT0`).

## 6. What this record does not claim

- No end-to-end proof through the installed application: the verified chain is build -> execute ->
  receipt -> stage -> shipped-bundle gate, plus the parent's live handshake, not a live GUI session.
- No claim that the pre-fix behavior is fully eliminated in running sessions; the dev-permissive
  no-manifest branch of the JS CLI still exits 0 by design (the Rust gate is the release lock).
- No claim about cross-architecture runtime behavior beyond the executed probes listed above.
- No all-feature live claim from the Unix first-spawn green: it covers spawning, connecting, closing and
  supervision, not every feature over the wire.
- No live-GUI claim from the 61 UI tests (hook, store and component level only).
- No Windows unit-regression claim beyond the two Windows root cases actually run (unit name, 37-byte
  boundary), both green at revision `8b47`.
- No all-live claim anywhere: the installed application is unchanged (v2026.930.1 carrying helper
  manifest 2026.917.1), and no GUI deployment has been approved.
- No claim that a home path longer than the documented ~37-byte macOS ceiling is handled; it is an
  unresolved edge with no preflight.
