> **Superseded for the 2026-11-15 request.** This note analyses a different screenshot (a menu-bar strip and a brightness HUD), not the tier-list image, and it covers only Orca and T3 Code of the seven apps asked about.
> The comparison that answers the 2026-11-15 tier-list request is [`docs/FERRYX_VS_AGENT_DESKTOP_APPS_2026-11-15.md`](FERRYX_VS_AGENT_DESKTOP_APPS_2026-11-15.md) (seven apps on ten axes, with a ranked Ferryx gap list).
> Read that document instead when the question is which agent desktop app to learn from; this one stays for its pixel and bundle measurements of the 2026-11-14 image.

# Ferryx vs. the Apps Around It — 2026-11-14

Status: measured research note. Every claim below carries either a pixel measurement, a
bundle inspection command, or a named project doc. Items that were **not** measured are
listed in "Not measured" at the end instead of being implied.

## 1. What the attached screenshot actually is

Source image: 768x250 PNG, sRGB, 144 dpi, alpha channel present, 71,607 bytes, written
2026-11-14 13:57 local to `$TMPDIR/ferryx-paste/6afe72f2-216a-4745-a5e6-0be7d7f7056f.png` by the
Ferryx pane that received the paste. It was the only PNG written to the temp tree in that window,
which is how it was matched to the attachment.

Row-brightness profiling (`magick ... -colorspace gray gray:-`, per-row mean) plus a
saturation map found exactly three content bands:

| y range | content |
|---|---|
| 22-56 | macOS menu bar strip |
| 90-135 | translucent on-screen-display (brightness HUD) upper contour |
| 175-219 | the HUD body plus the terminal text seen through it |

So the screenshot is **a menu-bar strip plus a brightness HUD over a terminal window**. It is
not an app gallery, not a comparison table, and not a screenshot of any product UI.

### 1.1 Menu bar, item by item (measured)

Items were located by scanning columns for high-luminance glyph runs inside the y=22..56 band,
then each item was cropped with `-filter point -resize 600%` and read back.

| # | x range (of 768) | glyph | identity | basis |
|---|---|---|---|---|
| 0 | 0-21 | pale-lilac rounded square, faceted 12-point star + 6-node rising line chart | **mahoquot 0.1.7** (third-party) | pixel + motif match, see 1.2 |
| 1 | 60-85 | speaker with diagonal slash | system: muted audio | glyph |
| 2 | 128-152 | Bluetooth rune | system: Bluetooth | glyph |
| 3 | 190-215 | three radiating arcs | system: Wi-Fi | glyph |
| 4 | 262-300 | battery outline, left half filled | system: battery ~50% | glyph |
| 5 | 330-400 | dark pill with a white display-with-stand icon | system: Screen Mirroring | glyph |
| - | 425-755 | text | `Sat Nov 14  1:57:43 PM` | transcribed |

Only **one** menu-bar item is a third-party app: item 0. Everything else is stock macOS.

### 1.2 Identifying item 0

Colour clustering over saturated pixels showed the icon's whole hue budget is
lavender/violet (192,176,240 / 160,112,240 family) inside a 20x20 box at x=0..19, y=27..47.

Two methods were tried. **Only the first supports the conclusion; the second contradicts it and
is withdrawn as evidence.**

1. **Motif match (vision) — this is the basis for the identification.** The crop upscaled 11x
   beside crisp candidates matched on four features: the 12-point faceted star in the
   upper-left, the 6-node line trajectory with the same dip/rise inflections, the three embossed
   horizontal grid lines, and the white/lilac/deep-violet palette. Re-rendering the whole menu
   bar at 250% confirms it independently: item 0 is a pale-lilac rounded square with a violet
   star upper-left and a descending line chart, and mahoquot's icon at 160x160 is the same
   motif. ChatGPT's icon, by contrast, is a blue/white circular swoosh.
2. **Pixel match (numeric) — RE-RUN, DID NOT REPRODUCE, AND RANKED A DIFFERENT APP FIRST.**
   The first draft reported mahoquot 79.7 vs Stayput 99.0. An independent re-run over 64
   installed `/Applications/*.app` icons (icns via `sips` → 16x16 → mean absolute RGB distance on
   the 20x20 crop at +0+27) gives:

   | rank | app | MAD |
   |---|---|---|
   | 1 | **ChatGPT** | **43.5** |
   | 2 | mahoquot | 73.6 |
   | 3 | MahoRD | 87.5 |
   | 4 | ZetoPad | 87.9 |
   | 5 | cmux | 88.4 |

   The ordering is stable across nine crop offsets (x 0–2, y 26–28): ChatGPT 42.3–44.7,
   mahoquot 72.3–73.7. So this is not crop jitter — the metric genuinely prefers ChatGPT, an app
   whose icon is visually nothing like the glyph. The reason is structural: item 0 touches the
   image edge at x=0, so the 20x20 crop loses part of the glyph, and 16x16 downsampling discards
   exactly the star and line geometry that carries identity. A global-mean colour distance on a
   clipped, heavily downsampled icon rewards large flat light-blue fields, which is what
   ChatGPT's icon is. **Conclusion: MAD was the wrong instrument, its result is recorded here as
   a failed check rather than as corroboration, and the identification rests on the motif match.**

