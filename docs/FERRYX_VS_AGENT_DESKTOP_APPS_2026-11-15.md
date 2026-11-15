# Ferryx vs. seven agent desktop apps — 2026-11-15

Status: measured comparison note. Every cell below is traceable to one analysis file under
`.omo/evidence/session-01a1d86e-continuation/apps/`; cells that the analysis could not establish
say so instead of guessing, and the whole set of things nobody measured is listed in
"Not measured" at the end.

This doc answers the tier-list screenshot the owner attached on 2026-11-15. The earlier note
`docs/FERRYX_VS_MENUBAR_AND_TERMINAL_PEERS_2026-11-14.md` answers a **different** screenshot
(a menu-bar strip and a brightness HUD) and covers only Orca and T3 Code of these seven; it now
carries a banner saying so.

## 1. Scope and method

**The seven apps:** T3 Code, Zeron, Capy, Paseo, MonoCode, Orca, Emdash. Compared against Ferryx
on `origin/main` at `6c12a7aeac30`. `origin/main` has since advanced four commits, all of them CI
workflow and one UI test file (`git diff --stat 6c12a7aeac30..origin/main`); no product source
changed, so every Ferryx cell below still describes the current tree.

**Owner decisions that bound this work:**

- No signups, logins, waitlists, trials or payments for any app. No downloaded app was launched.
- Only these seven. Bb, super.engineering, Synara, Diri, Traycer, Paradev and Conductor are out
  by owner decision.
- Static inspection only: repository reading and binary reading (strings, symbols, asar
  extraction, signing metadata), never runtime behaviour.

**Method, per app:**

1. Resolve an official source and a download that needs no account.
2. Where source exists, shallow-clone it, record the commit SHA, and analyse `git grep` hits on
   the same ten axes for every app. Every claim carries `path:line@sha` plus a quoted substring,
   and `verify-citations.mjs` re-opens each one in the clone.
3. Where a public binary exists, download it, record sha256, unpack it read-only, and check
   framework, signing, entitlements and `Info.plist`; for Electron, extract `app.asar` and read
   the shipped JS. The point is both to add binary-only facts and to confirm the shipped build
   matches the repo.
4. Ferryx was measured the same way from committed `origin/main` only (`git show <sha>:<path>`),
   so nothing in this doc comes from another session's uncommitted work.

**The ten axes** are (a) stack/runtime, (b) agent integration, (c) worktree/branch isolation,
(d) terminal implementation, (e) review/diff/PR flow, (f) remote/mobile access and push,
(g) attention/notification model, (h) checkpoint/rollback, (i) persistence/session recovery,
(j) license/telemetry.

**Reading the matrix:** a cell states what the analysis found, not what the vendor claims, except
where the cell is marked *(vendor claim)* — Capy is the only app with no public source, so its
rows mix binary findings with pages captured from capy.ai and docs.capy.ai. All ten axes are
compared; on (j) license/telemetry the absence of an analytics client is itself the finding, so a
cell there records what a search of the source and the shipped bundle did and did not turn up.

## 2. Evidence routes

