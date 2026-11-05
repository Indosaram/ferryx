# Terminal/build repair handoff (code-only)

Base: `35c984684b60c40dd6d29cd8703270fe86c30580`.
No builds, tests, lint, diagnostics, QA, or binary generation were run in this round.
Only source inspection and `git diff --check` were performed. Automated failures
are not covered by the user's macOS manual/GUI waiver.

## Causes and limits

### Windows bd4d3459 CLI link and headless cfg follow-up

Integration was fast-forwarded through `bd4d3459d` before this patch.
Evidence is in the sibling Windows worktree under
`.omo/evidence/rex-adoption-20261104/verify-win/bd4d3459/`:

- `runtime-build.ps1:3-6` sets a shared target directory, disables Cargo
  incremental compilation, and invokes `cargo build --manifest-path
  src-tauri/Cargo.toml --locked --bin ferryx-cli`. It does not select release
  or disable default features: this is a dev build with desktop and
  remote-desktop defaults, not a headless CLI build.
- Decoded UTF-16 `runtime-build.log:16-20` identifies Microsoft's x64
  `link.exe`, Visual Studio 18 BuildTools, MSVC `14.51.36231`.
  Lines 200-204 identify `debug/deps/ferryx_lib.dll`, `/DLL`,
  `/OPT:REF,NOICF`, `/DEBUG`, `/PDBALTPATH:%_PDB%`, and Natvis inputs.
  Lines 3025-3029 report LNK1140 and exit 101 compiling the library.
  The failing output is the library DLL, before a fresh CLI is available.
- No `/PDBPAGESIZE` is present in that invocation. Add `/PDBPAGESIZE:8192`
  only for Windows MSVC package links, including the cdylib and executables,
  without changing `/DEBUG`, stripping symbols, suppressing warnings, or
  substituting a binary. This raises page-based PDB capacity from 4 GiB to
  8 GiB and enlarges block maps. It does not prove which internal PDB limit
  this log reached or that 8 GiB is sufficient; rebuilding remains required.
- Compatibility reference: Chromium's Windows linker configuration documents
  support in VS 2022+ link.exe and consumption with circa-22H2 or newer tools:
  <https://chromium.googlesource.com/chromium/src/build/config/+/refs/heads/main/win/BUILD.gn>.
  The observed MSVC toolchain is newer than that minimum. No linker switch to
  LLVM, debug-level reduction, or `/PDB:NONE` is required by this patch.
- `NATIVE_DEBUG_SEQUENCE` is only consumed by desktop-gated
  `log_native_switch_debug`; its declaration now has the identical desktop
  cfg. Headless builds omit the unused counter rather than suppressing lint.
  Other shared atomics and native run-ID state stay available.

Deferred Windows rebuild, from the patched verification checkout in the same
MSVC developer environment (not run in this source-only round):

```powershell
$ErrorActionPreference = 'Stop'
$env:CARGO_TARGET_DIR = 'C:\Users\sook\ferryx-wt\.target'
$env:CARGO_INCREMENTAL = '0'
$env:CMAKE_POLICY_VERSION_MINIMUM = '3.5'
$env:LIBCLANG_PATH = 'C:\Users\sook\ferryx-wt\.llvm\clang+llvm-23.1.4-x86_64-pc-windows-msvc\bin'
cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli -vv
if ($LASTEXITCODE -ne 0) { throw "CLI rebuild failed: $LASTEXITCODE" }
```

This deliberately keeps the failing command's default features and dev
profile. The build-script change invalidates the package build; no clean or
older binary is needed. Inspect verbose build-script output for
`/PDBPAGESIZE:8192`. Existing OpenSSL LNK4099 warnings in the supplied log are
not repaired or hidden by this patch. Platform argument contract tests were
added but not executed; headless warning elimination is source-reviewed only.

Evidence paths below are relative to the sibling verification worktrees' shared
`.omo/evidence/rex-adoption-20261104/` directory.

- Linux `verify-linux/gha3/rex-verify-linux-gate/14.log` and macOS
  `verify-mac/hash-qa.log` report less than 10000 retained rows. At the pinned
  Ghostty revision, `PageList.zig:7087` prunes whole pages as soon as history
  exceeds the requested line limit, undershooting that limit. The mirror now
  allows one bounded page of internal slack for extended budgets, disables the
  independent default byte cap, and exports only the newest requested rows in
  both seed and hash. Added exact-10000 replay/continuation regression coverage.
  This changes physical parser retention, not the accepted external budget.
- macOS `verify-mac/raw/full-lib.log:6161`: OSC cwd None. `url` 2.5.8
  `set_host(None)` removes the serialized host but retains its cached Host;
  Unix `to_file_path` rejects it. `set_host(Some(""))` resets both. Added a
  host-qualified, percent-encoded local path regression.
