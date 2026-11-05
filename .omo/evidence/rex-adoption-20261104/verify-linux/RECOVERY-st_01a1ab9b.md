# Linux recovery receipt - st_01a1ab9b

Requested candidate: 4167f91e1c2ed9c29e6959274f6f497e163d0269. No product edits, local builds/tests, paid hosts, production process changes, publishing, or main pushes.

## Recovered state

Worktree branch rex/verify-linux remains at c9bfb6b1c before this evidence commit. The three inherited dirty files were read and preserved: 03e0d6b5/driver.log and bd4d3459/{account-qa.mjs,extra-qa.mjs}. Historical SUMMARY.md and verdict.json are retained unchanged because they explicitly refer to 35c984684, not this candidate.

| source revision | run | result / receipt |
| --- | --- | --- |
| 708e0cfb | 41960022396 | checks, plugin Rust/UI, typecheck pass; default and headless clippy exit 101. 708e0cfb/gha-recovered/ |
| bd4d3459 | 41964894364 | OSC, supervisor tests in both modes, both checks, measured Linux x64 helper pass. bd4d3459/gha-recovered/ |
| bd4d3459 | 41968711562 | account harness incorrectly rejected legitimate login 202; extra startup ENOENT. bd4d3459/live-corrected-recovered/ |
| 03e0d6b56 | 41970713685 | exact SSH refusal in both modes, both checks, bins exit 0; account/extra harnesses exit 1. 03e0d6b5/gha-recovered/ |
| 03e0d6b56 | 41974624352 | bins exit 0 (542s); account live exit 0 (1s); extra exit 1 (3s); fourth gate exit 0. 03e0d6b5/live-final-recovered/ |
| 03e0d6b56 | 41976072348 | bins exit 0 (861s), extra live exit 1 (1s). Workflow head dd7a782852bf2ce42b82dc562fe42b6697eb0fe3 is not the tested source revision. 03e0d6b5/grant-gha/ |

Newly recovered account proof from 41974624352 gate 2: unauthenticated PUT 401, login request 202, consume 200, authenticated onboarding PUT/GET 200, machine-only enroll PUT 400 MACHINE_STEP_NOT_SYNCABLE. ACCOUNT_SYNC_LIVE_OK and recorded account PID stopped. This closes historical account coverage at 03e0d6b56 ONLY.

41976072348 gate 2: foreign layout import, exact script timer and compiled clock approval mismatch/disable PASS. Grants FAIL with reactor panic at grants.rs:83. Agent inventory is informational only, not logged-in agent QA.

## Current execution blocker (observed 2026-11-05)

Initial `gh run view 41976072348 --repo Indosaram/ferryx --json status,conclusion,headSha,url` succeeded and confirmed completed/failure. Initial `git ls-remote origin 'refs/heads/ci/rex-verify-linux*'` exited 0 with no output: both inherited refs were already absent. No new refs were pushed.

Subsequent `gh run list --repo Indosaram/ferryx --limit 6 --json databaseId,headSha,status,conclusion,name` failed exit 1: error connecting to api.github.com. `curl -I --connect-timeout 8 --max-time 12 https://api.github.com` failed exit 28: Resolving timed out after 8009 milliseconds. Resolver configuration points at Tailscale 100.100.100.100. Diagnostic `dig +time=3 +tries=1 @1.1.1.1 api.github.com A` and github.com timed out. A bounded direct-IP HTTPS diagnostic using `curl --resolve api.github.com:443:140.82.114.5 --connect-timeout 5 --max-time 10 https://api.github.com/meta` also failed exit 28 / HTTP=000. No DNS, network, or service configuration was changed. No blind gate rerun was launched.

Canonical `scripts/pick-build-host.sh --help` is not a help route: its inspected implementation runs the picker. It returned no idle free host, creates no claim on this outcome. omarchy: load 11.12, 2 builds, idle=0; hark: free RAM 5G, free disk 5G, idle=0; other hosts unreachable. No lease was acquired; no release action is owed. No process was started remotely or left behind by this recovery.

