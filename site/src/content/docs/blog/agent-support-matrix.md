---
title: "CLI Agent Support Matrix: Who Works Where"
description: "Claude Code, Codex, Antigravity, OpenCode and others: status detection, resume, hooks, known quirks, and plain shells still work. Built for parallel AI agent workflows."
---

**CLI Agent Support Matrix: Who Works Where.** Claude Code, Codex, Antigravity, OpenCode and others: status detection, resume, hooks, known quirks, and plain shells still work. Built for parallel AI agent workflows.

![agent-support-matrix cover](/images/blog/agent-support-matrix/cover.png)

## Per-agent support

Every agent below ships a status-detection manifest, so its pane reports working, waiting on you, or idle. The table is generated from those manifests by `scripts/gen-site-agents.mjs`; the agent names, links, and count come straight from `src-tauri/src/agent_detect/manifests/`, so it cannot drift from what the app actually detects. Ferryx currently detects <!-- agents:count -->16<!-- /agents:count --> agents.

<!-- agents:matrix -->

| Agent | Status detection | Resume | Hooks | Remote history | Managed chat |
| --- | --- | --- | --- | --- | --- |
| [Aider](https://aider.chat) | Yes | No | No | No | No |
| [Antigravity](https://antigravity.google) | Yes | Yes | Yes | Yes | No |
| [Claude Code](https://www.anthropic.com/claude-code) | Yes | Yes | Yes | Yes | No |
| [Cline](https://cline.bot) | Yes | Yes | Yes | Yes | No |
| [Codex](https://github.com/openai/codex) | Yes | Yes | Yes | Yes | Yes |
| [Crush](https://github.com/charmbracelet/crush) | Yes | Yes | Yes | Yes | No |
| [Cursor CLI](https://cursor.com/cli) | Yes | Yes | Yes | Yes | No |
| [Droid](https://factory.ai) | Yes (blocked states only) | Yes | No | No | No |
| [GitHub Copilot CLI](https://github.com/features/copilot/cli) | Yes | Yes | No | Yes | No |
| [GJC](https://github.com/Yeachan-Heo/gajae-code) | Yes | Yes | No | Yes | No |
| [Grok](https://x.ai/build) | Yes | Yes | Yes | Yes | No |
| [Kimi](https://github.com/MoonshotAI/kimi-cli) | Yes | Yes | No | Yes | No |
| [OmO](https://github.com/code-yeongyu/oh-my-openagent) | Yes | Yes | No | Yes | No |
| [omp](https://github.com/can1357/oh-my-pi) | Yes | Yes | No | Yes | No |
| [OpenCode](https://opencode.ai) | Yes | Yes | No | Yes | No |
| [pi](https://github.com/badlogic/pi-mono) | Yes | Yes | No | Yes | No |

<!-- /agents:matrix -->

What each column means, and where it is sourced:

- **Status detection.** A shipped manifest reads the pane's screen. Droid's manifest covers only its login and tool-confirmation screens, so a Droid pane reports blocked but never working or idle.
- **Resume.** Ferryx can reopen the agent's own session after a restart with that agent's resume flag (`ui/src/lib/agentResume.ts`).
- **Hooks.** Ferryx installs the agent's native lifecycle hooks, so state arrives from the agent itself and the screen read is the fallback (`src-tauri/src/ferryx_scope/ssh/agent_hook_core.rs`).
- **Remote history.** The remote client can read the agent's transcript store to show past turns (`src-tauri/src/remote/reference_chat/history_*.rs`).
- **Managed chat.** Ferryx drives the agent through its app-server protocol instead of a terminal. Only Codex is supported (`src-tauri/src/ferryx_scope/chat/mod.rs`).

## Agents without a manifest

Any other command still runs in a pane: a test runner, a migration script, or an agent that is not in the table. It just runs without a status read, so the pane shows output but no working, waiting, or idle badge. Google's Gemini command-line tool is one of these: it runs in a Ferryx pane like any command, but has no status detection or resume. Google's supported agent is Antigravity (agy).

## How to read the table

1. Check the agent you plan to run. If it is in the table, its pane reports state without you reading the scrollback.
2. Prefer agents with **Hooks** for unattended runs: hook events report state the moment the agent changes it.
3. Use **Resume** agents for long tasks you may close the laptop on; the session reopens where it stopped.
4. Use **Remote history** agents if you plan to check in from a phone and want past turns, not just the live screen.
5. Re-check this page after upgrading Ferryx; the table is regenerated on every site build.

| Checkpoint | What good looks like |
| --- | --- |
| Isolation | Each parallel actor has its own worktree or a documented reason not to |
| Visibility | Status badges or attention counters answer "who needs me?" without polling |
| Persistence | Closing the window does not end agent processes |
| Evidence | Claims map to [product facts](/docs/facts/) or a linked source |

## How this fits Ferryx today

Ferryx is a source-available desktop terminal under the Ferryx License, free for individuals and small companies, written in Rust on Tauri v2 with a headless daemon. Comparisons on this site stick to licensing, architecture, and workflow facts rather than unmeasured performance claims.

Ferryx ships calendar-versioned releases for macOS (universal DMG), Windows (x64 installer), and Linux (AppImage and .deb). The desktop shell, daemon, and optional `ferryx-cli` headless binary are described with sources on the [product facts](/docs/facts/) page. If you are new to the workspace model, start with the [introduction](/docs/introduction/); if you want the mechanical details of sessions and replay, read [technical architecture](/docs/architecture/).

![CLI Agent Support Matrix: Who Works Where illustration](/images/blog/agent-support-matrix/body-1.png)

Related reading:
- [/compare/](/compare/)
- [/docs/facts/](/docs/facts/)

## FAQ

### Why does Ferryx publish no benchmarks?

Editorial policy: unmeasured numbers go stale and mislead. Claims here map to sources on the product facts page instead.

### Is Ferryx open source or source-available?

Source-available under the Ferryx License, which is not an OSI license. It is free for individuals, education, open-source work, non-profits, and companies under 50 people and USD 5M revenue; larger companies need a Team subscription for work use. Releases before the change remain under SUL-1.0.

### Which tool should I pick?

Write down your constraints first. If you need OSI copyleft, a tool with AGPL/MIT client code may fit better than source-available Ferryx; if you need daemon-owned sessions and worktree isolation today, evaluate Ferryx against that checklist.

## Limits and non-goals

Ferryx is early software: core flows work, but expect rough edges and breaking changes between calendar-versioned releases. Some OS integrations remain platform-specific (Dock badge counters and launchd supervision are macOS-only today), Windows builds are not code-signed yet so SmartScreen warns on first run, and remote features assume you control the host and pairing grants. None of that changes the core claim of this post—daemon-owned sessions, managed worktrees, and visible multi-agent workflows—but it is the honest boundary of what has shipped. Verify anything version-sensitive against the [facts page](/docs/facts/) and the [release notes](https://github.com/Indosaram/ferryx/releases/latest) before you standardise a team workflow on it. If something in this guide disagrees with your build, trust the repository sources linked from product facts and open an issue with your version string attached.

## How to verify these claims

Every concrete statement in this post ties back to a checkable surface: package names and license terms live on [product facts](/docs/facts/), session and buffer behaviour is described in [technical architecture](/docs/architecture/), and keyboard bindings are listed under [shortcuts](/docs/shortcuts/). Where a third-party tool is mentioned, treat vendor docs as authoritative for that tool and re-check dates before quoting them in a decision document. That discipline is why this site does not publish performance benchmarks it has not measured: reproducible architecture facts age better than marketing numbers.
