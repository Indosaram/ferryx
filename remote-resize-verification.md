# Remote resize verification handoff

The detailed report is retained on maho-win at `C:\Users\sook\ferryx-verify-resize-20261002\remote-resize-verification.md`.

Its SHA-256 is `dd22ec0d68405a354b22b1a9e39921a80adff403fdae9073a2a8acf40378571b`. The remote report includes exact commands, UTC+9 timestamps, exits, source hashes, and log receipts; its 12-entry raw-log/receipt manifest was checked against the remote files.

All 11 admitted resize source hashes matched. `cargo check --lib --message-format=json` exited 101 because `service.rs:1194` has an unexpected closing delimiter after a duplicated test fragment. Both focused resize test suites exited 101 on the same parser error. The lock-removal mutation was not run because the test binary cannot compile. A fresh baseline `tsc --noEmit -p tsconfig.json` run exited 2 and reproduced the known `ProjectDialogs.tsx(824,25)` TS2339 (`RemoteWorktree` has no `path`).

No local build/test, shared source edit, or foreign-process termination occurred. No codec 5a97, desktop, image/native/share, or handover changes were integrated.

## Corrected selectors and rerun gate

The test module is `terminal::service::preparation_tests` (declared at `src-tauri/src/terminal/service.rs:890`), not `terminal::service::tests`. The focused lock-barrier test is `test_real_backend_await_barrier_serializes_concurrent_preemption_and_rejects_stale_apply`. Once the source owner admits a repaired source hash, run one monitored remote job sequentially: first `cargo test --lib terminal::service::preparation_tests::test_real_backend_await_barrier_serializes_concurrent_preemption_and_rejects_stale_apply`, then (only if that succeeds) `cargo test --lib terminal::service::preparation_tests`. Use one unique target directory and do not overlap Cargo processes. Do not run either command against source hash `82bccb2b68ab541e5084fec4491a3f601c11d6cbe46874fbaf5479bfd3b42302`; the remote task root still has that stale hash and owner repair admission is pending.

All 12 raw logs and exit receipts are archived locally under `.omo/evidence/superlogical-parity/remote-resize-20261002/`. The lead independently compared remote `Get-FileHash` values with local `shasum` values; this session independently recalculated the local SHA-256 values, which match the recorded remote hashes for all 12 files, including the 574,848-byte diagnostics JSON log. The archive therefore resolves the earlier output-truncation concern. The earlier two test invocations overlapped and contended on Cargo's build-directory lock, so their wall times are not independent test timings.

## Current rerun status


At the latest admission check, the remote task-root service source still hashed to `82bccb2b68ab541e5084fec4491a3f601c11d6cbe46874fbaf5479bfd3b42302`; no repaired owner hash/manifest entry was available. Corrected selectors are prepared above, but no corrected tests, diagnostics, build, or manual behavior check ran against the stale hash. The historical compiler receipt's `CHANGED_DIAGNOSTIC_COUNT=0` is not a clean result: rustc failed while parsing before meaningful changed-file diagnostics could be established. The prior test outcomes are historical failed attempts only; they do not establish green verification.

The designated source owner is `87d`. Await its corrected `service.rs` admission and new hash before any rerun. The request to await is not authorization to rerun against the unchanged source.

## Owner 87d final admission and intermediate verification

The final owner manifest at `/tmp/ferryx-resize-syntax-followup/resize-syntax-followup.manifest` records baseline `82bccb2b68ab541e5084fec4491a3f601c11d6cbe46874fbaf5479bfd3b42302`, final work source `9141c5bd7cd06b7829a3578849b648fb9e28a3da5abe19938d288c2904874e75`, and patch `469ae60fd5459a1b0643b2c8b4422b5229a9ddb1eda68860e494ef1df4c2e1f7`. The manifest hash is `d622a32298783d8d199f4b9c0074b61970199ad1e3ad391f51c978ce9ce68207`.

The remote check and focused test below ran against the intermediate source hash `432295effca53c7a05b038b7e9788486accb126a4a97a51896a58a0f974121ba`, not the final hash. Final source comparison found a one-blank-line difference; this is still a source hash mismatch and these results are not final-source verification.

| Command | UTC+9 start–end | Exit | Result |
|---|---|---:|---|
| `cargo check --lib --message-format=json` | 2026-10-02 02:41:44–02:45:04 | 101 | Six error diagnostics: `src/remote/server.rs:2448` E0433; `src/daemon/session_service.rs:2463` E0308; `src/remote/server.rs:1960` E0382; `src/remote/server.rs:3100` E0382; `src/terminal/service.rs:739` E0308; `src/terminal/remote.rs:631` E0599. Raw JSON contains broader diagnostics; no fixes were made. |
| `cargo test --lib terminal::service::preparation_tests::test_real_backend_await_barrier_serializes_concurrent_preemption_and_rejects_stale_apply -- --nocapture` | 2026-10-02 02:46:27–02:50:48 | 101 | Lib-test compilation failed with 38 errors and 47 warnings; zero tests executed. The sequential wrapper skipped `terminal::service::preparation_tests` and `terminal::resize_lease::tests` after this failure. |