Thus new-head clippy, Linux acceptance and live QA are BLOCKED by execution access, not waived or passed. Per-todo status for the 23 inherited Linux assignments is in 4167f91e/per-todo-receipts.json. Coverage still owed from CHECKPOINT remains open except the explicitly historical account receipt above. Foreground secret suppression, OSC streaming, browser isolation, logged-in agents, AppImage/dbus links, crash recovery, plugin daemon death/adversarial live cases and protocol/GUI matrix have not been newly exercised. Release/proptest/fuzz and release A/B benchmark additionally require valid full helper staging.

## Helper source-hash diagnosis and Windows coordination packet

4167f91e/helper-divergence.json compares the actual bd4 Linux and Windows receipts against Git source bytes. All 19 Linux source hashes equal bd4d3459 Git blobs. All 19 Windows hashes equal those blobs expanded to CRLF. This is checkout representation divergence, not an unexplained compiler difference. Existing receipt generation hashes actual bytes and freshness validation demands those bytes; never rewrite old receipt hashes or normalize only during staging.

For Windows verifier st_01a1ab77: use an owned clean checkout with LF representation (`core.autocrlf=false` before materializing tracked source), verify all closure files against the chosen common revision, then rebuild and execute the helper and regenerate its receipt on Windows. Do not reuse an old binary with a rewritten source hash. Linux x64 receipt 8f02c2b160b5cb7369ab5c1c4a4e78d37b79ebcaa099d6c1621fbb37df23c295 and Windows 400dca582fba8fe412ca1158f9a6adcc87470f72271ac3826827112cfb26d56d cannot currently stage together. New-head source closure freshness must also be checked, not inferred from bd4. Still missing measured native pairs: aarch64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-apple-darwin. No cross-compilation or image string scan substitutes for native capability receipts. Direct delivery to the sibling's session was unavailable; this durable packet is the coordination handoff.

## Clippy source findings (NOT a new runtime receipt)

Historical exact commands and exits: revision-results.json plus 708e0cfb/gha-recovered/rex-verify-linux-gate/{7,8}.log, both exit 101. Default reports items_after_test_module at runner.rs:99 with entry:143 and run_stdio:148 after tests. Headless additionally reports unused NATIVE_DEBUG_SEQUENCE at ipc/debug.rs:34.

Source inspection of 4167f91e is saved in 4167f91e/clippy-source-inspection.txt. runner.rs still places tests before entry/run_stdio. This file is absent from local main and was introduced in candidate history by a9dfb2c4a, so this is candidate-introduced rather than a main baseline failure. Smallest repair: move the complete test module to the end of runner.rs, with no suppression or behavior change. No edit made.

NATIVE_DEBUG_SEQUENCE now has cfg(feature = "desktop"), matching its consumer; inspected diff 078431350^..078431350 shows the guard. It was an earlier candidate headless issue, source-repaired on 4167f91e, not remotely reverified here. No claim that new-head clippy passes or has only these diagnostics. Same-runner main comparison remains unexecuted; no additional failures are classified as pre-existing.

## Ready-to-resume commands (remote only)

Use a revision-pinned one-off ci/* workflow on ubuntu-24.04. Import native commit 216426b657b93d8fadd11d7506615f744581221e using the documented source bundle in docs/qa/rex-terminal-build-repair-20261104.md; do not attempt an ordinary upstream fetch. Existing /tmp/rex-03-live.sh contains historical transport logic but has a hard-coded old source SHA and polling loops: do NOT run it unchanged. Arm completion monitoring before dispatch; preserve exact run/workflow/source SHAs, command exits, raw logs and per-scenario results, then delete only owned ci refs after consuming artifacts.

First eligible new-head commands:
- cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
- cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --no-default-features --locked -- -D warnings
- cargo build --manifest-path src-tauri/Cargo.toml --bins --features local-split-qa --locked
- Execute preserved account-qa.mjs and extra-qa.mjs against isolated daemon/account processes with explicit PID teardown; grants is a known product failure until the repair lane revision is integrated. Keep each revision's result separate.

## Outcome

Recovery and source-hash diagnosis PASS. Historical grants scenario FAIL. New-head execution BLOCKED. Cleanup PASS for resources owned by this recovery (none created); inherited temporary refs were already absent at the successful initial lookup. No claim of overall acceptance or plan completion.
