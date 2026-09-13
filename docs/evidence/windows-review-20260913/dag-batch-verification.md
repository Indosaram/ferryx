# Remaining-work DAG batch verification and next execution boundary

Verifier: st_01a09a0f. Original session: 01a0983f-c995-753d-afa9-593f6d118788.
Evidence cutoff: 2026-09-13, approximately 09:34 UTC. Report only; moving shared
tree, not a freeze or final approval. G001 is BLOCKED; C001/C002/C003 are pending
in the inspected goals.json (updated 09:28:41.862Z).

## Adjudication of the five producers

PASS below means the stated bounded evidence is accepted, not user completion.
FAIL means a required check failed or a current-state claim is not supported.
BLOCKED means the required behavior has not executed because a prerequisite is
missing. No producer has completed the aggregate objective.

| Producer | Adjudication | Evidence and next boundary |
|---|---|---|
| `dag-pr2.md` | Review PASS; implementation/native acceptance BLOCKED | Current windows.rs still creates an enabled child; pointer test file is absent. PR head unchanged, OPEN. Native one-test same-assertion RED/GREEN and actual WebView2 sentinel selection are absent. Execute in an owned Windows allocation, not on Darwin. |
| `dag-pr3.md` | Review PASS; native/disposition BLOCKED | Current App accepts optional terminal kind and routes through confirmation-aware selected-leaf close. PR remains OPEN/DIRTY. Sixteen-case harness is preparation, not executed Windows process/GUI evidence. Preserve current routing rather than replaying the obsolete direct-close patch. |
| `dag-frontend.md` | Aggregate tests FAIL; historical build PASS; current combined verification BLOCKED | Raw log proves 217/219 files and 2445/2449 tests pass, four failures, exit1. Build reaches Vite success. Both precede later UI changes; no run-time source manifest binds them to today's aggregate. |
| `dag-windows-runtime.md` | Preflight/compile preparation PASS; runtime BLOCKED | Retained SSH receipts exit0; input helper compiles but dispatch was not run. No eligible existing owned checkout was found in inspected locations. Creation approval remains pending; installed processes are not an allocation. P14 identity compatibility is an additional independent blocker. |
| `dag-remaining-register.md` | Inventory accounting PASS; current-status accuracy FAIL | All packet IDs remain accounted below, but many “no implementation/test run” rows are superseded by actual P02/P06/P09/P13/P18/P26/P29 evidence. P15 supplemental review is no longer pending. Register is historical planning evidence, not current completion state. |

## Actual commands, outputs and source identity

### PR state gap closed with one read-only refresh

The producer markdown retained command claims but not raw GitHub response
artifacts. To close that specific evidence gap, this verifier executed, starting
09:30:37Z:

```sh
gh pr view 2 --repo Indosaram/ferryx --json state,headRefOid,mergeStateStatus,statusCheckRollup
gh pr view 3 --repo Indosaram/ferryx --json state,headRefOid,mergeStateStatus,statusCheckRollup
```

Both exit0. Actual responses:

```json
{"headRefOid":"79b02ab6ea4753bf87080dd567368a870c018057","mergeStateStatus":"UNSTABLE","state":"OPEN","statusCheckRollup":[]}
{"headRefOid":"99c7086b61d7a590530fb8df924e34ce9e91e90b","mergeStateStatus":"DIRTY","state":"OPEN","statusCheckRollup":[]}
```

One `gh api graphql -f query=...` queried canonical Indosaram/ferryx,
pullRequest(number:2) and number:3, each reviewThreads(first:100) with totalCount,
pageInfo.hasNextPage and nodes.isResolved: exit0; both returned totalCount0,
hasNextPage=false, nodes=[]. No review thread or pending check exists to wait on.
Producer `gh pr checks` exit1/no-checks claims remain producer receipts, not a
new execution here. Empty checks are NOT CI GREEN. PR2 UNKNOWN is superseded by
UNSTABLE; PR3 still requires conflict/disposition work.

Read current windows.rs and App.tsx:1767-1834. Current SHA256:

| Path | SHA256 |
|---|---|
| src-tauri/src/native_terminal/platform/windows.rs | `1d12a6fcc858afbd26eb224330ba6cf022c2019ac097d5355a9632efece89fa7` |
| ui/src/App.tsx | `8766559e57a5e94641910cf53a0342f9ca3c3310d5c32315c2edfdc7327982ca` |

These are verifier-time hashes, not retroactive execution manifests. No PR diff,
completed local close audit, Cargo target or native command was rerun.