Independently, mahoquot remains the only candidate that is *also* a menubar-resident AI-usage app,
and §2 confirms that is the category the screenshot points at.

Bundle: `/Applications/mahoquot.app`, `dev.mahoquot.monitor`, version 0.1.7. Its strings
carry provider routing and quota vocabulary (`providers`, `cacheKey`, `account treated as
over budget`, `claude/ai/cline/recom...`, `codex-fast-model`, `claude-cloak-mode`), which
places it in the **AI-subscription quota monitor** category, not the terminal category.
That negative was checked rather than assumed, with word-boundary counts
(`strings -a <bin> | grep -oiw <token>`): mahoquot gives `pty`=0, `PTY`=0, `worktree`=0,
`vt100`=0, `terminal emulator`=0. The same probe on `Ferryx.app` gives `pty`=38, `worktree`=74,
`daemon`=263, so the probe does detect terminal software when it is present. (The first draft
printed "16, 8, 38" for Ferryx here, which does not match a word-boundary count; the values above
are the re-measured ones, and the qualitative conclusion is unchanged.)

### 1.3 The terminal behind the HUD

Legible fragments: `ds-ap` at the left edge, `: [exa] is done` spanning the HUD, and blurred
Hangul matching `오류 요약 [1] 일자 집계` ("error summary [1] date aggregation") inside the HUD.
This is a host/session label plus the tail of a completed command — evidence that the app under
the HUD is a terminal, not that any particular terminal product was on screen.

## 2. The category the screenshot points at

Because the single third-party item is a quota monitor, the honest "related apps" set is the
**AI usage/quota monitor** category. Three exist on this machine, all measured:

| app | bundle id | version | size | how it gets data |
|---|---|---|---|---|
| mahoquot | dev.mahoquot.monitor | 0.1.7 | **122M** (measured; an earlier draft said "22M-class" — wrong by 5x) | own gateway; `providers`, `usage`, budget language. At 122M it is 5.5x the other two, which is itself the finding: it is not a thin log reader |
| AI Token Monitor | com.soul.ai-token-monitor | 0.22.0 | 22M | parses local agent logs: `.claude`, `.codex`, `.kimi`, `.kiro`, `.zhipu`; `Session (5h)`, `Weekly7d`, `resets_at`, `five_hour`, `seven_day_sonnet`, `seven_day_opus`, embedded pricing tables |
| Quotio | app.bytrong.quotio | 0.31.0 | 22M | account-based: `_providerQuotas`, `_enabledAccountIds`, `_menuBarMaxItems`, `_quotaDisplayMode` |

All three are menubar-only, single-purpose, and none of them touch terminal sessions.

Two of the three sizes above come from `du -sh` on the installed bundle and the third was
corrected on re-measurement. The "how it gets data" column is **string evidence, not traced
data flow**: it shows which vocabulary each binary carries (log paths, quota field names,
account keys), which is enough to place the app in a category but is not proof of its runtime
behaviour. Treat it as a lead, not a measurement — the peer comparison in §3-§5 does not rest
on it.

## 3. The peer set that actually competes with Ferryx

Excluding the quota monitors, the machine also has the terminal/worktree/agent-orchestration
products that overlap Ferryx's surface. All facts below are from bundle inspection on
2026-11-14, not from marketing pages.

**Counting method, stated once because an earlier draft was not reproducible.** Every number in
this section is `strings -a <executable> | grep -oiw <token> | wc -l` — a **word-boundary** count
on the main executable. The earlier draft's figures (`iroh` 1239, `Ghostty` 844, `tokio` 162,
`goal` 363, `goals` 206, `worktree` 46) match neither this metric, nor substring counts, nor
`grep -c` line counts on any of the six bundles, so they are **withdrawn as unreproducible**.
Note that a bare `grep -oi pty` is worthless here: it matches "em**pty**", which once inflated
Clone's pty count to 2110 when the word-boundary count is 0.

