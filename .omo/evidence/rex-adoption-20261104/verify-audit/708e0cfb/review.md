# F1/F2 bounded source re-review: 708e0cfb

Product: `708e0cfb767a02e3d93db1661d6bf5035480f1f9`; repair author anchor `c5976f1f` is an ancestor (exit 0).

**F1: APPROVE scoped to source, with the explicitly authorized migration exception below.**
**F2: APPROVE scoped to the bounded R2-1/R2-2 source repairs.**

No remaining blocking finding in the reviewed repairs. This supersedes the two open R2 findings for this head only; earlier REJECT reports remain historically accurate and unchanged. This is not F3/F4, a whole-plan completion verdict, a runtime security proof, or approval of unexecuted automated gates. T13 remains release-gated; Windows recovery remains runtime-unproven. No waived macOS GUI QA was requested or performed. No builds/tests or product edits occurred.

## R2-1: reviewed identity reaches the approval transaction — CLOSED in source

- UI `PluginsSection.tsx:6-7,20-24,33-40` captures the exact manifest object rendered with name/version/module/digest/capabilities. Enable submits it as expected; disable sends null. A stale UI object is safe: mismatch refuses rather than consenting to current unseen content.
- IPC `src-tauri/src/ipc/layout.rs:24-45` accepts Option<Manifest> and forwards it without reconstructing identity. Protocol `protocol.rs:948-954` adds defaulted optional expected: old callers deserialize successfully but cannot enable without identity. Null and omitted expected both become None. Disable does not require an identity.
- Server `server.rs:7995-8012` supplies expected to approve and preserves PLUGIN_APPROVAL_REQUIRED as a machine error; IPC maps this to the typed PluginApprovalRequired error. No enable-side fallback retries without expected.
- Registry `plugins/registry.rs:124-151`: consent mutex spans compare/persist/stop. In the blocking worker, approvals lock protects cloned state. Enable first requires expected, then loads/validates current manifest and compares full Manifest equality before checking module bytes. Manifest equality includes name/version/absolute path/lowercase SHA256/capability set (`mod.rs:16-24,28-49`). Reordering capabilities is harmless set equivalence, not a permissions omission.
- Missing expected and unequal current manifest return IdentityChanged before save or in-memory replacement. Malformed/missing manifest, hash mismatch, unavailable config, join and persistence errors propagate; they do not enable. Removing old approval happens only in a local clone until save succeeds. Existing approval is intentionally left unchanged on refused reapproval; this does not authorize the changed manifest.
- `approved_bytes` (`mod.rs:52-65`) hashes the capped file snapshot. Runtime rechecks on every attempt and sends that same byte vector to the fixed runner. Changing manifest/module after the comparison cannot substitute a different identity or different digest bytes: a subsequent list/start mismatch or digest mismatch prevents launch. It may cause a reported error/availability failure, not broaden consent.
- `next.save` precedes `*current = next` (`registry.rs:145-146`); persisted approvals contain the compared manifest, not a new reload. `Approvals::load/enabled` retain full identity matching. `write_private_json` (`remote/auth.rs:1820-1880`) serializes to a restricted temporary file, syncs file then renames under a store lock. Parent-directory sync is best-effort in this existing helper; this review does not certify crash durability of directory metadata. No new crash-durability promise is needed to close R2-1.

### Race/lock/error review

Approve releases approvals before awaiting stop; start holds running then briefly takes approvals. Stop holds running while awaiting supervisor but the supervisor only takes views, never approvals/consent/running. No lock cycle is introduced. Consent serializes independent approve operations; starts between approval persistence and stop can only use an exactly approved manifest, and stop subsequently cancels that supervisor. Starts after a disable see approvals missing and refuse. An earlier authorized operation may finish before disable completes; disable is not instantaneous rollback of an in-flight operation.

Persistence precedes teardown: teardown/start/list error can therefore report failure after consent was saved. The error is propagated, not concealed; repeat enable still requires reviewed identity. Cancellation of an outer approval future does not undo an already running spawn_blocking persistence operation (existing lifecycle characteristic), but that operation can only persist the identity explicitly supplied and compared. It is not a consent substitution bypass. Full owner-native filesystem compromise remains outside the guest threat model.

## R2-2: daemon retained render state bounded — CLOSED in source

`plugins/registry.rs:33-73` introduces private document storage behind Views with the sole insertion API:
- Per plugin: 16 views, 512 KiB key+document bytes.
- Global: 128 views, 4 MiB key+document bytes.
- Each entry includes UTF-8 byte lengths of plugin name, view name and final wrapped document, not just guest HTML. The CSP wrapper and guest-chosen long keys cannot evade accounting.
- Existing key subtracts its old complete entry size, adds its new size and zero count; novel key adds one count. Shrinking/replacing a view frees capacity without an ever-increasing counter.
- All checks precede map mutation. Error preserves prior contents and count/byte state. Global totals are recomputed from the actual map, so no drift on failed replacement or removal.
- The write lock covers calculation plus insertion, serializing all plugins and cleanup. There is no check/unlock/insert race. Stored sizes are already capped and incoming requests are limited to MAX_LINE at the stdio boundary; arithmetic cannot practically overflow from guest values. Private documents prevent bypassing Views::insert elsewhere.
- Payload limits are not a precise allocator-RSS ceiling: BTreeMap/String metadata and short-lived request, wrapper, clones and response/event allocations add bounded overhead. This is no longer multi-gigabyte retained document accumulation from distinct view keys.

