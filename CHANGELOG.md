# Changelog

All notable changes to Ferryx, one section per date-based release tag (newest first).
Generated from Conventional Commit subjects by
`bun scripts/lib/release-changelog.mjs --write-changelog`; do not edit sections by hand.

## Unreleased

### Changed

- Managed worktrees now use Ferryx names: new worktrees are created in `.ferryx-worktrees/<workspace>/<slug>` on branch `ferryx/<workspace>/<slug>`. The branch prefix can be changed per repository (lowercase letters, digits, `.`, `_`, `-`; up to 31 characters) and is stored in `worktree-settings.json` in the Ferryx data directory.
- Existing worktrees are not moved or renamed. A repository that already has `.orca-worktrees` keeps the `orca/` prefix and that directory for new worktrees until you set a prefix, so one repository does not end up with two layouts. Ferryx keeps recognizing `.orca-worktrees`, `orca/` branches and the old SSH `.orca-worktrees/wt-<slug>` layout.
- If your scripts match `orca/*` branches or `.orca-worktrees` paths, update them to also match `ferryx/*` and `.ferryx-worktrees` (or your configured prefix) for repositories without legacy worktrees.
- Remote (SSH) worktrees now use the same `<dir>/<workspace>/<slug>` layout as local ones instead of `.orca-worktrees/wt-<slug>`.

## v2026.10.11.1

### Features

- **remote**: automatically discover and provision Tailnet SSH machines (1aa91a3d)
- **chat**: compose reference chat routes and acceptance producers (c3432218)
- **chat**: author original-pane input and native prompts (c934856b)
- **chat**: author reference history and presentation foundation (cb3c6116)

### Bug Fixes

- stabilize integrated GPU and SSH release paths (9820ed57)
- **split**: register the split workspace over its dedicated daemon connection (b92b0bea)
- **remote**: list project roots for non-Git folders (50e622b1)
- **daemon**: release metadata guards before journal persistence (f0d5ff7a)
- **agents**: discover omo sessions through open transcripts (2fc1ad04)
- **pty**: initialize Winsock before Windows ConPTY input (4b6c81c8)
- **ssh**: collect finite Windows stdout through a temporary file (9aba27c8)
- **ssh**: allow complete large worktree inventories (22d2d578)
- **ssh**: honor registered platform during daemon spawn (1147c959)
- **dev**: resolve macOS runner root before cargo shell callbacks (44697062)
- **release**: reuse frontend assets for Windows NSIS build (fb968853)
- **release**: isolate source bundles across parallel platform builds (260f290b)
- **terminal**: roll back native reservations after attach timeouts (ecfa3b1f)
- **ui**: streamline output decoding and align test fixtures (4995fdf5)
- **scope**: enforce controller leases and push endpoint policy (c9dd54fe)
- **machine**: preserve snapshot deltas and stabilize owned socket contracts (c8125db8)
- **remote**: isolate machine inventory from mirror clients (30b125bb)
- **native-terminal**: honor absent snapshot bounds and raster formats (8d590809)
- **daemon**: disambiguate handover sockets and expose lifecycle completion (39431d46)
- **terminal**: batch VT text scanning and track published output (20a693f5)
- **daemon**: recover handshakes rejected by retiring predecessors (2db1e443)
- **session**: roll back unreachable spawns and preserve restore ownership (bcef7dcc)
- **daemon**: restore predecessor readers and route legacy remote commands (c0b7bb46)
- **daemon**: refresh cached connections and terminate detached streams (81aa878f)
- **terminal**: fence native reattach and preserve active stream ownership (ebf25532)
- **terminal**: preserve fast-exit output and contiguous replay history (c2a44c7b)
- **ssh**: preserve helper transport ownership and connection diagnostics (8e3aaad6)
- **ssh**: keep remote sessions recovering through transport outages (d02656be)
- **session**: preserve durable identity through recovery and native reattach (ccc1f655)
- **ssh**: preserve frontend recovery budgets and host subscribers (a0976ade)
- **terminal**: preserve inactive pane focus after attachment (102f3d49)
- **activity**: keep attention episodes in workspace reducer state (1c15fc17)
- **terminal**: let queued readers publish close completion (5ef0a5e0)
- **qa**: keep browser screenshots in temporary storage (1bc86721)
- **test**: initialize headless CLI fixture directory (4c6fc41a)
- **chat**: isolate pending prompts across reference targets (5d1901d7)
- **qa**: scope the non-local admission to ssh and cover the corroboration refusals (23d4f36e)
- **reference-chat**: close F-1/F-3/F-4/F-5 from the independent review (10c9af00)
- **reference-chat**: typed refusals, ssh-leg contract, host-independent provenance, macOS test stability (b7ed4b6c)
- **terminal**: bound close completion and count shared reader reads (6b69f10b)
- **terminal**: observe reader completion independently of join ownership (34d7e11d)
- **qa**: read published owner from gateway fixture record (6ad0376b)
- **terminal**: drain output before cancelling reader teardown (003b3b57)
- **terminal**: retain cancellable Windows readers through teardown (d2b1c609)
- **test**: use native paths for SSH inventory fixtures (a083c2ea)
- **chat**: close ingress ownership errors and QA fixture failures (f17842fb)
- **chat**: preserve provider identity and repair acceptance fixtures (0393c99b)
- **chat**: enforce gateway owner authority across reference clients (4dff2544)
- **qa**: validate numeric identities and capture failure state before cleanup (029b10bb)
- **test**: synchronize remote terminal selection and preserve diagnostics (020f1d57)
- **qa**: retain request identity in workspace refusal details (fee96915)
- **qa**: use unescaped workspace refusal string delimiters (45d430a5)
- **chat**: retain route fixture state and escape workspace refusals (442d129d)
- **chat**: repair parity fixtures and isolated QA lifecycle (c3c96345)
- **qa**: handle daemon stream errors and retain diagnostics (d51e2085)
- **test**: preserve projector child wait errors during cleanup (dfc8767b)
- **chat**: align reference contracts and complete isolated QA transport (b64da933)
- **chat**: integrate parser repairs and isolated gateway fixtures (e9b15101)
- **chat**: resolve prompt borrow and initialize QA runner before entry (f1ebd2cb)
- **chat**: reject backslashes in reference mention paths (cb16d153)
- **chat**: repair integrated contracts and QA entrypoints (d207a331)
- **chat**: restore contiguous image context memo (ef2d02b3)

### Maintenance & Internal

- refresh bundled remote helpers for release source (a86df627)
- **ssh**: capture portable loopback startup diagnostics (82452058)
- **remote**: join machine session lifecycle before fixture teardown (deacb57e)
- **terminal**: measure overflow cancellation separately from PTY drain (6880568a)
- **terminal**: use full-size chunks in live socket throughput fixture (0a2949cf)
- **ui**: load CLI settings fixture before test deadlines (27373f1b)
- **remote**: saturate TCP before blocked writer assertions (6877f950)
- **remote**: isolate CLI download candidate fixtures (bacb8ea2)
- **release**: refresh helpers for the current source closure (d2722f88)
- **ssh**: stabilize loopback and Windows native input proofs (1618f9e7)
- **machine**: isolate owned session metadata and handover fixtures (1ed35d62)
- **qa**: await inherited pipe closure before cleanup absence checks (94efba59)
- **release**: match portable host and extended keepalive contracts (f4d1c64f)
- **release**: inject process spawning for input fault contracts (dafff8d3)
- **qa**: await grandchild signal readiness in process cleanup fixture (0372909b)
- **release**: isolate CLI fixtures from developer checkouts and services (95ce674b)
- **release**: inject archive checker instead of mutating ESM builtins (819f39a0)
- **release**: include required CLI artifacts in release fixtures (1054764c)
- **dag**: isolate known project hash from host filesystem (7e55cbc1)
- **ci**: align workflow contracts with local-only releases (8c865a2b)
- **onboarding**: mock account enrollment in App wizard (419e7ee6)
- **paired**: accept additive capability renegotiation (e6777087)
- **terminal**: follow expanded hover toolbar boundary (f6c2c467)
- **qa**: use the Bun runner for script contracts (23526541)
- **terminal**: model event recovery and durable attachment (1795d0f3)
- **release**: provision a pinned minisign oracle when absent (f07abd6b)
- **app**: follow detailed terminal spawn contract (69dac82c)
- **release**: bump app version to 2026.1010.1 (2aff67be)
- **qa**: remove deferred execution declarations (0630227b)
- **chat**: cover host-staged attachment resource lifecycle (35cc1999)
- **qa**: drive both refused transports across the admission boundary (dd866780)
- **qa**: regression-test the ssh-scoped admission and pin the relay mirror (4134e7a5)

### Other Changes

- **test**: checkpoint cross-platform fixture repairs (c4770fd8)
- **herdr-reference**: capture browser screenshots alongside JSON artifacts (candidate 7) (ac394f66)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.10.09.2...v2026.10.11.1

## v2026.10.09.2

### Features

- **ferryx-host**: add the session host process and its local IPC (9e94beb9)
- **ferryx-vt**: add the libghostty-vt adapter for the session host (0d373e62)
- **ui**: add the session binding FSM reducer (spec 5.5) (39fc5025)
- **session-core**: add host and client state machines for session restore (04464d33)
- **fxsh**: add FXSH wire codec crate for the session host (7bc8cb91)
- GitHub issue to local worktree (dbbbbb58)
- **qa**: retry the bursty session-1 delegation stall with a sequence budget (2a741884)
- **qa**: settle the pre-split pane binding from a measured daemon inventory delta (bc078f70)
- **site**: restore landing layout and add Chinese localization (abd9b8d1)
- **qa**: drain the app's stdout and stderr into per-run artifacts (562144b3)
- **qa**: serve the frontend the debug binary expects and bind a real pane before splitting (48b4ed93)
- **qa**: construct the requested fixture sessions from the real daemon spawn paths (1df40271)
- **qa**: declare the scenario's fixture kinds to the product and fix the gated control test (5245e6ba)
- **qa**: author the native-scenario producer and consumer surface behind the local-split-qa gate (c34b90fc)
- **site**: polish landing accessibility and production performance (e5079c70)
- **terminal**: compose pane-liveness completion candidate for consolidated verification (5464da0d)

### Bug Fixes