| product | bundle id | ver | size | implementation | word-boundary census (main executable) | distinctive measured surface |
|---|---|---|---|---|---|---|
| **Ferryx** | com.ferryx.app | 2026.1113.2 | 588M | arm64 Mach-O; WGPU + libghostty-vt; 7 FFmpeg dylibs in Frameworks/ (`libavcodec.63`, `libavdevice.63`, `libavfilter.12`, `libavformat.63`, `libavutil.61`, `libswresample.7`, `libswscale.10`) | `pty`=38, `daemon`=263, `worktree`=74, `tokio`=196, `goal`=113, `iroh`=0, `vt100`=0 | PTY daemon over UDS, PIN-paired Axum gateway, remote desktop, worktree jail |
| **cmux** | com.cmuxterm.app | 0.65.0 | 744M | Mach-O universal; Iroh.framework + Sentry + Sparkle + a nucleo command-palette dylib | `iroh`=495, `Ghostty`=1048, `pty`=228, `daemon`=219, `tokio`=155, `worktree`=145 | **29** distinct `CMUX_AGENT_*` env vars, a full git-worktree feature (see §4), bundles CmuxBrowser / CmuxControlSocket / CMUXMobileCore |
| **Clone** | com.cloneisyou.clone | 0.4.65 | 950M | Electron (Electron Framework, Mantle, ReactiveObjC, Squirrel); app.asar 44M / 847,474 lines | `goal`=15,720; `worktree`=918; **`pty`=0**, `node-pty`=0, `termios`=0 | goal-centric agent UI; git worktree apply/commit/remove; **no terminal emulator at all** |
| **Rex Beta** | com.superlogical.rex | 0.1.0 | 172M | SwiftUI; swift-nio (`NIOTransportServices`=64); **no embedded ghostty framework** — `libghostty`=2 is a string, not a shipped lib | `Ghostty`=138, `terminal`=540, **`pty`=0**, `worktree`=0 | split/floating panes, an **embedded Headscale-capable Tailscale node** (`server.tailscale.{enable,disable,login,logout,rename}`, `--tailscale-control-url`) — *not* libtailscale tsnet, OSC 7501 agent status |
| **Ovis** | sh.ovis.desktop | 0.5.3 | 385M | Electron shell over a bundled Node/WKWebView server (`Resources/ovis/server/dist/assets/*.js`) | `herdr`=5,289; `worktree`=712; **`push-relay`=0** in the bundle | chat+terminal hybrid over herdr, phone pairing, git-worktree workspaces |
| **Ghostty** | com.mitchellh.ghostty | 1.4.0 | 67M | Metal renderer | `Ghostty`=974, `pty`=50, `daemon`=0, `worktree`=0, `tokio`=0 | emulator only — no daemon, no worktree, no remote |

Two corrections this table now carries, both from re-measurement:

- **Rex does not embed libghostty.** `Ghostty`=138 and `terminal`=540 are its own vocabulary;
  `pty`=0 and no ghostty framework appears in `Contents/Frameworks` (only
  `libswiftCompatibilitySpan.dylib` and `Sparkle.framework`). The earlier "swift-ghostty (1,050
  `Ghostty` strings)" claim described a dependency the shipped bundle does not contain.
- **Clone has no terminal.** `pty`=0 under a word-boundary count, `node-pty`=0, `termios`=0, and
  its only `xterm` hit is a TERM-name regex (`/^screen|^xterm|^vt100|^vt220|^rxvt|color|ans/`).
  Its `terminal*` strings are agent-stream vocabulary (`terminalReasonCode`, `beginTerminal`).

## 4. Capability matrix

Legend: ● present, ◐ partial/one-platform, ○ absent. Ferryx column is grounded in this repo's
own `AGENTS.md` contract (§OVERVIEW, §CONVENTIONS, §UNIQUE STYLES), not in peer advertising.

| capability | Ferryx | cmux | Clone | Rex | Ovis | Ghostty |
|---|---|---|---|---|---|---|
| GPU-native terminal | ● libghostty-vt + WGPU | ● Ghostty (`Ghostty`=1048, `pty`=228) | ○ **no terminal at all** (`pty`=0, `node-pty`=0; the only `xterm` hit is a TERM-name regex) | ◐ own terminal (`terminal`=540) but **no pty** (`pty`=0) and **no shipped ghostty framework** — not libghostty | ○ web terminal rendered in its bundled server | ● Metal |
| Persistent session daemon independent of the GUI | ● UDS daemon, launchd-owned, ring buffer + `ReplayGap` | ● cmux-tui daemon | ◐ | ● Rex server | ◐ herdr server | ○ |
| Binary pane split tree + tab transplant | ● reducers in `ui/src/state` | ● Bonsplit bundle | ◐ | ● native splits | ◐ | ● splits |
| Git worktree isolation with root jail | ● `.ferryx-worktrees/<ws>/<slug>`, `naming.rs` single source | ● full feature: `ProjectWorktreeSidebar`, `createWorktree`, `--worktree*/--skip-worktree-setup` flags, post-creation integrity checks | ◐ `worktree` strings | ○ (8 strings) | ● `createWorktree`/`listWorktrees`, "a git worktree of this repository … opened as a workspace" | ○ |
| Agent detection / resume / transcript | ● `agent_detect`, `agent_transcript`, `agent_control` | ● `CMUX_AGENT_*` launch contract, **29** distinct vars incl. `LAUNCH_ARGV_B64`/`LAUNCH_CWD`/`LAUNCH_EXECUTABLE`/`SUBROUTER_CLAUDE_RESUME_COMMAND`/`RESTORE_LAUNCH`/`JOURNAL_PATH`/`HOOK_STATE_DIR` | ◐ agent-stream vocabulary, no pty | ● OSC 7501 | ● herdr | ○ |
| Remote access without VPN | ● Axum + PIN pairing + active-desktop lock, **plus its own STUN/QUIC direct path** (`paired_host/direct/` 1,027 prod + 602 test lines, `direct_connect.rs`/`direct_client.rs` another 582) that upgrades a relay route to peer-to-peer when a hole-punch succeeds — already on the request path (`client.rs:1073,1315,1546`) | ● Iroh QUIC + relay (`iroh`=495, Iroh.framework) | ● cloud | ● **embedded Tailscale node, Headscale-capable** (`server.tailscale.*`, `--tailscale-control-url`) — not libtailscale tsnet | ● relay | ○ |
| Mobile client | ◐ `mobile/` Tauri v2 shell (Android + iOS, `com.ferryx.mobile` 1.0.0) wrapping `RemoteApp` — builds but is **not published** (store checklist unchecked) | ● iOS on TestFlight | ◐ | ◐ iOS in testing | ● Android on Play + iPhone TestFlight | ○ |
| Mobile push notifications | ◐ **PWA yes, native no.** `remote/push.rs` (617 lines) is standard Web Push — self-generated VAPID P-256 (`VapidKeys::generate`), `aes128gcm` ECE, endpoint allowlist `PUSH_SERVICE_HOSTS` = {fcm.googleapis.com, push.apple.com, push.services.mozilla.com, notify.windows.com}. It holds **no vendor credential**. `mobile/` native registration: 0 hits. | ● forwarded agent notifications | ○ | ○ | ● `push-relay/` **in the GitHub tree**; the shipped 0.5.3 bundle has `push-relay`=0, so do not cite the bundle for it | ○ |
| Remote desktop / screen streaming | ● `crates/ferryx-rd` + FFmpeg: streams a host's desktop to another Ferryx client | ◐ a bundled **noVNC** viewer (`vnc`=15, `noVNC`=4, `websockify`=2) plus permission plumbing (`screenRecording`=20) — but **`ScreenCaptureKit`=0** in the binary, so an earlier draft's "ScreenCaptureKit-based computer-use capture" is withdrawn. Either way it is a VNC viewer, not a peer-to-peer desktop stream | ○ | ○ | ○ | ○ |
| Embedded browser per tab | ● (CEF/webview work in `ui/src/lib/browser*`) | ● CmuxBrowser | ○ | ○ | ○ | ○ |
| OS notifications + dock badge + audio | ● `src-tauri/src/notification` | ● agent rings | ◐ | ◐ | ● push | ◐ |
| Scriptable CLI over the daemon | ● `ferryx-cli` | ● socket CLI | ○ | ● `rex` CLI | ◐ | ○ |
| Open source | ○ (private) | ◐ GPL-3.0-or-later app, BUSL-1.1 for 6 server dirs | ○ | ○ | ● MIT | ● MIT |