| App | Route | Source | Version analysed | Tried and rejected |
|---|---|---|---|---|
| T3 Code | OSS_REPO + PUBLIC_BINARY | `github.com/pingdotgg/t3code` @ `256dce7f1f6c` | 0.1.9 / `v0.1.9` dmg | name collision with the `t3` chat app; resolved by repo description, homepage and release assets |
| Zeron | OSS_REPO + PUBLIC_BINARY | `github.com/zeronsh/zeron` @ `68e460af252d` | 0.3.6 / `v0.3.6` dmg | HelloZeroNet/ZeroNet, zerotier/zeronsd, ZeroNVS, zeronode (all unrelated); accepted on the "control plane for coding agents" description plus `zeron.sh` linking the same releases |
| Capy | PUBLIC_BINARY | no public source | 0.8.0 / `Capy-0.8.0-arm64.dmg` | `gh search repos capy` (capybara, capy-ui, capyreader, capytaine), `gh search repos 'capy agent'`, `gh repo list scrapybara` (no app source; `capy-vscode` and `vscode-fork-26` rejected, `scrapybara-oss` is 104 KB Python), Homebrew core casks (absent). Accepted because capy.ai's `schema.org` sameAs and footer point at Scrapybara, and the tap cask `scrapybara/homebrew-tap` resolves to the same download. Download needs no account; signup is only for using the app and was not attempted |
| Paseo | OSS_REPO + PUBLIC_BINARY | `github.com/getpaseo/paseo` @ `1c175b36b3cd` | 0.15.4 / `v0.15.4` dmg (repo HEAD is `0.16.0-beta.2`) | none needed; repo description, homepage and cask all agree |
| MonoCode | OSS_REPO + PUBLIC_BINARY | `github.com/hardbeat920/monocode` @ `09d04436babb` | 0.16.0 (Tauri) and 0.16.0-preview.14 (native) | `aaronmbos/monocode` (font), `Mono-Code-Scheme/*` (colour scheme), MonoCoder (research) all rejected; accepted on `usemono.dev` linking the repo |
| Orca | OSS_REPO + PUBLIC_BINARY | `github.com/stablyai/orca` @ `684d36274a02` | 1.4.259 dmg (repo `package.json` says 1.4.214) | OrcaSlicer, hundredrabbits/Orca, plotly/orca rejected; the `orca` Homebrew cask is plotly/orca and was rejected |
| Emdash | OSS_REPO + PUBLIC_BINARY | `github.com/generalaction/emdash` @ `ccc6cce4a416` | 1.2.11 / `v1.2.11` dmg | `emdash-cms/emdash` (Astro CMS), `dmotz/emdash`, `getify/emdash` rejected; accepted on the repo description and `emdash.com` linking it |
| Ferryx | OSS_REPO (own repo) | `origin/main` @ `6c12a7aeac30` | source only | not applicable |

No row is `WEB_ONLY` or `NOT_FOUND`: all seven resolved to a public source, a public binary, or
both without an account. The lookup procedure was checked for invention — a deliberately bogus
name run through the same steps produced `NOT_FOUND` with seven recorded attempts and no
fabricated source (`task-14-acquisition.log`).

## 3. Per-axis matrix

Each cell cites the analysis file it came from. Footnote map at the end.

