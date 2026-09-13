# Verified gap findings: provisional packet ownership

2026-09-13. Addendum to `repair-packets.md`, not a frozen implementation plan.
Remaining source lanes may add findings. No source changes or test execution
are authorized by this document itself; isolation approval remains pending.
Exact proposed RED/GREEN commands and binary conditions are in
`gap-verification.md` section 5 and `remaining-powershell-wrappers.md`.
Before each fix, register the final command, assertion and packet digest in
loop criteria. Earlier registered packet hashes do not cover this addendum.

## Extend existing owners; do not create competing writers

- P06 owns GT-05, GT-07 and GT-08 in its already assigned renderer-contract
  tree and surface-host contract. Preserve RF-08 aliases. GT-05 may consume
  compiler-artifact output without changing the production example target.
  If an additional runner file is needed, allocate it before editing.
- P09 owns HIST-01 only as a separate sequential UI applicability increment:
  add `ui/src/components/settings/TerminalSection.tsx` and its proposed
  `.test.tsx` to that packet. Gate the Mac-specific affordance without changing
  actual Alt, paste, close or link semantics. This is not a new input policy.
- P18 owns HIST-02 in `scripts/check-tree-quiescent.sh`, with an isolated
  metadata-format regression allocated before editing. A diagnostic helper
  fix must not become a new mandatory CI quiescence gate.

## New disjoint owners

### P23: relay enrollment and integration validators

IDs GB-01, GB-03 and GB-04. Own
`src-tauri/src/remote/relay_server.rs` and its embedded tests.
This consolidates the native-shell fixture and its denial/output/admission/
lease assertions under the same writer. It extends DS-09's fixture family,
not P08's file ownership.

The durable enrollment transaction may require
`src-tauri/src/remote/auth.rs`. Reserve it to P23 for that transaction only.
P08 and P12 must consume its agreed API or request an explicitly serialized
change; they must not independently edit the auth writer while P23 runs.
No general authentication redesign is implied.

### P24: DAG watch recovery and blocking work

IDs GB-02 and GB-05. Own `src-tauri/src/dag/watcher.rs` and embedded tests.
Keep one writer for recovery state and scan offloading. Production callers
are read-only unless an exact necessary API change is allocated first.
Do not confuse watched-directory loss with the foreign rescan findings.

### P25: helper and survival harness safety

IDs GT-01, GT-02, GT-03 and GT-04, including the PowerShell-wrapper extensions.
Own these existing files:

- `scripts/qa/ssh-bridge-survival.mjs`
- `scripts/qa/ssh-helper-setup.mjs`
- `scripts/qa/ssh-helper-survival.mjs`
- `scripts/qa/ssh-process-survival.mjs`
- `scripts/qa/verify-ferryx-resume-cwd.mjs`
- `scripts/qa/check-remote-helper.ps1`
- `scripts/qa/verify-remote-helper.ps1`
- `scripts/build-remote-helpers.mjs`

Proposed new isolated tests are `scripts/build-remote-helpers.test.mjs` and
`scripts/qa/helper-wrapper-contracts.ps1`. These names are not claims of
existing executable harnesses. Add import-safe modes before invoking any
currently unsafe `--self-test` form. Preserve helper protocol 1 versus daemon
protocol 3; no universal version replacement.

Do not overlap P22's singular `script/qa/win-daemon-e2e.mjs` or P19's daemon
persistence target. The wrapper executable-path issue is related to GT-05,
but its code belongs to P25, not P06. Coordinate native helper artifact
provenance with P10 and P18; debug-only build and no installed replacement.

### P26: permission capability and safe contract

IDs HIST-03 and GT-06. Own `src-tauri/src/permissions/mod.rs`,
`src-tauri/src/ipc/permissions.rs`, and
`src-tauri/tests/permissions_contract.rs`.
P21 retains the separate PermissionsSection UI warning. Establish a
non-launching contract seam before executing the permissions target on
macOS. Test machine-consumed capability/result values, not reason prose.

### P27: remote wheel units

GAP-UI-06 aliases existing WIN-UI-09 remote behavior and historical
L5-UI-FRONTEND-8; it is not three defects. Own
`ui/src/remote/RemoteTerminal.tsx` and
`ui/src/remote/RemoteTerminal.contract.test.tsx`.
P02 continues to own native-pane normalization. Agree on unit/sign/bounds
semantics, but do not introduce a shared abstraction merely to merge these
two components. Extend this same owner with remote RC-04: host-scoped
preferences require this component's settings caller, so a competing
preferences writer is not disjoint. Reserve `ui/src/lib/terminalSettings.ts`,
`ui/src/lib/tauri.ts`, `ui/src/lib/remoteClient.ts`, their existing tests and
new `ui/src/remote/RemotePreferences.contract.test.tsx` to P27 for this
contract only. P13 keeps its separately named browserTauri test. Do not
change desktop-embedded local-versus-remote appearance policy without a
decision; the proved defect is authenticated web host preference loading.

