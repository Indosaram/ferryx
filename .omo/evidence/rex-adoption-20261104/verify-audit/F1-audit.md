# F1 plan compliance audit

Head: `35c984684b60c40dd6d29cd8703270fe86c30580`; base: `d59508fa9`.
Verdict: **REJECT** for literal F1 guardrail compliance; merge inventory **PASS with T13 BLOCKED**. This is not a completion verdict for todo acceptance/QA or F4.

## Inputs and audit limits

Read the entire 31-todo plan, F1-F4 and success criteria, final-verify-checklist, and execution ledger from the canonical tree. They are absent from this worktree; their locations are `/Volumes/T9-Mac/project/ferryx/.omo/{plans/rex-adoption-20261104.md,evidence/rex-adoption-20261104/final-verify-checklist.md,ulw-execute/rex-adoption-20261104.jsonl}`. Source and diff were reviewed at the assigned audit head. Writes are exclusively under this evidence directory. No build/test/runtime QA command ran on this Mac or a remote host in this leg. The parent runs the one combined host verification.

## Every todo disposition

`git merge-base --is-ancestor <commit> 35c984684` returned 0 for each named implementation below. `todo-ancestry.json`, `merge-history.txt`, and the T25/T31 ancestry logs are the command-output observables. MERGED does not mean acceptance PASS. The seven already-verified dispositions are inherited from the ledger, not reverified here.

|Todo|Implementation / merge anchor|Disposition|
|---|---|---|
|1|`97dedfd33`|MERGED; previously verified in ledger|
|2|`e99573577`|MERGED; combined acceptance/QA still required|
|3|`b2048809c`|MERGED; combined acceptance/QA still required|
|4|`3b9d35ead`|MERGED; combined acceptance/QA still required|
|5|`c55972eb4`|MERGED; combined acceptance/QA still required|
|6|`caf8cabb9`|MERGED; previously verified in ledger|
|7|`feb63c972`|MERGED; combined acceptance/QA still required|
|8|`403c53eba`|MERGED; combined acceptance/QA still required|
|9|`a6bb6e506`|MERGED; combined acceptance/QA still required|
|10|`a77961f04`|MERGED; combined acceptance/QA still required|
|11|`17f16b42f`|MERGED; combined acceptance/QA still required|
|12|`5ac05fd10`|MERGED; combined acceptance/QA still required|
|13|-|BLOCKED: no published release includes T12; must wait for a later release cut. `git tag --contains 5ac05fd10` exits 0 with empty output.|
|14|`9c14d0c31`|MERGED; combined acceptance/QA still required|
|15|`4b0f204f8`|MERGED; previously verified in ledger|
|16|`313128b1f`|MERGED; combined acceptance/QA still required|
|17|`70abbf890`|MERGED; combined acceptance/QA still required|
|18|`44df0c820`|MERGED; combined acceptance/QA still required|
|19|`259007893`|MERGED; previously verified in ledger|
|20|`572f97932`|MERGED; combined acceptance/QA still required|
|21|`3dadd2410`|MERGED; combined acceptance/QA still required|
|22|`454eb5e27`|MERGED; combined acceptance/QA still required|
|23|`941506750`|MERGED; combined acceptance/QA still required|
|24|`7485dd228`|MERGED; combined acceptance/QA still required|
|25|`c73df3967 (merge)`|MERGED; previously verified in ledger|
|26|`e12c302af`|MERGED; combined acceptance/QA still required|
|27|`03208b10e`|MERGED; previously verified in ledger|
|28|`b22a4b7d9`|MERGED; combined acceptance/QA still required|
|29|`fbec910bf`|MERGED; combined acceptance/QA still required|
|30|`5e9922dc8`|MERGED; combined acceptance/QA still required|
|31|`1fa362974`|MERGED; previously verified in ledger|

T25 used maho-mac instead of XenoCI per recorded owner decision (no paid job required): confirmed items 1/2, inconclusive 3, refuted 6. The ledger already marks it verified; that is verification of claims, not a claim all Rex claims were true. The owner superseded per-lane verification with implement-first/one final verification, approved Linux GitHub Actions, and replaced the originally requested reviewer model. These recorded changes are not silently treated as plan violations.

## Every global Must NOT line

