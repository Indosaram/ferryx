# F1 compliance re-review: d8313422

Frozen product head: `d8313422bde72275f08a1db6bf789597edc46d34`.
Verdict: **REJECT for final approval**. Merge inventory passes with T13 release-gated; the literal leafId rule has a precise migration-only discrepancy, not a demonstrated daemon visual-ID ownership regression. F2 still rejects on two concrete plugin boundary defects.

Historical `../F1-audit.md` and `../F2-security-review.md` are preserved without edits. Product was merged into the existing audit branch; that merge incorporates supplied product commits, not reviewer-authored product changes. `product-identity.log` establishes the resulting product tree equals frozen d8313422 outside audit evidence (exit 0). No product edits/builds/tests performed. Full base-to-frozen diff and repair delta saved in `full-integration.diff.gz` and `repair.diff.gz`.

## Inventory and guardrails

- `todo-ancestry.json` reruns ancestry to d8313422 for all 30 delivering commits/merge anchors from the historical 1-31 inventory: all exit 0. T13 remains BLOCKED because no published release contains T12; tag query exits 0 with empty output. This is an inventory pass, never a claim all acceptance/QA has passed.
- Global no-local-build rule: adhered to in this leg. Only git/source reading/Python evidence capture used; no diagnostics that implicitly build, no runtime tests, hosts or production processes operated. Historical operational constraints cannot be proven by diff alone; prior audit limits still apply.
- No newly added Rust allow/expect/cfg_attr allow across d59508fa9...d8313422 (`guardrails.json`). No suppression/lint config edits or foreign AGENTS/picker edits (`lint-config.log`, exit 0); mandated T19 stronger lint command wiring remains as previously reviewed.
- Protocol version declarations unchanged: normal v5 and existing experimental-v6 declaration. `protocol-diff.log` records additive request changes, not a bump.
- FFmpeg LGPL build/resources diff empty (`ffmpeg-boundary.log`, exit 0); no new macOS entitlements or MSIX capability changes beyond the already-reviewed ferryx protocol extension. No production deploy, paid CI action, system SSH configuration, foreign dirty-file modification, or ref push by this leg. Existing compile-only workflow is not release signing/publishing.
- No new Gemini support work, OSC52 read capability, project-local script/plugin auto-discovery, or unconfirmed destructive deep-link action in the repair delta. Native command plugins are now explicitly rejected, not silently supported or migrated.
- Existing .orca-worktrees parsing and legacy storage compatibility remain as prior audit; repair does not remove them. Repair source pin advances Ghostty, so it is not correct to claim native terminal source unchanged. Local canonical submodule lacks commit 216426b... (git diff failed with invalid revision range); source-level native implementation review is therefore limited to the superproject delta and supplied repair rationale, not independent inspection of that unavailable native commit. Its transport/verification remains owed; no product checkout was altered to hide this gap.

## leafId: exact scope and truthful result

Literal broad F1 text says 'no leafId in src-tauri/src/daemon'; the frozen tree still has matches. `guardrails.json` enumerates them, concentrated in `layout_migration.rs`:
- line 120 reads the required legacy field;
- migration fixtures use historical leafId/activeLeafId shapes;
- the regression assertion checks serialized Layout lacks leafId.

Semantic Must NOT is **no storing leafId in daemon-owned layout identity**, while T9 explicitly requires reading old frontend layout and returning a one-time leafId->node_id mapping. These cannot be implemented honestly by banning every lexical mention inside the migration boundary. The migration convert functions mint numeric node IDs, store the mapping only as the first View's ephemeral migration_map, and persist Layout separately (`layout.rs:160-165,716-745,851-853`). The old source and backup files intentionally retain legacy IDs and are not deleted. Node/Tab/Layout fields use node_id/active_node_id/expanded_node_id (`layout.rs:25-66`). General flattened content metadata maps are not an absolute arbitrary-key prohibition; the meaningful property is that pane identity is daemon-issued numeric IDs rather than UI leaf IDs.

**Assessment:** migration-only field references and fixtures are necessary, justified semantic compliance with T9; broad empty-grep acceptance is literally false, and remains a plan wording exception needing explicit recorded resolution. No moving/hiding references, renaming strings to evade search, or breaking migration is recommended. Bounded documentation repair: state 'no persisted UI pane identity outside legacy backup; leafId references allowed only in migration reads/fixtures and transient migration mapping', then retain checks on persisted Layout identity and round-trip fixtures. This re-review does not convert the original historical literal FAIL to a pretend empty search.

## Repairs and still-open approval conditions

- Original SEC-1/2/3 and expansion forwarding mechanisms are closed in source; see revision-specific F2 for exact paths and limits. They are not reported as automatic-test PASS.
- User-selected Wasmi capability-only out-of-process SDK replaces the plan's native executable manifest. This is an explicit requested architecture change, not scope drift to conceal. Sole host bridge is inspected; F2 finds consent identity race and retained-render-state exhaustion despite the interpreter boundary.
- iframe versus isolated-native-webview implementation remains a documented deviation. macOS manual/GUI QA is waived; security and automatic tests are not. Do not reimpose waived manual work or treat waiver as native IPC/CSP isolation proof. Remaining host security checks are identified in F2.
- Windows ancestry recovery is source-reviewed, runtime-unproven; packaged headless supervisor managed_externally=false remains unchanged. GUI harness feature exclusion is not a passing harness result. T10/T11 interrupted cross-file transfer/swap recovery and all previously owed non-waived automatic gates remain for parent verification.
- Helpers/native pin/build repair documents explicitly defer actual rebuilding/verification. Existing acceptance tests inspected are not execution evidence. This audit does not approve F3/F4, assert zero-session-loss, or mark the whole plan complete.

## Findings, evidence and cleanup

F2 REJECT: R2-1 approval does not bind the identity shown to owner (`plugins/registry.rs:63-79`, UI :19-21); R2-2 per-view retained daemon documents lack aggregate/cardinality cap (`server.rs:6812-6816`, registry :18). Both have bounded repair shapes in F2. No runtime exploit or baseline failure claimed, no product repairs applied.

Raw command receipts, numbered source, full diffs and ancestry are under this revision directory. No CI refs created/pushed, remote worktrees created, host claims held, or production daemon touched. Local audit worktree retained for evidence handoff. Review model remains user-selected gpt-6.1-sol.