| Axis | T3 Code | Zeron | Capy | Paseo | MonoCode | Orca | Emdash | Ferryx |
|---|---|---|---|---|---|---|---|---|
| **(a) Stack / runtime** | Electron 44.10.1, Node server, React, Expo mobile[^t3] | Native Rust, gpui UI over a detached engine daemon, Swift iOS[^zer] | Electron 45.2.3, cloud-first device bridge[^capy] | Electron 44.2.0, Node daemon, one Expo app for iOS/Android/web[^pas] | Tauri 2 (React+Rust) plus an experimental gpui line; Expo mobile[^mono] | Electron 43.8.7 plus a detached PTY daemon; Expo mobile[^orca] | Electron 40.10.2 plus a `workspace-server` daemon[^emd] | Tauri 2 (Rust), React 18 + Vite, Tauri mobile shell[^fx] |
| **(b) Agent integration** | Claude Agent SDK, Codex `app-server`, Cursor SDK, ACP child processes[^t3] | Per-harness structured protocols: Claude `stream-json`, Codex `app-server`, ACP[^zer] | Agents run in Capy's cloud; "runs on the Claude Agent SDK" *(vendor claim)*[^capyweb] | Claude Agent SDK `query()`, Codex `app-server`, ACP SDK, OpenCode bridge[^pas] | Claude `stream-json`, Codex `app-server`, Cursor `acp`, ported to Rust[^mono] | Either raw CLI in a PTY **or** a structured session (SDK/`app-server`/ACP); hooks + OSC 7501 for status[^orca] | ACP-first via `@agentclientprotocol/sdk`, plus TUI agents in PTYs with installed hooks[^emd] | PTY only: 17-CLI registry, screen-rule manifests, `agent-hook` reporter; no SDK/ACP[^fx] |
| **(c) Worktree isolation** | `git worktree add --lock`, user-set root[^t3] | `~/.zeron/worktrees/<repo>/<name>` on `zeron/<name>`[^zer] | Managed worktree base vs "linked" workspaces; cloud VMs *(vendor claim)*[^capy] | `worktree add -b --no-track`, `~/.paseo`, metadata-tagged[^pas] | `worktree add` (or `-b`); sibling `<repo>-worktrees`[^mono] | `worktree add`, `~/orca/workspaces`, nested-repo discovery[^orca] | One worktree + branch per task, per-host root[^emd] | `<repo>/.ferryx-worktrees/<ws>/<slug>`, `spawn_in_worktree`[^fx] |
| **(d) Terminal** | `node-pty` on the server; renderer is libghostty-vt in WASM[^t3] | `portable-pty` + `alacritty_terminal`; Ghostty linked into the UI via zgx[^zer] | `@lydell/node-pty`; Ghostty VT parser in WASM over a custom renderer[^capy] | `node-pty` + headless xterm mirror; xterm.js 6 + WebGL client[^pas] | Tauri PTY host + xterm.js 6; `alacritty_terminal` in the native line[^mono] | `node-pty` in the detached daemon; xterm.js 6 + WebGL[^orca] | `node-pty` + xterm.js 6; PTYs may live on a remote host[^emd] | Native libghostty-vt through FFI with a wgpu child-view renderer[^fx] |
| **(e) Review / diff / PR** | Review service, `@pierre/diffs`, create-PR across many forges[^t3] | Unified-diff pane + turn snapshots; PR read via `gh`; handing a PR to an agent[^zer] | Threads own their PR, inline GitHub comments, squash-merge *(vendor claim)*[^capyweb] | Streams `checkout_diff_update`; native `createPullRequest` via a forge abstraction[^pas] | Turn recordings + tool diff previews; `gh pr create`[^mono] | Monaco diff with inline review comments; `gh pr create`; review-comment API[^orca] | Diff view, create-PR modal, CI checks and merge; no inline comments found[^emd] | Diff/stage/unstage/commit/push per worktree; no PR creation; read-only PR lane[^fx] |
| **(f) Remote / mobile / push** | Own relay → APNs direct; Tailscale package[^t3] | Cloudflare DeviceRoom relay; edge sends APNs; native iOS app[^zer] | Hardcoded device tunnel: Capy's cloud drives this Mac; no APNs[^capy] | `relay.paseo.sh` + Noise NK end-to-end channel; Expo push[^pas] | Direct mutual-auth QUIC on port 38416, no vendor relay; Expo push[^mono] | Hosted relay (desktops dial out); own push service to APNs[^orca] | SSH/SFTP to your own machines only; no mobile, no push[^emd] | Axum gateway + 6-digit PIN + own relay server; mobile Tauri shell; Web Push only[^fx] |
| **(g) Attention / notification** | SDK phases incl. `waiting_for_approval` / `waiting_for_input`; `Notification` + snooze[^t3] | Four-state indicator, 45 s staleness, desktop banners, background-only preference[^zer] | Electron `Notification` + permission addon; alert controls *(vendor claim)*[^capy] | finished/error/permission; presence-aware routing suppresses when a client is focused[^pas] | Turn-end/stall banners; per-category preferences[^mono] | Four states from hooks/OSC 7501; tray attention plus away/focus gating[^orca] | permission/idle/elicitation; in-app feed + sound + OS banner[^emd] | Two reasons (Waiting, Done) + Dock badge + custom sound; phone is not notified[^fx] |
| **(h) Checkpoint / rollback** | Per-turn git checkpoints in hidden refs `refs/t3/checkpoints/`; capture + restore[^t3] | Turn snapshots + discard; no rewind to an earlier turn[^zer] | Cloud checkpoint of uncommitted work *(vendor claim)*[^capyweb] | Rewind per message: conversation / files / both[^pas] | `refs/monocode/checkpoints/` + `undo_turn`[^mono] | Conversation rewind by forking a checkpoint; no git snapshots[^orca] | None found; git-level discard only[^emd] | None for agent edits; "checkpoint" means a hub-DB archive moved between machines[^fx] |
| **(i) Persistence / recovery** | SQLite event store; provider session resume passed to the SDK[^t3] | Engine outlives the window and restarts with backoff; Loro CRDT in SQLite[^zer] | `node:sqlite` local state; threads live server-side (unproven)[^capy] | JSON agent records; restart re-dispatches a resume prompt[^pas] | SQLite WAL; a remote host owns provider processes[^mono] | Detached daemon; cold restore replays scrollback[^orca] | better-sqlite3 + Drizzle; pid-file daemon[^emd] | Headless daemon + 512 KiB output ring + retained history + daemon-to-daemon handover[^fx] |
| **(j) License / telemetry** | MIT; PostHog on by default, `T3CODE_TELEMETRY_ENABLED`[^t3] | MIT; no product analytics client found[^zer] | Proprietary; no app-level analytics proven (Sentry appears only as a trigger source)[^capy] | Apache-2.0; no analytics client found[^pas] | MIT; none found[^mono] | MIT; PostHog with a consent state[^orca] | Apache-2.0; PostHog on by default[^emd] | "Ferryx License", source-available; no telemetry[^fx] |

