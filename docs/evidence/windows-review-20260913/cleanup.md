# Windows review resource cleanup

PARTIAL. This covers completed local mocked tests and owned TCP peers. It is not the
required native Windows process, scheduled-task, port and fixture cleanup.

## Completed lead-owned local test runs

All completed session outputs below were read and their process exits
observed. No test session was a desktop launch, daemon launch or SSH command.

- Transport RED: bash_3, exit 1 (intended assertion).
- Transport corrected oracle GREEN: bash_4, exit 0.
- Transport drop/force mutations: bash_5 and bash_6, exit 1 each
  (intended assertions).
- Focus existing baseline: bash_7, exit 0.
- Focus subscription-order RED: bash_8, exit 1 (intended assertion).
- Focus source-repair GREEN: bash_9, exit 0.
- Focus/coordinator expanded integration: bash_10, exit 0.
- Focus stale-snapshot mutation: bash_11, exit 1 (intended assertions).
- Opera original-precedence mutation: bash_12, exit 1 (intended assertions).
- Combined six-file tests: bash_13, exit 0.
- UI TypeScript/Vite build: bash_14, exit 0.
- Original digit-binding mutation: bash_15, exit 1 (intended assertions).

Exact monitor IDs, counts, times and failure sites are in red-green.md.
No kill command was necessary for these completed one-shot runs.

## Temporary owned files

- ui/vitest.transport-mutation.config.ts: deleted after its two processes
  exited. Existence checked false after deletion and again during this
  cleanup increment.
- ui/vitest.focus-mutation.config.ts: deleted after its process exited.
  Existence checked false after deletion and again during this increment.
- ui/vitest.opera-mutation.config.ts: deleted after bash_12 exited;
  existence checked false and parser source hash unchanged.
- ui/vitest.shortcuts-mutation.config.ts: deleted after bash_15 exited;
  existence checked false and shortcut source hash unchanged.
- All mutations operated through loader transforms. Production source
  hashes were checked unchanged across mutation runs.

The source/test repairs and evidence documents are deliverables, not
temporary fixtures; they remain uncommitted. Foreign tracked changes were
compared with binary diffs excluding the three then-owned files and were
identical after focus mutation cleanup. No foreign files were deleted.

## Still open