- **daemon**: publish sessionExited for desktop GUI sessions again (23e457ff)
- keep main's swipe order and HMR config, publish desktop sessionStarted, drop untestable receipt suite (a7b34dfc)
- **ferryx-host**: make the host work on Windows and record P3 results (08262a63)
- load GitHub issue preview on Enter without submitting (3d54ca69)
- **worktree**: refuse to delete a slot whose checkout was swapped (ec1be0d5)
- **agent**: pass the seven-field pane binding when a reconnected agent attaches (42a8a402)
- **native**: keep a completed spawn's terminal visible after restart (29e60760)
- **daemon**: report the owner epoch with each session description (08aee786)
- **ssh**: re-arm remote sessions the daemon parked in disconnected (de9c67ed)
- **clipboard**: write desktop copy text through the native clipboard (87a45b0f)
- **dag**: find omo journals written outside the repository (98a3cf26)
- **daemon**: give the background daemon a lifetime independent of the GUI (0467210e)
- **clipboard**: write desktop copy text through the native clipboard (01916ed7)
- **native**: keep a completed spawn's terminal visible after restart (4c2c49cb)
- **daemon**: report the owner epoch with each session description (00c11af8)
- **dag**: find omo journals written outside the repository (6790cb5e)
- **dag**: find omo journals written outside the repository (740572bf)
- **daemon**: give the background daemon a lifetime independent of the GUI (029fc5fd)
- **qa**: complete pane-liveness fixtures, native linux driver, and environment passthrough (fa5a5c28)
- **daemon**: give the background daemon a lifetime independent of the GUI (25337fbf)
- **qa**: await the marker receipt that attests a submitted frame, not index 0 (e9c44176)
- **qa**: make the QA-owned shell start, and stop losing its diagnostics (e5f9e77f)
- **daemon**: add the desktop-GUI projector and the requested-session-id field (07c7bb5f)
- **daemon**: reject a reused split identity that carries different parameters (b8e807fb)
- **qa**: give each concurrent-batch wave its own geometry, or the conflict is invisible (78e37cdb)
- **split**: mark a created split session as starting up, or its shell never prompts (0a4a00c4)
- **qa**: isolate Windows app data too, not just HOME (6aa6a54f)
- **ui**: repair the spawnTerminalDetailed return type and record a spawned pane's identity (7e8fdfb7)
- **ui**: carry incarnation through REBIND_SESSION_BACKEND, or no pane can bind (c1b35602)
- **qa**: seed the first-run state so the onboarding dialog stops hiding the native surface (0902a692)
- **split**: carry the attach tuple, or split attach is refused outright (747c78d5)
- **split**: send an integer remainingMs, or the whole local-split path dies (6056a342)
- **pane-liveness**: make a refused split attributable and end controller ownership (7940bbf3)
- **qa**: call the reader's budget method on the budget, not bare (4c7b5ede)
- **ssh**: make remote folder listing survive zsh nomatch globs (128c60e6)
- **ssh**: make remote folder listing survive zsh nomatch globs (19e99a14)
- **remote**: size the chat drawer to the device that opens it (b2995f14)
- **qa**: repair the five failures the frozen pane-liveness gate reported (a3dd8603)
- **qa**: parenthesise the block in the two let-else reads (297515a5)
- **qa**: run the armed presentation hold off the runtime worker (40dd04ba)
- **qa**: offload the retry and handover settlements from the runtime (987c007f)
- **qa**: offload the remaining synchronous QA barrier writers (e9e7e8ca)
- **qa**: move QA producer file I/O off the async runtime threads (b5fb0610)
- **pane-liveness**: apply the F2 repair batch and close the split-flow gaps (ecf80277)
- **qa**: exclude fixture sessions from the split delta and drop an unused capture import (b1f249f4)
- **qa**: judge the attempt window by the scenario's own time, not by instrumentation (0726c70c)
- **qa**: keep the split measurements out of the verdict and inside the ceiling (57ef3d2d)
- **qa**: search the window scope when focus cannot be resolved, and wait for exit-file content (14744c8a)
- **qa**: raise the delegation retry sequence to outlast the measured 153s burst (cfb4374b)
- **remote**: serve chat history through the relay and scope account API URLs (68773596)
- **qa**: isolate FERRYX_SESSION_DIR so the app stops restoring the host's real profile (5c423880)
- **qa**: re-issue the UIA attach in a bounded loop, and report what the pane step read (04cf8c7a)
- **qa**: warm the lazily-built UIA tree before querying it (757f8414)
- **qa**: restore the runner suite to green and un-truncate the window evidence fields (beb80b72)
- **qa**: resolve the split affordance across owned windows and report the truth when it is absent (42fba06f)
- **qa**: force-reap the runner's own tree so the cleanup gate and the node hang both close (7a452a77)
- **qa**: newline-join every PowerShell script builder so here-strings parse (120bc965)
- **qa**: reach the interactive Windows session and scope the split affordance (15042b0f)
- **qa**: drop the managed-state proof from the spawned fixture futures (91d447e1)
- **qa**: repair the local-split-qa compile errors in the QA producer set (314251e0)
- **qa**: correct the barrier role map and lock the pre-arm contract (799a582d)
- **remote**: describe every live session and publish desktop focus (a15fdcf9)
- **qa**: install the barrier channel on the GUI boot path and reconcile the invented adapter names (dd9e6813)
- **daemon**: adopt a transferred session against the predecessor's authority, not the successor's empty registry (70eefafe)
- **daemon**: give paired and ssh-remote sessions a provable lifetime incarnation (39e722ce)
- **ipc**: wrap the attach fixture's incarnation in Some for the tuple field (426d1b27)
- **daemon**: make the split journal temp name portable on Windows (abd9e890)
- **native**: clone the completion window before moving it into the render closure (764934a4)
- **pane-liveness**: land round-3 backend, native, frontend and adapter repairs (21dea3c0)
- **ui**: repair pane-liveness pass-1 findings — TS build errors, lifecycle assertions, caller fixtures, runner config root (172baa87)
- **remote**: repair baseline server relay contract and replace logic (d82b35e4)
- **ui**: restore input queue contract and resolve baseline compiler closure (01c20797)

### Documentation

- **session-restore**: amend spec to v11.2 from implementation property tests (a685fcc5)
- **session-restore**: add host-owned session spec, plan, and P0 evidence (11356f34)
- **daemon**: the launchd helper is no longer unwired (de671596)
- **daemon**: the launchd helper is no longer unwired (c00c889e)
- **daemon**: the launchd helper is no longer unwired (b8546682)
- **pane-liveness**: dispose the remaining F1 holes by category (cbda33c9)
- **pane-liveness**: confirm the zero-split-tags finding is not a build artefact (238f7b3d)
- **pane-liveness**: write the F3 and F4 audits and the final report (f50d74a8)
- **pane-liveness**: record why the split-cancel capture gap cannot land alone (2bd2fc05)
- **qa**: close H-14, H-15 and H-17a with executed evidence (ee0b8ddd)
- **qa**: record the replace_relay contract decision (19a27450)
- **qa**: dispose F3 and F4 at the code level (d2769d44)
- **qa**: commit the pane-liveness completion evidence bundle (3f2103d2)

### Maintenance & Internal

- **release**: bump app version to 2026.1009.2 (1d0eb43a)
- align merged main-tree tests with main's remote and desktop-inventory contracts (68345d38)
- **ci**: make skipped remote fixtures independent of host networking (e72c274d)
- remove obsolete issue intake verification artifacts (e0ce406a)
- correct the precheck deadline comment and value (f2cb6c57)
- fix the integration failures the first full precheck run exposed (5411019f)
- add a dispatch input to settle the integration targets alone (b4d40ee9)
- keep a manual dispatch on the macOS precheck unless the legacy legs are asked for (5a060b3f)
- give the worktree IPC contract tests the daemon their commands now require (09797a8d)
- let the overlay gate tests see a CGNAT address on any local interface (da684de2)
- harden the overlay alias and the isolated SSH trust file (31902077)
- provision the overlay address on a non-loopback interface (76a38ad9)
- harden the overlay address seam and the live SSH supervision marker (ff1b3bdf)
- fix the host-dependent and drifted tests the macOS precheck exposed (0e557935)
- serialize Rust compilation on XenoCI (57e60db0)
- reduce XenoCI test compilation disk usage (5b077f80)
- make the pull-request precheck a single macOS job (f478520a)
- make the PR precheck matrix run on hosted runners (2670cc92)
- keep video production out of the public repository (3ca7044a)
- **release**: stage remote helper 2026.1009.1 and bump app to 2026.1009.1 (6584a4b3)
- **release**: bump app version to 2026.1008.2 (cc91ac90)
- **release**: bump app version to 2026.1008.1 (8ce5f89e)
- **pane-liveness**: give split-cancel the same capture the sibling scenarios have (9be5eb3f)
- **qa**: split the pane-binding test from the socket transports (3e8c36b3)
- **qa**: measure the split click's session delta and preserve the barrier hub (bcb76cfd)
- **qa**: bind split-concurrent PASS to the conflict wave really being rejected (89a363a0)
- **native**: drive the real deferred presentation path in the four bounds-IPC tests (e57685ec)
- **ipc**: supply the persisted seven-field binding to the two attach fixtures (11c3a46a)
- **daemon**: update the last two all-targets constructors for the widened wire types (ad0ffb5a)
- **daemon**: complete the remaining all-targets fixtures for the new wire fields (6c69715f)
- **daemon**: complete the integration fixtures for the new wire fields (d97233c1)
- **daemon**: reconcile the create stage before asserting the spawn write (b0ed4bef)
- **native**: script one acquisition for the presentation accessor test (17295029)
- **ui**: bind the transport listSessions fixture to the incarnation contract (7c0fed30)

### Other Changes

- carry the uncommitted main-tree work into history (51cd3e85)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.10.04.2...v2026.10.09.2

## v2026.10.04.2

### Features

- **remote**: drop files onto ssh and paired terminals and paste the remote path (09051bac)
- **onboarding**: sign in and link this computer as the first step (cc88ddc2)
- **clipboard**: upload images to detected manual SSH sessions (00d0d2c7)
- **paired-host**: direct QUIC path with relay fallback (8fb42f88)
- **onboarding**: move CLI install into its own setup step (44383427)
- **onboarding**: full five-step setup with features tour and inline agents (42c9be8d)
- **file-open**: backend for remote SSH fetch, OSC 8 hyperlinks, open-with and CLI open (34a7d1f0)
- **terminal**: move debug copy to pane hover toolbar (8bb936f4)
- **preview**: Open-with menu and Markdown export to PDF (a2473f8f)
- **terminal**: link hover hints, actions popover, OSC 8 and SSH file open (d7253c61)
- **onboarding**: redesign Welcome wizard and first-run surfaces (12908119)
- **tabs**: file tab menu, labels, icons, middle-click close, worktree reuse, reopen closed (f89caa1f)
- **file-open**: route terminal file clicks by kind, location suffix, worktree root, and per-type target (c5f95869)
- **preview**: header, change banner, syntax highlight, reading controls, CSV/notebook/image viewers (b0df2810)
- **file-preview**: resolve, change-detect, and sniff unknown files in the backend (fad51f4b)
- **tabs,agent-detect**: live agent/terminal tab titles and senpi idle/ask detection (f58b5c09)
- **onboarding**: replace macOS-only permissions dialog with cross-platform Welcome wizard (27f31f2c)
- **ui**: add Copy Debug Info to terminal tab menu (2ebf8121)
- **remote**: support self-hosted relay configuration and guide (29da7fbb)
- **account**: sign in from the magic link without re-entering the code (67ecb7fd)

### Bug Fixes

- **ssh**: upload the Windows helper script without a here-string (ad79b791)
- **ssh**: install Windows helper uploads through raw stdin command (f8cb8512)
- **relay**: carry release 2026.1003.1 remote server compile repairs to main (2b7c9404)
- **ui**: carry release 2026.1003.1 frontend compiler repairs to main (0441f8e2)
- **ui**: quiet automation-turn completions so unattended agent runs stop re-arming attention (66e3567b)
- **terminal**: preserve physical rows when resolving file links (50c5ca99)
- **release**: keep DMG app bundle name stable (b41d0703)
- **terminal**: surface daemon-side suspension so restarted GUIs can resume (83bb0a23)
- **remote**: revoke account logout and restore saved worktree context (9d70fee8)
- **dag**: restore SSH and paired remote graph ownership (320514f8)
- **ssh**: preserve delayed spawn completion in owning workspace (016936d4)
- **native-terminal**: bound set_bounds wait for frame presentation (74d560e2)
- **daemon**: route local control preflight through per-session slot (ce0bcce8)
- **remote**: preserve account sessions through transient failures (af953a62)
- **ssh**: keep the durable helper out of login sessions and reap leaked ones (c4d96f6c)
- **terminal**: bound native reconnect attachment (e6253804)
- **terminal**: bound stalled daemon attachment (7e74f598)
- **remote**: enter an empty shell with account worktree picker (e5402630)
- **terminal**: retry failed stream listener registration (b2e3ea0e)
- **terminal**: retain stream disconnects during recovery (2cde3baf)
- **ssh**: retain expired target state after failed close (d9c96ebd)
- **terminal**: reattach native output streams after disconnect (a1997d3b)
- **ssh**: recover imported transports after connection loss (b3089af7)
- **reconnect**: propagate agent recovery failures to panes (7709da32)
- **reconnect**: preserve adopted sessions on recovery failure (cf22d7fa)
- **ssh**: preserve concurrently committed manual transport module (6c561865)
- **ssh**: own transport lifetimes and retain helper agent state (2714c8d8)
- **file-preview**: preserve source session and worktree ownership (2c8e4fe5)
- **ssh**: honor remote platform when probing worktree shells (75e3b32a)
- **native-terminal**: verify DirectWrite fallback coverage via cmap, not GDI (b0965d3d)
- **inbox**: acknowledge notifications on explicit pane interaction (38276c9c)
- **workspace**: deduplicate concurrent worktree tab creation (912a7feb)
- **paired**: accept additive host capabilities (ef736d94)
- **paired**: recover stale project IDs from host inventory (36881c72)
- **relay**: exempt issued data sessions from proxy IP admission limits (b3b1870e)
- **browser-cli**: portable stale-port and missing-webview test paths (f1b8a2ae)
- **native-terminal**: locale-independent Windows font fallback for GDI raster (7e50f837)
- **ui**: reserve toast offset for clear-all row so it never covers popover actions (d85e3caf)
- **file-link**: treat root-relative paths as absolute and join with native separators on Windows (b4aa3448)
- **terminal**: close adopted sessions by the shell's own process group (40fa275e)
- **ssh**: keep registered folder separate from parent worktree (353d5e0c)
- **terminal**: isolate local input connections by session (be54a3fe)
- **remote**: pin the served UI directory with FERRYX_UI_DIST_DIR (7da27ec7)
- **relay**: apply CORS after the account and grant routers are merged (e7823567)
- **terminal**: hand wheel events to DOM overlays above native surfaces (7567ff80)
- **layout**: equalize adjacent split panes on divider double-click (a4e85816)
- **terminal**: hide native panes on inactive tabs (686e8239)
- **updater**: relaunch after update toast installation (4ffde6bb)

### Maintenance & Internal

- **ssh**: cover both Windows executors in the stdin-upload regression (a2d78bda)
- **perf**: add automated terminal throughput and framing regression suites (9c07425a)
- **remote**: remove uncommitted session helper dependencies (8bdcff97)
- **terminal**: verify bounded stream recovery and manual retry (83300a49)
- **ssh**: synchronize reader ownership and handle ConPTY queries (4324ef2a)
- **ssh**: resolve Windows helper executable in bridge fixtures (ea146cec)
- **native-terminal**: keep committed pane tests consistent with committed code (d08b62ad)
- **browser-cli**: pin missing-token failure to the token read (ENOENT) (2ac52b92)
- **native-terminal**: pin Mac host for Cmd-modifier pane contracts (928b6c22)
- **terminal**: make mouse-tracking link-action gate assertion deterministic (ed71d834)
- **file-open**: QA harness scenarios for link actions and open-with (b4026935)
- **file-open**: browser QA harness for the terminal file-open path (965e933f)
- **release**: bump app version to 2026.928.7 (f2e9124a)
- **release**: bump app version to 2026.928.6 (6a69a9ce)
- **release**: bump app version to 2026.928.5 (98b85f46)
- **release**: bump app version to 2026.928.3 (07f7ed08)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.28.1...v2026.10.04.2