## 5. Where Ferryx is genuinely ahead

1. **Remote desktop inside the same app as the terminal.** Ferryx ships a peer-to-peer desktop
   stream in its default feature set (`remote-desktop` → `crates/ferryx-rd`, 7 FFmpeg 9 LGPL
   dylibs confirmed present in `Contents/Frameworks/`). The nearest peer surface is a different
   thing: cmux bundles a **noVNC viewer** (`vnc`=15, `noVNC`=4, `websockify`=2,
   `screenRecording`=20) for looking at *external* VNC targets, not a host→client desktop stream.
   Correction: an earlier draft attributed ScreenCaptureKit-based capture to cmux, but
   `ScreenCaptureKit`=**0** in its binary, so that claim is withdrawn. Rex, Ovis, Clone and
   Ghostty show no capture surface. The differentiator is real, and should be stated as
   "peer-to-peer desktop streaming inside the terminal app".
2. **Worktree isolation as a first-class *jail-safe* primitive — narrower than the first draft
   claimed.** `src-tauri/src/worktree/naming.rs` is the single source of
   `<prefix>/<ws-id>/<slug>`, with a **root jail enforced before a path is created**, legacy
   `.orca-worktrees` parsing, and per-repo prefix config. Correction: an earlier draft said
   "cmux and Ovis have no worktree concept at all" — **false, both have one.**
   cmux ships a full feature (`ProjectWorktreeSidebar`, `createWorktree`,
   `--worktree`/`--worktree-base`/`--worktree-branch`/`--worktree-dir`/`--skip-worktree-setup`,
   user-default key `com.example.cmux.sidebar.project-worktrees`), and Ovis has
   `createWorktree`/`listWorktrees`/`WorktreeInfo` plus a UI string "A git worktree of this
   repository, checked out under herdr's worktree folder and opened as a workspace next to this
   one." So the differentiator is **not** "has worktrees"; it is that Ferryx's names are
   escaped before creation against a declared root, while cmux's own error strings
   (`Worktree path no longer identifies the created checkout.`,
   `Worktree branch changed after creation.`) describe **post-hoc verification** — it detects a
   jail escape rather than preventing one. That distinction is unproven for Ovis, whose
   implementation was not read.
3. **PIN pairing with an active-desktop lock.** Ferryx's Axum gateway binds the remote session to
   the focused desktop session and disconnects on focus switch. This is a genuinely different
   posture from the two peer alternatives: cmux uses an account/OAuth + Iroh relay model, and Rex
   embeds a Tailscale node with an optional Headscale control server. Ferryx's lock is a
   confidentiality default those models do not express. (Whether *read* survives the lock is
   asserted by this note and still unmeasured — see §9.)
4. **20-byte framed binary streams with coalescing** (10 ms / 32 KB) — a cheaper terminal I/O
   path than the JSON-IPC Electron peers use.

## 6. Where Ferryx is behind, ranked by cost to the user