- macOS `verify-mac/raw/full-lib.log:6173`: resize restoration count 0.
  The new default `detach_size` was a no-op instead of delegating to legacy
  `restore_desktop_geometry`. Restored fallback, kept daemon's authoritative
  ownership override, and gated raw-socket detachment on an accepted resize.
  Replaced fixed sleeps in this regression test with exact backend signals.
- Windows `verify-win/release.utf8.log` ends with ffmpeg-sys-next build script
  STATUS_STACK_OVERFLOW, not an SDK/header/linker diagnostic. The existing
  Cargo dev-only bindgen optimization documents this same 1 MiB Windows stack
  cause. Cargo release build dependencies are also unoptimized by default;
  added the missing release bindgen override. This is a source-grounded repair
  hypothesis, NOT a remotely reproduced or verified stack trace.
- macOS `verify-mac/REPORT.md`: short UDS roots and explicit async iterator
  fix the two observed QA harness API/path errors without changing assertions.
- Helper freshness is intentionally still FAIL until all five artifacts are
  rebuilt and staged. Found an additional omitted compiled dependency:
  standalone.rs imports worktree/naming.rs; it is now in the fingerprint
  closure, with stale-receipt regression coverage. Never rewrite only the hash.

### Malformed-stream seed property: history-boundary root cause repaired

Linux `verify-linux/gha3/rex-verify-linux-gate/1.log:7844-11953` records
proptest seed `ffb19fa8fe082322db4d10035ec8c3a6ee1eac4c3b4745cd834cd5192abf0dc6`.
The exact 4096-byte input at 115 columns, 2 rows remains an unchanged checked-in
fixture. Source tracing establishes a stale spacer head rather than a lost
physical `+` cell:

1. The input contains a wide CJK character (UTF-8 at offset 769), controls that
   scroll rows into history, and subsequent overwrites at the left edge. At
   this two-row height a wrapped wide character can reach active row zero with
   its preceding spacer head already in history.
2. Both `Terminal.writeCell` cleanup branches (`.wide` and `.spacer_tail`)
   required `cursor.y > 0` before clearing the preceding spacer head. That
   incorrectly equates active row zero with the first physical retained row.
   It leaves a stale spacer head after the wide character is overwritten.
3. `selection::screen_row_text` uses a non-rectangular, unwrapped selection;
   native `PageListFormatter.format` deliberately extends an endpoint on a
   spacer head to the first cell of the following row. Thus original row 12
   reports the following row's `+` as its final character. The VT formatter,
   using physical rows (`unwrap=false`), skips the spacer head and does not
   emit this phantom `+`. Replaying VT therefore exposes the stale source
   invariant rather than losing a legitimate physical cell.
4. Native commit `216426b657b93d8fadd11d7506615f744581221e` makes
   `cursorCellEndOfPrev` optional and uses the physical pin's `up(1)` in both
   cleanup branches, including when the previous row is history. Only an
   actual spacer head is cleared; real narrow text and wide spacer tails are
   preserved. The formatter, FFI ABI, VT row/wrap/cursor serialization, and
   existing property acceptance are unchanged.

The native deterministic test constructs both wide-cell and tail overwrites
at 115x2 after the head scrolls into history. The exact Rust fixture test now
also requires unnormalized physical-row equality and exact grid hashes after
SGR output, final-column pending wrap, subsequent text, and cursor rewriting;
it additionally requires seed fixed-point equality. No mismatch is folded away.
These are source-grounded fixes and unexecuted tests, NOT a passing runtime
claim. The native pin and source lock are advanced together, preventing stale
native artifact-cache reuse.

Integration `ebf2150a9` was merged into the lane at `8815f82b2`; the watcher
now retains both explicit async iteration and the UI lane's cancellation/error
handling, alongside the short macOS socket root.

### Native source transport before consolidated verification

The new native commit is local and has NOT been pushed upstream. The parent
must transport this source commit as well as the superproject revision; an
ordinary upstream submodule fetch cannot find it yet. From the owned native
repository (not the canonical read-only clone), create a source bundle for
remote transfer:

```sh
git -C src-tauri/vendor/ghostty bundle create /tmp/rex-history-spacer.bundle rex/repair-history-spacer ^83fb95624b0439ee660c2679b43b38c8b63769cd
```

On each remote isolated verification worktree, first initialize the upstream
base, then import the transferred source bundle before updating the pin:

```sh
git -C src-tauri/vendor/ghostty fetch /tmp/rex-history-spacer.bundle refs/heads/rex/repair-history-spacer
git -C src-tauri/vendor/ghostty checkout --detach 216426b657b93d8fadd11d7506615f744581221e
git submodule update --init --recursive
```

For a fresh remote worktree, initialize the submodule at superproject base
`35c984684` before checking out the consolidated superproject revision; otherwise
initialization attempts to fetch the private native commit from upstream.
On Windows use the actual transferred bundle path instead of `/tmp/...`.

## Remote provisioning (deferred, do not run on owner's Mac)

### Windows cwd follow-up on 708e0cfb7