## v2026.09.28.1

### Features

- **remote**: polish mobile chat view with token design system and a11y (1adcc3ad)
- **remote**: render mobile chat transcripts like T3 Code (4a01fe5d)
- **attention**: keep worktrees as the sidebar default and open the inbox from its icon (3fe22421)
- **browser**: bring the in-app browser to cmux/Orca parity (cc44c5ee)

### Bug Fixes

- **sidebar**: explain an empty worktree list instead of showing nothing (a2e50d18)
- **ui**: return to previous tab after closing file or browser (fa8f8c55)
- **remote**: block mobile chat send while attachments are attached (b0c56dcf)
- **remote**: restore the desktop PTY size when a resizing remote socket closes (07cb57a2)
- **remote**: keep the mobile chat terminal drawer from resizing the desktop PTY (98d739d6)
- **daemon**: credit restorable SSH sessions only for a predecessor that counted them (958e5418)
- **daemon**: successor credits SSH sessions restorable from the durable snapshot (cdc3c952)
- **daemon**: hand over only local PTYs; keep SSH sessions via the durable snapshot (a45a3622)
- **daemon**: raise the descriptor limit so sessions stop dying at ~25 terminals (6bb87469)
- **release**: refuse a local install that does not advance the app version (fbf29ea9)
- **daemon**: preserve adopted sessions across a v5 handover and enable it by default (f26941a6)
- **dev**: prepare bundled remote assets (5d144ce0)
- **qa**: stage Windows edge probe driver (70f50b40)
- **windows**: keep the main window opaque (96660a74)

### Documentation

- **windows**: checkpoint transparency audit (8d1190ef)

### Maintenance & Internal

- **release**: bump app version to 2026.927.4 (648979a2)
- **archive**: preserve detached release worktree changes (6a5c3355)
- **browser**: import afterEach in the devtools toolbar test after the merge (7b1876e7)
- **terminal**: a paired pane keeps its reason on screen over a slow tunnel (1ab7c7bd)
- **ui**: trace worktree refresh failure stage (e2ed6c5f)
- **attic**: park the unwired chat/design/push/ssh skeletons (90e06ecf)
- **hardening**: track Cargo.lock and serialize boot-trace rejections (5f0fe80f)
- **windows**: preserve platform opacity split (3b88a43e)

### Other Changes

- remove manual pane/tab suspend controls (16ed929e)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.27.3...v2026.09.28.1

## v2026.09.27.3

### Features

- **worktrees**: let an empty worktree list say so (2e5c059a)
- **attention**: make the sidebar a two-state inbox for finished work and questions (dab628bb)
- **remote**: render agent conversations in the mobile chat (79a0486e)
- **remote**: serve agent conversation history to the mobile chat (52657db9)
- **settings**: let the desktop redeem an account machine grant (334dc571)
- **attention**: fold the inbox into the notification center and add a range-scoped ask (b8663b22)
- **agents**: deliver design feedback to remote sessions (63f2c79f)
- **agents**: browser design feedback, cross-project attention inbox, provider history (94fbdbcc)
- **settings**: sign in to the account and issue enrollment codes; retire the PIN surface (65faadcc)
- **account**: deliver grants to the machine, unify the attach gate, add browser Noise attach (ddc66b8e)
- **ui**: equalize split panes on a divider double-click (7a822165)
- **relay**: embed account service into ferryx-relay for single-binary deployment (8be60e79)
- **site**: publish demand-backed blog series with per-post FAQ SEO and JSON-LD (82291532)
- **account**: add direct headless CLI login via Device Flow and Magic Link (7f90ea21)
- **account**: let a signed-in phone type into existing terminals (54a1a727)
- **relay**: configure account public key pin via env and CLI flag (1027315b)
- **account**: serve the grant gate from the relay router (5cd3d2ba)
- **account**: gate relay grant submissions with the account's signature (1c7978f0)
- **account**: bind the account attach router into the gateway (3b21659d)
- **account**: serve the attach handshake directly on LAN and Tailscale (8e5273c6)
- **account**: let the desktop ask its daemon for an attach session (f85de48f)
- **account**: let the daemon hand a client an attach session (687d2f1e)
- **account**: let the inventory link a saved SSH host explicitly (99d2c09d)
- **account**: pick an attach path by measured round trip (78f932e1)
- **account**: give the desktop a relay attach entry point (98331617)
- **account**: let the daemon mint an attach session for a client (9836f74f)
- **account**: give clients one entry point into the attach handshake (1ef76ee8)
- **account**: splice opaque sessions on the relay without reading them (9ed3587d)
- **account**: deliver sealed grants over the daemon control socket (6624e8f3)
- **account**: let a caller ask the relay for an opaque session (024587ac)
- **account**: switch the daemon onto the attached session path (9530a09c)
- **account**: proxy an attached session to the gateway (d14923d9)
- **account**: bind a device's attach key to the grant it redeems (79867dcd)
- **account**: let a relay session carry an attached stream (50f2e5d8)
- **account**: deliver sealed grant offers over the control channel (84053ac6)
- **account**: apply an offer that arrived for this machine (1d935a0f)
- **account**: hand the sealed grant offer to the daemon (03ee5e43)
- **account**: let a daemon open a sealed grant offer (5960ef5d)
- **account**: seal pairing capabilities to the daemon attach key (12f6b4f1)
- **account**: point the add-machine surface at account enrollment (f690f0a5)
- **account**: enroll machines and retire PIN issuance (43bd0efd)
- **account**: add account service foundations for grant-based remote access (a3c6fa91)
- **browser**: add element picker, DevTools, and link click targets (a8a9d9b5)
- **preview**: open read-only files as workspace tabs (885ba725)
- **ui**: clear-all pill for sonner toasts (11e76eef)
- **daemon**: implement v5 rolling handover session ownership transfer (1aa37f01)

### Bug Fixes

- **windows**: gate unix-socket tests and set scale_factor so tests compile (b22a3310)
- **native-terminal**: keep Cmd line-editing chords on macOS only (308745bb)
- **windows**: fix the GlobalFree import and a Windows-only type inference error (321158ba)
- **settings**: stop sending users to fetch a retired machine PIN (a7a88f85)
- **tabs**: suspend or restart the pane you are in, not the tab's first pane (4b7045f8)
- **worktrees**: tell users how to remove a worktree Ferryx did not create (d5693c35)
- **sidebar**: say when a project's worktrees could not be listed (71909433)
- **sidebar**: make a stale worktree row explain itself instead of going dead (3565a234)
- **remote**: name phone sessions after their desktop tab, and page with before (8da2b496)
- **settings**: stop swallowing failures of actions the user just clicked (a32affd7)
- **native-terminal**: keep scrollback when an overflow replay must be rebuilt (ea77e131)
- **remote**: never show another session's chat, and show the tools an agent ran (f482dfb2)
- **startup**: tell the user when a stored project fails to register (fc004180)
- **daemon**: never request a handover that would lose sessions (d9cb5a89)
- **terminal**: stop calling a restored standby shell "Shell exited" (afbacefb)
- **terminal**: keep a wedged PTY from freezing input in every session (29422576)
- **handover**: stop the predecessor from killing sessions it just exported (766acd4e)
- **handover**: abort when the predecessor could not export a session (f948630f)
- **sessions**: bound persisted scrollback so saving state stops flooding the disk (08a45fec)
- **terminal**: keep a session readable when its handover export fails (e1dc14a3)
- **daemon**: name the sessions a handover drops instead of skipping them (9c611fcc)
- **daemon**: abort a handover that would strand sessions instead of committing it (ec1ff4db)
- **ux**: stop silently swallowing dropped input and unseen completions (8b64db3f)
- **daemon**: pass the handshake token at the call sites c58be31f missed (38a1a3bc)
- **ux**: close the P0 gaps the journey audit found (71e9035d)
- **tests**: repair the targets that kept cargo check and vitest red (ea93b8a2)
- **ui**: declare the crypto and bun type dependencies the tree imports (52481e07)
- **ui**: export updatesManagedExternally and skip polling when managed externally (fbf21b69)
- **daemon**: send the loopback transport token in the handshake (c58be31f)
- **remote**: request a machine-scope grant from the account Connect path (b65d1bb3)
- **remote**: admit account-enrolled machines to the relay control tunnel (4be496f6)
- **daemon**: write every daemon record, and the handover counts, to disk (d38b6200)
- **attention**: open the notification popover on the side with room (6066fd29)
- **terminal**: keep history for top-anchored scroll regions (2726e22d)
- **remote**: re-render the account terminal when its daemonEpoch resolves (74b75bbe)
- **remote**: require daemonEpoch on the account terminal path (5ee1b6ae)
- **daemon**: preserve adopted sessions across a v5 handover and enable it by default (7d190112)
- **install**: accept the CLI's exit-1 usage banner on Windows and publish the installer (ef007b82)
- **remote**: drop the legacy PIN path from the committed client (fa0b5973)
- **remote**: add the mobile chat modules the committed client imports (acd754e8)
- **relay**: make control/grant channel registration atomic (4e2c01fd)
- **daemon**: raise the inherited descriptor limit so sessions survive past ~25 terminals (246e0535)
- **account**: remediate P0 review findings and close the F3 evidence gap (af5d37e9)
- **remote**: resolve the account API origin so magic links work from the app-served client (c384ead1)
- **remote**: make the account login and terminal attach reachable from the UI (be643a59)
- **attach**: make the granted-capability attach gate real in the tree (72c899b3)
- **cross-platform**: repair non-macOS paths the 2026-09-24 audit found silently dead (6df7f4c4)
- **remote**: deliver browser SSH terminal input as generation-fenced remoteWrite (b78ee328)
- **remote**: apply browser SSH terminal geometry through the SSH generation (99062ab0)
- **account**: remove fake bypasses, enforce strict email magic link token verification and real mailers (0ef8aecd)
- **site**: simplify blog body diagrams to glanceable single-idea images (188e0901)
- **account**: address P0 review findings for proxy safety, frame limits, and identity binding (5ea6215e)
- **account**: load attach identity lazily on the direct attach path (9cc16012)
- **daemon**: stop wrapping truncated control descriptors (818be4ae)
- **terminal**: never treat a non-path cwd as a session directory (e2d945bf)
- **account**: keep the account origin's port when normalizing (9a2ea6b4)
- **account**: report expired login codes and prove the tests that were not (7e62ce40)
- **account**: stop the attach identity lock colliding with the store lock (26214711)
- **native-terminal**: render Kitty unicode placeholder images (6b969295)
- **notification**: request permission instead of toasting UNKNOWN (f3741c9c)
- **ui**: bounded transport retry for paired worktree listing (f0d959f3)
- **release**: fix live-executor probe spawn in install guard (d9bdca64)
- **release**: gate local installs behind notarization verification (bbfe41d3)
- **ssh**: admission-order FIFO input queue with per-session I/O isolation (48e06c93)
- **remote**: bare-UUID request ids, idempotent detach, retry budget reset (cd142e41)
- **ui**: register stored local projects before restoring workspace tabs (d58ba25b)
- **daemon**: widen readiness budget for first-launch signature validation (6e59f6fb)
- **build**: align ghostty pin with committed submodule revision (cd2e49ca)
- **terminal**: snap to bottom on keystroke and guard partial-region scrollback (a42a3912)
- **daemon**: keep remote sessions attachable across handover and reboot (8f35204c)
- **lifecycle**: auto-suspend off switch and PTY output ground-truth veto (f0d0d322)
- **terminal**: preserve scrollback and reconstruct clean state on image-heavy sessions (796f8abd)
- **native-terminal**: keep stale GPU frames from cancelling reattached renders (eda68899)

### Performance

- **terminal**: stop making each keystroke wait for the previous one's receipt (3a870228)

### Documentation

- **plan**: write the licensing, trial and self-host plan for the paid model (4996cee2)
- **research**: attention-surface research and the design it implies (6021fd10)
- **plan**: name the concrete blockers for wiring the control plane (c6bd16a9)
- **cross-platform**: record the commit SHAs in the audit report and its evidence notepad (b687ffa4)
- **cross-platform**: publish the 2026-09-24 audit report with its review and execution evidence (a77afa0d)

### Maintenance & Internal