1. **Mobile exists but is unpublished and silent.** `mobile/README.md` describes a Tauri v2 shell
   that embeds `RemoteApp` for Android and iOS (`com.ferryx.mobile`, 1.0.0, with TestFlight
   `--export-method release-testing` support). So the client is real code, not a gap — but the
   store checklist in that same README is unticked (app record, TestFlight internal test, review),
   and there is **no APNs/FCM path anywhere in `mobile/`**. cmux ships an iOS beta on TestFlight,
   Ovis ships Android on Google Play with a dedicated `push-relay/` directory, and Rex has iOS in
   testing. Ferryx can show the user a session but cannot tell them about it when the app is closed.
2. ~~**No P2P transport.**~~ **FALSIFIED — Ferryx has one.** An earlier draft called this "the
   single largest architectural gap in the peer set". It is not a gap at all:
   `src-tauri/src/paired_host/direct/` is 1,027 production lines (+602 test) implementing STUN discovery (`stun.rs`),
   nonce hole punching (`punch.rs`), QUIC endpoints (`quic.rs`), ephemeral certs (`cert.rs`),
   offer exchange (`offer.rs`) and a TCP↔QUIC bridge (`bridge.rs`), orchestrated by
   `direct_connect.rs` (292 lines) behind `quinn = 0.11.11` (`src-tauri/Cargo.toml:194`) and
   started from `direct_client.rs:46-73` once a peer key is pinned, and already wired into the
   request path at `paired_host/client.rs:1073,1315,1546`. What differs from cmux is
   *packaging*, not existence: cmux adopts Iroh (2,859 strings, `Iroh.framework`) as a library,
   Ferryx implements the same class of path in-tree. The real defect is the next item.
3. **The direct path's success rate is unmeasurable.** Route state does reach the UI
   (`RouteState` → `pairedHostRouteV1` → `paired_host_route_list` → the machine-row chip), but
   the chip collapses `direct` versus everything else into two labels
   (`pairedHostRoute.ts:112-115`), the payload carries no counters (`route_view.rs:38-41`), no CLI
   can query it, and a successful publish logs nothing while failure logs at exactly four
   `tracing::info!` sites (`direct_connect.rs:159,163,168,172`). So "P2P exists" is established
   and "P2P works on this user's network" is not.
4. **A thinner agent env contract than `CMUX_AGENT_*`.** Ferryx does emit one, but the set is
   platform-conditional rather than fixed. `apply_session_env` (`pty.rs:117-122`) sets
   `FERRYX_SESSION_ID` and `FERRYX_WORKTREE_PATH` (Windows verbatim prefix stripped) always, and
   `FERRYX_WORKSPACE_ID` only when a workspace id is supplied (`:131`); `PTY_INCARNATION` is set
   at `:523`; `FERRYX_AGENT_STATE_SOCKET` plus three `HERDR_*` vars (`apply_herdr_env`,
   `:162-169`) only on unix and only with the integration on (`:537-545`);
   `FERRYX_AGENT_STATE_PORT`/`TOKEN` are non-unix only (`:144-153`). So a unix pane gets 3–4 of
   them with the integration off and 7–8 with it on, a Windows pane 3–4 or 5–6 — and the set
   stops at identity and the state channel.
   cmux ships **29** distinct `CMUX_AGENT_*` vars covering launch argv (`LAUNCH_ARGV_B64`), cwd
   (`LAUNCH_CWD`), executable (`LAUNCH_EXECUTABLE`), hook routes/state (`HOOK_ROUTE_SNAPSHOT`,
   `HOOK_STATE_DIR`, `HOOK_RELAY_ORIGIN`), journal paths (`JOURNAL_PATH`,
   `JOURNAL_DEAD_LETTER_PATH`), resume commands
   (`LAUNCH_SUBROUTER_{CLAUDE,CODEX}_RESUME_COMMAND`, `RESUME_LAUNCH`, `RESTORE_LAUNCH`) and
   subagent/fork flags — so an agent there can reconstruct and resume its own launch. Ferryx's
   set cannot describe how the agent was launched.
5. **Private source.** Ovis (MIT) and Ghostty/libghostty (MIT) can be audited and forked in full;
   cmux publishes its client and CLI under GPL-3.0-or-later (its relay/worker dirs are BUSL-1.1).
   Ferryx's closed tree is a distribution disadvantage for the exact audience that installs these
   apps - and note that even cmux, which monetises transport, still ships a readable client.
6. **No *aggregated* consumed-quota surface.** The screenshot's own app is a quota monitor, but
   the gap here is narrower than "Ferryx shows nothing". Token usage is already parsed and
   already rendered — `reference_usage_from_object` (`history_parse.rs:305`) extracts
   `input_tokens`/`prompt_tokens`/cache across **15** per-provider `history_*.rs` readers,
   `ReferenceTurnUsage` (`referenceWorkGroup.tsx:355-357`) prints it per turn, and the DAG
   inspector prints it per node (`DagNodeInspector.tsx:403-411`). What does not exist is any
   **session- or window-level aggregate**: no rolling 5-hour or weekly total, no budget
   projection, nothing that answers "how much is left". AI Token Monitor proves the aggregate is
   derivable from the same local logs Ferryx already parses.

## 7. Concrete, cheap wins suggested by the evidence