|Plan guardrail|Audit result / evidence and limitation|
|---|---|
|No cargo/rustc/bun/build/test on owner's MacBook|This leg PASS (git/Python/read-only source assertions only). Diff cannot prove all historical process activity; ledger puts gates on GHA/maho-mac/maho-win. No independent historical host attestation claimed.|
|No allow/expect/cfg_attr allow or lint config changes|No added Rust suppression attributes: `rust-attribute-check.json`. Existing suppressions are not newly introduced. Broad text matches in compiler logs/generated search prose are not Rust attributes. No eslint/clippy configuration diff; `lint-config-boundary.log`. `ui/package.json` adds mandated hardcoded/token checks before unchanged eslint invocation (strengthening, not a suppression). Literal 'no lint config change' requires interpreting this planned lint wiring as an explicit T19 exception.|
|No production daemon/process restart; isolated QA and recorded PIDs only|No daemon or process operated by this leg. QA scripts inspected use isolated arguments/recorded children; historical safety cannot be established from a diff alone. T12 per-SID/per-build pipes are NOT isolated by runtime dir; host Windows leg must prove distinct build flavor and no production collision before execution.|
|No foreign dirty files/shared-tree edits; worktrees only|Audit branch started clean at assigned head. AGENTS.md and pick-build-host unchanged in integration diff. All this leg's writes evidence-only in assigned worktree. Historical shared dirty-state cannot be reconstructed from diff.|
|No production deploy, no relay.checka.cc|No deploy executed by this leg. No added relay.checka.cc product reference found. Workflow added is compile-only on temporary ci/rex-buildgreen refs, not production publish. Configured relay.ferryx.dev references do not prove a deploy.|
|No hosted CI release/sign/publish; no GPL FFmpeg|Added workflow builds/checks but has no release/sign/publish step. `scripts/release-workflow-policy.mjs` unchanged. FFmpeg LGPL build script/resources diff empty (`ffmpeg-boundary.log`). Existing remote SDK fetching is not a pipeline license change.|
|No paid XenoCI without approval|Ledger owner override uses maho-mac; no paid call by audit leg. T25 ancestry and ledger decision recorded above.|
|No system SSH config writes, OSC52 reads, project script/plugin auto-load, destructive unconfirmed deep link|No new system SSH config writer identified. OSC read callback denies; script path is owner config + explicit enabled; plugin root owner config + full-manifest approval; deep links confirmation routing inspected by source. F2 details remaining authorization bypass; runtime confirmation/negative cases must be proved by host legs.|
|No Gemini CLI support work (agy only)|Shared ~/.gemini path side effect is documented in integration installer intent, not a distinct Gemini CLI integration. No Gemini support target added.|
|No macOS entitlements, no MSIX capability except protocol extension|No changed entitlements path; tauri config adds ferryx scheme, not entitlements. MSIX diff only adds windows.protocol ferryx under Application; capability block unchanged (`config-and-supervisor.diff`).|
|No daemon leafId storage; no removal of .orca-worktrees parsing; no hardcoded legacy storage prefixes|Semantic no-storage property is represented by typed node IDs and migration-only ephemeral map. However the exact F1 acceptance 'no leafId in src-tauri/src/daemon' FAILS: `layout_migration.rs:120,171-172,241-243,257` includes leafId migration reads/test fixtures. `layout.rs` itself has no hits. This is not evidence daemon persists visual IDs. Resolve the plan's literal-vs-migration contradiction explicitly or relocate migration boundary; do not claim literal empty search. No removal of legacy .orca-worktrees parsing identified in changed worktree registry/rescan code. Historical prefixes appearing in generated docs are not executable storage keys.|

DAEMON_PROTOCOL_VERSION: normal build remains 5; experimental-v6 remains 6, both declarations unchanged from base (see protocol-constant-diff.log). Additive capabilities/variants are present. No protocol bump found.

## Material compliance gaps / required host checks

1. F1 literal daemon leafId scan fails (above). Exact command `python3 .omo/evidence/rex-adoption-20261104/verify-audit/static-review.py`, exit 1, raw `static-review.log`; leafId assertion location `layout_migration.rs:120`. Smallest repair shape: document an approved migration-only exception to the literal F1 search, or move old frontend parsing outside daemon source while preserving migration; never remove needed legacy migration merely to satisfy grep.
2. T30 isolated-native-webview requirement is only provisionally waived for an iframe pending native isolation QA on all three OS. BLOCKED approval until host evidence; see F2. No audit claim that this condition has passed.
3. T10 `layoutSync.ts:300-306` does not classify TOGGLE_PANE_EXPANDED; daemon supports it. Plan asks every layout action to be sent. Smallest repair: translate and classify that existing action and verify expansion survives rehydrate/restart. `layoutSync.ts:233,239` null-active rejection, `supervisor.rs:624` headless managed ownership default, and headless excluded GUI harnesses are carried into F2 rather than ignored as build-green.
4. Cross-window transfer and swap sequential-file durability risks (T10/T11) remain runtime recovery obligations; no interrupted-transfer/swap reproduction here. T17 exact hash folding/page-granular retention/fuzz duration, T18 signature environment isolation/MSIX, T26 NO-GO and Windows UDP measurements, and all checklist host QA gaps remain obligations of the combined host legs, not inferred from ancestry.
5. F3/F4 cannot approve from this audit. The success table's IS-1..IS-19 requires real same-head scenario evidence and four-way compatibility/handover, Windows/MSIX, headless and default release builds. T13 is legitimately blocked, meaning full IS-6 TCP-removal end state is not yet delivered. Never label whole plan complete merely because merge inventory passes.

## Review outputs and cleanup

`F2-security-review.md` independently REJECTS security with three concrete findings. `integration.diff` is the exact integration diff saved for review; source snapshots and command outputs are adjacent. No product fixes applied. No CI refs created/pushed, remote worktrees created, host claims acquired, or production processes touched by audit leg; cleanup is not applicable for those resources. The local audit worktree is intentionally retained to hand off its evidence commit.
