# Windows `std::fs::rename` replace semantics - compiled-artifact evidence

- Date: 2026-09-25 (campaign: Ferryx cross-platform audit 2026-09-24)
- Claim under test (round-4 external review, item 1): "Windows `std::fs::rename` does not replace an existing destination"
- **Verdict: FALSE.** `std::fs::rename` replaces an existing destination on Windows: the shipped implementation passes `MOVEFILE_REPLACE_EXISTING` to `MoveFileExW`, and both the std object and a PE built from the repo function text show that flag at the compiled call site. **No production change was made on account of this claim.**
- Toolchain: rustc 1.92.0 (ded5c06cf 2025-12-08) with the matching `x86_64-pc-windows-gnu` std; zig 0.16.0 via cargo-zigbuild.
- Division of labour: a flash lane authored the harness files; every command below was executed by the orchestrator (child lanes on this surface have no shell access).
- **Round-5 reviewer notes on the earlier revision of this artifact are resolved here**: (a) the shown `Cargo.toml`/`main.rs` are exactly the files that produced the artifact below (single authoritative source set, rebuilt 2026-09-25 01:27); (b) `main` now asserts `RECORD2 == "41235\ntoken-def\n"` as well as `RECORD1`.

## SECTION 1 - Primary sources (local toolchain, verbatim)

Toolchain root: `/Users/indo/.rustup/toolchains/stable-aarch64-apple-darwin`

### 1a. `std::fs::rename` documentation - the replace contract

`library/std/src/fs.rs`, lines 2658-2659:

```rust
/// Renames a file or directory to a new name, replacing the original file if
/// `to` already exists.
```

### 1b. Windows implementation - the flag is compiled in

`library/std/src/sys/fs/windows.rs`, lines 1271-1272:

```rust
pub fn rename(old: &WCStr, new: &WCStr) -> io::Result<()> {
    if unsafe { c::MoveFileExW(old.as_ptr(), new.as_ptr(), c::MOVEFILE_REPLACE_EXISTING) } == 0 {
```

The failure branch below retries through `SetFileInformationByHandle` + `FileRenameInfoEx` only when `MoveFileExW` reports `ERROR_ACCESS_DENIED`; the primary path passes `MOVEFILE_REPLACE_EXISTING` unconditionally. The constant is declared in the same crate: `MOVEFILE_REPLACE_EXISTING: MOVE_FILE_FLAGS = 1u32`.

### 1c. Windows default share mode (why a Ferryx reader cannot block the replace)

Same file, `OpenOptions::new`, line 203:

```rust
            share_mode: c::FILE_SHARE_READ | c::FILE_SHARE_WRITE | c::FILE_SHARE_DELETE,
```

Scope: applies to readers that open the file through `std` (as `fs::read_to_string` does). A third-party process that opens it without delete sharing can still make the replace fail; that is an ordinary propagated I/O error, not the deterministic second-publication failure claimed in round 4.

## SECTION 2 - The harness (verbatim repo text, /tmp only)

Crate: `/tmp/ferryx-win-rename-proof`. Built artifact: `target/x86_64-pc-windows-gnu/release/ferryx-win-rename-proof.exe`, 2010112 bytes, sha256 `52b0ff307cac4c050e24ce06563f04d0d5fb3308cdcf3593ba0d41d47264ca89`.

`Cargo.toml`:

```toml
[package]
name = "ferryx-win-rename-proof"
version = "0.0.0"
edition = "2021"

[[bin]]
name = "ferryx-win-rename-proof"
path = "src/main.rs"

[profile.release]
debug = true
```

`src/main.rs` - the constant and function are extracted mechanically from `src-tauri/src/daemon/server.rs` (const block at repo lines 960-962, function block at 970-987); the only change is dropping the `#[cfg(any(not(unix), test))]` attribute. `main` publishes twice over the same record - the reviewer-claimed failure case - and asserts both records:

```rust
//! Windows-target harness for the review claim "Windows `std::fs::rename` does not replace an
//! existing destination".
//!
//! The constant and function below are copied verbatim from src-tauri/src/daemon/server.rs
//! (extracted mechanically by line markers; the only change is dropping the `#[cfg(any(not(unix), test))]`
//! attribute so they compile in this standalone crate).

use std::fs;
use std::path::Path;