**Registration correction:** the exact PR2 test is now registered in C001,
including old enabled-style RED, WS_DISABLED-only repair and one-test GREEN.
The runtime report's search of C002 alone gives a stale blocker. Registration
is not missing merely because that name is absent from C002. PR3's report-only
toolkit failure also does not establish current global unavailability: runtime
`dag-toolkit.receipt.json` contains ok=true/accepted registration response.
Lead must reconcile literal PR3 matrix/oracles against the current criterion
before any extension; annotations or harness existence never establish PASS.
No regression/fix was added by this verifier, so no new registration was needed.

### Frontend aggregate: retain the failure, do not relabel it

Inspected the raw failure section and final summary of `dag-frontend-test.log`:

- `CI=1 bun run --cwd ui test`: 2 failed / 217 passed files (219);
  4 failed / 2445 passed tests (2449); duration105.83s. Raw footer explicitly
  says `error: script "test" exited with code 1`.
- SettingsDialog:614 expects `/authorized browser profiles reconnect/i`;
  actual copy says “Authorized browsers reconnect automatically while this
  stays on.” The following assertion also pins old re-pair prose. Current
  source confirms a prose-only stale test, not evidence of broken reconnection.
- Push: exact link returns null; enable returns enabled instead of denied;
  disable resolves disabled instead of rejecting offline. Current client.ts is
  still the three-method stub. Repository search found no production use beyond
  declarations; git log identifies commit831ae2c67da8143294be4526dd417bb07531b897.
  Its denied test also restores globals only after a failing assertion: fixture
  teardown belongs in unconditional cleanup when its owner repairs that lane.
- Prior binding/App/SSH groups' 55/23/33 passes are historical aggregate results,
  not proof that all subsequent edits are integrated.
- `bun run --cwd ui build`: raw `$ tsc && vite build`, 1892 modules, built2.23s;
  exit0 is producer-reported, consistent with complete success output, but no
  separate numeric exit receipt is retained. Existing >500kB warning remains.

Raw SHA256: test `eb112e74a8d173c28a7722e838c32dc922962e3df58a89f70276714259c28d36`;
build `e19cb11506f1e7010585de27d533caf92c9ecc01fb41b694c6b5f1c2ec9661ab`.
Logs finalized08:55:10/08:55:21Z. Demonstrated later source drift includes
NativeTerminalPane.tsx09:10:56, TerminalSection.tsx09:10:12 and
browserTauri.test.ts09:18:07. File mtimes are drift indicators, not source identity.
There is no retained whole-UI before/after hash manifest for this run. Do not
rerun a moving aggregate: finish the disjoint repairs, freeze bytes, then one
monitored full suite/build on that new snapshot with counts/exits/hashes.

Current failure-source hashes:
SettingsDialog.test.tsx `2faa384e1145a975df0faa06e591b4cf07a48bcde87375c96acec7e09f20df72`;
push/client.ts `3501e236850b6b56294af9bfc078c4c3657f61c9cf9d5e0b0bbf5794624c889d`;
push/client.test.ts `a766767af9732045450f3a577a4d7497834a405bd344c7db5e380efe0b1ae7d6`.
Do not rewrite product copy to green this suite or invent unrelated push features.
Lead/spec disposition is required; “out of Windows scope” does not turn exit1
into an all-checks PASS or authorize deleting/skipping failed tests.

### External owners P15 and P14

**P15 local PASS, native pending; ownership remains st_01a099e2.** Read
`dag-p15-review.md`, all four primary/companion exit files, receipt JSON and raw
libtest results. RED primary101: 0pass/1fail, actual route error versus
Ok(192.168.50.7). RED companion0:1pass. GREEN primary0 and companion0:1pass each,
1053 filtered, zero ignored. All before/after hashes agree; both GREEN manifests
match current state.rs/Cargo.toml/ignored Cargo.lock respectively:

```
c8d20a92b46084e8c57b8f6234ffc3b1047e46146a78407b2e585ccb4dd01cd4
1fdf8ec46e62043b6d75a541bc60682e60f701dcdfe6551a8c03a4492ec6efdc
1b2d01f2d268c2183c5be10cb1d0db464c349afffe9999b79cb93077f11169a5
```