### 3.1 Where Ferryx already leads

Four things in the matrix have no equivalent in the other seven files:

- **Terminal core.** Ferryx and Zeron are the only two whose analysis records a natively linked
  Ghostty VT: Ferryx through a pinned `libghostty-vt.a` behind FFI with a wgpu child view, Zeron
  by linking Ghostty into its zgx UI. The other five use xterm.js, a headless xterm mirror, or a
  Ghostty VT compiled to WASM.[^fx][^zer]
- **Recovery.** Ferryx is the only one of the eight whose analysis records a daemon-to-daemon
  handover keeping PTYs across a daemon upgrade; its ring has an explicit `ReplayGap` resync and a
  separate retained-history path.[^fx]
- **Telemetry.** Ferryx ships no analytics client at all, and no third-party reporting appears in
  its tree. Zeron, Paseo and MonoCode also come up clean; T3 Code and Emdash are on by default,
  Orca is consent-gated.[^fx][^t3][^emd][^orca]
- **Relay ownership.** Ferryx ships its own relay server binary; five of the seven dial a
  vendor-hosted one by default and MonoCode is the only other app whose pairing needs no relay at
  all. Whether Ferryx's relay is operated for users by the vendor is not recorded in the evidence.

## 4. Per-app notes

**T3 Code (0.1.9).** The most structured of the seven: agents are driven through SDKs and
`app-server` protocols rather than a TUI, per-turn checkpoints live in hidden git refs, and PRs
are created through per-forge packages. Telemetry is on by default with an env opt-out. The
shipped 0.1.9 build matches the repo exactly (Electron 44.10.1 both sides), and the binary adds
Clerk passkey/webauthn entitlements that the source read did not touch.

**Zeron (0.3.6).** Fully native, two binaries: a UI (wgpu/Metal, Luau scripts, Ghostty embedded)
and a detached engine daemon that owns PTYs and survives window close. One divergence matters for
anyone reading its own docs: `ARCHITECTURE.md` and `release.yml` describe a gpui frontend, but the
shipped 0.3.6 has the layout of the zgx build (224 `zgx` string hits vs 3 `gpui`) — the DMG was
built from `apps/zgx`. Turn snapshots exist; rewind to an arbitrary earlier turn does not.

**Capy (0.8.0).** The only app with no public source, so its rows are binary findings plus vendor
claims. The binary shows the interesting part: Capy is a remotely driven device. Its cloud reaches
into the Mac over a hardcoded `wss://device-tunnel-production.cloud.capy.ai/tunnel`, runs PTYs,
uses worktrees, and — with a per-device grant — performs macOS computer-use through a bundled Rust
helper (`kappu-macos`, Accessibility + `SCScreenshotManager`). Terminal rendering is Ghostty's VT
parser in WASM. Axes (e), (h) and (i) could not be established from the binary; review and
persistence live server-side.