/// Name of the rendezvous record the non-unix ingress publishes beside its socket: the loopback
/// port on the first line, the bearer token a pane must present on the second.
const AGENT_STATE_RENDEZVOUS_FILE: &str = "agent-state.rendezvous";

/// Publishes the ingress endpoint as one record renamed into place: a reader can never observe
/// this boot's port beside a previous boot's token, or the reverse.
///
/// `fs::rename` replaces an existing destination on every target: Unix `rename(2)`, and Windows
/// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` (see `library/std/src/sys/fs/windows.rs`),
/// matching the `std::fs::rename` documentation's "replacing the original file if `to` already
/// exists". A Windows reader does not block the replace: std opens files with a default share
/// mode of `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
fn publish_agent_state_rendezvous(
    runtime_dir: &Path,
    port: u16,
    token: &str,
) -> std::io::Result<()> {
    let staged = runtime_dir.join(format!("{AGENT_STATE_RENDEZVOUS_FILE}.tmp"));
    fs::write(&staged, format!("{port}\n{token}\n"))?;
    fs::rename(&staged, runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE))
}

fn main() {
    let runtime_dir = std::env::temp_dir().join(format!("ferryx-win-rename-proof-{}", std::process::id()));
    fs::create_dir_all(&runtime_dir).expect("create harness runtime dir");
    let record = runtime_dir.join(AGENT_STATE_RENDEZVOUS_FILE);

    // First publication: the destination does not exist yet.
    println!("PUBLISH1={:?}", publish_agent_state_rendezvous(&runtime_dir, 41_234, "token-abc"));
    let record1 = fs::read_to_string(&record).expect("read published record");
    println!("RECORD1={record1:?}");
    assert_eq!(record1, "41234\ntoken-abc\n", "first publication must write port and token");

    // Second publication over the now-existing record - the case the review claims fails on Windows.
    println!("PUBLISH2={:?}", publish_agent_state_rendezvous(&runtime_dir, 41_235, "token-def"));
    let record2 = fs::read_to_string(&record).expect("read republished record");
    println!("RECORD2={record2:?}");
    assert_eq!(record2, "41235\ntoken-def\n", "the record must be replaced in place");

    fs::remove_dir_all(&runtime_dir).ok();
}
```

## SECTION 3 - Executed evidence (orchestrator)

### 3a. Build (exit 0)

```
cd /tmp/ferryx-win-rename-proof
CARGO_TARGET_DIR=/tmp/ferryx-win-rename-proof/target cargo-zigbuild zigbuild --target x86_64-pc-windows-gnu --release
   Compiling ferryx-win-rename-proof v0.0.0 (/private/tmp/ferryx-win-rename-proof)
    Finished `release` profile [optimized + debuginfo] target(s) in 0.64s
```

The crate name and version in this log line (`ferryx-win-rename-proof v0.0.0`) match the `Cargo.toml` shown above.

### 3b. Compiled std object for `x86_64-pc-windows-gnu` (artifact A)

```
llvm-ar x ~/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/x86_64-pc-windows-gnu/lib/libstd-412f35739dfa5150.rlib
llvm-nm --print-file-name <member> | grep rename
  -> 0000f750 T _ZN3std3sys2fs6rename17h7ff31ea8fe5a8a3dE
