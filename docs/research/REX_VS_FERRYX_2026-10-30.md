# Rex (Superlogical) vs Ferryx - 2026-10-30

Source basis: the public Rex docs (`https://www.superlogical.com/rex/docs/*.md`, 21 pages, fetched
2026-10-30) and the Ferryx source at the time of writing. Rex itself was **not** installed or run:
the macOS app is invite-only (mailing-list waves) and every package URL answers
`401 Rex packages require a token`. Everything below about Rex is documented behavior, not measured.

## What Rex is

- Terminal multiplexer by Superlogical (Mitchell Hashimoto, Jack Pearkes, Alasdair Monk, Hector Simpson).
  Public beta started 2026-10-30, macOS app only; Linux/FreeBSD servers go to separate testing waves.
- One `rex` binary is both server and CLI. The macOS app embeds a local server and can connect to
  other Rex servers. Clients for iOS, Linux, Windows exist but are not in testing yet.
- Free forever, no account, no telemetry, fully self-hostable. Commercialization not announced.

## Feature comparison

| Area | Rex (documented) | Ferryx (source) |
|------|------------------|-----------------|
| Session server | `rex server` per user; sessions survive client close | `ferryx --daemon` UDS v5, launchd-owned on macOS |
| Server upgrade without losing sessions | Personal/login servers keep the **old version** running until sessions end; shared server upgrade **kills all sessions** | `upgradeBinary` rolling handover moves live PTYs to the new daemon |
| Server modes | personal, shared (`rex` group), login server (root, PAM, per-account servers) | per-user daemon only |
| Remote access | Unix socket only by default; built-in Tailscale (tsnet), Tailscale Serve, tailnet address, or anonymous HTTP. No real auth yet ("SSH keys planned") | relay (relay.ferryx.dev) with PIN/account pairing, SSH remote hosts, Axum gateway |
| Clients | macOS native (beta), web, iOS planned | Tauri desktop (macOS/Windows/Linux), remote web/mobile client |
| Live session sharing | Built in "from the start" (marketing page) | Remote client bound to the active desktop session; no multi-user sharing |
| Rendering | Ghostty lineage (author), native scrollback/selection | libghostty-vt + WGPU native surfaces |
| Automation surface | `rex` CLI, self-describing server API (`rex api list/describe/call`, JSON Schema in/out), Lua 5.1 scripts (`rex do`), Lua event subscriptions (`rex.on`) | Tauri IPC + daemon protocol; `src-tauri/src/cli` holds agent hooks; no public scripting API or schema discovery |
| Agent integration | `rex skill setup` installs a skill + session-start hook for Claude Code and Codex (skill-only for OpenCode, Pi); agents can split panes, read screens, drive the app | Agent detection manifests (`src-tauri/src/agent_detect/manifests`), agent hooks (`src-tauri/src/cli/agent_hook.rs`), supervisor/hub |
| Program status | **OSC 7501** open protocol: `state=idle/working/blocked/done/failed`, `kind=permission`, base64 `msg`/`title`, hierarchical record ids. Drives session picker badges and tab spinners | Per-agent heuristics/hooks; no OSC 7501 parser (grep: 0 matches); OSC 9;4 only mentioned in grok manifest |
| Config | Single Lua `init.lua` (keys, key sequences, key modes, custom actions, servers) | Settings UI + JSON |
| Git worktrees | Not mentioned | Core feature (`.ferryx-worktrees/`) |
| Platforms (server) | macOS, Linux (deb/rpm/arch/apk/Nix + NixOS/home-manager modules), FreeBSD; Windows server exists, not in test | macOS, Windows, Linux |

## Where Ferryx is ahead

1. **Zero-loss daemon upgrade.** Rex avoids upgrade loss by leaving old servers running until idle; its
   shared server explicitly drops every session on upgrade. Ferryx's handover replaces the daemon live.
2. **Worktree-first workflow.** Rex has no repo/worktree model at all.
3. **Remote reach without Tailscale.** Rex's remote story today is Tailscale or anonymous HTTP; Ferryx's
   relay pairing works for hosts with no tailnet (e.g. daas).
4. **Windows desktop client today.** Rex's Windows client is not in testing.

## Where Rex is ahead / worth adopting

