# Windows 03e0d6b verification

Head 03e0d6b56e77ad4fd8a3defee5abd98a3fd4b942; native source bundle imported at 216426b657b93d8fadd11d7506615f744581221e. Local evidence branch merged candidate. No product changes, local builds, paid resources or production manipulation. Commands/environment and per-command HEAD are in gates.ps1/exits.log. One sequential remote run, no duplicate builds.

- FFmpeg, fresh native helper debug/release + measured receipt, UI frozen install/build: exit 0.
- Exact default CLI invocation `cargo build --manifest-path src-tauri/Cargo.toml --locked --bin ferryx-cli -vv`: exit 101. link.exe exit 1140, LNK1140 persists despite four /PDBPAGESIZE:8192 occurrences in verbose log. Raw cli-default.log, readable cli-default.utf8.log. Build log also reports unavailable rustfmt during bindgen; fatal gate remains linker error. No symbols stripped, linker swapped, workaround flags or old binary used. Repair packet: terminal/build author must investigate actual PDB capacity/limit using preserved full invocation; page-size change alone is insufficient on this host.
- Focused two-test command: inventory parent passed; SSH remote-stage test stalled with almost no CPU since 13:59, identity explicitly printed. CDB non-invasive all-thread stack captured for runner 17444 (stall-stack-17444.log, exit 0); symbols unresolved Rust module offsets but OS wait frames retained. Stopped only recorded runner at 14:33, command exit -1. Not a pass, not baseline/flake. Same test subsequently passes in full serial suite; investigate focused parallel fixture teardown/shared state rather than infer nondeterministic success.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked --test platform_build_contract`: exit 101 before assertions, same LNK1140 library link failure. No assertion pass claimed.
- `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features --locked --all-targets`: exit 0; no warning lines. NATIVE_DEBUG_SEQUENCE warning absent.
- Full library `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib -- --test-threads=1 --nocapture`: completed naturally, exit 101; 5116 passed, 27 failed, 10 ignored, 1111.95 test seconds. BOTH repaired named tests pass here. failure-comparison.json lists every failure and exact set comparison with bd4d3459/failure-classification.json: no new identities and none of prior 27 absent. These are exact baseline test matches, not same-root-cause or flake assertions. Prior 70/21 receipts remain history, not rerun proof.

Individual blockers:
- Live pane-denial: no newly linked default CLI due LNK1140.
- Idle-restart live QA: same binary prerequisite.
- Enabled-plugin daemon-death live QA: same binary prerequisite.
- Fresh default release: complete five-target staged helper tree unavailable. Fresh Windows helper executable and measured receipt handed off in sibling Linux verifier evidence; fingerprint and remeasured binary hash in HELPER-HANDOFF.txt.
- Windows GUI remains previously blocked by unusable MahoRD capture; no retries this run and no waiver/pass.

Cleanup: logs/receipt/executable copied first; remote worktree and per-worktree target removed, prune run, owned remote scripts/bundles removed. No tunnels opened this run. Production/foreign processes and shared target untouched. See cleanup.log. Evidence committed after report creation; no push.