- **remote**: page a 3,200-message transcript back to its first message (27ca1891)
- **handover**: cover the UpgradeBinary path an app update takes (ca678018)
- **release**: bump to 2026.927.1, the first version whose handover keeps exported sessions (c5e763fe)
- **native-terminal**: pin that a keystroke no longer waits for a busy main thread (11906f84)
- **release**: bump to 2026.926.2 for the T2 typing-latency QA build (7cea18b5)
- **terminal**: cover the dropped-input notice added in 8b64db3f (3df93b08)
- **notifications**: seed a real interaction where focus used to be enough (6538898b)
- **release**: bump the app version to 2026.926.1 (b3b9d8f5)
- **release**: bump the app version to 2026.925.9 (3a0dba12)
- **attic**: park the unwired skeletons and settle the remaining scope decisions (c268cf2f)
- **settings**: guard the retired QR/PIN control against reintroduction (268977c4)
- **release**: bump the app version to 2026.925.3 (6e852e3d)
- **remote**: prove browser SSH grid resize reaches the remote PTY over real SSH (21aad437)
- **ui**: exclude tests from production tsconfig build (3218e2af)
- **account**: integration suites for sentinel secrecy and saved ssh daemon forwarding (06ed17b0)
- **account**: verify path selection picks fastest authenticated candidate (db575b87)
- **daemon**: port a03 owner CLI fixture to retired PIN exit 2 and account grant token (69de340e)
- **ipc**: serialize the tests that share the pending-work registries (72d7523c)
- **ipc**: answer unparsable daemon requests instead of dropping them (1fa4d434)
- **account**: prove the relay carries an account attach only as ciphertext (8673f7be)
- **account**: prove a live PTY survives enrollment (2c5a4cb1)
- **account**: prove enrollment leaves existing device grants alone (b7e62668)
- **account**: align store and identity tests with the verified behaviour (26c47e54)
- **hardening**: track Cargo.lock and serialize boot-trace rejections (430c05e8)
- **release**: bump version to 2026.922.1 (ebc0e61f)

### Other Changes

- **native-terminal**: rustfmt the T9 encoder-path test (d233b709)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.8...v2026.09.27.3

## v2026.09.20.8

### Bug Fixes

- **release**: keep Windows signing secrets out of argv (9c38adbc)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.7...v2026.09.20.8

## v2026.09.20.7

### Bug Fixes

- **release**: pass updater signing keys to remote builds (a1c1fd29)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.6...v2026.09.20.7

## v2026.09.20.6

### Bug Fixes

- **release**: preflight Linux Tauri CLI (04638e47)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.5...v2026.09.20.6

## v2026.09.20.5

### Bug Fixes

- **release**: keep remote artifact transfers alive (d21427b2)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.4...v2026.09.20.5

## v2026.09.20.4

### Bug Fixes

- **release**: rebuild notarized macOS DMG from signed app (b5cbd7d7)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.3...v2026.09.20.4

## v2026.09.20.3

### Bug Fixes

- **native-terminal**: complete deferred surface presentation (5d4fcdc5)
- **release**: extend ssh keepalive intervals during long release builds (1fdd7134)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.2...v2026.09.20.3

## v2026.09.20.2

### Bug Fixes

- **release**: support darwin helpers in msix and sign embedded Mach-O binaries (fc0580da)

### Performance

- **terminal**: expose HistoryRange publicly to satisfy interface visibility (b444a388)
- **daemon**: use zero-copy Bytes slices for history segments and attach response (833f3757)
- **terminal**: split raw broadcast capacity and add lifecycle watch signal (b307a0cd)
- **daemon**: deduplicate attach snapshot materialization into single flat buffer with HistoryRange segments (e24e9570)
- **terminal**: emit replay-gap through binary output frame v2 with JSON fallback (2c91054e)
- **terminal**: bound raw-broadcast backlog and skip copy without raw receivers (93245e2f)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.20.1...v2026.09.20.2

## v2026.09.20.1

### Features

- **core**: terminal, browser, and workspace service enhancements (78bbd381)
- **remote-helper**: multi-platform helper binaries, assets, and SSH survival QA scripts (48cccb11)
- **native-terminal**: implement lock-free SnapshotSlot and resolve all four gate review blockers (4917bb95)
- **site**: canonical HTTPS redirects, accurate comparisons, and QA traffic filtering (666a46d0)
- **site**: add consent-gated GA4 download attribution (40485666)
- **site**: improve search metadata and sourced product facts (7ab8ee48)
- **native-terminal**: bounded reclaim for a value lent to another thread (341c61db)
- **native-terminal**: add compiler-enforced thread-ownership tokens (0280264a)
- **remote-browser**: implement remote browser screencast and drive subsystem (Phase 1-7B) (33352135)
- **file-preview**: wire terminal link routing, native pane source, and CSP (34a748ce)
- **file-preview**: implement text, markdown, image, and video preview renderers (89ee77fe)
- **file-preview**: implement frontend preview controller and base modal dialog (56b3583c)
- **file-preview**: implement rust backend capability service and tests (dbc4fc79)
- **preview**: add shared file preview contracts (728dead5)

### Bug Fixes

- **cross-platform**: Windows MSVC compilation and dynamic SSH path resolution (7b12e2ec)
- **daemon**: rolling handover session ownership transfer and instance lock deadline clamp (bb061a51)
- **dag**: remote SSH/paired stream, generation filter, and badge lifecycle (da6ab5f3)
- **native-terminal**: add non-macos fallback for platform compositor target surface creation (47bcfa48)
- **remote**: assemble Jamo sequences, update service worker cache, and stamp build (d1bae845)
- **remote**: compose Korean input from composition-free IMEs (468dd3ef)
- **shortcuts**: exclude tabless inactive worktrees from shortcut navigation (99ac78c9)
- **native-terminal**: omit an unexpandable image instead of blanking the pane (63c5255b)
- **native-terminal**: suspend an occluded surface instead of retrying it (9f3c7d36)
- **native-terminal**: pace dropped-frame retries instead of re-dispatching inline (b0d0c926)
- **ssh-terminal**: deliver native pane resize to remote PTY (15e5c543)
- **paired-terminal**: recover unavailable paired panes and enable paired shell replacement (44cef470)
- **native-terminal**: stop remote attach from painting the replay scroll (1cf00e44)
- **terminal**: support local clipboard image paste via safe temp file storage (2c574d2c)
- **terminal**: prevent false-positive agent detection and recover unresumable sessions (ea3cce67)
- **updater**: allow re-checking while an update is already offered (038e216f)
- **remote-browser**: address round-6 review blockers and findings (R6-1..R6-14) (dca5061f)
- **remote-browser**: address round 5 review findings (R5-1 through R5-16) (f9472808)
- **remote-browser**: enforce R4-7 identity, execution and publication fencing (87720425)
- **remote-browser**: address round 4 review findings (R4-1 through R4-17) (9a7654f3)
- **remote-browser**: address round 3 review findings across full stack (5c604857)
- **remote-browser**: resolve all round 2 review findings (R1 through R10) (552aca9e)
- **remote-browser**: address code review defects (P1-01 through P2-04) (3cfff00e)
- **file-preview**: address senior-engineer review findings R1-R14 across backend and frontend (a1f7dd88)

### Performance

- **native-terminal**: stop re-entering AppKit on every macOS present (b7425ada)
- **native-terminal**: pace frames on a FrameClock measured from frame start (c07bba1e)
- **native-terminal**: latch Wayland subsurface geometry too (44c69e23)
- **native-terminal**: stop re-applying an unchanged AppKit child frame (2643e63b)
- **native-terminal**: apply the geometry latch to the X11 child window (8d10c576)
- **native-terminal**: stop relayouting the child surface every frame (dfb69bb7)

### Refactoring

- **native-terminal**: split the GPU leg off the child view (dc8b740e)
- **native-terminal**: hoist update_viewport to the render callers (4877939e)
- **native-terminal**: lift presentation finish out of render_snapshot (dfef209c)

### Documentation

- add remote DAG viewer and daemon rolling handover specifications (9e9d5f9f)
- **native-terminal**: the real Phase 3 blocker is drop order (1ce851e5)
- **native-terminal**: correct 12.1 - the serializing lock is hosts (357fb9c1)
- **native-terminal**: record the gate rule that locks C1 (1a8febcf)
- **native-terminal**: the last AppKit call needs all three callers moved (2df13b81)
- **native-terminal**: pin the split line at present(frame) (b59250f7)
- **native-terminal**: blocker C is one call, not a signature overhaul (df10e575)
- **native-terminal**: blocker C - render_snapshot takes a tauri::Window (d1db0ca7)
- **native-terminal**: record the borrow workaround that swallows input (71f3c918)
- **native-terminal**: mark which root causes are actually resolved (e6f34c06)
- **native-terminal**: explain why the freeze is intermittent (1cbe9be3)
- **plan**: track the plan and reconcile it with what actually happened (59ead7ce)
- **native-terminal**: specify the two blockers Phase 3 actually hits (07296c3d)
- **native-terminal**: warn that the geometry latch does not make the frame movable (a5206498)
- **native-terminal**: record what Phase 3 actually requires (15b024b6)
- **native-terminal**: correct the design's phase order and verification plan (f90ffbb5)
- **file-preview**: complete final verification wave (F1-F4) audits (240da668)

### Maintenance & Internal

- **native-terminal**: reject GPU submission and presentation on UI dispatch (d8835a35)
- **native-terminal**: run the child-surface and Wayland suites on Linux (8ccb2606)
- **native-terminal**: actually run the thread-policy gate (518b219e)
- **native-terminal**: prove suspension is not a one-way door (d44ebbc3)
- **native-terminal**: pin frame pacing in time, not just as a state machine (9561bd56)
- **native-terminal**: look for the POC example where cargo actually emits it (b8c73eca)
- **native-terminal**: close the thread-policy gate's blind spot (c2c516d0)
- **native-terminal**: pin that an idle surface issues zero render passes (a927095c)
- **native-terminal**: forbid GPU acquisition inside run_on_main_thread (87eabebd)
- **preview**: import uncommitted terminal file-link prerequisite (d0aee651)

### Other Changes

- **native-terminal**: record that R4 actually started (8d50a644)
- **native-terminal**: pin the exact R4 resume boundary (42572124)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.18.1...v2026.09.20.1

## v2026.09.18.1

### Features

- **terminal**: support clipboard image paste for all remote workspace types (db4154ab)
- **dag**: render full OMO DAG information and deliverable inspector (4a439b83)
- **release**: automate changelog generation with GitHub PR notes and conventional commits (3fa84f8c)
- **release**: unify persistent cargo-target across all builders for automatic incremental builds (064a30b5)

### Bug Fixes

- **paired-host**: remediate round-5 audit findings and gate fixes (216272f2)
- **daemon**: eliminate memory leak and socket accumulation on idle attach disconnects (fca2adc0)
- **ipc**: reconcile ambiguous create, persist pending cleanups immediately, check NoContent close outcome (P10, P11, P12) (20701450)
- **paired**: remediate round-3 findings (P10, P11, P12, N2, R3-N1, R3-N3, R3-N4) (0211f6ca)

### Maintenance & Internal

- **release**: bump version to v2026.09.18.1 (6a59a922)
- **release**: allow FERRYX_BUILD_TIMEOUT_MS override for platform builds (cfb6b0bc)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.17.1...v2026.09.18.1

## v2026.09.17.1

### Features

- **ssh**: provision remote helper with version-aware install/upgrade (1f369e5f)
- **browser**: wire phase-3 eval/wait/console/focus/screenshot/cookies/storage + FERRYX_WORKSPACE_ID export (722fdf5c)
- **ui**: adopt CLI-created browser tabs via browser_session_created (fa13a86c)
- **terminal**: export FERRYX_WORKTREE_PATH and FERRYX_SESSION_ID into worktree PTYs (7902120d)
- **browser**: CLI socket open/navigate/close/identify with URL scheme enforcement (60f0daec)
- **session**: native process suspend lifecycle, concurrency expansion, and remote stabilization (13eaf3fc)
- **sidebar**: swap sidebar toggle and add-project buttons and bind project.add shortcut (f86736eb)
- **session**: implement lazy session resume, auto-hibernation, and lifecycle management (5ffaf441)
- **terminal**: auto-restart sessions and batch auto-resume agents on daemon restart (2baec6a0)
- **remote**: unify machine settings and add SSH password auth (47ab352b)
- **settings**: redesign remote access and ssh into unified remote section (41549748)
- **worktree**: enable deletion and removal for remote SSH worktrees (7450a9a0)
- **watchdog**: fix reset event ordering, add bounded headless daemon logging and outcome handling (87ea4907)
- **worktree-disk**: complete scan lifecycle, deletion preview guards, and prunable cleanup repairs (6bb783fd)
- **paired-host**: multi-host paired daemon + frozen-backend rehearsals (session 01a097f8) (7f8a1690)
- **worktree**: auto-detect external worktree changes with a 30s fingerprint-gated rescan (a3c4d38b)
- **agents**: release stale agent state on process evidence (4382c7c3)
- **worktree**: add portable disk scanner and worktree management dialog (c569f6b8)
- **dag**: add drag panning and anchored viewport zoom (010179ff)
- **site**: serve ferryx.dev from a Cloudflare Worker (02e62e49)
- **site**: publish to Cloudflare Pages on ferryx.dev (c28ba086)
- **site**: add opt-in analytics and document the domain move (4e32a625)
- **site**: publish comparison and use-case pages (13a26c9a)
- **site**: close the landing page and docs SEO gaps (eb3ec503)
- **daemon**: persist project catalog across isolated restarts (e1a00339)
- **notifications**: record inbox entries before focus gate with navigation seen-bridge (33933eac)
- **notifications**: sidebar bell with notification-center popover (ff0e5f36)
- **notifications**: forward workspace label into desktop notification payloads (8cdac4f9)
- **notifications**: renderer-owned notification-center inbox store (7c713f66)
- **notifications**: carry project and worktree context in desktop notification body (43707cff)
- **remote**: add owner-issued machine pairing grants (0ce84a11)
- **remote**: define paired daemon wire and persistence contracts (cd16c90e)
- **remote**: add multi-host relay pairing and fix startup workspace registration (d7e5a02a)