Production dispatch `server.rs:6812-6820` creates the document, attempts bounded insertion, returns PLUGIN_VIEW_BUDGET_EXCEEDED on error, and only then sends PluginViewChanged and renderOk on success. No failed insertion publishes a success event. Public PluginRender still refuses at :8015; stdio captures the manifest plugin name, so the guest cannot charge another plugin or insert directly. Read clones do not retain a second unbounded map.

### Cleanup and supervisor ordering

Registry production start uses supervise_with_cleanup (`registry.rs:176-178`), not the no-cleanup wrapper retained for tests. Runtime invokes cleanup after child reap before returning on disable or announcing exit/retrying (`runtime.rs:122-130`). Registry additionally clears on every normal Result return from supervise, covering early hash, process, I/O/reap errors and restart exhaustion. Registry::stop clears even on JoinError (:185-202); the prior early-return-on-JoinError hole is removed. Disable waits supervisor teardown; restart cleanup precedes the next launch. Running mutex excludes a replacement supervisor while stop awaits, so old cleanup cannot erase a newly started generation under the same name.

On task panic not followed by stop, already retained state can remain, but it is still within the global budget; this is not the original unbounded accumulation. No panic path added by bounded accounting is evident. Cleanup does not publish a refresh event: an already rendered iframe can show its last document until another refresh, while daemon accounting is freed. This is a display-liveness limitation, not renewed stored-state exhaustion or an event announcing failed render success. Budget refusal returns a small response without triggering a restart; guest may handle it. Event-send failure after successful insertion is ignored as existing best-effort broadcast semantics, not a false insertion success.

## F1 explicit migration exception

The user explicitly requested this exception; it is recorded here rather than claiming literal matches disappeared:

**Legacy import may read visual leafId/activeLeafId and preserve the original legacy/backup file. Migration fixtures and a one-time transient leafId-to-node_id mapping are permitted. Newly daemon-owned persisted pane identity must use numeric node_id/active_node_id/expanded_node_id, never persisted visual leaf IDs.**

`leaf-references.json` lists actual matches including `layout_migration.rs:120`, fixtures and serialization-negative assertion. Nothing is hidden, relocated, or renamed to satisfy grep. `layout.rs` persists Layout rather than View.migration_map; legacy reads mint numeric IDs. This reconciles T9 import with F1 no-persisted-visual-identity intent; it does not approve arbitrary future leafId storage. The historical empty-search literal FAIL is not rewritten.

The 9-file repair delta introduces no daemon layout identity changes, protocol bump, entitlements, lint suppression, deployment or FFmpeg pipeline changes. The prior source closure of SEC-1/SEC-2 native socket escape/SEC-3 and expansion forwarding is unaffected. Existing iframe deviation, automated security gates and full host acceptance are not magically verified by this bounded source approval.

## Native revision inspection (prior access gap resolved)

The dated worktree `/Volumes/T9-Mac/project/ferryx-wt/rex-repair-terminal-build-20261104/src-tauri/vendor/ghostty` contains 216426b. Saved native diff against 83fb95624 changes only Screen.zig and Terminal.zig (30 insertions, 11 deletions):
- `Screen.zig:813-819` makes previous-row cell lookup optional, using physical page_pin.up(1) rather than asserting active cursor y>0. At active row zero it may correctly find history; at physical history start it returns null.
- `Terminal.zig:1712-1716,1738-1742` handles the optional cell at both wide-cell/spacer-tail overwrite sites, clearing only a previous spacer_head, including when that row has scrolled into history.
- Source search finds both production callers handling optional and the existing Screen test updated to unwrap. Added Terminal fixture covers both overwrite columns with the head in history. No added suppression or ambient authority changes; no source blocker found in this bounded native delta. Tests were not run and this does not certify the full terminal implementation.

## Evidence and verification limits

Merged frozen integration into the existing audit branch. Product equality outside evidence: git diff --exit-code 708e0cfb returns 0. Historical report/d831 evidence equality against 2138dfb2b returns 0. No production operations, remote worktrees, ref pushes or host claims. Full repair diff, native diff and numbered source captured losslessly with gzip. Source verdict is based on the actual implementations, not inferred from added tests. No executable build, automatic-test or GUI QA result is claimed. The initial attempt at the undated native path failed because that path does not exist; using the worktree-list path resolved it without modifying either checkout.
