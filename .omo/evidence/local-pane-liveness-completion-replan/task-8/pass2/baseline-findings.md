# Pass2 baseline classification

Protected pass1 A/B-confirmed failures: TerminalSearchOverlay.test.tsx:332 and updater.test.ts:141, Mac base d82b35e4 native1 twofailed32passed. No other failure is pre-existing without exact A/B proof.
Mac first lib-test list native10172errors52warnings; full raw mac-first-rust-list.log and verbatim index mac-compiler-findings.jsonl. Additional Mac type-annotation errors server.rs100/115; backend/native findings otherwise independently captured, not inferred from other hosts.

Linux and Windows full suites and matching-host base A/B are complete:99/99 and11/11 failed files covered. Linux classifications:97 pre-existing,1 candidate-caused,1 mixed/unattributed. Windows:7 pre-existing,2 candidate-caused,2 mixed/unattributed. Raw named assertions and full blocks are retained in platform AB classification JSONL; baseline-classification.md carries the per-file tables. Mandatory TerminalSplitView.paneHandleReach.test.tsx and pairedDaemonRollout.test.ts reproduce their exact assertions on both bases. Mac full suite and matching-host A/B remain pending. Candidate-caused and masked changes reopen frontend; no repairs performed.

Linux and Windows runners selected26 each, native1 fivefailed21passed, scripts reopened; Mac exact runner NOT_RUN for no-GUI boundary. Linux first Rust list native101:70 compiler errors, full archive linux-first-rust-list.log inspected and routed. No repair performed.

Linux first Rust lib test compile diagnostics read in full (1483-line raw log). Backend reopened: missing fs2 import/lock_exclusive, missing HandoverManager import, IpcErrorCode Display, stale protocol/IPC test constructors and patterns, service type inference, pty path comparisons, shell closure lifetime. Native reopened: surface_host.rs971/972 missing active_presentation_generation and active_presentation_epoch methods on Linux. Full verbatim blocks indexed in linux-compiler-findings.jsonl; these are routed findings, not baseline claims.

Windows independently observed first Rust lib-test list native101,25errors42warnings. Backend additionally references cfg-excluded handover_transaction in handover.rs516/517/533 and server.rs49/55/111. Native same missing presentation methods971/972. Full raw windows-first-rust-list.log and verbatim index windows-compiler-findings.jsonl. No inference from Linux required.

Runner failed adapter fixtures do not reach trailing rmSync; owned temp roots use pane-liveness-test-* and barrier run IDs run-cancel/run-stall/run-ho/run-so. Before teardown inspect matching JSON run IDs and creation timestamps, remove only roots tied to this pass; do not broad-delete shared temp prefixes.
Linux QA-feature list native101 now74errors49warnings: native surface_host.rs651/706 schedule_cancellation_receipt,728 try_claim,778 release_claim missing on QaBarrierChannel; verbatim linux-qa-list.log586-609. Native owns caller reconciliation with backend barrier contract; no verifier edit.
