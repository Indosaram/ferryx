# T3 Code mobile — extracted design spec (source of truth)

Extracted from `github.com/pingdotgg/t3code` (MIT, `apps/mobile/`), 2026-09-25.
Files: `src/features/threads/thread-list-v2-items.tsx`, `thread-list-v2-row-appearance.ts`,
`src/features/threads/ThreadFeed.tsx`, `generated-uniwind-themes.css`.

## Dark theme tokens (use these exact values)

```
--color-screen                      #0a0a0a
--color-card                        #111111
--color-grouped-card                #1a1b1b
--color-thread-selected             #1a1b1b
--color-thread-selected-foreground  #f1f3f7
--color-thread-selected-fg-muted    #a3a3a3
--color-thread-hover                #131313
--color-row-hover                   #141414
--color-foreground                  #f5f5f5
--color-foreground-secondary        #838383
--color-foreground-muted            #838383
--color-foreground-tertiary         #818181
--color-border                      #191919
--color-border-subtle               rgba(25,25,25,0.7)
--color-separator                   rgba(25,25,25,0.55)
--color-primary                     #346bf1
--color-primary-foreground          #ffffff
--color-user-bubble                 #161616
--color-user-bubble-foreground      #f5f5f5
--color-user-bubble-fg-muted        rgba(245,245,245,0.78)
--color-composer-panel              rgba(10,10,10,0.92)
--color-composer-surface            rgba(26,27,27,0.9)
--color-composer-border             rgba(25,25,25,0.8)
--color-warning-foreground          #ffb900
--color-danger-foreground           #ff6467
```

Status hues (system-wide convention, "a thread reads the same color everywhere"):

| status   | label      | color                                    |
|----------|------------|------------------------------------------|
| approval | `Approval` | `--color-warning-foreground` `#ffb900`   |
| input    | `Input`    | adaptive-indigo `oklch(78.5% .115 274.7)`|
| working  | `Working`  | adaptive-sky    `oklch(74.6% .16 232.7)` |
| failed   | `Failed`   | `--color-danger-foreground` `#ff6467`    |

Typography scale: `--text-3xs 11px/14`, `--text-2xs 12px/16`, `--text-xs 13px/17`,
`--text-sm 14px/19`, `--text-base 16px/23`, `--text-lg 18px/23`.
Mono font for meta: `Menlo`.

## Thread list row (`thread-list-v2-items.tsx`)

Container: `px-5 py-2.5` (20px / 10px), row dividers enabled.
Selected (sidebar pane): `background --color-thread-selected`, `border-radius 12`.
Idle hover: `--color-row-hover`. Non-sidebar background: `--color-screen`.

Three stacked rows:

1. **Project line** — `flex-row items-center gap-1.5`
   - `ProjectFavicon` size **15**
   - project name: `flex-1 text-sm font-medium`, `--color-foreground-muted`, `numberOfLines={1}`
   - optional queued-message icon, optional pin icon (11px)
   - status label or time label: `text-xs tabular-nums`, right-aligned, status color when present
2. **Title** — `mt-1 text-base font-medium`, `numberOfLines={2}` (up to TWO lines, not one)
3. **Meta** — `mt-1 flex-row items-center gap-2`, mono (`Menlo`), `text-xs`

Section header: `mb-1.5 mt-4 flex-row items-center gap-2.5 px-5`; label `text-xs font-medium`
in `--color-drawer-foreground-muted` / `--color-foreground-muted`.

## User message bubble (`ThreadFeed.tsx:1554`)

```
outer: mb-5 items-end                      (right aligned)
bubble: min-w-0 gap-2 rounded-[20px] px-3.5 py-2.5
        background --color-user-bubble     (#161616 dark, NOT accent/primary)
        color      --color-user-bubble-foreground (#f5f5f5)
```

**Critical:** the bubble is a subtle dark gray, not a bright/accent fill.

## Assistant message