1. **OSC 7501 Program Status Protocol** - a generic, terminal-agnostic status escape. Highest-value
   item: Ferryx could parse it in the PTY output path and feed the existing agent-status UI, replacing
   per-agent heuristics for any program that adopts it. Cheap to support, and being early on a
   Hashimoto-backed protocol keeps Ferryx compatible with whatever agents start emitting.
2. **Self-describing automation API** (`api list/describe/call` with JSON Schema). Ferryx's daemon
   protocol is typed but not discoverable from a CLI.
3. **Agent skill installer** (`rex skill setup`): one command writes the skill + hook into
   `~/.claude`, `~/.codex`, `~/.agents`. Ferryx has the hooks but not a one-shot installer surfaced to users.
4. **Multi-user server modes** (login server via PAM / Tailscale identity) and live session sharing.
5. **Packaging breadth**: apt/dnf/zypper/pacman/apk repos, Nix flake, NixOS + home-manager modules,
   `xterm-rex` terminfo with an SSH `xterm-256color` fallback.

## Not verified (needs a Rex invite)

- Actual remote latency/responsiveness vs Ferryx relay.
- Whether the macOS app survives app quit / server crash the way the docs imply.
- Rendering quality and input latency side by side.
- Session sharing UX (only claimed on the landing page, no docs page yet).

To settle these: join the waitlist at https://www.superlogical.com/updates/public-testing-beginning ,
install the invited macOS build on a remote Mac (not the primary MacBook), and run the same
scenarios as the Ferryx remote QA.

## Benchmark plan (re-analysis of "worth adopting")

Priority order: 1 > 3 > 2 > 4 > 5. Items 1 and 3 reuse machinery Ferryx already has; 2 is a
contract decision; 4 conflicts with a current security invariant; 5 is not worth it now.

### 1. OSC 7501 Program Status Protocol - adopt (highest value, smallest change)

Current Ferryx path (source):
- One status vocabulary already exists: `AgentControlState { Working, Blocked, Idle, Unknown }`
  with `AgentStateSource { Provider, Screen, None }` and `derive_status` preferring a
  self-reported provider over the screen (`src-tauri/src/agent_control/mod.rs`).
- Self reports only arrive through the Ferryx agent extension/hooks as `AgentStateReport`
  frames over the UDS (`daemon/protocol.rs:1641`), i.e. only for agents Ferryx installed hooks into.
- Everything else is screen guessing: `daemon/agent_observer.rs` feeds PTY bytes into a headless
  `NativeTerminal` and runs per-agent TOML regex manifests (`agent_detect/manifests/*.toml`)
  after debounce/quiet timers. Grok's OSC 9;4 rules were dropped because the engine has no
  `osc_progress` region. There is no OSC 7501 handling anywhere (0 matches).

Gap: any program that is not a hooked agent (builds, deploys, terraform, our own supervisor/mhc
runs) can only be guessed from the screen, with timer lag.

Design:
- Parse OSC 7501 from the PTY byte stream in the observer loop (`incoming = output.recv()`
  branch, before `engine.feed`). Pure byte parsing, so it is cross-platform by construction.
  It must survive chunk splits and replay (`ReplayGap` / `rebuild`), so keep a small carry buffer.
- Map `state`: working -> Working, blocked -> Blocked (+ `kind`, decoded `msg` into
  `detail` so the inbox shows the question), idle/done -> Idle, failed -> Idle plus a failure
  flag. Add `AgentStateSource::Program` (or reuse Provider with an origin tag) and rank it with
  self-reported provider frames in `derive_status`; a prompt on screen still forces Blocked.
- Honour the spec's limits, the "each report replaces its record" rule and hierarchical ids;
  surface only the root record in v1.
- Feature detection (`OSC 7501 ; ? ST`) needs a reply written to the PTY. The observer engine
  runs with PTY writes suppressed, so the reply has to come from the one live surface; decide the
  owner before shipping (otherwise do not answer, which the spec allows programs to tolerate).
- Emit it ourselves too: Ferryx bundles mhc and the supervisor, so emitting OSC 7501 there makes
  them visible in Rex and any other adopting terminal.