Integration was fast-forwarded into the owned repair branch before this fix.
Raw `verify-win/708e0cfb/original-failures.utf8.log:78-83` confirms the
unchanged expectation table still reports None instead of `C:\Temp`.
The previous empty-host mutation was not portable: url 2.5.8's
`set_host(Some(""))` stores `Host::Domain("")`, and its Windows
`to_file_path()` passes that as `Some("")` to the UNC conversion branch.
That branch prepends two backslashes and appends `\C:\Temp`, instead of
taking the drive-letter branch. The resulting malformed UNC path fails
`Path::is_absolute`, causing parse_cwd to return None. This is downstream
of the callback; the fixture is valid and remains unchanged.

The fix reparses `file://localhost` plus the still-encoded URI path. The
file URL parser canonicalizes localhost to no host; Windows conversion
then takes its existing drive-letter branch and percent-decodes once.
Query/fragment rejection and the absolute-path check remain unchanged.
Added Windows direct conversion tests for remote/localhost/empty hosts,
encoded paths, and query/fragment rejection, plus split-input callback
and muted seed replay tests using both BEL and ST terminators. All are
unexecuted in this code-only round; the original cwd assertion is intact.

The requested stall log is at
`verify-win/d8313422b/full-lib-timeout.utf8.log`, not under 708e0cfb.
It records PID 33368, 1707.21875 CPU seconds, no output for over 27
minutes, and a bounded timeout. It identifies no test, stack, module,
or parser operation. Therefore this evidence does NOT attribute the
stall to terminal changes or exclude them. No source stall investigation
or new execution was undertaken absent an identifying trace.

Use isolated remote worktrees of the final consolidated source revision, with
the pinned submodule initialized. Hold the normal host claim. On native Linux
x86_64, Linux aarch64, macOS aarch64, macOS x86_64, and Windows x86_64 hosts,
respectively choose one exact TARGET from:

```
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
aarch64-apple-darwin
x86_64-apple-darwin
x86_64-pc-windows-msvc
```

Unix host commands, from that isolated repository root (TARGET must match host):

```sh
git submodule update --init --recursive
mkdir -p .omo/helper-receipts
cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target "$TARGET" --target-dir remote-helper/target
bun scripts/helper-build-receipt.mjs "$TARGET" "remote-helper/target/$TARGET/release/ferryx-remote-helper" ".omo/helper-receipts/$TARGET.json"
# Required debug helper prerequisite for existing SSH tests:
cargo build --manifest-path remote-helper/Cargo.toml --locked --target-dir remote-helper/target
```

Windows native PowerShell, same isolated final-source root:

```powershell
git submodule update --init --recursive
$target = 'x86_64-pc-windows-msvc'
New-Item -ItemType Directory -Force .omo/helper-receipts
cargo build --manifest-path remote-helper/Cargo.toml --locked --release --target $target --target-dir remote-helper/target
if ($LASTEXITCODE -ne 0) { throw 'helper build failed' }
bun scripts/helper-build-receipt.mjs $target "remote-helper/target/$target/release/ferryx-remote-helper.exe" ".omo/helper-receipts/$target.json"
if ($LASTEXITCODE -ne 0) { throw 'helper execution/receipt failed' }
cargo build --manifest-path remote-helper/Cargo.toml --locked --target-dir remote-helper/target
if ($LASTEXITCODE -ne 0) { throw 'debug helper build failed' }
```

Transfer each measured receipt AND its exact executable to a remote staging
worktree at the identical final source. Put binaries at
`.omo/helper-artifacts/<target>/ferryx-remote-helper[.exe]`; put receipts in
`.omo/helper-receipts/<target>.json`. From the staging worktree:

```sh
bun --eval 'import {readFileSync,writeFileSync} from "node:fs"; import {resolve} from "node:path"; const targets=["x86_64-unknown-linux-gnu","aarch64-unknown-linux-gnu","aarch64-apple-darwin","x86_64-apple-darwin","x86_64-pc-windows-msvc"]; const receipts=targets.map(t=>({...JSON.parse(readFileSync(`.omo/helper-receipts/${t}.json`,"utf8")),executable:resolve(`.omo/helper-artifacts/${t}/ferryx-remote-helper${t.endsWith("msvc")?".exe":""}`)}));writeFileSync(".omo/helper-stage.json",JSON.stringify({repoRoot:process.cwd(),receipts}));'
bun scripts/build-remote-helpers.mjs .omo/helper-stage.json
bun scripts/build-remote-helpers.mjs --check-bundled "$PWD"
```

Stop on any nonzero command. Staging checks every source digest, actual binary
hash, version, capability evidence and target ABI before publishing the manifest.
No fingerprint bypass or artifact discovery fallback is permitted. Transfer the
resulting entire `src-tauri/resources/helpers` tree to each verification worktree
before release/proptest/fuzz gates. Run locked default Windows release with a
fresh release output directory to evaluate the bindgen hypothesis. Preserve raw
logs and report any continued stack overflow as unresolved, not an SDK failure.