All four raw log hashes match the supplemental review. Actual command is direct
Rust1.98.1 Darwin Cargo `test --manifest-path src-tauri/Cargo.toml --lib
remote::state::tests::p15_offline_lan_survives_both_route_failures -- --exact
--nocapture` plus the receipt's companion command. Companion is invariant GREEN,
not a second repaired RED. No rerun justified. Four Windows ABI fixtures, actual
GetAdaptersAddresses/full resolver startup and authenticated offline-LAN peer
access are still pending; local GREEN cannot compile Windows cfg or prove peer
reachability. A released Cargo slot is not this verifier's acquired slot.

**P14 BLOCKED, owner st_01a099e3.** Read current notification report; only JS
syntax success exists. Both builder sound omissions remain unrepaired. XML,
activation, audible System/Silent and native RED/GREEN have not executed.
Independent blocker: report requires identity use outside target/debug/release,
whereas authorized desktop launch is only `bun tauri dev`, debug. Owner must
qualify exclusive AUMID under that permitted launch plus toast/audio permission;
do not direct-launch a copied binary or use installed/shared identities. Keep
the four targeted/idless x System/Silent cases and click/drain proof separate.

### Runtime receipts genuinely establish only readiness

Read `runtime/dag-preflight.receipt.json`, stdout JSON, existing-checkout and
remaining-root receipts, input-compile receipt and toolkit response. Exact encoded
PowerShell/SSH argv are retained there. SSH preflight08:53:00Z, provenance08:55:16Z,
root inspection08:59:43Z all exit0. Interactive sook Session1 exists; SSH is
Session0. Installed GUI17288 and daemons1756/20196 have recorded paths/start
times; listener53986 belongs to installed daemon20196. These identities must be
revalidated at future allocation, not treated as current handles or touched.

Historical owned checkouts absent. Shared winbuild checkout is e2a19066 with six
foreign tracked changes; Ghostty6a508fd5. ferryx-qa contains three historical
files and no repository. This supports “no eligible checkout among inspected
locations,” not a universal filesystem census. Do not repeat the same audit.
Creation of the proposed owned root/branch needs explicit approval; reversible
sync into a proven existing owned checkout would not require extra sync approval.
Input compile exit0/DAG_INPUT_COMPILE_OK proves parser/C# INPUT layout only.
No debug binary, native dispatch, GUI screenshot or PTY survival was produced.

## Complete packet carry-forward, including new work after the register

All nonreserved packets below retain native acceptance. “Local” preserves prior
bounded evidence, not a fresh re-audit. Recent logs were inspected, not rerun.

| Packets | Current local state and unfinished increment |
|---|---|
| P01 | Unrepaired native HWND delivery; PR2 exact RED/GREEN then real selection. |
| P02 | Debug6pass/exit0; input16pass/3fail/exit101 after four new assertions green; frontend context168pass. Detached-error fixtures still fail; isolate preferences before further input execution; drop target-shell/focus/IME/capture/native matrix open. |
| P03, P04 | Retain local punctuation7pass and shortcut156pass; physical Ctrl+backslash/right-bracket bytes and Ctrl-vs-Alt digits remain. |
| P05 | Retain selected-pane close/store/browser local proofs; native keyboard/menu close with same sibling process and live browser isolation remain. |
| P06 | Pixel reply local2pass exists; Windows raster/flush/face/metrics/cache/DPI and safe preference/example/close fixtures remain. |
| P07 | Retain local toast-overlay210pass; actual child-HWND yielding/dismissal/session preservation remains. |
| P08 | Upgrade-admission/cwd seams staged, no GREEN in inspected handoff; control/attach auth, SID/DACL, agent listener readiness and integration still implementation work. |
| P09 | Local resolver/install3pass and settings3pass; native .cmd/.bat launch, authoritative provider metadata and target-shell contract remain. |
| P10 | SSH/config/argv/namespace tests staged; parser/quoting/target-platform validation and safe fixture repairs await exact RED/implementation. P20 merged here. |
| P11 | Retain local13pass store preservation; native deny-read handle remains. |
| P12 | Raw TCP rejection tests staged; credential publication/authenticated client/listener implementation not GREEN. |
| P13 | Extracted production seams6pass, UI14pass; Windows HistoryChanged/native navigation unimplemented, Cargo/native proof pending. Needs serialized Cargo.toml dependency handoff. |
| P14, P15 | Separately owned as above; P14 blocked unrepaired, P15 local GREEN only. |
| P16 | Retain updater210pass; actual Store bypass/installer probe pending. |
| P17 | Packaging correction staged; Node8pass source contracts only. Native identical original/repaired16-case MakeAppx proof unrun; repair was staged before observed native RED, not compliant chronology by inference. |
| P18 | Portable prerequisite7pass; workflow16/17, missing deploy-pages.yml failure retained. Cargo/MSVC/GNU/native/CI execution not certified. |
| P19 | Owned persistence harness staged but PID check deliberately permissive for RED; exact rejection, panic cleanup and native counterpart pending. |
| P21, P22 | Retain local capability UI/QA transport proofs; real hidden unsupported card and isolated ConPTY/cwd/cleanup pending. Use qualified explicit port-file, not ambient daemon/default port. |
| P23 | Enrollment transaction seam staged; lock repair, admission/lease/revocation/framing/native gateway cases remain. |
| P24 | Watch-loss/off-thread scan tests staged; repair and unconditional cancellation cleanup remain before full acceptance. |
| P25 | Active worker: retained harness8fail and stager10fail RED logs; no accepted GREEN report at cutoff. Import safety, artifact provenance, protocol1-vs3 and native wrapper integration remain pending. |
| P26 | Injected Windows/Linux policy10pass each, source hashes match final log; full Cargo/native registered IPC still pending. |
| P27, P28 | Retain wheel/preferences/selection/worktree/Opera local proofs; live socket, response gate, cross-worktree selection and actual browser wheel pending. |
| P29 | New report arrived: signing4pass and SEO24pass, both EXIT=0 raw logs; native Bash/path/build behavior pending. |
| P30 | Registered active image-cap worker; no accepted GREEN producer report at cutoff. 4097x4097 RGB cap/recovery/text snapshot/native presentation pending. |
| P31 | Retain sortable222pass/mutation; grouped displacement/drag/reload real GUI pending. |
| P32 | Retain ancestry/paging successes within failed5-test/exit101 sandbox (symlink failure); native ancestry executable pending. |
| P33, P34 | Retain menu120pass and focus50pass/mutations; actual menu/clipboard and foreground/bell/unread runtime pending. |
| PX | Foreign owner only: 300s forced scan via virtual time and refs traversal defects unresolved; do not acquire silently. |