**Paseo (0.15.4).** The closest thing to a direct peer in breadth: Electron shell over a Node
daemon, SDK and ACP adapters, `relay.paseo.sh` with a Noise NK end-to-end channel, Expo push, and
per-message rewind in three modes. Its attention policy is the most explicit of the seven — three
reasons and a 3-minute presence window that suppresses the notification when a focused client is
already showing that agent. Two caveats: the shipped 0.15.4 asar declares an unpacked
`claude-agent-sdk-darwin-arm64` bundle that is missing from the DMG, and the repo HEAD is
`0.16.0-beta.2`, ahead of the shipped binary.

**MonoCode (0.16.0).** Two products at one version from one repo: the GitHub release is a 41 MB
Tauri/WKWebView app with xterm.js 6, and the website's download button serves an 85 MB native gpui
rewrite with `alacritty_terminal`. Both carry a direct QUIC mobile link with UPnP/NAT-PMP and no
APNs — the phone talks straight to the desktop, no vendor relay. Checkpoints are git refs with
`undo_turn`, bounded at the first tool-start event.

**Orca (1.4.259).** The largest of the eight by tracked files (35,564) and the widest bundle: a
computer-use app, an agent browser, a daemon template, WSL relays, APNs device registration and
the broadest TCC entitlements of the set. Its dual agent mode is the one Ferryx's PTY-only model
should be measured against: the same conversation can run as a raw CLI in a PTY or as a structured
session, with hooks and OSC 7501 feeding one status model. Version drift to note: repo
`package.json` says 1.4.214 while the shipped build says 1.4.259, so the release number is stamped
at build time.

**Emdash (1.2.11).** ACP-first with bundled `claude-acp` / `codex-acp` adapters, plus TUI agents in
PTYs. Its distinguishing surfaces are VCS-shaped rather than agent-shaped: create-PR modal, CI
checks, merge, and issue intake from Linear, GitHub, Jira, GitLab and others. No mobile app, no
push, no checkpoint system found. It is the only one of the seven whose notarization ticket is not
stapled.

**Ferryx (2026.1115.1 on main).** PTY-first by design: 17 CLIs in one registry, screen-rule
manifests plus an `agent-hook` reporter for state, no SDK or ACP. Strongest on the terminal core
and recovery (native Ghostty, replay gap handling, handover), weakest exactly where the others
converged (structured protocols, per-turn code rollback, phone push). Its mobile shell embeds the
remote web UI and has no PTYs of its own, and phone push on main is Web Push only — which an iOS
WKWebView cannot use, so the installed iOS app currently gets no push at all. Native APNs exists on
the `apns-ios-native` branch and is not on main.

## 5. Ranked Ferryx gap list