### Bug Fixes

- **remote**: implement get_interface_name_for_ipv4 fallback for non-unix targets (537d3dd6)
- **browser**: import parking_lot::Mutex unconditionally across platforms (5736b02b)
- **paired**: remediate round-2 findings (N1, N2, R2-N1, P10, P11, P12, P14, P19, N5) (82752f16)
- **ssh**: harden helper upgrade path per type-safe review (3095c1a5)
- **ui**: satisfy typecheck in paired-host test fixtures (55fe1851)
- **ui**: unblock dev typecheck with lib ES2021 and AddMachineModal onSuccess props (e67e9183)
- **ipc**: paired attach routes through descriptor validation and proxy reinstall with after_sequence (P13) (ce7615a3)
- **terminal**: descriptor store lock-order safety and atomic durable saves (N1, N2) (ef0f87ad)
- **ipc**: background-create reconciler, production cleanup reaper with dead-letter retention (P10, P11) (4fa8993f)
- **daemon**: cover all paired mutation phases in outer budgets and preserve remote-close outcomes (P09, P12) (21cdb883)
- **remote**: stale worktree rows honored in UI, structured listing errors, gate status surfaced, overlay proof required (P17, N4, N5, P19) (3c00ef02)
- **paired-host**: preserve structured pairing errors, emit real inventory events, fail closed on unreadable revocation journal (P05, P14, N3) (7648264f)
- **ui**: sync pane focus store on native terminal focus events (fixes typing focus jump) (3636fdd0)
- **remote**: gate insecure direct LAN gateway and enforce encrypted relay URL policy (P19, P20) (fa252016)
- **ssh**: reject UNC cwd spawn with explicit cmd.exe incompatibility error (489c63be)
- **ui**: adopt live agent session owner on AGENT_SESSION_CONFLICT instead of failing reopen (5ff85a43)
- **ui**: Add Machine pairing supports custom relay origins and invite-link parsing (P04) (29a70bc3)
- **remote**: workspace-scoped worktree listing that honors the requested workspaceId (P15) (619c9df8)
- **ui**: resolve pre-existing UI test suite failures (baseline remediation) (18962db0)
- **terminal**: structured daemon wire errors, typed IPC error mapping, durable paired descriptors, and app-wide inventory events (P05, P07, P08, P13, P14) (f6764b7a)
- **remote**: await snapshot rebuild on refresh-interval expiry instead of serving stale data (P16) (b97e2727)
- **paired**: surface relay socket-ticket failures as typed errors, gate Authorization fallback to legacy direct (P06) (1cf9de6f)
- **ui**: mark stale/offline paired worktree rows with bounded TTL invalidation (P17) (5a731be1)
- **ssh**: report terminal helper readiness via bounded non-installing probe (7eeb7179)
- **ssh**: normalize Windows verbatim cwd before PTY spawn (93efacc0)
- **terminal**: hierarchical paired-host timeouts, journal reconciliation for ambiguous creates/cleanups, and detach/close separation (P09-P12) (72904a03)
- **remote**: roll back pairing claims on downstream failure so a valid PIN stays retryable (P01) (14399e2c)
- **remote**: durable pairing/revocation persistence with fail-closed journal (P02) and non-panicking auth transactions (P03) (6d5e3e1a)
- **remote**: keep remote terminal websockets open on transient focus None (P18) (889bd8bd)
- **ui**: copy error toast via shared clipboard util and make toast text selectable (22be8e76)
- **worktree**: resolve paired machine worktree path duplication and restore gating (e4b0e9e2)
- **remote**: expand paired host timeouts, cache worktree registration, and fall back to base workspace (566f6c06)
- **session**: resume active worktree sessions and wake standby split panes on restore (2489b894)
- **daemon**: reconnect with fresh stream on legacy handshake ProtocolMismatch (06b719ad)
- **daemon**: compatibility handshake and cross-version rolling handover (52610931)
- **worktree**: instant worktree selection and paired external worktree CWD resolution (34c79673)
- **shortcuts**: preserve dispatch with browser child webviews (c7e4414f)
- **terminal**: select whole URLs on double click (2dc5b372)
- **agent-state**: prevent stale running status after agent completion (637af677)
- **notification**: sync agent highlights with notification center and dismiss entries on read (a9502b4a)
- **terminal**: suppress close confirmation on single pane close and confirm active tab close only (6eb1a145)
- **review**: resolve 26 defects from repo-wide mass-ulw code review (dc43a515)
- **notification**: isolate system settings opener in tests with mockable launcher (4b32d1e2)
- **review-findings**: resolve P2 logging panic, scan cancel event, TMPDIR test dependency, and P3 docs (a604a227)
- **windows**: resolve PRs #2/#3, normalize wheel scroll, and remediate Windows review defects (a0d76b20)
- **remote**: constrain mobile worktree lists and enable new terminal tabs (da6eec06)
- **remote**: stabilize mobile IME, keyboard focus, and touch scrolling (b279339f)
- **dag**: preserve modal focus and narrow viewport controls (3a826a15)
- **terminal**: route Cmd+W to the focused pane regardless of split root and confirm active agent exits (d6ebb720)
- **terminal**: render native Kitty images and enable client detection (bd2ac7c7)
- **dag**: release viewport captures and verify gesture delivery (165ce821)
- **terminal**: retain exited surface ownership through effect handoff (40c2ae5c)
- **windows**: restore new terminal shell choices (1313d387)
- **terminal**: ensure generation-bound VT query delivery and safe SSH split reconnect (8dee7072)
- **dag**: resolve camera verifier blockers B1–B7 (915d5801)
- **site**: clip the preview backdrop so its top edge stays washed (b3198aea)
- **site**: stop routing every request through a Worker (a7dc9ed8)
- **notifications**: isolate keyboard row activation and clamp popover to viewport (fa429aac)
- **site**: close the findings from the SEO gate review (99e0450d)
- **remote**: harden remote terminal reconnection and split failure handling (7f9babdf)
- **terminal**: wire VT report writes and reconnect SSH sessions (b5db47b8)
- **linux**: resolve browser child webview displacement, terminal input recovery, and sidebar alignment (2ebebfe9)
- **release**: pre-sign universal binaries with hardened runtime and sign dmg before notarization (9b5b9c61)
- **remote**: require Control permission for terminal resize (a2534ff4)
- **remote**: retain real Ghostty VT in headless builds (4de9d285)

### Performance

- **daemon**: serialize single-flight daemon spawning and remove cold-start latency (c97b3ab5)

### Refactoring

- **ui**: remove desktop host switcher from sidebar (3d5e55a3)
- **daemon**: share headless workspace and session authority (fe9011d5)

### Documentation

- **ssh**: record typesafe review verdict and helper upgrade hardening (e135a33d)
- **plans**: remote browser screencast implementation plan (Phase 0 contract, Jev-decided product choices) (f4cf7560)
- **ssh**: record helper auto-provisioning landing and v2026.917.1 bundle refresh (4542807c)
- **plan**: cmux browser parity scope + mass-ulw execution record (cc6d8a2f)
- **ui**: record error toast copy root cause analysis (99a47c66)
- **release**: add macOS app replacement report for 2026-09-16 (de62d1bc)
- **audit**: add remote paired-machine connection audit report (P01-P20) (55126a7c)
- add headless linux daemon (ferryx-cli) download and pairing guide (0b2abf88)
- **anchors**: refresh source line citations on merged integration tree (59c982d7)
- **headless-privacy**: accurate headless deployment guide, privacy declaration, and verified source anchors (af90e2a1)
- **dag**: update viewport navigation verification record (6475e474)
- **dag**: record interim viewport verification (2bc97bf0)
- add headless Linux deployment guide and source-anchored privacy page (f4f63395)
- root-cause Cmd+W tab-vs-pane close and the absent agent close confirmation (ab08b94f)
- verify cold Windows shell menu and identify installed bounds error (e06db23d)
- record Windows terminal verification and remaining gaps (7f7ecd8e)
- add top-10 competitive user sentiment research (2026-09-12) (b5c97127)
- link the guides and comparisons from the README (b1a81856)
- **dag**: plan viewport pan and zoom navigation (f4ab00e2)

### Maintenance & Internal

- **ssh**: stage fixed remote helper bundles v2026.917.1 (win-x64, linux-x64) (98ac2894)
- **ssh**: bump helper version to 2026.917.1 ahead of fixed-bundle restaging (010428f1)
- **integration**: repair compile drift in src-tauri/tests targets (f058e934)
- **scripts**: agent session claim probe scripts and adoption gate review report (bed7ee79)
- **paired**: relay ticket failure and transport classification coverage (P06) (88f46b67)
- **daemon**: align a03 owner CLI fixture with current handshake and capability contracts (583535be)
- **remote**: adapt remaining revoke_device call sites to Result return (P02/P03 follow-up) (8b5d505e)
- **terminal**: assert native pane retains input and focus during error toasts (75dbc742)
- **release**: bump version to 2026.916.1 (34301bae)
- **license**: relicense from MIT to SUL-1.0 (48ee5a9c)
- **q4**: headless skip guard + final Linux frozen-backend rerun green (6eabe633)
- **regressions**: correct latency soak percentiles and record duplicate-daemon prospective evidence (c0ed82aa)
- **dag**: cover wheel modes with and without Ctrl (d1418d30)
- add latency soak harness, selection and duplicate-daemon regressions (bb945b4b)
- **windows**: document compositor regression and completed debug verification (b7ad4516)
- drop the remote deploy workflow in favour of manual deploys (bc6fa4d0)
- **dag**: harden viewport QA baseline observations (194333ad)
- **dag**: add real-host viewport navigation harness (ad56d26a)
- install site dependencies before the landing site tests (9f0fab74)
- **devtools**: mount notification center in activity QA harness (e31c36ee)
- **daemon**: isolate handover lifecycle and order owned cleanup (f9042c8d)
- **ui**: guard recursive jsdom top-layer selectors (a8c6d1ec)
- **daemon**: isolate persistence contract processes and state (921fa4e0)
- **remote**: add host-isolated paired daemon fixtures (9553dc6a)

### Other Changes

- Remove the sidebar host switcher restored by the merge (e697fb81)
- Prepare 2026.915.1 release: version bump and vendored portable-pty (de0b2588)
- Restore merged-away contracts found by the post-merge lib suite (aa924115)
- Preserve in-flight remote UI work for release build (0dc875d4)
- Preserve in-flight notification and browser IPC work for release build (379aa3b3)
- Scope machine session listing by workspace (4087778c)
- Achieve Herdr machine parity: remote worktrees, PTY, and PIN-only pairing (e3e412cf)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.11.1...v2026.09.17.1

## v2026.09.11.1

### Features

- **cli**: add standalone headless ferryx-cli binary and default relay URL to relay.checka.cc (e8b99fa4)
- **remote**: deduplicate device pairings via stable installation IDs, custom labels, and idle expiry (2c2058de)
- **remote**: bind the control signature to the relay that issued the challenge (eed3a9eb)
- **remote**: issue single-use socket tickets on the direct gateway (915f0259)
- **remote**: authenticate direct candidate probes with bearer token and extend test socket deadlines (a9209d83)
- **relay**: enforce device token authorization on tickets, support colon session targets, and harden proxy headers (8526deb4)
- **ssh**: implement Phase 1 helper asset inventory, target resolution, and bundling (d8e605fa)
- **ui**: implement host-scoped transport and persistent multi-host drawer (43ef6f99)
- **ui**: streamline zero-config settings UX with live countdown and QR capability (3178c9cd)
- **relay**: implement browser WebSocket adapter and ticket-based authentication (f83238ce)
- **relay**: implement reverse HTTP proxying for pairing exchange and APIs (f8b7df31)
- **remote**: implement daemon pairing coordinator with linearizable lifecycle (3aee9812)
- **relay**: implement Ed25519 machine authentication and multi-machine registry (57640815)
- **remote**: implement Ed25519 machine identity and zero-env startup (2c0eda8e)
- **remote**: define zero-config sync wire protocol and state models (a19352ba)
- **remote**: implement phase 5 deprecation cleanup and final verification (1f21e66a)
- **remote**: implement phase 4 ui and settings simplification (b3883d4f)
- **remote**: implement phase 3 client direct path upgrade and pna handling (536bbb39)
- **remote**: implement phase 2 native rust relay server and reverse tunnel client (d365956c)
- **remote**: implement phase 1 credential hierarchy and push endpoint authentication (1faaf70f)
- **remote**: add relay server configuration in backend gateway and settings UI (c3711238)
- **remote**: complete wave 4 worktree host-scoped sessions and design mode staging pipeline (ad75df47)
- **remote**: complete wave 3 multi-host UI and mobile push notifications (5bd93d21)
- **remote**: complete wave 2 3-tier discovery engine (tailscale, mdns, ssh tunnel) (82b199e4)
- **remote**: complete wave 1 core decoupling, headless pairing, and secure dual-binding (c7b5c27f)