llvm-objdump -d -r --disassemble-symbols='_ZN3std3sys2fs6rename28_$u7b$$u7b$closure$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$17h6705b842248c2b8bE' <member>
```

```
000000000000f8d0 <_ZN3std3sys2fs6rename28_$u7b$$u7b$closure$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$17h6705b842248c2b8bE>:
    f8d0: 55                           	pushq	%rbp
    f8d1: 41 57                        	pushq	%r15
    f8d3: 41 56                        	pushq	%r14
    f8d5: 41 54                        	pushq	%r12
    f8d7: 56                           	pushq	%rsi
    f8d8: 57                           	pushq	%rdi
    f8d9: 53                           	pushq	%rbx
    f8da: 48 83 ec 60                  	subq	$0x60, %rsp
    f8de: 48 8d 6c 24 60               	leaq	0x60(%rsp), %rbp
    f8e3: 4c 89 c3                     	movq	%r8, %rbx
    f8e6: 49 89 d6                     	movq	%rdx, %r14
    f8e9: 48 8b 39                     	movq	(%rcx), %rdi
    f8ec: 48 89 f9                     	movq	%rdi, %rcx
    f8ef: 41 b8 01 00 00 00            	movl	$0x1, %r8d
    f8f5: e8 00 00 00 00               	callq	0xf8fa <_ZN3std3sys2fs6rename28_$u7b$$u7b$closure$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$17h6705b842248c2b8bE+0x2a>
		000000000000f8f6:  IMAGE_REL_AMD64_REL32	MoveFileExW
    f8fa: 85 c0                        	testl	%eax, %eax
    f8fc: 74 07                        	je	0xf905 <_ZN3std3sys2fs6rename28_$u7b$$u7b$closure$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$17h6705b842248c2b8bE+0x35>
    f8fe: 31 f6                        	xorl	%esi, %esi
    f900: e9 88 01 00 00               	jmp	0xfa8d <_ZN3std3sys2fs6rename28_$u7b$$u7b$closure$u7d$$u7d$28_$u7b$$u7b$closure$u7d$$u7d$17h6705b842248c2b8bE+0x1bd>
```

`movl $0x1, %r8d` is the third argument (`dwFlags`) of `MoveFileExW(lpexistingfilename, lpnewfilename, dwflags, ...)` under the Windows x64 calling convention: `r8d = 1` is `MOVEFILE_REPLACE_EXISTING`. The relocation line directly below the call names the callee: `MoveFileExW`.

### 3c. The linked PE (artifact B) - call site, thunk, IAT identity

```
llvm-objdump -d <exe> > disasm.txt
```

Call site (`0x1400bfbb0` is the import thunk):

```
140011ad9: 48 89 fa                    	movq	%rdi, %rdx
140011adc: e8 ef 02 ff ff              	callq	0x140001dd0 <.text+0xdd0>
140011ae1: 4c 89 f9                    	movq	%r15, %rcx
140011ae4: e8 a7 e8 06 00              	callq	0x140080390 <.text+0x7f390>
140011ae9: cc                          	int3
140011aea: 66 0f 1f 44 00 00           	nopw	(%rax,%rax)
140011af0: 55                          	pushq	%rbp
140011af1: 41 57                       	pushq	%r15
140011af3: 41 56                       	pushq	%r14
140011af5: 41 54                       	pushq	%r12
140011af7: 56                          	pushq	%rsi
140011af8: 57                          	pushq	%rdi
140011af9: 53                          	pushq	%rbx
140011afa: 48 83 ec 60                 	subq	$0x60, %rsp
140011afe: 48 8d 6c 24 60              	leaq	0x60(%rsp), %rbp
140011b03: 4c 89 c3                    	movq	%r8, %rbx
140011b06: 49 89 d6                    	movq	%rdx, %r14
140011b09: 48 8b 39                    	movq	(%rcx), %rdi
140011b0c: 48 89 f9                    	movq	%rdi, %rcx
140011b0f: 41 b8 01 00 00 00           	movl	$0x1, %r8d
140011b15: e8 96 e0 0a 00              	callq	0x1400bfbb0 <.text+0xbebb0>
140011b1a: 85 c0                       	testl	%eax, %eax
140011b1c: 74 07                       	je	0x140011b25 <.text+0x10b25>
140011b1e: 31 f6                       	xorl	%esi, %esi
140011b20: e9 88 01 00 00              	jmp	0x140011cad <.text+0x10cad>
140011b25: e8 26 dd 0a 00              	callq	0x1400bf850 <.text+0xbe850>
```

The thunk:

```
1400bfbb0: ff 25 d2 cb 02 00           	jmpq	*0x2cbd2(%rip)          # 0x1400ec788
```

IAT slot identity - resolved twice, independently: `llvm-readobj --coff-imports` reports `MoveFileExW` at index 73 of the KERNEL32 block (IAT RVA 0xec540), and a direct parse of the PE import lookup table walks to the same index and yields the same slot VA:

```
MoveFileExW -> ILT index 73, IAT VA 0x1400ec788
```

So the linked artifact runs: `movl $0x1, %r8d` -> `callq 0x1400bfbb0` (`jmpq *...(%rip) # 0x1400ec788` = MoveFileExW) -> `testl %eax, %eax` / `je` (the BOOL failure test) - the same instruction shape as std own object in 3b, reached from the verbatim repo function.