Recent evidence qualification: P02 JSON exits/raw counts agree. Current GREEN
input manifest matches all four recorded paths; debug GREEN's native_terminal.rs
hash differs because the later input repair changed it. Debug source itself
matches ca3093ae...; this is focused proof, not whole-tree stability.

P06's terse new PASS header conceals an intermediate failed GREEN: actual retained
temporary roots `ferryx-p06-st_01a09a00-{bcDcYC,Pit2Hb,eYT8ul}` under
`/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/` contain receipt.json and
red.log/green.log. Same exact `--lib native_terminal::surface_host::tests::p06_
-- --nocapture`: RED101/0pass2fail (empty replies), first GREEN101/0pass2fail,
final GREEN0/2pass. Final log SHA256
`e46c8ea5921c18a3bcdcec0c4611a220282569f5ef62b8877e7c0bf11118c516`.
Final surface/font/lock hashes match current files, but receipts omit newly
changed bell/lifecycle/constants paths, so do not infer a complete-source freeze.
Lead should retain these already-executed artifacts durably, not rerun them.

P17's three recorded hashes independently match current repaired/test/original
files; hashes do not substitute for native execution. P26 current source/test
hashes match fbf52d3e.../d75a0f47... in final-green.log. P09/P13/P18 logs confirm
reported focused counts; extraction/probe tests do not certify whole crate or
Windows cfg. P29 raw logs show4/24pass and EXIT=0, with verification.log carrying
source hashes/cleanup. No whole-batch build or independent review of all new
implementation diffs is claimed here.

Inventory JSON parsing independently counted1854 census entries,23316 syntax
entries,86 explicit gaps; ledger has968 source-file scopes within2057 file
records. The register's362 candidates is historical lexical census, not semantic
closure. Retain nine global qualifications, alias/Cargo/macro expansion and
shared caller/ancestor gaps outside59 closure/25 bounded/7 shared-native/5
transport receipts. P20 reserved; CONTRACT-RC-04 and TOOLING-GAP-01/02 excluded;
optional RF-05 and refuted/downgraded IDs retain register dispositions, not new
repair lanes. GNU linking remains unprovisioned, not demonstrated GREEN.

## Next execution batch: disjoint implementation lanes, not verify-per-packet nodes

Resume existing owners; these are scheduling groups, not permission to overwrite
their files. One lead-owned monitored Cargo queue serves independent lanes;
resource contention is not a semantic dependency. No sleeps/polling or new cache
farms. Exact commands/oracles remain in registered packet runners and C001/C002.

