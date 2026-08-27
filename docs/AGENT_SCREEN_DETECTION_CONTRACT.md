# Agent screen-rule detection: implementation contract

Authoritative interface spec for the herdr-style screen-rule detection engine. Both the Rust
engine track and the TypeScript surface track build against this file. Do not deviate; if
something here is wrong, say so instead of silently changing it.

Validated against the shipped herdr 0.8.2 binary (128 embedded rules, 20 agent manifests).

## Why screen rules

Measured real agent titles (captured from a real pty by feeding `pty.fork` output):

| Agent | Title | Title-only verdict |
| --- | --- | --- |
| omo | `OmO - orca-lite` | no status |
| codex | `orca-lite` | no status |
| codex | `⠸ orca-lite` | working |
| opencode | `OpenCode` | no status |
| gemini | `◇  Ready (orca-lite)` | done (glyph luck) |

Most agent titles carry NO status word, so title-only inference cannot work. The status is on
the screen (`esc to interrupt`, `Do you want to proceed?`), which is what these rules read.

## Rust module layout

New top-level module `src-tauri/src/agent_detect/`, registered in `src/lib.rs`.

NOT feature-gated: it takes plain text as input and must compile and test without
`--features native-terminal`. Only the driver that reads a ghostty `RenderSnapshot` is gated.

```
agent_detect/
├── mod.rs          // pub API re-exports
├── manifest.rs     // serde TOML schema + load/validate
├── matcher.rs      // matcher tree evaluation
├── region.rs       // region resolution
├── engine.rs       // priority resolution + hold semantics
└── manifests/*.toml
```

Dependencies: add `regex = "1"` and `toml = "0.8"` to `[dependencies]`. Both are ALREADY in
`Cargo.lock` as transitive deps (regex 1.13.1, toml 0.8.2), so no new downloads.

Rust's `regex` crate has NO backtracking: lookahead/lookbehind is unsupported. Manifest
patterns must avoid them. `\x{2800}`, `(?i)`, `(?s)`, `\A`, `\p{Alphabetic}` are all fine.

## Screen input

```rust
pub struct ScreenInput {
    /// Viewport rows, top to bottom, already reconstructed to plain text.
    pub rows: Vec<String>,
    /// Current OSC 0/1/2 window title.
    pub title: String,
}
```

Ferryx's `RenderSnapshot` is viewport-only (no scrollback), so `whole_recent` is approximated
by the viewport. This is a known, accepted deviation from herdr.

## Manifest schema

```toml
id = "codex"
version = "2026.08.27.1"
min_engine_version = 1
aliases = ["codex-cli"]

[[rules]]
id = "interrupt_hint_working"
state = "working"              # working | blocked | idle | unknown
priority = 110                 # higher wins; ties broken by declaration order
region = "whole_recent"
visible_working = true         # optional affordance flags (working/blocker/idle)
skip_state_update = false      # optional; true => HOLD previous state
contains = ["esc to interrupt"]
```

### Matcher node

A matcher node may carry any combination of:

- `contains = [..]` — ALL substrings must be present (case-insensitive). AND semantics.
- `regex = [..]` — ALL patterns must match the region text as a whole.
- `line_regex = [..]` — at least one line matches; ALL listed patterns must each find a line.
- `any = [node, ..]` — at least one child node matches (OR).
- `all = [node, ..]` — every child node matches (AND).
- `not = [node, ..]` — VETO: if any child matches, this node fails.

Keys present on the same node combine with AND. Nodes nest arbitrarily. A rule's top level IS
a matcher node, plus the rule-level fields (`id`, `state`, `priority`, `region`, flags).

Matching is case-insensitive for `contains`; `regex`/`line_regex` control their own case via
inline `(?i)`.

### Regions

| Region | Meaning |
| --- | --- |
| `whole_recent` | all viewport rows joined by `\n` |
| `osc_title` | the title string only |
| `bottom_non_empty_lines(N)` | last N non-empty rows, original order |
| `top_non_empty_lines(N)` | first N non-empty rows, original order |

Unknown region strings are a load-time validation ERROR, not a silent skip.