### Bug Fixes

- **remote**: repair pairing identity persistence, relay forwarding, and web client fallback (0af6411b)
- **ssh**: improve Windows remote SSH process bridge and add disabled preedit stub (fed910e2)
- **ui**: provide safeRandomUUID polyfill for non-secure remote web contexts (95bd2a26)
- **remote**: cap device name length and filter idle devices from in-memory list (68bcfe55)
- **agent**: follow the conversation id an agent rotates with /new (a4db54ea)
- **remote**: stop accepting a device token from the URL query string (3c08b3a4)
- **remote**: mint tickets on the direct path so no URL carries a device token (aab4cc72)
- **remote**: mint a ticket for the events socket and record the direct-path gap (52621c08)
- **remote**: separate the transaction lock from the store write lock (6e704604)
- **remote**: make the private auth store durable and prove the path delegation (e1b11b49)
- **remote**: preserve pairing permission, surface daemon refusals, clear stale coordinator (5cf27265)
- **daemon**: resolve upgrade target to an executable that exists (db60ece3)
- **cli**: only defer pairing to a daemon that is already running (35e237ab)
- **relay**: close the claim-to-dispatch replacement race and route CLI pairing via the daemon (050e8a65)
- **remote**: serve daemon pairing from the relay-registered coordinator (fcd0f484)
- **relay**: gate pairing claims on the relay generation, not the client attempt (93dc0060)
- **remote**: resolve one machine identity across the GUI daemon and pair CLI (30555f24)
- **relay**: bind pairing exchange to the live control identity (cb1812a5)
- **relay**: bind stream halves to the control generation that authorized them (7ab68c2a)
- **relay**: make ownership persistence merge-safe and pairing registration idempotent (1da56842)
- **remote**: move device tokens out of request URLs into Authorization headers (6261a9f0)
- **remote**: close F09 credential leak and F04 cold-cache fail-open (c5fea8b1)
- **daemon,ssh,terminal**: deliver agent state across handover and remediate review findings (7404a44f)
- **relay**: permit health endpoint in shared-origin host proxy route allowlist (54b08011)
- **remote**: eliminate race in SSH reconnect socket test and disallow raw binary on SSH sessions (c5638810)
- **ui**: resolve F05 host-scoped terminal routing, 32-hex QR capability, and F09 verified direct-path upgrading (eba89353)
- **relay**: resolve F04 socket ticket auth, F05 grid query, F06 shared-origin isolation, and F07 PIN rate limiting (f4777fdb)
- **remote**: resolve F03 machine ownership durability and F08 pairing state machine lifecycle (8d37f24a)
- **remote**: resolve F01 zero-env relay client startup and F02 unified pairing contracts (7ad983e2)
- **ssh**: use Get-Item and AccessControl to prevent Get-Acl module autoload failure on Windows (4ba217b5)
- **ssh**: resolve Windows OpenSSH exit 1 normalization for missing helper (b7ce8154)
- **terminal,ssh**: remediate abnormal exit attach error and ssh detect timeout fallback (6a96a31e)
- **remote**: support cross-platform interface discovery and cross-process CLI pairing (9d10b029)
- **remote**: secure direct-path upgrade, fix QR parsing, and route host switching (370471b4)
- **remote**: harden relay server security, enable TLS, and wire daemon relay client (0cf6e0ac)
- **native-terminal**: suppress off-viewport cursor artifact and preserve bottom lock on warm reattach (47f29ba3)
- **ssh**: resolve duplicate host id routing and remote worktree ownership (5d2ab1bd)
- **remote**: decouple headless WS attach and unify remote pair subcommands (21e59761)
- **shortcuts**: restore cross-platform shortcuts and child webview focus handling (57abc83d)
- **ui**: render local worktrees immediately on project open and switch (75610100)
- **ssh**: apply lenient decoding to snake_case identity_file and jump_host legacy aliases (4d4bfdf8)
- **ssh**: leniently ignore non-string legacy fields in SshHost deserialization (c6b3d477)
- **ssh**: make ssh host deserialization resilient to legacy and missing fields (c01b2613)
- **activity**: time-box auto-resume suppression and notify on done<->waiting transitions (9f161ab5)
- **ssh**: allow repository root as remote terminal cwd (6d34210b)
- **release**: isolate bundle verification and resolve annotated tags in ls-remote (8601bd0f)

### Refactoring

- **remote**: drop the query extractors left unused by header-only auth (bcd35dff)

### Documentation

- **audit**: record the Gen4 remediation, probe inversion, and deploy scope (107c35d6)
- **macos**: add TCC permissions runbook (598310d5)
- **audit**: record the Gen3 conditions closed, deployed, and the vacuous-test disclosure (c96e6ff2)
- **audit**: verify the F01 identity path change strands no existing keypair (aed0dcaf)
- **audit**: record the F02/F03/F08 relay deployment (64ad2ee1)
- **audit**: bind the F10 E2E receipt to commit a50a465 (6fce7b2e)
- **audit**: verify the F04 fix live on the hardened relay (68bd4294)
- **audit**: record live reproduction of the F04 cold-cache fail-open (410d4d6a)
- **audit**: bind the E2E receipt to a verified deployed artifact (b7ef99bd)
- **audit**: scope-limit and redact the live E2E receipt (aa2dc0cb)
- **audit**: record Gen2 remediation verdict and reproduction sources (4ad46038)
- **audit**: update live E2E receipt with verified security response headers from relay.checka.cc (28032e33)
- **audit**: record ChatGPT Web worker final remediation verdict and reproducible observations (07a8e3e3)
- **audit**: record live Omaki relay E2E zero-config pairing receipt (496cad7b)
- **audit**: record ChatGPT Web worker final security audit and reproducible test probes (a405b34d)

### Maintenance & Internal

- **daemon**: send the remote-select bearer in a header, not the URL (8bfbed9c)
- **remote**: land the Gen5 APPROVED verdict and its acceptance regressions (dd1c74b4)
- **relay**: lock in the pairing generation repairs as permanent regressions (b335d9cf)
- **remote**: carry the real-gateway E2E through an authenticated terminal attachment (a50a4653)
- **ui**: integrate zero-config security probe suite verifying capabilities, metadata persistence, and direct candidate auth (e9f09838)
- **remote**: record live end-to-end zero-config pairing receipt (818b68db)
- **remote**: implement comprehensive security and negative testing suite (fbbc5ff6)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.09.1...v2026.09.11.1

## v2026.09.09.1

### Features

- **ssh**: process-preserving automatic reconnection and UI lifecycle (6ead7827)
- **ssh**: restore persistent remote sessions after daemon restart (764aa5f3)
- **ssh**: add process-preserving helper transport (437730f9)
- **ssh**: add persistent remote helper runtime (0c4304fa)
- **browser**: remember terminal link destination (6c0199ac)
- **ssh**: spawn remote worktree terminals at the worktree path (15d1bb64)
- **ssh**: manage remote git worktrees from the sidebar (66f1f14d)
- **sidebar**: show branch names and truncated machine badges for remote worktrees (e2e930d8)
- **release**: implement operator-controlled local release pipeline and inventory validation (aacb8aed)
- **shortcuts**: reveal contextual hints while holding modifiers (22a1f945)
- **ssh**: autocomplete remote project directory paths (e708ab19)

### Bug Fixes

- **browser**: route split pane navigation and reload to child browser ID (f9d952e6)
- **terminal**: preserve window handle lookup with embedded browser webview (eeb9e1dc)
- **terminal**: remove scrollback byte ceiling and expose settings control (dea42216)
- **ssh**: preserve host trust during reconnect (06a8037c)
- **sidebar**: show SSH agent activity indicators (d384a376)
- **ssh**: show workspace connection progress instead of empty tabs (5ab5d5d1)
- **daemon**: preserve session routing across rolling handovers (53f70559)
- **sidebar**: reveal primary worktree when expanding empty workspaces (70fcf783)
- **projects**: group checkouts by Git identity (d5afb0eb)
- **terminal**: keep terminal content visible during error recovery (fad98c9d)
- **terminal**: render full-height panes beneath hover overlays (70f4365d)
- **sidebar**: keep empty worktrees collapsed and reopen restored tabs (1ff1afa4)
- **browser**: preserve native masking behind overlays (e8c6de9c)
- **terminal**: retain macOS terminal frames behind overlays (0e605bbd)
- **terminal**: prevent obsolete deferred bounds replay (829cc9c1)
- **terminal**: restore OMO sessions in their original directory (f3e0ef97)
- **site**: publish privacy policy and correct Store URLs (f52ab8ad)

### Documentation

- record ssh reconnection and store privacy verification evidence (b7f4de66)
- **release**: record local macOS app replacement report for 2026.908.1 (c4cd4ede)
- **daemon**: record handover regression and compatibility evidence (dccd57af)
- **terminal**: record macOS replacement and daemon session verification (7f8d4810)
- **qa**: record desktop shortcut and SSH platform evidence (03db4c8e)
- **qa**: bound continuation verdict to recorded snapshots (b2b21d58)
- **qa**: record three-session continuation verification (cafa7412)
- **terminal**: record repair evidence and pending desktop QA (fa7bf22f)
- **store**: record privacy deployment and certification resubmission (a539d690)

### Maintenance & Internal

- **terminal**: supply default scrollback in theme contract test fixtures (b263ddfb)
- **terminal**: cover both deferred resize completion orders (bb6262c2)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.08.1...v2026.09.09.1

## v2026.09.08.1

### Features

- **ssh**: browse remote folders when adding projects (20d2bb8f)
- **ssh**: support Windows and POSIX remote runtimes (c85f17ea)
- **release**: verify minisign payload and trusted comment signatures (4829b600)
- **ssh**: paste clipboard images into remote panes (799e0e4c)
- **ssh**: expand tilde in config path and guard file picker in web mode (280b4ce2)
- **ssh**: import hosts from a user-selected config file (db9551a4)
- **deps**: upgrade outdated Cargo and UI dependencies (9f92cc1d)
- **projects**: unify remote SSH repositories as worktrees under matching projects (a4fb7171)
- **terminal**: compute and handle wheel scroll outcomes in native terminal (8c8c1a49)
- **native-terminal**: upgrade wgpu to v30 and remove local wgpu-hal vendor patch (29ea50be)
- **terminal**: file drag-and-drop path insertion and Cmd+click link/path opening (37272f52)
- **projects**: choose local or SSH location before adding projects (b708194b)
- **settings**: connect SSH machine management to shared inventory (39602ef0)
- **ssh**: register host-qualified projects and route remote terminals (8efc26ed)
- **permissions**: platform-appropriate status for Windows and Linux (049c6739)
- **permissions**: first-run onboarding modal and settings completions (e2a67859)
- **notification**: surface macOS dock badge capability and re-request guidance (4a7803da)

### Bug Fixes

- **terminal**: preserve Ctrl+C delivery and recover exited sessions (a7e24d39)
- **release**: make MSIX packaging fail closed on native errors (3a19bdf7)
- **release**: enforce local-only release workflow policy (bb7e4a21)
- **release**: validate versions and preserve files on stamp failure (8604d21f)
- **dag**: show pane badge only for the exact owning session (a9f680e3)
- **remote**: expose and control registered SSH sessions (5d549980)
- **terminal**: preserve opaque backing and atomic resize frames (dc352504)
- **remote**: reconnect event streams and resync desktop state (60c5dc8b)
- **dev**: sign debug bundles with Developer ID (09951de1)
- **sidebar**: collapse workspaces without open tabs (1e4c4643)
- **browser**: stop the child webview from covering the native terminal (8b53b726)
- **browser**: trust the engine for history and count finds with it (c4fb2f12)
- **macos**: deliver Finder file drops to the terminal (e4c7ace7)
- **workspace**: accurately detect and restore running session states across app restart (a0dbdac5)
- **ui**: sanitize native bounds error and safely clean up retry ref (331f8005)
- **ssh**: let an explicit config import restore deleted hosts (92b9809e)
- **workspace**: stop restore from manufacturing a phantom attention transition (ba90ceb7)
- **ui**: satisfy TerminalSession type in browser lifecycle test fixtures (4f880559)
- **terminal**: exit overlay on lifecycle exited and skip dead-session attach (af164e54)
- **terminal**: whitelist spawn request fields before IPC serialization (eef04721)
- **terminal**: whitelist spawn request fields before IPC serialization (b427c274)
- **dag**: route multi-hop edges via upward arcs, add modal multi-run tabs, and remove wave labels (a31a76c6)
- **terminal**: ensure TabGroupDropSurface is transparent for macOS native terminal (dc9e8e02)
- **native-terminal**: bridge D3 geometry accessors onto the v30 cfg-split frame target (146a1ff2)
- **terminal**: unify Wayland buffer geometry and report effective IME scale (D3) (b1e7d7e8)
- **terminal**: destroy Windows child surfaces on their owner thread (b115230a)
- **terminal**: invalidate obsolete input recovery owners (1e9bcbda)
- **native-terminal**: adapt D5 frame-retry harness to wgpu v30 and restore wheel module chain (7535fbdd)
- **native-terminal**: rearm dropped direct frames after guard release (D5) (11a2755b)
- **native-terminal**: bounded dynamic atlas growth and truthful accounting (D1, D4, D7) (b840b903)
- **terminal**: order native NSView below WKWebView on macOS for DOM overlays (1e975681)
- **remote**: repair path traversal, socket revocation, view boundary, and pairing rate limits (ff1d8ba7)
- **macos**: synthesize missing dev Info.plist in dev bundle runner (4156f41a)
- **native-terminal**: stop mid-frame glyph atlas clears from corrupting frames (4cab8ab8)
- **terminal**: never measure the grid from a font CoreText silently substituted (05fad139)