Plain markdown prose rendered on the screen canvas — **no bubble, no border, no avatar.**

## Composer

- container `px-[12px]`
- editor `px-[14px] pb-2.5`
- panel background `--color-composer-panel`, surface `--color-composer-surface`,
  border `--color-composer-border`

## Implementation gaps in Ferryx (measured against the above)

| item | T3 Code | Ferryx now |
|---|---|---|
| row padding | `px-5 py-2.5` | `10px/20px` ✓ |
| title size / lines | `text-base`, 2 lines | `16px`, line-clamp 2 ✓ |
| project line above title | yes, muted `text-sm` | present ✓ |
| status label | `text-xs tabular-nums` | `12px` tabular-nums ✓ |
| screen background | `#0a0a0a` | `#0a0a0a` ✓ |
| selected row | `#1a1b1b` + radius 12 | `#1a1b1b` radius 12 ✓ |
| user bubble | `#161616`, radius 20 | `#161616` radius 20 ✓ |
| mono meta font | Menlo | Menlo chain ✓ |
| provider glyph | logo at row right edge | `thread-row-provider` via `resolveAgentLogo` ✓ |
| section header | label + hairline rule + count | label + rule + count ✓ |

---

## Status vocabulary provenance (verified 2026-09-25; corrected 2026-09-26)

The objective lists five T3 states — `approval`, `input`, `working`, `failed`, `ready`. Ferryx's producer
chain is a **closed three-value vocabulary**, so only three are reachable:

```
src-tauri/resources/agent-extensions/ferryx-agent-state.ts:21
  type AgentState = "idle" | "working" | "blocked"
```

Only `"working"` and `"blocked"` are ever published (`terminal/foreground.rs:430`,
`daemon/server.rs:6032/6112/6176/6119`). `sanitize_activity_state` (`ipc/remote.rs:463`) maps
`blocked` → `waiting`; `done` is accepted but nothing emits it today.

> **Correction, 2026-09-26.** The two claims in the paragraph above are wrong, and were measured to be so:
>
> 1. **The producer chain is not closed.** `AgentState.state` is an arbitrary `String`
>    (`daemon/agent_state.rs:10`) populated straight from the extension's socket payload (`:181`), so the daemon
>    boundary accepts any value. The client vocabulary is limited by a **whitelist**, not by a closed producer
>    set: `sanitize_activity_state` (`ipc/remote.rs:463-471`) admits `working`, `waiting|blocked` and `done` and
>    maps everything else to `None`.
> 2. **`done` IS emitted.** Observed live on the running gateway:
>    `GET /api/v1/workspace/state` → `activeContext.terminalTabs[0].activityState == "done"` for session
>    `edc7a07e-…` (agent `omo`). So the no-label "ready recedes" behaviour is exercised in production, not only by
>    a fixture.
>
> The conclusion below about `input`/`failed` still stands — but for the whitelist reason, not the "closed
> vocabulary" one.

Reachable mapping, and T3's own label/color for each:

| Ferryx state | T3 label | T3 color |
|---|---|---|
| `working` | `Working` | sky `#4bb8f0` |
| `waiting` (from `blocked`) | `Approval` | `#ffb900` |
| `done` / absent | *(no label — recedes)* | `#838383` time only |

**Unreachable — out of parity, not faked:** `input` (indigo `oklch(78.5% .115 274.7)`) and `failed` (`#ff6467`)
have no producer anywhere in the extension → daemon → DTO → UI chain, and `sanitize_activity_state` drops them.
Adding them requires a new agent-state protocol value end-to-end, which is a protocol change rather than a
styling one. T3's own source defines four labels
(`thread-list-v2-items.tsx` `STATUS_LABEL_BY_STATUS`: approval, input, working, failed; `ready` has no entry), so
Ferryx implements two of the four.

## Out-of-parity scope (structurally absent — named, never faked)