1. **Terminal input/render lifecycle:** P01/02/03/06/30 remaining fixes and
   native focus/capture work. First finish P06 explicit preference isolation;
   then register and correct the three P02 detached-error fixtures using typed
   SessionDetached plus session identity, not weaker errors. Carry font/GDI,
   image recovery and final-motion/IME controls together. P01 native HWND RED
   is not blocked on all other portable repairs.
2. **Daemon/session and shell transport:** P08/09/10/11/19/22. Agree control/attach
   auth and provider/target-shell metadata once; run already-staged exact safe
   REDs through exclusive Cargo queue, repair, identical GREEN, then owned
   integration. P19 safety must precede broad persistence execution. P09 metadata
   feeds P02 drop quoting; do not guess shell from host OS.
3. **Browser listener and native navigation:** P12/13, preserving P05 targeting.
   Coordinate capability publication with lane2 without sharing authorization
   scopes. P13 HistoryChanged needs P15 manifest-owner release/serialized edit
   for webview2-com; adapter fixture GREEN does not authorize editing Cargo.toml.
   P12 auth must precede real CLI action acceptance, not P13's pure resolver work.
4. **Relay and DAG state:** P23/24, separate file owners. Cross-process enrollment,
   admission/lease/revocation and watch recovery/offloaded scan can proceed
   independently; both need exact REDs and robust cleanup. Do not create a
   false dependency between a watcher repair and relay lock repair.
5. **Packaging/tooling and safe contracts:** P17/18/25/26/29. Finish active P25,
   retain P29 GREEN, execute queued safe Cargo contracts once. Resolve missing
   Pages-workflow expectation with its owner/spec, not a fabricated workflow.
   Native scratch MakeAppx/PowerShell checks need owner-controlled scratch/tool
   authorization, not inherently a complete GUI checkout/build; use that smaller
   boundary when authorized. Actual packaged-app resources still need real app.
6. **Frontend contract reconciliation:** SettingsDialog prose-only failure and
   separately owned push prototype disposition; preserve machine behavior and
   all failing tests until authorized reconciliation. No unrelated feature
   expansion. Then final changed-snapshot full suite/build including P02/P09/P13
   additions, not a repeat of the08:55 source. Carry already-local UI packets
   into shared native acceptance rather than reopening completed repairs.

**External lanes remain separate:** P14 owns sound/identity proof and repair;
P15 owns native adapter/API/peer acceptance; PX stays foreign-owned. None becomes
complete because another lane supplies launch infrastructure.

**Shared Windows acceptance boundary:** sole owner st_01a099f8 requires eligible
owned allocation/creation approval and immutable transfer manifest including
dirty sources, tests, locks, generated inputs and Ghostty SHA. Lead provides
monitor; launch only `bun tauri dev`, debug, in the interactive account. PR2
exact native RED/GREEN first, then real pointer/wheel/selection plus PR3 sixteen
keyboard/native-menu cases and additional busy/pinned/browser/remote controls.
Subscribe exact DOM/backend/PTY/render events before native input; assert selected
exit plus unchanged sibling backend/PID/start time and new shell output. Add
physical key/Command Prompt/cwd/DPI/browser/overlay/menu/clipboard/focus/runtime
matrix from packet handoffs. Native P15 tests/API/peer and P14 identity/toasts
have additional prerequisites; no dispatch-only, XML-only, process-sentinel-only
or screenshot-only shortcut. Cleanup must preserve installed app/user daemons.

**Final gate:** all lane repairs and evidence, semantic inventory qualifications,
frozen-source aggregate diagnostics/tests/build/native acceptance and independent
combined diff review precede any atomic commit/push/PR external write. PR2/3
audited completed dispositions and remote main at the reviewed final commit are
still requirements, not performed actions. Neither DAG node completion nor
blocked preparation satisfies the user's stop condition.

## Verifier execution boundary and limitations

Only reads/searches/JSON parsing/SHA256 comparisons and the single GitHub refresh
ran here, plus apply_patch to this report. Read-only Python artifact inspection
initially exited1 because with_suffix dropped `.primary`; corrected filename
construction exited0. That was a verifier path bug, not a test rerun/failure.
No source edits, tests/builds/GUI/SSH reruns, daemon operations, branch/worktree
changes, commits/pushes or GitHub writes occurred. No official toolkit mutation
or hand-edit of goals/ledger occurred. Foreign dirty files and exclusive P14/P15
paths were left untouched. Newly arriving producer results after this cutoff
need hash-bound adjudication by the lead, not an inferred PASS from task status.