These six logs/receipts are archived under `.omo/evidence/superlogical-parity/remote-resize-20261002/`. Local SHA-256 values: `resize-check-raw.log` `35b8aa76ab30ea84eb4176c6c8b9ebcb28e52c3efdc72aa87855df09686d803d`; `resize-check-stderr.log` `aaa7b28f62fd73afd52d318a1a92d3c23858e49de58439da8c6f5159a798cc85`; `resize-check-exit.txt` `d7f06b6ff7d956a488f92d549edc6ac7d46f9b43aad24b3ba8a9ca4bbff95fa6`; `preparation-barrier-raw.log` `f01a374e9c81e3db89b3a42940c4d6a5447684986a1296e42bf13f196eed6295`; `preparation-barrier-stderr.log` `32a79e1d373e57cb94b99af2c116805f24c5c902ffee5c83151079dab22255a3`; `preparation-barrier-exit.txt` `fe53fdb11651adb745be1b07339c81c01df92840481a98e369ca17acf87e584a`.

No verification result is green.

## Final admitted-source run (owner 87d hash)

Final `service.rs` hash `9141c5bd7cd06b7829a3578849b648fb9e28a3da5abe19938d288c2904874e75` was installed by compare-and-swap over the previously tested intermediate hash. `cargo check --lib --message-format=json` ran 2026-10-02 02:54:24–02:55:20 UTC+9 and exited 101 with six diagnostics: `src/remote/server.rs:2448:25` E0433; `src/daemon/session_service.rs:2463:26` E0308; `src/remote/server.rs:1960:90` E0382; `src/remote/server.rs:3100:32` E0382; `src/terminal/service.rs:739:9` E0308; `src/terminal/remote.rs:631:42` E0599. Full JSON, stderr, and exit receipt are archived as `final-resize-check-raw.log`, `final-resize-check-stderr.log`, and `final-resize-check-exit.txt` in `.omo/evidence/superlogical-parity/remote-resize-20261002/`.

The corrected focused barrier command ran 2026-10-02 02:56:00–02:57:07 UTC+9, exited 101, and executed zero tests because lib-test compilation failed with 38 errors and 47 warnings. The sequential wrapper skipped `terminal::service::preparation_tests` and `terminal::resize_lease::tests`. The exact final-source stdout, stderr, and receipt are `final-preparation-barrier-raw.log`, `final-preparation-barrier-stderr.log`, and `final-preparation-barrier-exit.txt` in that archive. These are failed historical attempts, not green verification. No test was deleted or skipped to manufacture a pass.

The final-source test diagnostics include 27 E0433 references under `preparation_tests` at `service.rs:1015–1391`, as well as errors in `server.rs`, `session_service.rs`, `service.rs:739`, `pty.rs:190/192`, `shell.rs:638/640`, and `remote.rs`. Frozen baseline comparisons show `session_service.rs`, `pty.rs`, and `shell.rs` are byte-identical between the remote baseline and tested tree; their E0308, E0277, and HRTB diagnostics are therefore source-identical baseline failures. The admitted resize source causes the 27 invalid `super::resize_lease` / `super::remote` references under `preparation_tests` and `service.rs:739` heartbeat return mismatch. `server.rs` and `remote.rs` differ from baseline and contain five additional feature-source diagnostics (`ServerControlMessage` unresolved, moved `device`, moved `state` Arc, and missing `InvalidState`); these were sent to owner `87d` as resize closure fixes. No code was edited by this verifier.

Final-hash run SHA-256 values: `final-resize-check-raw.log` `10172cc0376c9c8b7a2ab9a1f4b2dfcce8196cd15343fa41c8af7817e163ba2e`; `final-resize-check-stderr.log` `e420b26bf3c811a72daf82fe20bfb4c918a60d596598f930c09aed6c7eb99e4c`; `final-resize-check-exit.txt` `067790dfc7c79d49da1eb617189b53516e25f7add834984be2ec2123b7b09381`; `final-preparation-barrier-raw.log` `f01a374e9c81e3db89b3a42940c4d6a5447684986a1296e42bf13f196eed6295`; `final-preparation-barrier-stderr.log` `61730da4847e4d559eeb660f10bfda394fdd0e1a36018ba8a6ab8e462d9d0ce3`; `final-preparation-barrier-exit.txt` `415ea3e19f0142401db6868f73d365c29d6d4020c4f5b7171342525854d69702`.

Resize is not runtime-admitted. Owner `87d` has been asked for two distinct admissible patches: (1) the five resize feature-source fixes above; and (2) a separate private baseline compile overlay for the source-identical `pty.rs`, `shell.rs`, and `session_service.rs` diagnostics, preserving the known session-service error branches. No baseline file was copied into the feature closure. Do not rerun resize until both new patches are admitted with exact hashes and the combined private closure is reviewed. The current 0-test/38-error run is final for this attempted closure; no old-suite rerun is pending.

## Queued independent codec candidate

Codec v4 remains in a separate private closure at `/tmp/ferryx-codec-v4-verify-20261002`. Patch SHA-256 is `5608c9dd694595a35e4737633a37a7815a0a66055e68d962dc0e95d97d2d7968`. The isolated post-v4 source hashes match its report: `snapshot_codec.rs` `7e5ceb1781cd715b7936014bab60e9b6c02bd4b8d758d95a867115c5acc666ee` and `superlogical_snapshot_codec.rs` `27bba25e411f5f8a5c5f05433b358dcb211bf1b267b3e7ea47ca51aef2c4aa1c`. It is not applied to the resize or shared tree and has not been built or tested. Native drag-mask patch `ab626d2eb39bde0d1b607c8d95779aa7a9f395644edfc09e22163bddd01a79f0` remains queued after codec v4, with Wayland GUI validation unperformed.