P04 and P28 have returned; lead inspected their artifacts, independently
reproduced both original defects, and completed combined GREEN and UI build.
The generated ui/dist build output remains as a local build artifact; it
was not installed or launched.
Second batch lead runs bash_16 (toast mutation, exit 1), bash_17
(210-test GREEN, exit 0), bash_18 (UI build, exit 0) and bash_19
(P16/P21 mutation, exit 1) have completed with full outputs read.
ui/vitest.toast-mutation.config.ts and ui/vitest.boundary-mutation.config.ts
were deleted after their exits and checked absent; source hashes unchanged.
P07/P21/P16/P22 producers have returned. P22 lead bash_20 exited 0
(13 TCP cases), bash_21 exited 1 (12 intended original-boundary failures),
bash_22 exited 1 (two intended cleanup failures), bash_23 exited 0
(16 TCP cases). Full outputs read. Every scenario closed its owned loopback
listener and sockets; final GREEN had zero residual deadlines. Mutation
deadlines were explicitly reported/disposed. script/qa/p22-mutation.mjs was
deleted after exit and checked absent. No user daemon was contacted.
P31 returned and lead read all its allocated artifacts. Lead bash_24 baseline
exited 0, bash_25/26 pairing mutations exited 1 as intended, bash_27 combined
222-test run and bash_28 UI build exited 0, bash_29 sortable mutation exited 1
as intended. Complete outputs read. Both ui/vitest.pairing-mutation.config.ts
and ui/vitest.sortable-mutation.config.ts were deleted after exit and checked
absent. Three affected production hashes were unchanged across mutation.
P02 and P05 children returned. Lead bash_30 wheel mutation exited 1 with
15 intended failures; bash_31 store baseline and bash_32 extended store
validator exited 0; bash_33 ownership mutation exited 1 with eight intended
failures; bash_34 exposed a missing runtime flag in the repaired App fixture,
then bash_35 all128 passed. bash_36/37 original App guard/startup mutations
failed as intended. bash_38 all575 combined tests and bash_39 build exited0.
Full outputs read. The wheel, close-ownership and App mutation configs were
deleted after process exits and checked absent; production hashes unchanged.
P05 child logs moved unchanged from /tmp/st_01a0994d-red.log and -green.log
to p05-child-red.log and p05-child-postfix.log in this evidence directory.
Content equality and old-path absence checked. The latter name deliberately
does not claim GREEN: it records the then-existing updater validator failure
(1 failed / 127 passed), superseded by lead bash_35 and bash_38.
P27 child returned; lead original-handler mutation bash_40 /
mon_184VCM1AGGQ3SQAE exited 1 with 17 intended failures and 41 passes,
full output read. ui/vitest.remote-wheel-mutation.config.ts was deleted
after exit and checked absent; production source hash unchanged.
P28 request-lifetime st_01a0995f and P05 browser-targeting st_01a09964 returned.
Lead bash_41 historical selection mutation exited 1 (seven intended
failures / 43 passes), full output read; temporary selection config
deleted/absent, source hash unchanged. bash_42 old browser fixtures exited
1 (four expected contract mismatches / three passes); output lost its first
124 lines of a large DOM dump, retained four failure blocks inspected.
bash_43 adapted fixtures exited 0 (seven passes). bash_44 combined exited
0 (24 files / 722 cases), bash_45 tsc/Vite build exited 0. All GREEN output
read. Browser child logs moved unchanged from /tmp/st_01a09964-*.log into
p05-browser-receiver-red.log, p05-browser-producer-red.log and
p05-browser-green.log; content equality and old-path absence checked.
No child or lead test/build remained active at that checkpoint.
P28 RC03 subsequently returned. Its full original RED (three failures /
12 passes) and final GREEN (15 passes) logs were read and moved with
apply_patch into p28-target-worktree-red.log and p28-target-worktree-green.log.
The original /tmp/st_01a09972-{red,green}.log paths are absent. Lead bash_46 /
mon_CH8WDE9H1ZDMNWE5 exited 0 (five files / 130 cases), all output read.
P27 preferences st_01a0997a and P32 history st_01a0997b are now active,
with disjoint registered ownership. Their resource receipts and combined
build remain pending. P32 was explicitly steered to inspect its pure-file
fixture and use owned environment roots without creating branches/worktrees.
P33 native menu ownership st_01a0997e and read-only exact branch inventory
st_01a0997f are also active with disjoint ownership. Windows read-only probe
bash_47 / mon_7C1QYV7KEW9ZCFP7 exited 0; it acquired no runtime resources.
Its complete JSON confirms the original installed GUI and two daemon
identities to millisecond precision; see windows-preflight-current.md.
P27/P32/P33 subsequently returned; lead bash_48 (32 IPC cases), bash_49
(201 remote cases), bash_51 (tsc/Vite build) and bash_52 (120 menu/caller
cases) all exited 0. bash_50's three stale DnD-fixture failures are preserved
and superseded by bash_52, not hidden. Full output read. P32 locator absence
checked; its Python wrapper deviation and remaining checks are recorded in
preferences-menu-combined-verification.md. Only the read-only inventory
worker remains active at this checkpoint.
Native Windows QA has not
been started by this session; there are no owned Windows runtime cleanup
receipts yet. Existing user daemons, installations and borrowed resources
must remain untouched. Final cleanup must be updated after combined checks,
Windows execution and final gate, with exact ownership and exit receipts.

## P03/P11 continuation

P11 RED bash_56 exited 101 with the intended data-loss assertion; GREEN
bash_58 exited 0 with 13 passes. Full output and exit receipts were read by
the lead before the child archived them in p11-store-preservation.md.
The child verified archived content equality and removed its owned root
/private/tmp/ferryx-p11-st_01a099d0.OHtDkm. Lead independently checked the
runner path absent and current source SHA256 equal to the GREEN receipt.
The shared P32 Cargo target/cache was not a P11 cleanup target.

P03 RED bash_57 exited 101 with exactly two intended byte mismatches;
GREEN bash_59 exited 0 with seven passes. Full output was read by the lead
and archived with receipts in p03-key-encoding.md. The lead independently
matched all three current source/test hashes to GREEN. P03's evidence root
/tmp/ferryx-p03-st_01a099cf-oQzzlk and the shared P32 build target remain
retained; their removal is still pending, not silently assumed complete.

The full frontend rerun bash_60 is active after the narrow nwsapi override.
Settings tests were unchanged; the prior focused run now has 40 passes and
one independent prose failure. Its report contains raw install/test output.
P14/P15 workers are preparing separate source regressions, not running OS
notifications, changing network interfaces, or launching native GUI/daemons.