Exact additional RED/GREEN:
`bun run --cwd ui test src/remote/RemotePreferences.contract.test.tsx`.
Keep the real token migration/settings/client chain; record selected-host
request, prefix, header and distinct preference values with no local
override. Host switch must not reuse the other host's credential or cache.
Current Windows browser runtime must reproduce those requests and resolved
values after reload and host switch; numeric settings are not font-pixel QA.

### P28: remote selection request lifetime and target identity

Remote RC-01, RC-02 and RC-03 from `remaining-remote-callers.md`. Own
`ui/src/remote/RemoteApp.tsx`, `ui/src/remote/RemoteUI.test.tsx`, and
`ui/src/remote/RemoteAttention.test.tsx`. RemoteSessionList and server
validation are read-only contracts, not an excuse to accept mismatched
targets. P27 consumes the resulting host context without editing RemoteApp.

Exact additional commands, with new cases in existing test files:

- `bun run --cwd ui test src/remote/RemoteUI.test.tsx -t 'releases selection when the request never settles'`
- `bun run --cwd ui test src/remote/RemoteUI.test.tsx -t 'ignores an obsolete selection response while a newer selection is pending'`
- `bun run --cwd ui test src/remote/RemoteAttention.test.tsx -t 'uses the published target worktree for swipe and waiting selection'`

Register deferred responses before triggering requests. A never-settling
POST must release pending controls and optimistic override at the bounded
deadline while retaining authoritative selection. After authoritative
replacement permits B, late A success and failure must not clear B, alter
B's acceptance or replace B's deadline. Previous/next swipe and waiting
selection must send the published target worktree, falling back only when
that target omits it. The fixture must reject mismatched tab/worktree pairs
like the real server. Runtime uses an owned relay response gate and Windows
browser to exercise these exact orderings and real cross-worktree selection,
with selected socket and sibling-session receipts.

### Contract validator extensions: qualified identifiers

`remaining-contracts.md` local RC identifiers are CONTRACT-RC identifiers
here; they are not aliases of the remote application defects.

- P10 adds `src-tauri/src/ssh/helper_setup_tests.rs` and, only if necessary
  for a transport recorder seam, `src-tauri/src/ssh/helper_setup.rs`.
  CONTRACT-RC-01 replaces the unowned assumed-missing absolute path with an
  owned missing child and proves zero transport spawn. Never create the
  global path as RED. CONTRACT-RC-05 replaces diagnostic-prose matching with
  the structured error fields while preserving the existing test.
  Commands are the two exact `--lib ssh::helper_setup::tests::... -- --exact
  --nocapture` invocations in remaining-contracts section 2; Windows native
  executables must exercise local failure and sentinel mapping without SSH.
- P02 owns the input fixture part of CONTRACT-RC-02; P06 owns its Wayland
  fixture part, adding
  `src-tauri/tests/native_terminal_wayland_subsurface_contract.rs` and
  reserving `src-tauri/src/terminal/preferences.rs` for the explicit fixture
  configuration seam. P06 provides that seam before P02 consumes it.
  Existing surface-host ownership remains P06. Require zero ambient
  Ghostty CLI/profile access, deterministic valid metrics and restoration
  of prior state on all exits, not global environment mutation.
  Exact commands:
  `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract -- --nocapture`
  and
  `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract -- --nocapture`.
- P03 adds
  `src-tauri/tests/native_terminal_engine_contract/mouse_encoding.rs` for
  CONTRACT-RC-03. Exact command:
  `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract mouse_encoding::test_mouse_encode_disabled_and_sgr_enabled_tracking -- --exact --nocapture`.
  Wrong coordinate/button mutation must be rejected; real encoder output
  must equal `\x1b[<0;3;3M` on Windows. This is an oracle repair, not a claim
  that the current encoder emits wrong bytes.
- CONTRACT-RC-04 concerns a macOS-only main-thread prerequisite. It remains
  a recorded out-of-Windows-scope observation, not a new Windows fix or
  authorization for macOS desktop execution.

### Remaining native UI validator ownership