**Ranking rule:** score = (number of apps whose analysis records the capability) × (relevance to
Ferryx's positioning, 1-3). Ties break toward the gap whose Ferryx-side absence is proven by a
recorded search rather than merely unrecorded. "Ferryx state" names that difference, so a low rank
never hides a missing measurement.

| # | Gap | Apps that have it | Evidence | Ferryx state | Covering plan |
|---|---|---|---|---|---|
| 1 | **Structured agent protocol** (provider SDK, `app-server`, ACP) instead of a PTY plus screen rules | 7 of 7 — T3 Code, Zeron, Paseo, MonoCode, Orca, Emdash in source; Capy by vendor claim | `apps/t3code.md` (b), `apps/zeron.md` (b), `apps/paseo.md` (b), `apps/monocode.md` (b), `apps/orca.md` (b), `apps/emdash.md` (b), `apps/capy.web.md` | Absent by design: a search of `src-tauri/Cargo.toml`, `ui/package.json`, `crates/*/Cargo.toml`, `src-tauri/src` for `agent-client-protocol` / `claude-agent-sdk` returns 0 hits | Partial: `.omo/plans/t3code-control-plane-port.md` T3 adds a capability boundary over the 16 manifests and T4 a refusal gate, but no plan adopts a structured protocol |
| 2 | **Native iOS/Android push from the daemon** | 5 of 7 — T3 Code (own relay → APNs), Zeron (edge → APNs), Orca (own push service → APNs), Paseo (Expo), MonoCode (Expo) | `apps/t3code.md` (f), `apps/zeron.md` (f), `apps/orca.md` (f), `apps/paseo.md` (f), `apps/monocode.md` (f) | Absent on main: Web Push/VAPID only, and `ui/src/lib/pushSubscription.ts:50` needs `PushManager`, which the iOS shell lacks. APNs is committed only on `apns-ios-native`, not on main | `.omo/plans/ferryx-action-plan-2026-11-14.md` P0-2a/P0-2b; this plan's Wave 6 (todos 20-25) |
| 3 | **Per-turn code checkpoint and rollback** | 5 of 7 — T3 Code (hidden refs, per turn), MonoCode (`undo_turn`), Paseo (rewind conversation/files/both), Zeron (turn snapshots + discard), Capy (cloud checkpoint, vendor claim) | `apps/t3code.md` (h), `apps/monocode.md` (h), `apps/paseo.md` (h), `apps/zeron.md` (h), `apps/capy.web.md` | Absent for agent edits: a search of `src-tauri/src/worktree`, `src-tauri/src/ipc`, `ui/src/components` finds only the browser-engine install rollback, the prompt-bearing supervisor lease checkpoint (`checkpointAt`), and the DAG run checkpoint, none of which is per-turn code state. Ferryx's "checkpoint" is a hub-DB archive moved between machines | No plan for the feature: `.omo/plans/t3code-control-plane-port.md` T4 ships refusal only and T9 defers conversation rollback |
| 4 | **Attention that knows where you are looking** — reason breadth and cross-device suppression | 6 of 7 record a state/preference model; 2 do presence suppression explicitly (Paseo's 3-minute window, Orca's away/focus gating); T3 Code has approval/input phases + snooze, MonoCode per-category preferences, Zeron background-only | `apps/paseo.md` (g), `apps/orca.md` (g), `apps/t3code.md` (g), `apps/monocode.md` (g), `apps/zeron.md` (g), `apps/emdash.md` (g) | Partial: two reasons only (Waiting, Done) plus badge and sound; neither presence/focus suppression nor a per-category preference is recorded | Partial: `.omo/plans/agent-attention-parity.md` IS-1..IS-3 unify the judgement, add a reason line and cross-surface acknowledge; presence suppression has no plan |
| 5 | **Native create-PR and multi-forge support** | 5 of 7 — T3 Code (many forges), Paseo (forge abstraction), Orca (`gh` + Azure/GitLab/Gitea/Bitbucket), MonoCode (`gh pr create`), Emdash (create-PR, CI, merge) | `apps/t3code.md` (e), `apps/paseo.md` (e), `apps/orca.md` (e), `apps/monocode.md` (e), `apps/emdash.md` (e) | Absent: no in-app PR creation; the panel's PR lane is read-only and the hub classifies `gh pr create` as a remote effect to gate. Zeron also has no native create, so this is not universal | Partial: `.omo/plans/t3code-control-plane-port.md` T7 is read-only by design; creation has no plan |
| 6 | **Inline diff review comments** | 2 of 7 — Orca (Monaco diff comments + review-comment API), Capy (posts findings as inline GitHub comments, vendor claim) | `apps/orca.md` (e), `apps/capy.web.md` | Absent: a search of `ui/src/components` finds only `ChangesPanel.tsx` and file previews | No plan |
| 7 | **Computer-use / GUI automation handed to agents** | 2 of 7 — Orca (`Orca Computer Use.app`, `agent-browser`, `computer-sidecar.js`), Capy (per-device computer-use grants + `kappu-macos`) | `apps/orca.binary.md`, `apps/capy.binary.md`, `apps/capy.web.md` | Not recorded: no computer-use surface appears in the Ferryx baseline's axis list, so absence is unproven | No plan for agent computer-use (`.omo/plans/maho-browser-merge.md` and `.omo/plans/ferryx-daily-browser.md` build a browser *for the user*, not GUI control *for an agent*) |
| 8 | **Native terminal renderer on the phone** | 1 of 7 — Paseo ships a separate native grid renderer for mobile | `apps/paseo.md` (d) | Partial: the mobile shell embeds the remote web UI and has no PTYs of its own; no native renderer is recorded | No plan |
| 9 | **Lane lifecycle: pin / settle / snooze / archive, auto-settle on merge** | 1 of 7 — T3 Code | `apps/t3code.md` (e), and the lifecycle work in `.omo/plans/t3code-control-plane-port.md` T5/T6 | Not recorded: the baseline's sidebar unit is worktree/project-shaped | `.omo/plans/t3code-control-plane-port.md` T5/T6 |
| 10 | **Issue/ticket intake** (Linear, GitHub, Jira, GitLab) | 1 of 7 — Emdash | `apps/emdash.md` (e) | Not recorded | No plan |

Two ranking consequences worth stating plainly. First, gap 1 is the only one where every app
converged, so it is a positioning question, not a feature request: Ferryx's PTY-only model is what
makes its 17-CLI breadth and its terminal core possible, and adopting a protocol would be a second
path, not a replacement. Second, gaps 6, 8, 9 and 10 are single-app features — they are listed so
the owner can see they were considered and can decline them on purpose.

## 6. Not measured

- **Runtime behaviour of any app.** Nothing was installed, launched or exercised. Every statement
  above is about source, shipped bytes, signing metadata or vendor pages.
- **Capy's review, checkpoint and persistence paths.** The binary does not establish them; the
  vendor claim is all that exists for (h) and the cloud keeps (e) and (i) server-side.
- **Capy's cloud, Zeron's edge, Orca's cloud, T3 Code's relay and Paseo's relay as running
  services.** Their client-side contracts were read; the services themselves were not.
- **Prices, limits and business models.** Not part of the ten axes and not compared.
- **Orca's default telemetry consent state.** The read establishes the env opt-outs and the
  banner-based consent check, not which state a fresh install starts in.
- **Whether any shrunk or diverged source claim holds at runtime** — e.g. Paseo's missing bundled
  `claude` binary is an inference from the unpacked layout, not from running the app, and Orca's
  repo/shipped version drift is a build-time stamp, not a code difference.
- **Ferryx's shipped mobile build.** The baseline is source at `6c12a7aeac30`; whether the store
  build matches that SHA was not checked.
- **Ferryx's direct-route behaviour.** `.omo/plans/transport-hardening-20261112.md` records that
  direct is never attempted for terminal attach at its own baseline; this doc neither confirms
  nor disputes that at `6c12a7aeac30`.
- **Windows and Linux behaviour of any of the eight.** All binary facts are macOS arm64.
- **The excluded apps.** Bb, super.engineering, Synara, Diri, Traycer, Paradev and Conductor were
  out of scope by owner decision, so any comparison with them is unanswered.

## 7. Footnotes

Cited analysis files. Each is a repo-root-relative path; `check-doc-links.mjs` re-opens every one
and exits non-zero if any is missing. Verbatim copies of all of them are committed beside this doc
under `docs/research/agent-desktop-apps-2026-11-15/`, because `.omo/` is not tracked by git.

[^t3]: `.omo/evidence/session-01a1d86e-continuation/apps/t3code.md` and `.omo/evidence/session-01a1d86e-continuation/apps/t3code.binary.md`
[^zer]: `.omo/evidence/session-01a1d86e-continuation/apps/zeron.md` and `.omo/evidence/session-01a1d86e-continuation/apps/zeron.binary.md`
[^capy]: `.omo/evidence/session-01a1d86e-continuation/apps/capy.binary.md`
[^capyweb]: `.omo/evidence/session-01a1d86e-continuation/apps/capy.web.md`
[^pas]: `.omo/evidence/session-01a1d86e-continuation/apps/paseo.md` and `.omo/evidence/session-01a1d86e-continuation/apps/paseo.binary.md`
[^mono]: `.omo/evidence/session-01a1d86e-continuation/apps/monocode.md` and `.omo/evidence/session-01a1d86e-continuation/apps/monocode.binary.md`
[^orca]: `.omo/evidence/session-01a1d86e-continuation/apps/orca.md` and `.omo/evidence/session-01a1d86e-continuation/apps/orca.binary.md`
[^emd]: `.omo/evidence/session-01a1d86e-continuation/apps/emdash.md` and `.omo/evidence/session-01a1d86e-continuation/apps/emdash.binary.md`
[^fx]: `.omo/evidence/session-01a1d86e-continuation/apps/ferryx.md`