### Documentation

- **release**: complete resumed local pipeline audit (43ea1c51)
- **release**: verify Windows local build prerequisites (4fd3270d)
- **release**: audit the three-machine local release pipeline (9c0197ef)
- resync the cross-platform audit with the corrected daemon lane (554d1809)
- audit cross-platform defects across Windows, Linux, and macOS (8d6bd2d6)
- update code review report with re-review findings on TabGroupDropSurface transparency (cb7c26ef)
- update below-ordering fix documentation with TabGroupDropSurface transparency (0d102da2)
- document daemon preservation rule across release replacement in AGENTS.md (4cf953b9)
- **remote**: record web remote security review, QA matrix, and verification evidence (57b1b69a)
- **rendering**: record 2026-09-06 rendering review, repairs, and verification evidence (776b4888)
- **projects**: record location plan, verification, and code review (60e3de04)
- add Windows GUI manual checklist for permissions port (25e59717)
- record Windows/Linux permissions port verification evidence (c430c971)
- **native-terminal**: record display-switch glyph corruption verdict and fix (0f32f8d3)
- **release**: record v2026.09.06.1 release report (b8f82d70)

### Maintenance & Internal

- pin mac host platform in PermissionsSection rerun-onboarding test (e404e76b)

### Other Changes

- Fix startup pane attention highlight and land in-flight work (6e0f76de)
- Fix built-in browser link routing, webview leaks, and guest bridge auth (3c627358)
- **native-terminal**: preserve in-flight alternate-screen chain and atlas test formatting (20567f55)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.06.1...v2026.09.08.1

## v2026.09.06.1

### Features

- **windows**: notify installer users to move to the Microsoft Store (fd72688c)
- **updater**: poll for updates periodically while the app runs (ac6d0f6e)
- **windows**: move distribution to Microsoft Store (ffef8ff6)

### Bug Fixes

- **site**: provide cn shim so ui button.tsx resolves in site bundle (9695104d)

### Documentation

- **release**: record Store-only transition completion for v2026.09.05.1 (6da0450b)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.05.1...v2026.09.06.1

## v2026.09.05.1

### Features

- **remote**: list registered sessions without desktop selection (3791ce18)
- **protocol**: define scoped agent routing and wire contracts (299af388)
- **permissions**: add macOS Full Disk Access, Accessibility, and Notifications management in Settings with toast guidance (30c2b978)
- **native-terminal**: scroll viewport to bottom on typing, preedit, and paste (3da9e02e)
- **perf**: implement daemon pipelining, zero-copy buffer sharing, and output batching (3beb4911)
- **workspace**: workspace removal and worktree deletion via context menu and trash icon (8aa2821c)
- **sidebar**: allow dragging workspace rows directly without separate grip handle (7e08fe32)
- **ui**: mark finished session activity as seen on native terminal focus (f6d6fe91)
- pane attention frame, native file drop, remote scroll history, daemon session router (ad55206b)
- **terminal**: native file-drop paste, drag instrumentation, and in-flight workspace fixes (44f1578d)
- **daemon**: self-upgrade via exec when the GUI detects a stale binary (138bd42d)
- **ui**: unify all toasts on sonner and add update-available notification (6d8574aa)

### Bug Fixes

- **windows**: qualify std::io::ErrorKind in daemon TCP connect (48825781)
- **notification**: route notification clicks to the originating pane (faad9666)
- **notification**: request macOS badge permission and allow recovery (d1d39def)
- **terminal**: await native attachment readiness on reuse (8ed71b3b)
- **terminal**: copy macOS selections through native pasteboard (a062fe52)
- **dag**: honor exact provider session ownership (ea85021f)
- **notification**: align attention and delivery across workspaces (a52042f5)
- **terminal**: preserve IME input and correct remote key encoding (f0a9dee5)
- **terminal**: update native bounds when display density changes (076e4294)
- **ui**: mask native browsers and reject stale search results (2aef6065)
- **terminal**: share preferences and preserve the startup background (83a32963)
- **workspace**: refresh activity and deduplicate terminal publishing (9c078252)
- **dag**: resolve 16 DAG viewer bugs across backend watcher, state ownership, and UI (ea27a710)
- **terminal**: preserve bracketed paste mode and multiline formatting on paste (5da70d61)
- **workspace**: suppress restart auto-resume attention noise on every pane (fd7f60fe)
- **native-terminal**: tolerate invalid initial bounds during surface attach and reattach (2446f6d6)
- **ui**: refine sidebar icons, hover actions, and indentation hierarchy (8e5b8f9a)
- **workspace**: make waiting attention frames dismissible (3427cfbe)
- **workspace**: align Dock badge count with pane attention signal (3ff11b07)
- **notification**: surface dispatch rejection reasons and custom sound failures (171c8de3)
- **notification**: accept kebab-case notification sources and bundle sonner styles (2e767bb9)
- **workspace**: reject filesystem root as startup workspace and disambiguate workspace ID collisions (97b3a273)
- **native-terminal**: encode Kitty keyboard protocol chords for omo/senpi (f846f845)
- **native-terminal**: snap pane bounds and configure the CAMetalLayer (e978064c)
- **terminal**: restore keyboard focus when clicking terminal panes (36a4d49f)
- **native-terminal**: remove invalid setDrawableSize on NSViewBackingLayer to prevent crash (199761a2)
- **native-terminal**: synchronize CAMetalLayer drawableSize and viewport dimensions on resize (7ab98cdb)
- **native-terminal**: gamma-correct glyph coverage so text keeps its stroke weight (cf1c00fc)
- restore ssh module and gjc manifest lost in the stash cycle (e1084148)
- restore untracked files lost in the stash cycle (696730fd)
- **bootstrap**: render the workspace immediately (e208a1ac)
- **bootstrap**: bound startup and trace boot stages (bc78bd02)
- **terminal**: spawn default shells as login shells so Homebrew PATH loads (ae131772)
- **windows**: restore complete desktop rendering (43071bcd)
- **windows**: enforce exclusive daemon locking (bd8af0c1)
- **remote**: resolve bundled SPA assets (41b999da)
- **native-terminal**: repair search on child surfaces (d76c77db)
- **windows**: read native terminal clipboard (5396f26b)
- **build**: restore native-terminal feature-off compilation (cefdf304)
- **windows**: normalize Git worktree paths (dc179316)

### Documentation

- **terminal**: record workspace repair commit provenance (13246c24)
- **notification**: record final gate approval and scope limits (899d2e49)
- **notification**: record fixes and commit review evidence (b6401336)
- record frontend rendering fixes and verification (3496f544)
- add notification/badge/highlight architecture review (22fce966)
- **native-terminal**: record scroll bottom-lock fix five-agent review report (a6994aa2)
- **native-terminal**: record the glyph compositing contract (606e4fd3)
- **windows**: record final ulw-loop completion audit (f9433882)
- **windows**: record exact rendering diagnosis (ab977dad)
- **windows**: record black-screen root cause (cdc1c467)
- **windows**: add independent black-screen review prompt (ce868051)
- **windows**: audit remediation completion (ed97e2fe)
- **release**: record Windows NSIS packaging (d5cf4375)
- **release**: document v2026.08.30.2 (33311826)
- **windows**: complete edge verification audit (c560acc2)
- **windows**: record final verification evidence (04ed078c)

### Maintenance & Internal

- consolidate in-flight dag, terminal, and packaging work (831ae2c6)
- **qa**: add isolated scoped feature harness and cleanup checks (7551b3cf)
- prune legacy dev docs, original-dist dead assets, and stray receipts (03eb6f6b)
- align devRuntimeContract with the dev-frontend beforeDevCommand (9d05627b)
- consolidate in-flight workspace, agent, and daemon work (1c838ed2)
- **release**: prepare v2026.08.30.3 (773aa61f)
- **release**: prepare v2026.08.30.2 (168b529b)
- **qa**: honor Windows E2E repo root (76ac81a4)
- **qa**: parse Windows PTY cwd output (59c03d52)
- **windows**: compare launcher paths portably (f5e35540)
- **qa**: verify Windows PTY output and cwd (7e47eb05)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.09.02.1...v2026.09.05.1

## v2026.09.02.1

### Features

- **terminal**: persist scrollback to disk and resurrect sessions after daemon restart (7e3630df)
- **ssh**: implement SSH remote continuity — resident daemon, relay-bridge, lease store, reconnect ladder, per-host routing, agent resume gating (1c37cb0a)

### Bug Fixes

- **terminal**: spawn default shells as login shells so Homebrew PATH loads (94527f41)

### Maintenance & Internal

- **release**: stamp v2026.09.02.1 (e2a19066)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.30.3...v2026.09.02.1

## v2026.08.30.3

### Documentation

- **windows**: record final ulw-loop completion audit (f9433882)
- **windows**: record exact rendering diagnosis (ab977dad)
- **windows**: record black-screen root cause (cdc1c467)
- **windows**: add independent black-screen review prompt (ce868051)
- **windows**: audit remediation completion (ed97e2fe)

### Maintenance & Internal

- **release**: prepare v2026.08.30.3 (773aa61f)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.30.2...v2026.08.30.3

## v2026.08.30.2

### Bug Fixes

- **windows**: restore complete desktop rendering (43071bcd)

### Documentation

- **release**: record Windows NSIS packaging (d5cf4375)
- **release**: document v2026.08.30.2 (33311826)

### Maintenance & Internal

- **release**: prepare v2026.08.30.2 (168b529b)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.30.1...v2026.08.30.2

## v2026.08.30.1

### Features

- **site**: add workflow diagnostic routes (4cc1828d)
- **site**: add sourced diagnostic scoring (227d081c)
- **terminal**: selectable default shell (PowerShell/cmd/WSL/custom) (296e9a22)
- **dag**: glowing graph badge with multi-project live run detection (510caaf5)
- **ui**: dag view entry point and initial run loading (65db3634)
- **ui**: dag run graph pane (e6733c37)
- **dag**: live dag run watcher and ipc surface (10031a9b)
- **dag**: parse omo dag journal checkpoints (99421158)
- **ui**: show authentic brand logos and a default-agent dropdown (2370fb52)
- **ui**: resolve agent logos from CLI command names via aliases (e73ba30e)
- **ui**: identify a pane's agent from the screen-detection manifest id (f9773fb7)
- **agents**: manage custom agents from settings (b9dc40b1)
- **remote**: prune revoked device tombstones and confirm cross-worktree selection (b9530f3d)
- **daemon**: bridge remote selection events and normalize Windows cwd (d3ce6279)
- **browser**: serve the browser CLI over a stream socket with a port file (f9ccb412)
- **native-terminal**: recover terminal input attach and typing (df846aaa)
- **native-terminal**: rasterize glyphs through CoreText instead of the fallback resolver (dbe79329)
- **ui**: unify settings sections and functionalize General tab (b1ffa097)
- **agents**: port screen-rule detection to antigravity, cline, copilot, cursor, grok, kimi (9ab3af85)
- **site**: terminal-only landing demo on mobile (6ea92316)
- **browser**: close Orca/cmux parity gaps in the embedded browser (5d883f05)
- **agents**: screen-based activity detection, session restore, and tab branding (86777e7d)
- **native-terminal**: renderer font fallback, scrollbar, wheel scroll, and bounds-race hardening (8f442439)

### Bug Fixes