`closure-nativeUi.md` extends P02 with
`ui/src/components/NativeTerminalPane.exitAttach.test.tsx`,
`ui/src/components/NativeTerminalPane.lifecycle.test.tsx`, and
`ui/src/components/TerminalPane.exitAttach.integration.test.tsx`.
NATIVEUI-GAP-01 preserves exact binding-key callback tuples and reducer
stale-binding protection. NATIVEUI-GAP-02 explicitly selects/restores both
Mac and non-Mac fixtures rather than inheriting the execution host.
NATIVEUI-GAP-04 must observe detach start before remount and completion,
retaining the distinct pre-invocation cancellation case. NATIVEUI-GAP-05
replaces completion polling/arbitrary clock advancement with exact signals
in those fixtures and the already-owned main pane test; actual timed
behavior still uses controlled clocks. The exact file-filter invocations
and binary conditions in that report are required before repairs.

NATIVEUI-GAP-03 is WIN-UI-03/P04's existing effective-chord collision
validator, not another product defect. Include aliases and platform
normalization while preserving explicitly allowed cross-surface pairs.
No new Windows live failure is claimed from any of these test weaknesses.

### P29: native Windows tooling test paths

TOOLING-GAP-03/04 from `closure-tooling.md`. Own
`scripts/macos-dev-runner.test.mjs` and `site/src/seo.test.ts`.
Exact commands: `bun test scripts/macos-dev-runner.test.mjs` and
`bun test --cwd site src/seo.test.ts`, in isolated native Windows fixtures.
The former also runs on macOS to retain intended fake-signing behavior;
never invoke a real signer or build as mock-tool fallback.

Signing test acceptance requires one owned debug target, reliable mock
tool interception, both success/failure paths and child reap before root
removal. SEO acceptance requires fresh real builds and URL-slash route
keys on either filesystem, all required content/link checks, and continued
failure when a required page is removed. Preserve both base and root-origin
build cases. Missing Bash/build dependencies are blocked prerequisites,
not intended RED. Do not weaken tests with platform skips.

TOOLING-GAP-01/02 remain recorded defects of an old Homebrew-bound image
mock that is neither a Windows acceptance harness nor used for this task's
evidence. No repair packet or permission to execute that mock is created.

### P30: accepted RGB expansion must preserve text frames

IMAGES-GAP-01 from `closure-images.md`. Own
`src-tauri/src/native_terminal/images.rs` and
`src-tauri/tests/native_terminal_images.rs`. P06 keeps renderer/surface-host
ownership; this repair need not change those callers or pinned vendor code.

Exact proposed RED/GREEN:
`cargo test --manifest-path src-tauri/Cargo.toml --features native-terminal --test native_terminal_images kitty_rgb_expansion_limit_does_not_block_text_frames -- --exact --nocapture`.
Real parser acceptance of 4097x4097 RGB uses 50,356,227 stored bytes but
would expand to 67,141,636 RGBA bytes, beyond the 67,108,864-byte limit.
Current capture returns an error and aborts the whole text snapshot.
Preserve the allocation limit while omitting unsupported expanded images
or rejecting them before protocol acceptance; do not add tiling or raise
the memory cap. Regression must retain a supported image and verify a
subsequent green text cell through actual snapshot/render readback.
Current Windows debug terminal must present following text and preserve
the supported image under the same stream, with owned-pane cleanup.

### P28 pairing validator extension

Add `ui/src/remote/deviceIdentity.test.tsx` for OTHERUI-GAP-01 only.
Exact invocation:
`bun run --cwd ui test src/remote/deviceIdentity.test.tsx -t 'renders device name pre-filled, allows editing, and submits installationId with deviceName'`.
Subscribe to onPaired before submitting, control response completion,
await it inside act and restore fetch/storage/DOM on all exits. Preserve
the real request/Response/callback chain and exact machine fields.
Wrong or missing completion must fail; controlled valid completion passes.
This is a validator synchronization repair, not a newly established pairing
product defect or a reason to introduce a new network feature.

### P31: workspace-qualified sortable row identity

BOUNDED-UI-01 from `bounded-ui.md`. Own
`ui/src/components/Sidebar.tsx`, `ui/src/components/WorktreeList.tsx`
and new `ui/src/components/Sidebar.sortableIdentity.test.tsx`.
Exact RED/GREEN:
`bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx`.
Retain real grouping, Sidebar, WorktreeList and DnD contexts. Registered
IDs must equal context IDs with nonnegative indexes. Same-path rows from
distinct workspaces must retain distinct drag data and saved ordering.
Keyboard reorder with deterministic geometry must preserve exact member
order on reload; keep legacy stored-path reading. No project-group redesign.
Windows debug runtime must show correct displacement and persisted reorder
of owned grouped member checkouts, including equal paths on distinct hosts.
No production store/types allocation is implied.

### P28 device suggestion extension