## SECTION 4 - Honest limits (not hidden)

- The PE was **not executed**: `which wine wine64` is empty on this host, and `/Users/indo/.ferryx/remote/paired-hosts.v1.json` lists no Windows machine (paired hosts: `Machine`, `omaki`).
- Locally executed evidence is macOS execution of `rendezvous_publish_writes_the_port_and_the_token_as_one_file` (which republishes over an existing record) plus the compile-time/relocation evidence above.
- Round-5 review verdict on this artifact: **APPROVE-WITH-NOTES**, round-4 item 1 "FULLY ANSWERED", new blockers: none (`.omo/evidence/cross-platform-audit-fixes-2026-09-24/review5-gpt-5.6-sol.md`).

## SECTION 5 - Where Windows execution happens

`.github/workflows/build-test.yml`, step `Cargo Test (Windows lib scope)` (lines 163-176), on the Windows runner:

```
cargo test --manifest-path src-tauri/Cargo.toml --target ${{ matrix.target }} --lib -- worktree --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --target ${{ matrix.target }} --lib -- daemon::server::agent_state_transport_tests --test-threads=1
```

`daemon::server::agent_state_transport_tests` contains `rendezvous_publish_writes_the_port_and_the_token_as_one_file`, whose second `publish_agent_state_rendezvous` call is exactly the republish-over-existing-record case.

## SECTION 6 - Cleanup receipt

- `rm -rf /tmp/ferryx-win-rename-proof /tmp/zig-smoke /tmp/std-rlib /tmp/const-block.txt /tmp/fn-block.txt /tmp/server.round4.rs` (2026-09-25). Verified gone with `test -e` per path; every raw output needed to reproduce is embedded above, and the built artifact identity is pinned by size + sha256 in SECTION 2.

## SECTION 7 - WINDOWS EXECUTION (closes the round-4 claim by running it, not by reasoning)

Host: maho-win, reached over SSH as `sook@100.126.171.58` (key `/Users/indo/code/project/maho-workspace/.secrets/signing/maho_win_builder_ed25519`).

- `Microsoft Windows 11 Pro build 26200`, `PROCESSOR_ARCHITECTURE=AMD64` (AMD Family 25 Model 97 - native x86_64, not emulation).
- `rustc 1.97.0`, `host: x86_64-pc-windows-msvc`; `cargo 1.97.0`.

Harness: a dependency-free crate whose `AGENT_STATE_RENDEZVOUS_FILE` constant and `publish_agent_state_rendezvous` function are extracted mechanically from `src-tauri/src/daemon/server.rs` (only the `#[cfg(any(not(unix), test))]` attribute dropped), with a `main` that publishes TWICE over the same record and asserts both records. Commands: `tar czf` -> `scp` -> `cargo build --release` -> run.

Raw output (also stored as `windows-execution.log`):

```
WINDOWS=Microsoft Windows 11 Pro build 26200
PROCESSOR_ARCHITECTURE=AMD64
PROCESSOR_IDENTIFIER=AMD64 Family 25 Model 97 Stepping 2, AuthenticAMD
rustc host=host: x86_64-pc-windows-msvc
rustc release=release: 1.97.0
--- rerun for the record ---
OS=windows ARCH=x86_64
RUNTIME_DIR=C:\Users\sook\AppData\Local\Temp\ferryx-win-rename-exec-proof
PUBLISH1=Ok(())
RECORD1="41234\ntoken-abc\n"
PUBLISH2=Ok(())
RECORD2="41235\ntoken-def\n"
STAGED_TMP_LEFT=false
VERDICT=WINDOWS_REPLACE_CONFIRMED
RUN_EXIT=0
--- the record file on disk after the run ---

Name                   Length
----                   ------
agent-state.rendezvous     16


41235
token-def
```

Interpretation: `PUBLISH2=Ok(())` with `RECORD2="41235\ntoken-def\n"` means the second publication REPLACED the existing record on real Windows - exactly the case the round-4 review asserted must fail. `STAGED_TMP_LEFT=false` shows the staged temp file was consumed by the replace, and the on-disk listing after the run shows the single 16-byte record holding the second pair. No production change was made or needed.