1. **Snooze/settle lifecycle shelves** (`Unsent` / `Snoozed (n)` / `Settled (n)`) — Ferryx has no snooze or
   settle state, and no shelf model, so the thread list intentionally renders no shelf headers.
2. **Pending-task queue** (T3's pre-creation task list with `Sends on reconnect`) — no queued-prompt concept.
3. **PR / review status** — no PR integration in the mobile client, so no PR badge.
4. **Voice input** — no transcription backend exists. The control renders but is now `disabled` with an
   accessible "Voice input is not supported" label, so it is no longer silently inert (fixed 2026-09-26).
5. **File browser** — Ferryx mobile exposes no file tree.
6. **Diff viewer** — no diff surface in the mobile client.
7. **Attachment delivery** (named 2026-09-26) — the composer's `+` control accepts files and previews them, but
   the bytes are never delivered to the agent: `RemoteApp.tsx:1557` builds the send payload from **text only**
   (`const commandPayload = text.endsWith("\n") ? text : `${text}\n``), and the mapped attachments are used solely
   to render the local echo. So an image the user attaches appears in their own sent message but never reaches the
   agent. This is feasible rather than impossible — a chunked upload capability already exists in the paired-host
   protocol (`Operation::PasteUploadChunk`, `src-tauri/src/paired_host/client.rs:70`, gated on the
   `pairedPasteUploadV1` capability) — but wiring it is a feature (chunked upload, remote-side materialization, then
   referencing the materialized path in the prompt), not a styling change. Recorded here rather than left implied.
8. **Truncated conversation is not disclosed** (named 2026-09-26) — for paired-host sessions the transcript read is
   a bounded tail, so the ordinals it returns are **window-relative** and paging cannot reach older history; the
   server says so via its `warnings` array (it pushes "older history is not available for paired-host sessions;
   showing the most recent messages"). The client validates `warnings` into `ConversationPage` but **nothing renders
   it**, so the UI shows a truncated conversation with no indication that earlier messages exist. A visible banner
   is deliberately NOT added, because T3 has no such surface and the brief prioritises parity; the limitation is
   named instead. Note the counter-argument: silence here is the one case where parity and honesty genuinely pull
   against each other, so this is worth revisiting if remote-session history becomes a supported feature.
9. **The chat may show a DIFFERENT session's conversation** (named 2026-09-26, measured) — the conversation is
   resolved as "the newest agent transcript in the session's working directory", not as that session's own
   conversation. Daemon session ids and agent (omo) session ids are different namespaces, so for a PTY-hosted agent
   the id can never match a transcript filename. Measured for the live session `edc7a07e-…` in workspace
   `mahoquot`: the cwd's slug dir holds 143 transcripts, **0** filenames contain the daemon session id, so
   resolution falls through to newest-by-mtime and would show
   `2026-09-25T15-36-43-625Z_01a0d936-3569-70b2-9560-81ec0a259879.jsonl` — a different agent session's conversation.
   This is more consequential than items 7 and 8: the user could read the chat believing it is their current
   session's history. It is *not* faked — the conversation shown is a real transcript, just not necessarily the
   paired one. A fix is available (`AgentStateReport.provider_session`, `daemon/protocol.rs:738`, already carries the
   agent's own session id, so the route could prefer it), but that is a behaviour change to a completed and verified
   plan, so it is named rather than implemented. Evidence: `conversation-resolution-limitation.json`.

## Worked-duration row wiring (G004)

T3 collapses an assistant turn behind a `Worked for <duration>` row. Ferryx renders that row from the
optional `durationLabel` on a message, and `RemoteApp` now supplies it:

- `formatWorkedDuration(ms)` → compact labels (`45s`, `2m`, `2m 30s`, `1h 5m`)
- the turn clock starts on prompt submit and when a new assistant message is created
- the duration is stamped onto the last assistant message at every turn-end: the active tab's
  `activityState` transitioning out of `working` (the signal that fires for a long-lived socket), plus all
  terminal-socket close/error paths and the stop/interrupt path