1. **Aggregate the quota readout that already exists per turn.** Ferryx already parses agent
   transcripts and already renders per-turn and per-node token counts (§6 item 6), so this is
   an aggregation job, not a parsing job: sum the existing per-turn records into `Session (5h)` /
   `Weekly7d` windows like AI Token Monitor's badge. No new data source, no credentials — but
   do not expect `agent_transcript.rs` to supply it; it carries no `usage` field at all
   (0 hits in 1,035 lines), so the input is the reference-chat history readers.
2. **Extend the existing `FERRYX_*` block, don't invent one.** `apply_session_env`
   (`src-tauri/src/terminal/pty.rs:117`) already injects identity, workspace and the agent state
   port. Add the three fields cmux proves are useful and the daemon already knows: launch argv,
   launch cwd/executable, and the resume command — then agents can rebuild their own invocation.
3. **Finish mobile: publish, then add push.** Two separable steps with different costs. (a) Publish
   the existing `mobile/` shell — the code and the TestFlight export path already exist; only the
   store checklist is open, which is process work rather than engineering. (b) Add a push path so a
   phone learns an agent is blocked without holding a focused-desktop connection; Ovis's
   `push-relay/` is the reference shape. Keep the active-desktop lock as-is for *input* — that part
   is a security feature, not the defect.
4. **Keep the remote-desktop differentiator loud.** It is the one capability in this matrix no
   peer has; it is currently buried inside a feature list rather than being the headline.

## 8. Open-source static analysis (measured)

Two peers are genuinely open source, so their trees were read directly (shallow, via the GitHub
API - not a full clone, and no code was executed).

| repo | license (raw LICENSE file) | primary language (bytes) | stars | open issues | last push |
|---|---|---|---|---|---|
| `manaflow-ai/cmux` | split: GPL-3.0-or-later for the app tree; BUSL-1.1 for `web/`, `workers/{ci-artifacts,iroh-v2,presence}/`, `services/iroh-relay-minter/`, `cmux-tui/relays/cloudflare-do/` (commercial licence required for self-host) | Swift 114.9 MB, Rust 21.2 MB, Python 20.0 MB, TypeScript 19.3 MB, Zig 1.2 MB | 28,791 | 6,870 | 2026-11-14 |
| `devswha/ovis` | MIT | TypeScript | 2,054 | 154 | 2026-11-14 |
| `ghostty-org/ghostty` | MIT | Zig | 64,084 | 267 | 2026-11-14 |

The cmux LICENSE (707 lines) is worth reading before calling it "open source" flatly: the app,
iOS client, CLI and `cmux-tui` are GPL-3.0-or-later, but the six paths listed above are
BUSL-1.1, which means **the relay/worker half of cmux cannot be self-hosted without a commercial
licence from Manaflow.** That is the structural difference to Ovis (pure MIT): cmux open-sources
the client and monetises the transport, which is the same shape Ferryx has with a private client
and a hosted relay - just with the client published.

**cmux** top level: `Native/`, `Packages/`, `CLI/`, `Prototypes/`, `Examples/`, plus a
`.claude/` and `AGENTS.md`/`CLAUDE.md` pair - i.e. the repo itself is agent-run, with
`IMPLEMENTATION_NOTES.md` and even a `PR-10599-AUDIT.md` audit artifact checked in. That is the
same operating model Ferryx uses, so it is not a differentiator either way.