- **macos**: dedupe app menu and drop accelerator dead ends (73503e0b)
- **linux**: keep the main window opaque with restored identity (5d7e27fa)
- **linux**: resolve fonts per glyph and reject missing coverage (e93298e6)
- **site**: simplify mobile navbar actions (e9fdefe5)
- **site**: prevent tablet nav badge overlap (3d3f5849)
- **windows**: enforce exclusive daemon locking (74008c70)
- **remote**: resolve bundled SPA assets (d70f4343)
- **native-terminal**: repair search on child surfaces (cb2358d1)
- **windows**: read native terminal clipboard (6ecd7436)
- **build**: restore native-terminal feature-off compilation (7de71cc4)
- **windows**: normalize Git worktree paths (b59a6b14)
- **windows**: enforce exclusive daemon locking (bd8af0c1)
- **remote**: resolve bundled SPA assets (41b999da)
- **native-terminal**: repair search on child surfaces (d76c77db)
- **windows**: read native terminal clipboard (5396f26b)
- **build**: restore native-terminal feature-off compilation (cefdf304)
- **windows**: normalize Git worktree paths (dc179316)
- **windows**: report CLI launcher as unsupported (9266644b)
- **windows**: resolve nested worktree ownership paths (9e50c8e7)
- **windows**: embed comctl32 v6 manifest for every executable target (022ecd09)
- **native-terminal**: restore Windows typing via WH_MOUSE_LL focus monitor (fe175b37)
- **windows**: spawn console children with CREATE_NO_WINDOW (51961d98)
- **site**: drop download options that have no artifact behind them (f1571913)
- **dag**: bind the pane DAG indicator to its owning run and fix graph rendering (d158e090)
- **dag**: show pane indicator only while running (8449ac96)
- **dag**: float run indicators in their terminal panes (97f0bba7)
- **dag**: preserve persisted dag pane compatibility (dad050f1)
- **terminal**: make native paste layout independent (58fc6127)
- **dag**: register dag_watch_project and drop the dead cwd watcher (a226b131)
- **ui**: restore the missing dag pane content exports (8c04b6ee)
- **ui**: gate session persistence on structural layout changes (1f2cc2f8)
- **daemon**: bound the handshake read with a five-second timeout (6b46ff39)
- **terminal**: focus the clicked split pane and keep Hangul composition intact (b1860b5d)
- **dag**: spawn watcher on the Tauri runtime (5302d544)
- **terminal**: render every SGR text attribute and keep PTY winsize in sync (f157685f)
- **terminal**: rasterize native terminal glyphs on Windows and Linux (b087c83b)
- **terminal**: give Windows and Linux isolated native child surfaces (3fa25a19)
- **terminal**: guard first-responder restore so repaints stop stealing keys (6e2aca59)
- **notifications**: tighten permission handling and refresh update evidence (89f3fb67)
- **ui**: make the settings page tell the truth (52f96679)
- **ui**: enlarge tab identity icons (b9475f61)
- **site**: mock plugin-dialog save for browser pane build (7e24400f)

### Performance

- **ui**: replay backlog chunks in order without a contiguous copy (9ff94639)
- **terminal**: keep the render coordinator pending until a frame completes (806da935)
- **remote**: optimize mobile terminal switching with cached snapshots and optimistic session remount (ed6fc000)

### Refactoring

- **ui**: remove AI slop from settings page (fee4a1e1)
- **ui**: rebuild settings UI on shadcn primitives (6eac2d95)

### Documentation

- **readme**: show the workspace instead of describing it (b551709e)
- **site**: explain measured terminal architecture (c3be0c46)
- **site**: correct architecture claims (989d62ac)
- **windows**: record atomic history correction (c046cda6)
- **windows**: align atomic evidence hashes (97f33fa2)
- **windows**: complete edge verification audit (20b76b47)
- **windows**: record final verification evidence (743f28e3)
- **windows**: complete edge verification audit (c560acc2)
- **windows**: record final verification evidence (04ed078c)
- **windows**: add evidence artifacts for checkpoint quality gate (65e3a8f2)
- **windows**: add Windows terminal fixes evidence and QA checklist (09247d87)
- **terminal**: add OTTY feature borrowing audit (b025e8f5)
- **dag**: manual e2e checklist for the activity badge flow (4678a5ea)
- **perf**: record the bottleneck audit and remediation evidence (6fc3b6de)
- **dag**: design, manual e2e checklist, and live-tail evidence (a191f574)
- **terminal**: record rebuilt-release visual proof (ce6a941b)
- **terminal**: record blink as deliberately non-animated (4cc8dd06)
- **terminal**: record the SGR attribute and PTY winsize rendering fixes (94ff9a4c)
- **terminal**: record the Windows and Wayland native terminal fixes (afb03c8e)
- **terminal**: postmortem for the first-responder input steal (2a644262)
- **updater**: record installed recovery E2E (6b3b8888)
- **ux**: settings panel consistency analysis and redesign spec (6ef65ba8)
- **native-terminal**: record scrollbar resize verification (af838999)
- **updater**: record relaunch recovery E2E evidence (7ff87abc)
- **updater**: record public release E2E evidence (d4315cbd)

### Maintenance & Internal

- align contract fixtures with shell field and platform stub (10ce3296)
- **qa**: honor Windows E2E repo root (a3172559)
- **qa**: parse Windows PTY cwd output (91eb17ce)
- **windows**: compare launcher paths portably (41a2461c)
- **qa**: verify Windows PTY output and cwd (9cb363e0)
- **native-terminal**: cover Windows child compositor (7ffa7a46)
- **qa**: honor Windows E2E repo root (76ac81a4)
- **qa**: parse Windows PTY cwd output (59c03d52)
- **windows**: compare launcher paths portably (f5e35540)
- **qa**: verify Windows PTY output and cwd (7e47eb05)
- **qa**: add Windows headless daemon E2E script (65caf692)
- **release**: stop shipping QA evidence harnesses inside release bundles (d6ad1456)
- **terminal**: expand selection rendering evidence (f6dbd454)
- **tauri**: drop the duplicate ui/dist resource packaging (f0de399a)
- **terminal**: assert the child shell observes a PTY resize (a8676bb6)
- **terminal**: render the real SGR probe through the live VT engine (a9c1ffbe)
- **terminal**: add a headless daemon typing round-trip verifier (5248e939)
- **ui**: point appearance theme contract at the split settings sources (f328a786)
- **ui**: adopt shadcn foundations (799cc5f0)
- **agents**: pin every screen-rule manifest id to a bundled brand icon (4212ed51)
- **native-terminal**: drop superseded agentIcon test, guard paneHandleReach event bridge (2f4273f6)

### Other Changes

- **agent-detect**: apply cargo fmt to the ported manifest tests (b109a4d8)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.11...v2026.08.30.1

## v2026.08.26.11

### Bug Fixes

- **updater**: recover catalog after installed relaunch (5040a5bc)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.10...v2026.08.26.11

## v2026.08.26.10

### Bug Fixes

- **updater**: preserve workspaces across update relaunch (7d92dfbb)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.9...v2026.08.26.10

## v2026.08.26.9

### Bug Fixes

- **updater**: publish Tauri v2 platform bundles (30cb5e75)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.8...v2026.08.26.9

## v2026.08.26.8

### Bug Fixes

- **ci**: skip WiX for date-based releases (1228c47a)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.7...v2026.08.26.8

## v2026.08.26.7

### Bug Fixes

- **ci**: link Ghostty static archive on Windows (fca2bc76)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.6...v2026.08.26.7

## v2026.08.26.6

### Bug Fixes

- **tauri**: scope macOS dev runner by platform (10aa3667)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.5...v2026.08.26.6

## v2026.08.26.5

### Bug Fixes

- **ui**: restore CLI installer icon import (3ca94717)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.4...v2026.08.26.5

## v2026.08.26.4

### Bug Fixes

- **ci**: upgrade Zig setup action for release builds (9bd850c2)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.3...v2026.08.26.4

## v2026.08.26.3

### Features

- **site**: rebuild landing page with live product demo (5ca3201c)

### Bug Fixes

- **ui**: remove stale updater icon import (57c6f0e3)
- **ui**: resolve pane splits from center lines (101ad5ef)
- **macos**: name development app bundle (c3870c21)
- **macos**: name development app bundle (38908a93)
- **workspace**: stabilize project and worktree switching (6d3b5023)
- **terminal**: honor macOS natural text editing chords (4cdf5b8f)

### Documentation

- **updater**: record repaired public asset evidence (415e5add)
- rewrite README around the landing page (b97a6c2d)
- update README with license and code signing policy (eac40076)

### Other Changes

- Create LICENSE (16b4ed03)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.2...v2026.08.26.3

## v2026.08.26.2

### Bug Fixes

- **updater**: reject AppleDouble archives (bf0a2ad8)
- **updater**: streamline install and relaunch (f932f8ee)
- **release**: publish from date version tags (59ee187e)
- **ui**: emphasize updater relaunch action (49b8a5aa)
- **release**: notarize macOS updater builds (ff8aaf47)

**Full Changelog**: https://github.com/Indosaram/ferryx/compare/v2026.08.26.1...v2026.08.26.2

## v2026.08.26.1

### Features

- **updater**: add signed in-app updates (14e8d056)
- **site**: showcase native Ghostty terminal (aba8ed7f)
- **terminal**: migrate desktop rendering to native ghostty (38f887e4)
- **daemon**: cross-platform Windows support for headless daemon IPC (4a42ee04)
- implement daemon-owned terminal session persistence (a6e18c65)
- consolidate Ferryx workspace updates (fc2ef4d7)
- **site**: unify landing and developer docs (c32d9e22)
- **shortcuts**: expand keyboard shortcuts, aliases, zoom, and native menu (a7351196)
- **branding**: capitalize visible app title to Ferryx (c40af0d7)
- **branding**: rebrand project to Ferryx (14dee2cc)
- **ui**: fix HMR stability, add tab actions popover, and polish tab controls (a4de244c)
- **branding**: update app icons and add cross-platform icon assets (bdd91f56)
- **ui**: implement sidebar collapse with floating header and fix HMR CSP (6518c442)
- **ui**: add draggable terminal split divider (90c3eccc)
- **ui**: add persisted sidebar resizing (78af70de)
- **ui**: complete worktree and terminal IPC flows (a253ee8d)
- **ui**: add terminal settings dialog (b2229a17)
- **ui**: add workspace command palette (88bbab7c)
- **ui**: add keyboard shortcut registry (08b5f7c2)
- harden backend workspace IPC (4e192cf8)
- enforce worktree writer leases (b94f9110)
- synchronize workspace runtime state (28393c66)
- stream terminal events as raw bytes (a5b4ffbc)
- stabilize frontend terminal sessions (7723259e)
- harden PTY session lifecycle (108da88c)

### Bug Fixes

- **ui**: align coverage dependency lockfile (83acc75b)
- **worktree**: report non-repository roots (8779814c)
- **ui**: align native terminal lifecycle (bcdba790)
- **ui**: complete native workspace migration (60674768)
- **ui**: restore clean release build (bb1965d0)
- **terminal**: scale glyphs by em so font size matches Ghostty (a62eeb3e)
- **site**: point macOS downloads at universal DMG (93dcaabb)
- **msix**: write manifest UTF-8 without BOM and scope Identity substitutions (a24d03d6)
- **ui**: include terminalThroughputMetrics module in tracking (7294bac8)
- **daemon,ci**: fix test assert quotes, metrics field pattern, and macOS unsigned build env (82258e47)
- **docs**: correct Pages onboarding links (a08cf96a)
- **ui**: clarify tab and pane separation (a9b34ea3)
- **branding**: remove remaining public Orca labels (0b61101d)
- **branding**: eliminate outer black box and enforce transparent margins on squircle icon (5ad9fe2e)
- **branding**: use transparent background vector asset and regenerate all platform icons (0c65029c)
- **branding**: restore high-res monochrome master icons and clean assets (a1e5e034)
- **worktree**: emit dirty and prune events (6eca8946)
- **ui**: enforce mirror split writer lease (f85cc797)
- **ui**: align frontend IPC contracts (4945a87e)
- **worktree**: resolve nested repo launch roots (6cf4dbe0)

### Performance

- lazy-load terminal renderers (b0e07b4d)

### Refactoring

- unify frontend IPC contract (ed165cf7)

### Documentation

- add persistence review, remediation, and evidence artifacts (b343571f)
- complete clone setup guidance (873749a5)
- update cleanup delivery evidence (1cd5bd33)
- remove obsolete root reports (129083cb)
- fix README validation command (c72520ea)
- record delivery evidence (318a0bcd)
- organize root documentation (c65b1963)
- refresh README and fix Pages build (c9d757f5)
- **branding**: formalize monochrome icon specification and prevent asset regressions (b995b9df)
- **perf**: update frontend verification count (3bf0dc64)

### Maintenance & Internal

- run the test suites instead of only type-checking (de31e94e)
- **release**: publish only from date-versioned tags (3c18d91a)
- run build checks on pull requests only (ace76a03)
- remove generated agent runtime state (039452d5)
- **release**: force bash shell for Tauri bundle step on Windows (aaf1adbb)
- **release**: fix macOS rust targets for universal-apple-darwin build (6502656b)
- sync session journal, CI workflows, and repo upkeep (c0bf0d16)
- **site**: deploy unified web to GitHub Pages (b455f16c)
- harden worktree lifecycle coverage (f5efc722)
- initial commit (85315b7f)

### Other Changes

- **ui**: add semantic design tokens (3c2bafa7)