`after_last_prompt_marker`, `after_last_horizontal_rule`, `prompt_box_body` and `osc_progress`
are intentionally NOT implemented: they need OSC 133 semantic prompt tracking Ferryx lacks
(only 2 of herdr's 128 rules use prompt markers).

## Engine semantics

```rust
pub enum AgentActivity { Working, Blocked, Idle }

pub struct Detection {
    pub state: AgentActivity,
    pub rule_id: String,     // for diagnostics
    pub manifest_id: String,
}

/// Pure: same inputs always produce the same output.
pub fn detect(&self, input: &ScreenInput, previous: Option<AgentActivity>) -> Option<Detection>
```

Resolution order, exactly:

1. Evaluate every rule of every loaded manifest against `input`.
2. Sort matches by `priority` descending; the highest wins. Ties: earlier declaration wins.
3. If the winner has `skip_state_update = true`, or its state is `unknown`: **HOLD** — return
   the previous state unchanged (never delete, never reset). This is the single most important
   rule in this document.
4. If NO rule matches: **HOLD** as well. Absence of evidence is not evidence of idle.
5. Otherwise return the winner's state.

### Deliberate deviation: manifests are evaluated globally, not per-agent

herdr knows which agent owns a pane (it tracks the pty's foreground process) and evaluates
only that agent's manifest. Ferryx cannot: agents are launched by typing `codex` into a shell,
so the session's spawn command is `zsh`, and codex's own title (`orca-lite`) contains no agent
name.

Therefore Ferryx evaluates ALL manifests and takes the global highest-priority match. This is
sound because the rules identify a STATE, and `esc to interrupt` on screen means something is
working regardless of which agent drew it. Agent attribution for the tab ICON keeps coming
from the existing title parser, which is unaffected.

Consequence for authoring: a rule must not be so generic that it fires on ordinary shell
output. Prefer anchored `line_regex` and multi-token `contains` over a single common word.

## Tauri event contract

```rust
pub const NATIVE_TERMINAL_AGENT_STATE_EVENT: &str = "native_terminal_agent_state";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTerminalAgentStatePayload {
    pub session_id: String,
    pub state: String,   // "working" | "blocked" | "idle"
    pub rule_id: String,
    pub manifest_id: String,
}
```

Added as a third variant of `NativeTerminalEvent` in
`src-tauri/src/native_terminal/surface_host.rs`, emitted through the existing
`emit_native_terminal_event` so the `set_event_sink` test seam keeps working.

Emit ONLY on state change (edge-triggered), never per frame.

## TypeScript contract

```ts
// ui/src/lib/tauri.ts
export type NativeTerminalAgentStatePayload = {
  sessionId: string;
  state: "working" | "blocked" | "idle";
  ruleId: string;
  manifestId: string;
};
export async function onNativeTerminalAgentState(
  handler: (payload: NativeTerminalAgentStatePayload) => void,
): Promise<UnlistenFn>;
```

Store action:

```ts
| { type: "SESSION_SCREEN_ACTIVITY"; tabId: string; sessionId: string;
    state: "working" | "blocked" | "idle"; ruleId: string }
```

### State mapping

| Screen state | `TerminalActivityState` |
| --- | --- |
| `working` | `working` |
| `blocked` | `waiting` |
| `idle` | `done` — but ONLY if the previous state was `working` or `waiting` |

`idle` with no previous activity, or with a previous `done`, must NOT create or change an
entry. Without this, every freshly-attached shell would immediately mark its tab unread.

### Title demotion

`TerminalActivity` gains `source?: "screen" | "title"`.

Once a session has a `source: "screen"` entry, `SESSION_TITLE_ACTIVITY` may still refresh
`title`, `isAgent` and `agentType`, but must NEVER change `state`. Screen evidence outranks
title evidence permanently for that session. This is the structural equivalent of herdr
placing `osc_title_idle` at priority 50.

`SESSION_TITLE_ACTIVITY` keeps its current hold-on-unclassifiable behavior for
title-only sessions.

## Test requirements

Rust (`cargo test --manifest-path src-tauri/Cargo.toml --lib`, no feature flag needed):

1. priority ordering — two rules match, higher priority wins
2. region scoping — a phrase outside `bottom_non_empty_lines(3)` does NOT match
3. `not` veto — a rule that would match is suppressed by its `not` child
4. nested `any`/`all` composition
5. `skip_state_update` HOLDS the previous state (feed a transcript-viewer screen while
   previous is `Working`, assert the result is still `Working`)
6. no-match HOLDS the previous state
7. every shipped manifest loads and validates (regions known, regexes compile)
8. per-agent realistic screens: for omo, opencode, codex, claude, gemini — a working screen,
   a blocked/permission screen, and an idle screen each reach the expected state

TypeScript (`cd ui && npx vitest run`):

1. `SESSION_SCREEN_ACTIVITY` working sets `state: "working"`, `source: "screen"`
2. `blocked` maps to `waiting`
3. `idle` after `working` maps to `done` and marks a NON-VISIBLE tab + its worktree unread
4. `idle` with no previous entry creates NO entry
5. after a screen entry exists, a `SESSION_TITLE_ACTIVITY` with a contradictory title does not
   change `state`
6. title-only sessions keep working exactly as before (existing tests must stay green)
