# Owning-lane repair routing, Task8 pass2

## Backend daemon/terminal/ipc-terminal
Compile blocker: split_journal fs2 import/lock_exclusive, missing HandoverManager imports, IpcErrorCode Display, stale protocol response/request fixtures, service mark_running type inference, pty PathBuf comparisons, shell Fn/FnOnce lifetimes. Windows handover_transaction references are cfg-excluded. Verbatim per-platform compiler JSONL and raw first-list/all-targets/full-lib logs are authoritative; no verifier repair.

## Native native_terminal/ipc-native_terminal
surface_host.rs971/972 active_presentation_generation/epoch missing on all3. QA-feature callers651/706 schedule_cancellation_receipt,728 try_claim,778 release_claim absent. QA errors Mac76/Linux74/Windows29. All selected counts unknown due compilation, not zero naming passes.

## Frontend ui/src
Candidate regressions and mixed baseline masking: frontend-regressions.md; every failure file and corresponding base assertions: linux/windows-ab-classification.jsonl and baseline-classification.md. NativeTerminalPane.presentation base14passed vs candidate3failed; Windows paired app split call adds second argument; App startup/HMR tests new candidate failures. Mac full suite timed out1200s/SIGKILL with final selection unknown; frontend verification gate reopened, no causal diagnosis claimed. Mac base A/B complete10files, candidate per-file assertion recovery active. NativeTerminalPane recovery emitted186failed11passed197selected native1. Protected baseline failures remain unchanged.

## Scripts adapters
Exact repaired runner selects26 on Linux/Windows,5fail21pass native1. H6 relative source digest path ENOENT;4 fake mock platform tests invoke real Darwin driver and fail osascript ENOENT. Mac exact runner NOT_RUN GUI_BOUNDARY because same tests would focus/click using fake PIDs. scripts-findings.md/raw runner logs. Adapter fixtures cleaned4 per host.

## Verification environment
Foreign Mac helper_setup test violates requested exclusivity but separate target; untouched, exclusive-verifier-audit.md. No GUI/production/daemon mutation/product edits/commits by Task8. Shell LSP unavailable and evidence MJS TypeScript installation unavailable; evidence runner node syntax checks pass, no dependency installation.