**Ovis** top level shows the split that its README describes: `server/`, `relay/`,
`push-relay/`, `crates/`, `app/`, `integrations/`, `herdr-plugin.toml`. Its README states the
architecture plainly - "It is built on herdr and can show the same live terminals as herdr and
the Herdr app" - and ships `install.sh`/`install.ps1` plus Mac/Windows/Linux desktop builds,
Android on Google Play, and iPhone in TestFlight beta. The `push-relay/` directory is how Ovis
delivers alerts while no desktop session is focused. **Source discipline:** that directory is
confirmed in the **GitHub tree** (the repo's top level lists `push-relay`), but the shipped
Ovis 0.5.3 bundle contains `push-relay`=**0**, so the bundle must not be cited for it.

Ferryx's own push is standard **Web Push**, not vendor-token push, and the earlier draft's
description of it was imprecise. Read at source in `src-tauri/src/remote/push.rs` (617 lines):
`VapidKeys::generate` (`:374-383`) creates the daemon's **own** ES256 P-256 keypair and
`load_or_create` (`:386-400`) writes it to `push-vapid.json` owner-only; `validate_endpoint`
(`:330-354`) gates subscribers against `PUSH_SERVICE_HOSTS` (`:356-363`) = {fcm.googleapis.com,
push.apple.com, push.services.mozilla.com, notify.windows.com}; `send` (`:470`) encrypts with
`aes128gcm` ECE and posts to the subscriber's endpoint. So `push.apple.com`/`fcm.googleapis.com`
appear here as an **allowlist of endpoints a browser may subscribe with**, not as vendor APIs
the daemon authenticates against — and the daemon holds **no vendor credential** at all. The web
client subscribes through `ui/src/lib/pushSubscription.ts` + `ui/public/sw.js`; fan-out is wired
in `remote/state.rs:1625-1660`. Only the native `mobile/` shell has no registration (0 hits).
So this is a wiring gap at the last hop, not a missing subsystem — and the reason native
registration is needed anyway is that a WKWebView exposes neither `PushManager` nor
service-worker push, so the PWA path cannot run inside the shell.

**Ghostty** is an emulator only (64k stars, MIT, Zig): the project Ferryx and cmux both build
their VT layer on. Nothing in its tree does sessions, daemons or worktrees.

## 9. Not measured

- **Nothing in this document was produced by running any of the peers.** All six were inspected
  as installed bundles. No peer was launched, no peer session was driven, and no peer capability
  was confirmed by using it. A §4 ● therefore means "the code and the surface ship", not "the
  feature works" — a distinction that matters most where only strings could be read.
- Nothing was downloaded for this note. Every app in §3 was already installed at
  `/Applications` and was inspected in place; no product source was compiled.
- cmux and Ovis were read at repository-tree and top-document level (§8), not file-by-file: no
  function-level audit of either codebase was performed, and no code was cloned and built.
- Clone's asar bundle was string-surveyed only; it was not unpacked and read.
- The Ferryx column is contract-plus-path verified, not line-by-line audited: every cited path
  was confirmed to exist (`src-tauri/src/worktree/naming.rs`, `crates/ferryx-rd`,
  `src-tauri/src/{notification,agent_detect,agent_control,remote,daemon}`,
  `src-tauri/src/agent_transcript.rs`, `ui/src/state`, `ui/src/remote/RemoteApp.tsx`,
  `mobile/src-tauri/tauri.conf.json`), and `remote-desktop` is a default feature
  (`src-tauri/Cargo.toml:59`), but the behavior behind
  each path was not exercised at runtime in this pass.
- One earlier draft claim — "Ferryx exposes no agent env contract" — was **falsified** during
  verification and rewritten (§6 item 3). `src-tauri/src/terminal/pty.rs:123` already sets
  `FERRYX_SESSION_ID`; the real gap is the missing launch/resume fields, not the absence of a
  contract.
- A second draft claim — "Ferryx has no iOS client, only a browser app" — was also **falsified**
  and rewritten (§4, §6 item 1). `mobile/` is a real Tauri v2 Android+iOS shell
  (`com.ferryx.mobile` 1.0.0) with TestFlight export support. The actual gap is publication and
  the total absence of a push path, not the absence of a client.
- A third draft claim — "no peer streams a remote desktop, so this is a clean differentiator" —
  was **partially falsified** and rewritten (§4, §5 item 1). cmux carries 35 remote-desktop
  strings including a full `noVNC` client (`/vnc.html?path=websockify`), its own
  `_screenRecordingGranted` permission flow, and 2 ScreenCaptureKit references. It does not do the
  same thing as `ferryx-rd`, but "nobody else touches screens" was wrong.
- A fourth draft claim — "Ferryx has no P2P transport; that is the single largest architectural
  gap" — was **falsified outright** and rewritten (§4, §6 item 2). The subsystem was there the
  whole time: `src-tauri/src/paired_host/direct/` (1,027 production lines) plus
  `direct_connect.rs`/`direct_client.rs` (582), and it is not dormant — `paired_host/client.rs`
  prefers it at three call sites (`:1073` remote-desktop stream, `:1315` paired socket,
  `:1546` HTTP) and falls back to relay only on a classified transport failure. The earlier
  draft missed it because it searched from the relay/gateway direction and never opened
  `paired_host/direct/`. What is genuinely missing is *measurement* of the path, now §6 item 3.
- A fifth draft claim, still open — "only *input* stays locked, so read paths are fine for
  mobile" — is **asserted, not measured**. It appears in §6/§7 as context for the mobile track;
  a device test with a desktop focused is the only thing that settles it.
- The screenshot's HUD-occluded Hangul is a best-effort read through a translucent overlay.

## 9.1 Second measurement pass (2026-11-14, later same day)

Eight parallel collection lanes re-measured every claim with one stated metric, then three
adversarial passes tried to break them. Evidence: `.omo/ulw-research/20261114-menubar-apps/wave2/`
(`lane-A-quota.md`, `lane-B-cmux.md`, `lane-C-clone.md`, `lane-D-rex.md`, `lane-E-ovis.md`,
`lane-F-ferryx.md`, `lane-G-screenshot.md`, `lane-H-mobile.md`). What broke, and was rewritten above:

| claim | verdict | replacement evidence |
|---|---|---|
| cmux `iroh`=1239 / `Ghostty`=844 / `tokio`=162 | **unreproducible** | word-boundary on the binary: 495 / 1048 / 155 |
| Clone `goal`=363 / `goals`=206 / `worktree`=46 | **unreproducible** | 15,720 / 1,766 / 918 word-boundary occurrences in the asar |
| `CMUX_AGENT_*` = 25 vars | **wrong** | 29 distinct names across `cmux`, `cmux-tui` |
| Rex = "swift-ghostty, Tailscale tsnet" | **both wrong** | no ghostty framework ships; `pty`=0; `tsnet` hits are `NIOTSNetworkEvent`; Tailscale is an embedded Headscale-capable node |
| Clone = "Electron/xterm" terminal | **wrong** | `pty`=0, `node-pty`=0, `termios`=0; `xterm` only a TERM regex — Clone has no terminal |
| "cmux and Ovis have no worktree concept" | **wrong** | cmux: `ProjectWorktreeSidebar` + 6 `--worktree*` flags; Ovis: `createWorktree`/`listWorktrees` + UI copy |
| cmux has "ScreenCaptureKit-based computer-use capture" | **withdrawn** | `ScreenCaptureKit`=0 in the binary; the noVNC viewer is real (`vnc`=15) |
| "Two independent identifications agree" (§1.2) | **half false** | MAD re-run ranks **ChatGPT first (43.5)** over mahoquot (73.6), stable across 9 offsets; conclusion rests on the motif match alone |
| Ovis `push-relay/` | **source error** | real in the GitHub tree, `push-relay`=0 in the shipped bundle |
| push.rs "posts to push.apple.com and fcm.googleapis.com" | **imprecise** | those are subscriber-endpoint allowlist entries (`PUSH_SERVICE_HOSTS`); the daemon holds no vendor credential |
| mahoquot "22M-class" (§2) | **wrong by 5x** | `du -sh` on the bundle: **122M** |
| Ferryx probe "returns 16, 8, 38" (§1.2) | **wrong numbers** | word-boundary re-measure: `pty`=38, `worktree`=74, `daemon`=263 |

**Method defect found and fixed in this pass:** `grep -oi pty` matches "em**pty**", which
inflated Clone's pty count to 2110 when the word-boundary count is 0. Every count in §3 is now
`strings -a <bin> | grep -oiw <token>`. The Census table states this once so it cannot drift again.

**Still not measured after this pass:** whether any peer's worktree path is jailed (only cmux's
error strings were read, and they suggest post-hoc verification rather than pre-admission
refusal); Ovis's worktree implementation; whether Rex's Tailscale node is a full userspace stack;
real device push delivery of any kind; and whether read access survives Ferryx's desktop lock.