BOUNDED-UI-02 from `bounded-ui.md`. Add
`ui/src/remote/deviceIdentity.ts` and new
`ui/src/remote/deviceIdentity.opera.test.ts` to P28.
Exact RED/GREEN:
`bun run --cwd ui test src/remote/deviceIdentity.opera.test.ts`.
Windows OPR+Chrome+Safari user agents must identify Opera; Edge and Chrome
controls retain their identities. Assert the submitted deviceName field
through the real pairing caller as well as the pure parser result.
Owned Windows Opera pairing must display and submit the correct unedited
name while retaining an edited name. No auth or UA-detection service change.

### P32: scoped-history test-target active ancestry

BI-01 from `bounded-infrastructure.md`. Own
`src-tauri/src/ferryx_scope/history/mod.rs` and
`src-tauri/src/ferryx_scope/history/hardening_tests.rs`.
Exact existing RED/GREEN:
`cargo test --manifest-path src-tauri/Cargo.toml --test scoped_history hardening_tests::claude_active_branch_excludes_abandoned_sibling -- --exact --nocapture`.
The real owned JSONL fixture must yield no abandoned-sentinel search match
and exactly a,c from History::read. Preserve the existing assertion and
shared paging/corruption coverage. Run the native Windows test executable;
this is its real affected surface, not a desktop history feature.
Do not expose the prototype through IPC or implement unrelated Unix
root-symlink behavior under this packet. No test skip or weakened oracle.

### P33: native popup ownership and listener lifetime

SHARED-NATIVE-01 from `shared-native-callers.md`. Own
`ui/src/lib/nativeMenu.ts`, new `ui/src/lib/nativeMenu.test.ts` and
`src-tauri/src/ipc/native_menu.rs`. Caller lifetime edits, if needed, are
restricted to popup handling in TabBar, Sidebar and WorktreeList and must
follow P31 and any existing TabBar owner, never write those files in parallel.
Exact regression: `bun run --cwd ui test src/lib/nativeMenu.test.ts`.
Keep real helper and two row callers; controlled bridge registration,
dismissal and selected-action signals must prove same-ID B action invokes
only B once, with zero obsolete A callback. Cover unmount/reopen while
registration/invoke is pending, rejection, dismissal, callback throw and
exactly-once cleanup. Mock only the bridge, not row ownership decisions.
Register any additional backend test command before edits.
Windows debug acceptance uses owned disposable rows: A/Escape then B/Copy
Path under controlled dismissal overlap, callback identity plus clipboard
proof, long-open selection, cancellation and removal cleanup. Do not delete
real worktrees or increase the grace delay as the repair.

### P34: initial native focus reconciliation

SHARED-NATIVE-02 and NATIVEUI-GAP-05 extension from
`shared-native-callers.md`. Own `ui/src/lib/nativeWindowFocus.ts` and
`ui/src/lib/nativeWindowFocus.test.ts`. Coordinate any edits to
`ui/src/lib/notificationCoordinator.test.ts` with P14 after its writes.
Exact regression:
`bun run --cwd ui test src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts`.
Exact subscribed setup/snapshot signals must prove the newest focus event
wins over a pending snapshot and the real coordinator's bell/unread decision
uses that state. Cover duplicate start, positive custom focus dispatch,
negative state and rejection/null fallback without polling or sleeps.
Subscription must finish before the initial query; protect against stale
query completion rather than merely swapping calls.
Windows debug acceptance gates startup focus transition and records native
foreground state, registration readiness, terminal bell and unread decision,
with a latest-focused control. This is not proof of OS sound/banner delivery.
P34 absorbs this test's P02 validator allocation to prevent concurrent writes.

## Moving-source boundary

Lead observed HEAD `d6ebb720bb343a2b055a47c7022aeed616bf0bd1`; prior
native-image/App changes are now committed by another session. Later
foreign uncommitted changes again touch App, tauri, RemoteTerminal,
RemoteUI tests, remote protocol/server and worktree paths. Reviewed
examples add terminal creation and mobile IME/touch behavior. P05/P27/P28
must preserve them, reread after ownership is settled and recheck only
affected conclusions against the frozen implementation base. This
document acquires no foreign edit ownership and does not certify those
new behaviors. Isolation approval is still pending.

## Scheduling constraints

These packets are proposals for one parallel implementation batch followed
by combined verification and parallel fixes. Dependencies protect actual
contracts and write conflicts, not a per-packet approval chain. Native
fixture safety, owned environment and build provenance must hold before any
live RED run. P23/P24/P25/P26/P27 have disjoint listed production files;
P06/P09/P18 extensions are sequential within their original owners.

Unknown hardware/browser/ACL behavior, explicit unsupported capabilities,
test-inventory observations alone and policy suggestions do not become
implementation packets. The accepted source-verification report retains
their exact distinctions. Foreign changes remain read-only until their
integration ownership is resolved.
