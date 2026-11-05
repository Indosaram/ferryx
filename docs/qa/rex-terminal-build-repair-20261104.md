# Terminal/build repair handoff (code-only)

Base: `35c984684b60c40dd6d29cd8703270fe86c30580`.
No builds, tests, lint, diagnostics, QA, or binary generation were run in this round.
Only source inspection and `git diff --check` were performed. Automated failures
are not covered by the user's macOS manual/GUI waiver.

## Causes and limits

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

### Still open: malformed-stream seed property

Linux `verify-linux/gha3/rex-verify-linux-gate/1.log:7844-11953` records
proptest seed `ffb19fa8fe082322db4d10035ec8c3a6ee1eac4c3b4745cd834cd5192abf0dc6`.
The exact 4096-byte input at 115 columns, 2 rows is now a checked-in fixture
and deterministic test. Original row 12 has a final-column `+` that is absent
after replay; no assertion or normalization was relaxed. The actual native
formatter/parser cause is NOT established or repaired. The new deterministic
regression is expected to remain red until that repair; do not approve this lane
as complete based on the other source changes.

## Remote provisioning (deferred, do not run on owner's Mac)

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