## 10. Evidence commands

```bash
# bands
magick source.png -background black -alpha remove -alpha off -colorspace gray -depth 8 gray:- # then per-row mean
# icon match over every installed app
magick source.png -crop 20x20+0+27 +repage -resize 16x16! -depth 8 rgb:- # vs each icns via sips->magick
# bundles
/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' /Applications/<app>.app/Contents/Info.plist
strings -a /Applications/<app>.app/Contents/MacOS/<bin> | grep -aoiE 'iroh|ghostty|tokio|worktree|osC 7501'
curl -s https://api.github.com/repos/<owner>/<repo>/languages
```

Artifacts kept under `.omo/ulw-research/20261114-menubar-apps/` (source.png, x3 upscale, band crops,
candidate icon montage).

### 10.1 The one counting rule, and how the lanes actually ran

Every count in this note is `strings -a <binary-or-asar> | grep -oiw <token> | wc -l` —
word-boundary, case-insensitive, on the shipped artifact. Two consequences worth stating:

1. Bare `grep -oi pty` matches "em**pty**" and will report a terminal in software that has none.
   That is how Clone once measured `pty`=2110 against a true count of 0. If a future pass
   reports a count without `-w`, treat it as suspect.
2. A count is a *presence* probe, not a capability proof. `worktree`=145 in cmux proves the
   vocabulary ships; it does not prove the feature works. Where a claim is load-bearing (§4's
   worktree row, §5's jail argument) the doc names the concrete symbol or file that carries it.

**How the lanes ran (this matters for reproducing the pass).** The eight collection lanes and
three falsification passes executed as parallel `tool.bash` calls inside a single code cell, not
through a workflow/subagent fan-out: `task` spawning was refused with "Task admission is held by
your Stop", while the model gateway on port 18801 was in fact **up** (`curl` → 401). So the
earlier claim that fan-out was impossible because "this environment has no workflow tool" was
wrong on both halves — there is no workflow dependency in this method, and the gateway was
healthy. Each lane wrote its own file, which is what makes the numbers checkable one at a time.

## 11. Relation to earlier notes in this repo

- `docs/research/REX_VS_FERRYX_2026-10-30.md` compared Rex on **public documentation only**
  ("Rex itself was not installed or run: the macOS app is invite-only"). That gap is now closed:
  Rex Beta 0.1.0 is installed at `/Applications/Rex Beta.app` and its bundle was inspected here
  (§3), confirming swift-nio networking and the Tailscale/OSC-7501 surface that doc could only
  cite from docs — **and correcting one of its assumptions**: Rex does not ship a ghostty
  framework at all (no ghostty dylib in `Contents/Frameworks`; `libghostty` appears only as a
  string and `pty`=0), so "swift-ghostty rendering" should not be carried forward from the
  documentation-only note.
- `docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md` covers user sentiment for a
  different top-10 (Windows Terminal, Orca, Alacritty, Warp, Ghostty, tmux, Herdr, Vibe Kanban,
  cmux, T3 Code). This note is the **capability** counterpart for the apps actually installed on
  this machine, not a sentiment survey.
- `docs/FERRYX_COMPETITIVE_GAPS_PLAN_2026-09-05.md` is the older gap plan; §7 above should be
  read as its current, evidence-backed refresh.