Benchmark:
- Spec conformance: unit fixtures from every example in the spec (split chunks, bad base64,
  control chars, over-limit, repeated keys, unknown keys).
- Time-to-status: same scripted program run (a) emitting OSC 7501 and (b) screen-only. Measure
  PTY write -> status event latency; target: OSC path well below the debounce/quiet timers.
- Cross-terminal: once invited, run the same emitter inside Rex and Ferryx and compare which UI
  states appear (session picker badge / tab spinner vs Ferryx sidebar/inbox).

### 3. Agent skill installer - adopt the skill half; Ferryx is already ahead on hooks

Current Ferryx path: `daemon/agent_hook_install.rs` already writes idempotent hooks for claude
(`~/.claude/settings.json`), codex (`~/.codex/hooks.json`), antigravity
(`~/.gemini/config/hooks.json`) and cursor, gated by the Agent Integration setting, and can apply
them to a remote file over SSH. Rex covers claude/codex hooks and skill-only for OpenCode/Pi, local only.

Gap: no skill that teaches an agent how to drive Ferryx (open pane, split, read another pane,
wait for a status), and no single user-facing `ferryx skill setup`/`--dry-run` command.

Design: ship a `ferryx` skill (SKILL.md generated from the binary, so upgrades update it, as Rex
does) installed next to the existing hooks by the same installer; it depends on item 2 for the
commands it documents.

Benchmark: Rex's own six agent tasks (open editor + narrow pane, start dev server and report when
listening, run tests beside you and fix, type into a REPL pane and read the result, switch the
desktop to this session, close what you opened). Run each with Claude Code in Ferryx with and
without the skill; record success, turns and tokens. Repeat in Rex after the invite.

### 2. Self-describing automation API - adopt as a curated facade, not the raw protocol

Current Ferryx path: `DaemonRequest` (`daemon/protocol.rs:620`) is a large serde-tagged enum
used as an internal, versioned (v5) wire protocol; no schema derivation (`schemars`/`specta` not
in Cargo.toml). The `ferryx` CLI (`cli.rs`) exposes browser, pair, supervisor and billing
commands, but no terminal/session/pane commands.

Recommendation: do not publish `DaemonRequest` as-is; that freezes internal handover/epoch
details into a public contract. Add a small stable facade (sessions, panes, split, send input,
read screen, wait for status, set label) with `ferryx api list | describe | call`, JSON Schema in
and out, typed error codes (matches the existing structured `{code,message,details}` rule).
Lua scripting is out of scope; JSON over the CLI is enough for agents and shell scripts.

Benchmark: every Rex agent task in item 3 must be doable through the facade; schema discovery
parity check: `describe` output must be enough for an agent to form a valid call first try.

### 4. Multi-user modes and live sharing - defer, design only

Current Ferryx path: the remote gateway deliberately binds to the active desktop session and
refuses others ("Forbidden: session is not the active desktop session", `remote/server.rs`).
Sharing a live session with a second person is the opposite invariant. Team seats already exist
in billing (`TEAM_MIN_SEATS`), so team sharing fits monetization.

Plan: wait for the Rex invite and observe how sharing works (read-only vs read-write, identity,
who can type). Then write a design for a relay-mediated read-only share link first, as an
explicit exception to the active-desktop lock, before any code. No PAM/login-server equivalent:
Ferryx's account/relay identity already covers that need.

### 5. Packaging and terminfo - skip for now

Ferryx already ships dmg, nsis, msi, appimage, deb, a Homebrew cask and install.sh/install.ps1,
with the Tauri updater handling upgrades. PTYs use `TERM=xterm-256color` (`terminal/pty.rs`),
so SSH to unknown hosts already works, which is the problem Rex's `xterm-rex` fallback exists to
solve. Revisit rpm/Arch/Nix only on user demand.

### Side-by-side measurement once the invite arrives

Run on a remote Mac, never the primary MacBook:
1. Status latency with the same OSC 7501 emitter (item 1).
2. The six agent tasks (item 3).
3. App quit/relaunch and server crash: are sessions and scrollback intact.
4. Upgrade: Rex personal server upgrade vs Ferryx `upgradeBinary` handover, session count before/after.
5. Remote typing latency: Rex over Tailscale vs Ferryx over relay and SSH, same host pair.
