# Ferryx Agent Session Restore Design

Authoritative architecture and operational contract for restoring coding agent sessions across Ferryx application and daemon restarts.

## 1. The Problem

When Ferryx restarts, whether through a GUI reload, an application upgrade, or a background daemon restart, terminal panes running coding agents lose their live processes. The root cause is pseudo-terminal file descriptor ownership in the operating system kernel.

In the Ferryx backend, `PtySession` allocates a Unix pseudo-terminal pair. Our daemon retains ownership of the master file descriptor (`pair.master`) in memory, while the slave end is handed to the child shell and closed on the parent side immediately after spawn. The daemon process remains the sole owner of the master file descriptor.

A file descriptor is not a serializable data structure that can be preserved on disk. It is a numeric index into a process-specific kernel table. When the daemon crashes, exits, or restarts, the kernel automatically closes all open file descriptors belonging to that process. Closing the master file descriptor instantly destroys the pseudo-terminal.

Signal handling on the child side cannot work around this kernel behavior. In an empirical test conducted on this workstation, a child process running `trap '' HUP; sleep 300` on a pty whose master owner exited DIED within 100ms. Signal-hardening against `SIGHUP` does not keep the child alive because closing the master invalidates standard input and output on the slave. Every subsequent read or write by the child fails with `EIO` (Input/output error). The child process terminates abruptly.

Because the daemon process cannot persist raw kernel file descriptors across restarts, every agent process running inside a daemon-owned pty dies when the daemon dies.

## 2. The Insight

Process death does not mean conversation loss. Modern coding agents continuously journal their conversation history, tool calls, and execution steps to local disk files. These tools provide first-class session resumption capabilities built directly into their command-line interfaces.

When an agent process terminates because its host pty collapsed, its journal remains intact on the local filesystem. Ferryx does not need to resurrect dead kernel file descriptors or hold zombie process handles. Full conversational recovery requires only three pieces of metadata:

1. Which agent was running in the pane.
2. Which session identifier the agent generated for that conversation.
3. Which working directory (`cwd`) hosted the run.

With this tuple, Ferryx can launch a fresh process in a new pty and invoke the agent's native resume command. The agent re-reads its own journal and reconstructs its full context.

## 3. Empirical Agent Matrix

The following matrix reflects empirically verified behavior on this machine across all supported coding agents:

| agent | resume invocation | identity kind | on-disk store |
| :--- | :--- | :--- | :--- |
| claude | `claude --resume <id>` (also `-c`/`--continue`) | uuid | `~/.claude/projects/<escaped-cwd>/<uuid>.jsonl` |
| codex | `codex resume <SESSION_ID>` (also `--last`) | uuid OR session name | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` |
| copilot | `copilot --resume <sessionId>` (also `--continue`) | uuid | `~/.copilot/session-state/<uuid>.jsonl` |
| kimi | `kimi --session <id>` (also `-C`/`--continue`) | uuid | `~/.kimi/sessions/<cwd-hash>/<uuid>/` |
| opencode | `opencode -s <id>` (also `-c`/`--continue`) | session id | `~/.local/share/opencode/` |
| gemini | `gemini -r <index\|latest>` | POSITIONAL INDEX, NOT a uuid | `~/.gemini/tmp/<project>/chats` |
| cursor | `cursor-agent --resume <chatId>` (also `--continue`) | chatId | `~/.cursor/chats` |
| omo | `omo --session <id>` (`--resume` takes NO value — it is a picker) | id | `~/.omo/sessions` |
| grok | NO resume support found | none | `~/.grok` has hooks/skills only |
| antigravity, pi, cline | NOT INSTALLED, capability unknown | unknown | unknown |

## 4. Read the agent's session id; never mint one

RULE: Ferryx MUST NEVER mint, generate, or inject a session id for an agent.

Using `claude --session-id <uuid>` is strictly forbidden. The `omo` binary also accepts `--session-id`, and injecting an id there is equally forbidden. Restore ALWAYS reads back the session id the agent itself generated and resumes with that id.

This is an invariant across the entire system. Ferryx must not include any minting fallback, nor any code path that generates a synthetic session id if discovery encounters an issue.

Two structural facts enforce this rule:

1. Incompatible agent CLI flags. Across the twelve agents recognized in `SUPPORTED_AGENT_LOGOS` (`ui/src/lib/agentIcon.ts`), only Claude and OMO provide CLI arguments to accept an injected session id at launch time. The remaining ten agents provide no mechanism to inject an external identifier.
2. Incompatible identity models. Gemini CLI resumes by positional index (such as `gemini -r 1` or `gemini -r latest`) rather than a UUID. No universal injection format can exist even in principle.

Injecting synthetic arguments also corrupts user settings. Ferryx allows developers to customize agent commands and arguments in `ui/src/lib/agentsSettings.ts`. Injecting a synthetic `--session-id` flag tampers with the user's explicit configuration, creating command parsing errors and subtle runtime collisions. Ferryx discovers identity from the agent; it never dictates identity to the agent.

## 5. Process-Keyed Discovery

Ferryx determines the active session id by inspecting the live process tree under each pane's pty.

An agent process is not a direct child of the Ferryx daemon. The daemon spawns an interactive user shell (`zsh`, `bash`, `fish`), and the agent runs as a descendant process under that shell. Discovery starts from the slave pty assigned to the pane, walking down the OS process tree to find the running agent executable.

Once the daemon resolves the agent's PID, it inspects the open file descriptors of that specific process. On macOS, this uses `proc_pidinfo` and filesystem descriptors; on Linux, it reads `/proc/<pid>/fd`. Coding agents keep their active journal file open while running. Reading the path of the open journal file immediately yields the active session identifier.

Process-keyed discovery guarantees unambiguous session tracking. If a developer opens two side-by-side panes running Claude in the exact same repository directory, both instances write logs to `~/.claude/projects/<escaped-cwd>/`. File modification timestamps cannot reliably distinguish which journal belongs to which pane. Inspecting the specific PID's open file table connects each pane to its exact journal file without race conditions.

## 6. Reconciling the auto-respawn anti-pattern

The Ferryx daemon persistence specification in `src-tauri/src/daemon/AGENTS.md` defines a critical architectural rule:

> "DO NOT Auto-Respawn Missing Sessions: On cold restore or epoch mismatch, missing backend sessions must be marked exited rather than spawning unexpected replacement shells."

Auto-launching an agent on restore would directly violate this rule. Starting background processes automatically after a crash or cold boot can trigger unintended token burn, repeat destructive tool calls, or flood system resources.

Ferryx reconciles agent resume with the daemon invariant through explicit user gating:

1. Restore marks dead backend sessions exited. On cold boot or daemon restart, missing backend sessions transition to the exited state as demanded by the daemon contract. No replacement shell or agent process spawns in the background.
2. The UI renders the dead pane with a restore affordance. Front-end components inspect saved metadata (agent type, working directory, and discovered session id) and display a clear, interactive resume banner on the exited pane.
3. Relaunch requires user consent. The agent process is launched only when the user clicks the resume action or presses enter on the prompt. Ferryx creates a new pty session and executes the agent's resume command, keeping the recovery process explicit, safe, and fully controlled.

## 7. Limits

Agent session restoration provides effective recovery, yet it operates under clear technical boundaries:

1. In-flight turns are lost. Coding agents flush journal entries at turn boundaries or upon tool completion. If the daemon terminates while an LLM response streams or while a tool runs, that incomplete turn does not commit to disk. Resuming replays the conversation up to the last completed journal record.
2. Non-agent panes cannot be resumed. Plain shell sessions, active build jobs, test runners, and dev servers (`npm run dev`) do not write structured conversation journals. When the daemon restarts, non-agent panes exit permanently and cannot be resumed.
3. Context rebuild consumes time and tokens. Resuming an agent is a new process launch. The agent must re-read its historical journal from disk, re-tokenize conversation messages, and establish a new connection to the model provider.
4. Scrollback requires separate persistence. Agent journals record structured conversational turns, not raw ANSI terminal output or interactive shell history prior to agent launch. Restoring terminal scrollback requires saving and replaying the daemon's ring buffer separately from the agent journal.

## 8. Per-agent identity kinds

Ferryx requires a dedicated adapter for each supported agent because identity formats and resume commands vary across implementations:

- UUID strings: Claude (`<uuid>.jsonl`), Codex (`rollout-<ts>-<uuid>.jsonl`), Copilot (`<uuid>.jsonl`), and Kimi (`<uuid>/`) use standard UUID structures.
- Custom identifiers and chat IDs: OpenCode uses opaque session strings, Cursor uses internal `chatId` tokens, and OMO uses unique session IDs.
- Positional indices: Gemini CLI identifies sessions by their position in a project chat index (`gemini -r <index>`) or via `latest`, referencing files in `~/.gemini/tmp/<project>/chats`.

Gemini's positional index is inherently unstable across restarts. Because the index reflects an item's position in an ordered list, creating a new chat in another terminal shifts the index of previous conversations. The Gemini adapter must inspect the project chat directory at restore time to resolve the target index accurately, or fall back safely to `latest`. Grok provides no CLI resume support, while uninstalled agents (Antigravity, Pi, Cline) require graceful capability detection before attempting recovery.

Each adapter encapsulates the CLI syntax, journal parsing, and identity resolution rules for its agent, ensuring the core restore coordinator remains clean and predictable.
