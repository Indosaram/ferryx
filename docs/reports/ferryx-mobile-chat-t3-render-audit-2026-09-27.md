# Rendering-Difference Audit: T3 Code Mobile Thread Feed vs Ferryx Mobile Remote Chat

**Date:** 2026-09-27  
**Auditor:** Senior Mobile & Frontend Systems Engineer  
**Status:** Complete  
**Reference Codebase (T3 Code, MIT authoritative):** `~/.cache/t3code-ref/t3code/apps/mobile/src/`  
**Target Codebase (Ferryx Remote Mobile Chat):** `<repo>/ui/src/remote/`  
**Live Wire Evidence:** `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/` (`wire-927.5-samples.json`, `baseline-dump.json`, `baseline-collapsed.png`, `baseline-expanded.png`, `after-dump.json`, `after-collapsed.png`, `after-expanded.png`, `after-row-drawer.png`)

---

## Executive Summary

A side-by-side rendering audit between T3 Code's mobile thread feed (`ThreadFeed.tsx`, `thread-work-log.tsx`, `threadActivity.ts`) and Ferryx's remote mobile chat (`agentConversation.ts`, `MobileChatMessage.tsx`, `MobileChatComponents.tsx`, `MobileChatWorkspace.tsx`) reveals a fundamental structural mismatch in turn aggregation, tool presentation, and wire parsing.

In T3 Code, a turn is treated as a cohesive conversational unit: all agent activities, tool executions, and reasoning steps occurring between a user prompt and the final agent answer are folded into **one single expandable "Worked for <duration>" row** (default collapsed). Tool calls are first-class structured activities with compact 32px work rows, distinct action icons, live shimmer, and scrollable output drawers (capped at 256px with top/bottom edge-fade gradients). Assistant prose contains **only human-readable text**, and timestamps/copy actions appear **once per turn**.

In Ferryx's current live deployment (daemon 927.5 wire), the remote frontend renders raw JSON records 1:1 without turn-level folding. As measured in `baseline-dump.json` (95 assistant records, 4 user messages):
1. **87 separate "Worked for" toggles** appear in a single conversation thread (one per assistant record, even for 0s durations).
2. **86 of 95 assistant bodies** leak raw CLI tool markers directly into prose (`→ eval`, `→ read → read`, `→ edit`).
3. **63 assistant messages** contain *only* tool markers with zero conversational text, rendering as empty floating boxes with redundant timestamps and copy buttons.
4. **0 tool cards** are rendered because the frontend expects structured `toolCalls` objects from an unreleased daemon, while the live 927.5 daemon sends flattened string markers in `text`.
5. Tool results (`role: "toolResult"`) are either dropped or presented outside their triggering tool call context.

Below is the comprehensive audit across all 9 design aspects, the exact parsing and turn-folding specification for wire-927.5, and the complete difference ledger (D1–D57: 57 items, 56 fixed, 1 rejected, 0 open).

---

## Aspect-by-Aspect Comparative Audit

### 1. Turn Structure & Turn Folds
* **T3 Code (`threadActivity.ts:1640–1740`, `thread-work-log.tsx:440–475`):**
  T3 aggregates all feed entries by `turnId` via `deriveThreadFeedTurnFolds`. A turn begins at the user message (`pendingUserBoundary`) and terminates at the final assistant message (`terminalAssistantMessageId`). All intermediate assistant message fragments, tool calls, and thinking blocks are assigned to `hiddenEntryIds` and hidden behind **exactly one** `turn-fold` entry per turn. The fold renders as:
  ```
  Worked for 14s  [v]
  ```
  Expanding this fold reveals the encapsulated work log and intermediate thoughts in place, without duplicating the conversation turn.
* **Ferryx (`agentConversation.ts:75–125`, `MobileChatMessage.tsx:145–165`):**
  Ferryx maps incoming records into a flat array of `MobileChatMessageProps` without turn aggregation. Every assistant record that contains tool calls or has a non-empty `durationLabel` renders its own separate fold header:
  ```tsx
  {durationLabel && (
    <div className="flex items-center gap-1.5 text-xs text-[#838383] py-1">
      <button onClick={() => setIsExpanded(!isExpanded)}>
        Worked for {durationLabel}
        <ChevronRight className={cn("w-3.5 h-3.5", isExpanded && "rotate-90")} />
      </button>
    </div>
  )}
  ```
  In the baseline trace, a single 2-minute agent turn produced **18 consecutive "Worked for" folds** stacked on top of each other.
* **Verdict:** Major Divergence. Ferryx requires turn-level folding that groups all intermediate records between user prompts into one turn with one master fold.

---

### 2. Tool Marker Placement & Prose Hygiene
* **T3 Code (`threadActivity.ts:150–220`, `ThreadFeed.tsx:1550–1610`):**
  T3 Code strictly separates agent text from tool executions. Tool invocations are never emitted as markdown text in message bodies. Assistant prose rendered in `ThreadFeed.tsx` contains solely natural language communication.
* **Ferryx (`agentConversation.ts:40–60`, `MobileChatMessage.tsx:170–190`):**
  In daemon 927.5, tool calls are streamed as inline text tokens such as `\n→ eval`, `→ read`, or `→ edit`. Because `agentConversation.ts` passes `item.text` directly into `MobileChatMessageProps.content`, `ReactMarkdown` renders tool invocations as raw text:
  ```
  Next is the final verification pass: tsc plus every related suite. → eval
  ```
  Worse, 63 records in `baseline-dump.json` contain *only* `→ eval` or `→ read`. These render as isolated lines of text inside full message wrappers.
* **Verdict:** Major Divergence. Ferryx must strip `\n→ <toolName>` and `→ <toolName>` markers from `content` and convert them into structured work items before rendering markdown prose.

---

### 3. Work Row Anatomy & Group Layout
* **T3 Code (`thread-work-log.tsx:380–450`, `720–850`, `layout.ts`):**
  * **Row Height:** Exact `min-h-8` (32px, `THREAD_WORK_ROW_MIN_HEIGHT = 32`).
  * **Row Gap:** `gap-px` (1px between rows).
  * **Icons:** 14px monochrome SF Symbols / Material symbols inside a 24x24 container (`h-6 w-6 shrink-0 items-center justify-center`). Semantic mapping:
    * `command` / `eval` -> `terminal`
    * `edit` / `write` -> `square.and.pencil`
    * `read` / `view` -> `eye`
    * `browser` -> `safari` / `public`
    * `agent` -> `sparkles`
    * `check` -> `checkmark`
    * `alert` / `error` -> `exclamationmark.triangle` / `xmark` (rose/danger tint)
  * **Typography:** `text-sm text-foreground-muted` (`#838383`, 14px/19px). Single line with `numberOfLines={1}` when collapsed.
  * **Group Container (`ThreadWorkGroupList`):** When activities are grouped, the list is constrained to `WORK_GROUP_MAX_HEIGHT = 256` (256px). If activities exceed 256px, the inner list scrolls independently.
  * **Edge Fade:** 12px top and bottom SVG linear gradient fades (`EdgeFade`) painted in the screen background color (`#0a0a0a`), masking content that extends beyond the scroll viewport.
  * **Live Shimmer:** Running activities display `ShimmeringWorkContent` with an active 72px linear gradient sweep across 1,350ms.
* **Ferryx (`MobileChatComponents.tsx:110–220`):**
  * `ToolCallCard` is styled as a heavy card block (`rounded-xl border border-[#191919] bg-[#111111]/80 p-2.5 my-1.5`).
  * Row height is dynamic and exceeds 44px–56px.
  * No scroll constraint: 20 tool calls create an unmanageable 1,000px+ vertical wall.
  * No edge-fade gradients or live text shimmer.
  * In wire-927.5, `toolCalls` is empty, so even this card is not rendered (0 cards in baseline).
* **Verdict:** Major Divergence. Ferryx needs compact 32px work rows with semantic icons, 256px max-height scrollable group containers, and edge-fade gradients.

---

### 4. Tool Output (`toolResult`) Presentation
* **T3 Code (`thread-work-log.tsx:860–920`):**
  * Tool results are **never** rendered as top-level chat messages.
  * Output is attached directly to the originating tool row's `detail` property.
  * Output is completely hidden until the user taps that specific row to expand it.
  * When expanded, it renders in an indented drawer:
    ```tsx
    <View className="ml-7 border-l border-border pb-1 pl-3 pt-0.5">
      <ScrollView className="max-h-60" showsVerticalScrollIndicator>
        <Text selectable className="font-mono text-2xs leading-normal text-foreground-muted">
          {fullDetail}
        </Text>
      </ScrollView>
    </View>
    ```
  * Drawer height is capped at `max-h-60` (240px) with internal scrolling and selectable monospace font (`text-2xs` / 12px).
* **Ferryx (`agentConversation.ts:50–70`, `MobileChatComponents.tsx:210–225`):**
  * Wire-927.5 sends `role: "toolResult"` records containing CLI stdout/stderr.
  * `agentConversation.ts` checks `isToolRole(item.role)`:
    ```ts
    if (isToolRole(item.role)) {
      // either pushed to openCalls or discarded
    }
    ```
    Because daemon 927.5 does not provide `toolCallId` linking `toolResult` to a specific call, `openCalls.get(...)` fails, and results are discarded or orphaned.
  * In `MobileChatComponents.tsx`, output is rendered inside `ToolCallCard` in an inline `<pre className="text-[11px] font-mono text-[#838383] bg-[#0a0a0a] p-2 rounded-lg max-h-48 overflow-y-auto">`.
* **Verdict:** Major Divergence. Tool results must be correlated sequentially to the preceding tool marker and nested inside that row's expandable detail drawer.

---

### 5. Thinking / Reasoning Presentation
* **T3 Code (`threadActivity.ts:180–205`, `1700–1730`, `ThreadFeed.tsx:1650–1720`):**
  * Thinking is classified as a work activity (`tone: "thinking"`).
  * During multi-step execution, thinking blocks fold seamlessly inside the turn's work log under the single "Worked for <duration>" toggle.
  * If a turn consists *only* of reasoning without tool calls, it renders as a single standalone "Thought" disclosure row rather than being concealed under a generic work toggle.
  * T3 maintains a single dynamic live slot (`type: "thinking"`) that updates in place while the model reasons, transitioning smoothly to tool rows.
* **Ferryx (`MobileChatMessage.tsx:140–150`, `MobileChatComponents.tsx:260–310`):**
  * `ThinkingBlock` is rendered as an isolated amber-tinted box (`bg-[#161616] border border-[#262626] text-amber-400`).
  * It sits outside the "Worked for" toggle.
  * Does not fold into the turn's work log, creating extra visual clutter when an agent alternates between thinking and tool calls.
* **Verdict:** Moderate Divergence. Ferryx should incorporate thinking into the turn-level work log or align its disclosure styling with T3's neutral thought row.

---

### 6. "Worked for" Duration Calculation
* **T3 Code (`threadActivity.ts:1610–1635`, `1740–1780`):**
  * Duration is calculated at the **turn level**:
    ```ts
    const elapsedMs = latestTurnMatches && latestTurn.startedAt && latestTurn.completedAt
      ? computeElapsedMs(latestTurn.startedAt, latestTurn.completedAt)
      : computeElapsedMs(
          group.startBoundary ?? firstEntry.createdAt,
          maxIsoTimestamp(terminalEntry?.updatedAt, lastEntryEnd) ?? lastEntryEnd,
        );
    const duration = elapsedMs === null ? null : formatDuration(elapsedMs);
    const label = duration ? `Worked for ${duration}` : "Worked";
    ```
  * Sub-second durations or zero durations do not generate redundant toggle clutter.
  * Durations accurately reflect the real user wait time (from user prompt to final answer).
* **Ferryx (`agentConversation.ts:75–115`):**
  * Calculates duration per assistant record using timestamp differences between adjacent records or summing `tc.durationMs`.
  * Because records in wire-927.5 arrive in rapid succession (e.g. `ordinal: 8366` at 07:13:13.232Z and `8367` at 07:13:13.275Z), the delta is 43ms.
  * `formatWorkedDuration(43)` rounds to `0s` or `1s`.
  * This produces 87 separate toggles with nonsensical labels like "Worked for 0s", "Worked for 5s", "Worked for 9s" across a single task.
* **Verdict:** Major Divergence. Ferryx must derive duration from the entire turn boundary (`lastAssistantTimestamp - turnStartTimestamp`), suppressing per-record micro-durations.

---

### 7. Assistant Prose, Code Blocks & Meta Row Placement
* **T3 Code (`ThreadFeed.tsx:1550–1620`, `t3code-mobile-design-spec.md`):**
  * **Canvas:** Assistant markdown prose renders directly on the screen canvas (`#0a0a0a`) with no enclosing card, border, or avatar icon.
  * **Code Blocks:** Styled with a dark subtle header (`px-3 py-1.5 bg-[#111111]/60 border-b border-[#191919]`), language tag in uppercase 10px tracking-wider (`#818181`), and a copy button with 2s green feedback.
  * **Metadata Row:** The timestamp and copy button appear **strictly once per turn**, positioned below the terminal assistant prose (`mb-5 mt-2 flex-row items-center gap-2`).
* **Ferryx (`MobileChatMessage.tsx:175–215`):**
  * Assistant prose is rendered inside a distinct message container with bottom margin.
  * **Metadata Row:** Every single assistant record renders its own footer:
    ```tsx
    <div className="flex items-center justify-between text-[11px] text-[#838383] mt-2">
      <span>{formatTimestamp(timestamp)}</span>
      <button onClick={handleCopy}>Copy</button>
    </div>
    ```
  * Because an agent turn is split into 15–30 raw records, the user sees 15–30 timestamps ("06:18 AM", "06:18 AM", "06:19 AM") and 15–30 Copy buttons scattered throughout what is logically a single response.
* **Verdict:** Major Divergence. Ferryx must show the timestamp and Copy button only once at the footer of the completed turn.

---

### 8. User Bubble, Spacing & Message Merging
* **T3 Code (`ThreadFeed.tsx:1554–1590`, `t3code-mobile-design-spec.md`):**
  * **Container:** Right-aligned (`mb-5 items-end`).
  * **Bubble Styling:** `min-w-0 gap-2 rounded-[20px] px-3.5 py-2.5`.
  * **Colors:** Dark subtle gray `--color-user-bubble` (`#161616`), text `--color-user-bubble-foreground` (`#f5f5f5`). Never bright accent/primary blue.
  * **Merging:** User messages establish turn boundaries. All subsequent agent records merge into that turn.
* **Ferryx (`MobileChatMessage.tsx:125–140`, `MobileChatWorkspace.tsx:90–120`):**
  * User bubble styling matches the dark theme tokens closely (`bg-[#161616] text-[#f5f5f5] rounded-[20px] px-3.5 py-2.5 max-w-[85%]`).
  * Spacing between consecutive items is handled via generic list gaps (`space-y-4`), which exaggerates the visual fragmentation of unmerged assistant records.
  * No concept of consecutive assistant record merging exists in `mapAgentConversation`.
* **Verdict:** Moderate Divergence. User bubble styling is already aligned; the defect is the lack of merging for following assistant records.

---

### 9. Baseline Visual Defects (Observed in `baseline-dump.json` & Screenshots)
1. **Wall of "Worked for" Toggles:** 87 disclosure toggles dominate the viewport. The user must scroll past screens of repetitive disclosure bars to find the actual conversational response.
2. **Exposed CLI Arrows:** Raw text like `→ eval`, `→ read → read`, and `→ edit` creates a cluttered, unfinished terminal-dump appearance rather than a polished mobile chat experience.
3. **Empty Ghost Messages:** 63 records in `baseline-dump.json` contain only tool arrows. When the tool arrow is either unstyled or stripped, an empty bubble or empty space is left with a stray timestamp.
4. **Complete Absence of Tool Activity Cards:** Despite extensive tool execution (file reads, test runs, code edits), 0 tool cards are rendered in the live UI because the frontend does not parse 927.5 wire strings into `ChatWorkItem`.

---

## Required Mapping for the 927.5 Wire

The running Ferryx daemon is version `2026.927.5` and cannot be upgraded during this deployment. The remote frontend (`ui/src/remote/agentConversation.ts`) is served as static disk assets and can be re-bundled and updated immediately without restarting the daemon.

The frontend must implement backward-compatible parsing that handles both:
1. **Legacy 927.5 Wire:** Plain text containing `\n→ <toolName>` or `→ <toolName>`, followed by `role: "toolResult"` records.
2. **Modern/Future Wire:** Structured `toolCalls: [...]`, `thinking: "..."`, and `toolCallId`.

### Parsing & Aggregation Algorithm

```
INPUT: Array of raw ConversationMessage sorted by ordinal
OUTPUT: Array of MobileChatMessageProps (one per turn)

1. Initialize turns = []
2. Initialize currentTurn = null

3. FOR EACH message IN sorted:
     IF message.role == "user":
       IF currentTurn != null:
         finalizeTurn(currentTurn)
         turns.push(currentTurn)
       currentTurn = createNewTurn(message)
       CONTINUE

     IF message.role == "assistant":
       IF currentTurn == null:
         currentTurn = createNewTurn(null) // Turn initiated without preceding user prompt

       a. Check for structured toolCalls (modern wire):
          IF message.toolCalls exists:
            append to currentTurn.workItems

       b. Check for legacy tool markers in message.text:
          Extract all instances of /(?:^|\n|\s)→\s*([a-zA-Z0-9_-]+)/g
          Strip these markers from message.text
          FOR EACH matched toolName:
            currentTurn.pendingWorkItems.push({
              kind: "tool",
              name: toolName,
              status: "running",
              output: null
            })

       c. Append cleaned prose:
          cleanedText = message.text.replace(markerRegex, "").trim()
          IF cleanedText.length > 0:
            currentTurn.proseFragments.push(cleanedText)

       d. Record timestamp:
          currentTurn.lastTimestamp = message.timestamp

     IF message.role == "toolResult" OR isToolRole(message.role):
       // Correlate with the earliest open tool call in currentTurn
       openTool = currentTurn.pendingWorkItems.find(t => t.output == null)
       IF openTool:
         openTool.output = message.text
         openTool.status = message.isError ? "error" : "success"
       ELSE:
         // Standalone tool result
         currentTurn.pendingWorkItems.push({
           kind: "tool",
           name: "tool",
           status: "success",
           output: message.text
         })

4. IF currentTurn != null:
     finalizeTurn(currentTurn)
     turns.push(currentTurn)

FUNCTION finalizeTurn(turn):
  turn.durationLabel = formatWorkedDuration(turn.lastTimestamp - turn.startTimestamp)
  turn.content = turn.proseFragments.join("\n\n")
  turn.toolCalls = turn.pendingWorkItems
  // Exactly ONE fold is created if turn.toolCalls.length > 0
```

### Worked Examples from `wire-927.5-samples.json`

#### Example 1: Multi-Tool Step with Intermediate Explanation (Ordinals 8366–8388)
* **Raw Wire Records:**
  * `8366` (`assistant`): `"→ read"` (ts: `07:13:13.232Z`)
  * `8367` (`toolResult`): `"#[cfg(test)]\nmod tests..."`
  * `8368` (`assistant`): `"→ eval"` (ts: `07:13:41.321Z`)
  * `8369` (`toolResult`): `"terminal/session.rs:188:pub struct PtySessionExport..."`
  * `8370` (`assistant`): `"→ edit"` (ts: `07:14:24.904Z`)
  * `8371` (`toolResult`): `"Successfully replaced 2 block(s)..."`
  * `8374` (`assistant`): `"That env-file path parser needs a real UUID...\n→ edit\n→ edit"` (ts: `07:15:14.255Z`)
  * `8375` (`toolResult`): `"Successfully replaced 1 block(s) in agents.rs..."`
  * `8376` (`toolResult`): `"Successfully replaced 1 block(s) in server.rs..."`
  * `8377` (`assistant`): `"→ eval"` (ts: `07:15:55.742Z`)
  * `8378` (`toolResult`): `"Command running in background with ID: bash_148..."`
  * `8379` (`assistant`): `"While that compiles, I'm checking the other half of the 404s...\n→ eval"` (ts: `07:16:02.832Z`)
  * `8380` (`toolResult`): `"16832 => FERRYX_SESSION_ID=..."`
  * `8381` (`assistant`): `"→ eval"` (ts: `07:16:07.831Z`)
  * `8382` (`toolResult`): `"   1 <volume>/project/EclipticRD-Rewrite/..."`
  * `8383` (`assistant`): `"→ eval"` (ts: `07:16:26.499Z`)
  * `8384` (`toolResult`): `"5854 |         let _ = state.auth_manager..."`
  * `8385` (`assistant`): `"Still compiling; I'll wait on the monitor."` (ts: `07:16:29.376Z`)
  * `8386` (`assistant`): `"→ eval"` (ts: `07:17:18.435Z`)
  * `8387` (`toolResult`): `"test ipc::agent_history::tests ... ok. 37 passed; 0 failed..."`
  * `8388` (`assistant`): `"The chat is empty because of the daemon handover at 13:56, not the chat UI...\nShould I build and hand over the new daemon?"` (ts: `07:17:43.678Z`)

* **Current Defective Ferryx Output:**
  * 12 distinct assistant message cards in the feed.
  * 11 "Worked for 5s", "Worked for 14s", "Worked for 28s" buttons.
  * Raw strings `"→ read"`, `"→ eval"`, `"→ edit"` rendered as prose text.
  * 0 tool output drawers.
  * 11 timestamps and Copy buttons.

* **Parsed & Folded Target Output (T3 Parity):**
  * **Single Assistant Turn** with:
    * **One Work Fold Toggle:** `Worked for 4m 30s [v]` (derived from `07:17:43.678Z - 07:13:13.232Z = 270s = 4m 30s`).
    * **Expanded Work Log Drawer:** 10 compact 32px work rows:
      1. `[eye] Read file` (detail: `"#[cfg(test)]\nmod tests..."`)
      2. `[terminal] Ran eval` (detail: `"terminal/session.rs:188..."`)
      3. `[pencil] Edited file` (detail: `"Successfully replaced 2 block(s)..."`)
      4. `[pencil] Edited agents.rs` (detail: `"Successfully replaced 1 block(s)..."`)
      5. `[pencil] Edited server.rs` (detail: `"Successfully replaced 1 block(s)..."`)
      6. `[terminal] Ran eval` (detail: `"Command running in background..."`)
      7. `[terminal] Ran eval` (detail: `"16832 => FERRYX_SESSION_ID=..."`)
      8. `[terminal] Ran eval` (detail: `"   1 <volume>/project/..."`)
      9. `[terminal] Ran eval` (detail: `"5854 | let _ = state..."`)
      10. `[terminal] Ran eval` (detail: `"test result: ok. 37 passed..."`)
    * **Assistant Prose Canvas:** Clean Markdown text without tool arrows:
      > That env-file path parser needs a real UUID, so my test fixture was unrealistic. I'll switch the test to real UUIDs.
      >
      > While that compiles, I'm checking the other half of the 404s: sessions whose agent process has no `--session` argument.
      >
      > Still compiling; I'll wait on the monitor.
      >
      > The chat is empty because of the daemon handover at 13:56, not the chat UI... [full final answer]
    * **Footer:** Exactly **one** timestamp (`07:17 AM`) and **one** Copy button.

---

## Difference Ledger (D1–D57)

| ID | Aspect | Status | T3 Reference | Ferryx Reference | Description | Fix File(s) |
|---|---|---|---|---|---|---|
| **D1** | Turn Aggregation | Fixed | `apps/mobile/src/lib/threadActivity.ts:1640-1740` | `ui/src/remote/agentConversation.ts:149-322` | T3 folds all intermediate messages/tools between user messages into one turn; Ferryx outputs 1 message per daemon record. | `ui/src/remote/agentConversation.ts` |
| **D2** | Tool Marker Stripping | Fixed | `apps/mobile/src/features/threads/ThreadFeed.tsx:1550-1610` | `ui/src/remote/agentConversation.ts:37-43, 287-289`, `ui/src/remote/chat/MobileChatMessage.tsx:219` | T3 never shows tool markers in prose; Ferryx prints `→ eval` and `→ read` directly into markdown prose. | `ui/src/remote/agentConversation.ts`, `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D3** | Legacy 927.5 Wire Tool Extraction | Fixed | `apps/mobile/src/lib/threadActivity.ts:1400-1480` | `ui/src/remote/agentConversation.ts:216-250, 272-282` | 927.5 daemon sends tool calls as `→ name` and results as `role: toolResult`; Ferryx fails to correlate them into `toolCalls`. | `ui/src/remote/agentConversation.ts` |
| **D4** | Work Fold Multiplicity | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:440-475` | `ui/src/remote/agentConversation.ts:285-307`, `ui/src/remote/chat/MobileChatMessage.tsx:189-204` | T3 shows 1 "Worked for" fold per turn; Ferryx shows 87 toggles across 95 records (one per record). | `ui/src/remote/agentConversation.ts`, `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D5** | Work Row Sizing & Typography | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:380-410` | `ui/src/remote/chat/MobileChatComponents.tsx:226-259, 298-323`, `ui/src/remote/chat/MobileChatMessage.tsx:160-163` | T3 enforces exact 32px height (`min-h-8`), 1px gap, 14px font; Ferryx uses bulky 56px+ cards. | `ui/src/remote/chat/MobileChatComponents.tsx`, `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D6** | Work Group Scroll & Height Cap | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:490-540` | `ui/src/remote/chat/MobileChatMessage.tsx:142-166` | T3 caps work group at `max-h-64` (256px) with internal LegendList/ScrollView; Ferryx expands unbounded. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D7** | Edge-Fade Gradients | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:690-740` | `ui/src/remote/chat/MobileChatMessage.tsx:142-165` | T3 applies 12px top and bottom SVG gradient fades over long work groups; Ferryx has hard cutoff borders. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D8** | Work Row Semantic Icons | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:340-380` | `ui/src/remote/chat/MobileChatComponents.tsx:163-192, 233-241, 305-313` | T3 uses specific SF Symbols/Lucide icons for terminal, edit, read, browser, agent, error; Ferryx uses generic icons. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D9** | Tool Output Drawer Presentation | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:860-920` | `ui/src/remote/chat/MobileChatComponents.tsx:261-279, 326-333` | T3 nests tool stdout/stderr inside row's expandable drawer with 240px max-height; Ferryx dumps into inline pre block or loses it. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D10** | "Worked for" Duration Calculation | Fixed | `apps/mobile/src/lib/threadActivity.ts:1740-1780` | `ui/src/remote/agentConversation.ts:111-147, 178-183, 289-291` | T3 calculates turn duration from user prompt to final answer; Ferryx calculates per-record deltas producing "0s" / "5s" toggles. | `ui/src/remote/agentConversation.ts` |
| **D11** | Metadata Row (Timestamp / Copy) Multiplicity | Fixed | `apps/mobile/src/features/threads/ThreadFeed.tsx:1600-1620` | `ui/src/remote/chat/MobileChatMessage.tsx:168-187, 267` | T3 shows timestamp and copy button once at the end of the turn; Ferryx renders them on every record fragment. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D12** | Empty / Arrow-Only Message Suppression | Fixed | `apps/mobile/src/lib/threadActivity.ts:1530-1550` | `ui/src/remote/agentConversation.ts:37-43, 185-192, 284-307`, `ui/src/remote/chat/MobileChatMessage.tsx:219` | T3 suppresses empty or whitespace-only messages; Ferryx renders 63 empty boxes for arrow-only records. | `ui/src/remote/agentConversation.ts`, `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D13** | Thinking Block Integration | Fixed | `apps/mobile/src/lib/threadActivity.ts:1680-1710` | `ui/src/remote/agentConversation.ts:232-237`, `ui/src/remote/chat/MobileChatComponents.tsx:288-336`, `ui/src/remote/chat/MobileChatMessage.tsx:206-215` | T3 folds thinking blocks into the unified turn work log; Ferryx renders a standalone yellow callout box per record. | `ui/src/remote/chat/MobileChatMessage.tsx`, `ui/src/remote/chat/MobileChatComponents.tsx`, `ui/src/remote/agentConversation.ts` |
| **D14** | Live Follow & Shimmer Sweep | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:240-340` | `ui/src/remote/chat/MobileChatComponents.tsx:214-250` | T3 animates an active 72px sweep gradient over running work rows; Ferryx uses static or pinging badge dots. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D15** | Prose Arrows Mistaken for Tool Markers | Fixed | `apps/mobile/src/features/threads/ThreadFeed.tsx:818` | `ui/src/remote/agentConversation.ts:33` | Unanchored marker regex matched any `→ word` inside prose; line-anchoring ensures only whole marker lines are extracted or removed. | `ui/src/remote/agentConversation.ts` |
| **D16** | Pending Approval / Activity Hidden Inside Collapsed Fold | Fixed | `apps/mobile/src/features/threads/ThreadDetailScreen.tsx:1034` | `ui/src/remote/chat/MobileChatMessage.tsx:252` | Approval card and activity indicator rendered inside collapsed fold, making pending approvals invisible. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D17** | Detail-less Rows Interactive; Duplicated Shimmer Style; Pulse + Shimmer | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:749` | `ui/src/remote/chat/MobileChatComponents.tsx:215` | Every row was an interactive button even with nothing to expand, duplicated shimmer style blocks per row, and combined pulse with shimmer. | `ui/src/remote/chat/MobileChatComponents.tsx`, `ui/src/index.css` |
| **D18** | Duration Excluded the User Prompt | Fixed | `apps/mobile/src/lib/threadActivity.ts:1747-1750` | `ui/src/remote/agentConversation.ts:193` | "Worked for" was measured from the first assistant record rather than from user prompt start. | `ui/src/remote/agentConversation.ts` |
| **D19** | Live Last Turn Duration Froze | Fixed | `apps/mobile/src/features/threads/floating-working-control.tsx:392-400` | `ui/src/remote/agentConversation.ts:126` | Turn duration caches froze the live turn's label at its first computed value. | `ui/src/remote/agentConversation.ts` |
| **D20** | Truncated Single-Line Thinking Not Expandable | Fixed | `apps/mobile/src/lib/threadActivity.ts:1018-1023` | `ui/src/remote/chat/MobileChatComponents.tsx:319` | Single-line thoughts of 50-80 characters truncated with ellipsis could not be opened because expansion required newlines or >80 characters. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D21** | System Records Hoisted Above Turn Prose | Fixed | `apps/mobile/src/lib/threadActivity.ts:426` | `ui/src/remote/agentConversation.ts:253` | System records inside an assistant turn span were emitted before the turn's final prose, breaking conversation chronology. | `ui/src/remote/agentConversation.ts` |
| **D22** | Error/Running States Conveyed by Color Only (A11y) | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:772` | `ui/src/remote/chat/MobileChatComponents.tsx:250-251` | Work rows signaled running or failed states solely through icon color or shimmer, leaving screen reader users without state feedback. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D23** | Dead initiallyExpanded Prop | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:778` | `ui/src/remote/chat/MobileChatComponents.tsx:160` | `initiallyExpanded` remained on `ToolCallCardProps` and in caller signatures after row-level default expansion was removed. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D24** | Live Last-Turn Duration Frozen When Span Has Multiple Timestamps | Fixed | `apps/mobile/src/features/threads/floating-working-control.tsx:389-416` | `ui/src/remote/agentConversation.ts:133` | Live last turn duration froze once the span had at least two timestamps, ignoring ongoing wall clock time. | `ui/src/remote/agentConversation.ts` |
| **D25** | toolResult Without toolCallId Orphaned Id-Keyed Cards | Fixed | `apps/mobile/src/lib/threadActivity.ts:529-532` | `ui/src/remote/agentConversation.ts:252` | Incoming `toolResult` records lacking a `toolCallId` failed id lookup and orphaned open cards. | `ui/src/remote/agentConversation.ts` |
| **D26** | CRLF Marker Lines Stripped but Not Extracted | Fixed | n/a | `ui/src/remote/agentConversation.ts:36` | CRLF-terminated marker lines were stripped by regex without matching the extraction pattern, dropping the tool call. | `ui/src/remote/agentConversation.ts` |
| **D27** | Marker Lines Inside Fenced Code Treated as Tool Calls | Fixed | n/a | `ui/src/remote/agentConversation.ts:45-46` | Lines starting with `→` inside markdown code fences were incorrectly extracted as tool markers. | `ui/src/remote/agentConversation.ts` |
| **D28** | Frames Test Clicked Work Rows by Position | Fixed | n/a | `ui/src/remote/chat/remoteAppChatFrames.test.tsx:668` | `remoteAppChatFrames.test.tsx` targeted work rows by brittle index positions that failed on reordered cards. | `ui/src/remote/chat/remoteAppChatFrames.test.tsx` |
| **D29** | Turn Id Migrated as Prose Arrived, Remounting the Fold | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:436` | `ui/src/remote/agentConversation.ts:361` | Turn id shifted from intermediate record ordinals to final record ordinal, remounting and collapsing open folds during streaming. | `ui/src/remote/agentConversation.ts` |
| **D30** | Matched Tool Result Dropped Reported Error Status | Fixed | `apps/mobile/src/lib/threadActivity.ts:1023` | `ui/src/remote/agentConversation.ts:295` | The matched branch dropped `item.status`, losing error status when `isError` was not set. | `ui/src/remote/agentConversation.ts` |
| **D31** | Matched Tool Result Dropped durationMs | Fixed | `apps/mobile/src/lib/threadActivity.ts:1747-1757` | `ui/src/remote/agentConversation.ts:296` | The matched card did not copy `item.durationMs`, breaking the summed per-tool duration path. | `ui/src/remote/agentConversation.ts` |
| **D32** | Approval Card and Activity Rendered Above Assistant Prose | Fixed | `apps/mobile/src/features/threads/ThreadDetailScreen.tsx:1034` | `ui/src/remote/chat/MobileChatMessage.tsx:263,333` (body at 263, approval at 333) | Pending approval card and activity indicator rendered above assistant prose instead of after it. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D33** | Dead Global-Flag Marker Regex Export | Fixed | n/a | `ui/src/remote/agentConversation.ts:33` (export removed; the local `markerRegex` remains) | Unused `/gm` regex export was dead code and risked stateful regex reuse issues. | `ui/src/remote/agentConversation.ts` |
| **D34** | Prompt Timestamp Parsed Twice | Fixed | `apps/mobile/src/lib/threadActivity.ts:1601` | `ui/src/remote/agentConversation.ts:167` | Prompt timestamp was parsed redundantly instead of using the normalized timestamp directly. | `ui/src/remote/agentConversation.ts` |
| **D35** | Historical Turn Could Cache a Now-Relative Label | Fixed | `apps/mobile/src/lib/threadActivity.ts:1747-1749` | `ui/src/remote/agentConversation.ts:188` | Non-last spans evaluated the `activeTurnStartedAt` branch, allowing completed turns to freeze a now-relative duration. | `ui/src/remote/agentConversation.ts` |
| **D36** | Result With an Id Consumed an Id-less Card | Fixed | `apps/mobile/src/lib/threadActivity.ts:740-751` | `ui/src/remote/agentConversation.ts:268` | Results with a `toolCallId` fell through to consume id-less open cards before checking id-keyed cards. | `ui/src/remote/agentConversation.ts` |
| **D37** | Duplicated, Untrimmed Content Computation | Fixed | `apps/mobile/src/features/threads/ThreadFeed.tsx:818` | `ui/src/remote/agentConversation.ts:370` | Terminal assistant prose content was recomputed redundantly without standard prose cleaning. | `ui/src/remote/agentConversation.ts` |
| **D38** | Folded Assistant Prose Labelled Thinking | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:1133-1140` | `ui/src/remote/agentConversation.ts:317`, `ui/src/remote/chat/MobileChatComponents.tsx:310,316-325` | Intermediate assistant prose fragments folded into work items were mislabelled as "Thinking" with brain icons. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D39** | Thinking Test Asserted a Nonexistent Threshold | Fixed | n/a | `ui/src/remote/chat/MobileChatMessage.test.tsx:350` | Thinking test checked a tautological length threshold rather than actual expandability contract. | `ui/src/remote/chat/MobileChatMessage.test.tsx` |
| **D40** | Shimmer Test Verified a Deletion Only | Fixed | n/a | `ui/src/remote/chat/MobileChatMessage.test.tsx:335` | Test checked only absence of pulse class without asserting the running screen-reader announcement. | `ui/src/remote/chat/MobileChatMessage.test.tsx` |
| **D41** | Tests Pinned a Color Constant and a Dead Prop | Fixed | n/a | `ui/src/remote/chat/MobileChatMessage.test.tsx:40` | Tests pinned implementation-detail CSS classes and passed the removed `durationMs={450}` prop. | `ui/src/remote/chat/MobileChatMessage.test.tsx` |
| **D42** | Redundant and Misnamed Mapper Tests | Fixed | n/a | `ui/src/remote/agentConversation.test.ts:344,428` | Mapper test suite contained redundant test pairs and lacked a size assertion on duration cache maps. | `ui/src/remote/agentConversation.test.ts` |
| **D43** | Rejected: WorkRowsContainer Effect Deps | n/a (rejected) | `apps/mobile/src/features/threads/thread-work-log.tsx:512-519` | `ui/src/remote/chat/MobileChatMessage.tsx:132` | Removing `children` from deps breaks overflow detection when streamed content grows rows without changing row count. | `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D44** | Idle Last Turn Lost Its Settled Duration and Kept Running Cards | Fixed | `apps/mobile/src/lib/threadActivity.ts:1747-1749` | `ui/src/remote/agentConversation.ts:151-170` resolveDuration (`turnActive`) | When the agent goes idle, RemoteApp clears activeTurnStartedAt, but the mapper still treated the last span as live, so it ignored the settled label in turnDurationsMap (the label snapped to "0s" on the 927.5 wire) and left unanswered cards running. | `ui/src/remote/agentConversation.ts` |
| **D45** | Empty-Output Placeholder Parsed Without a Named Producer | Fixed (comment only) | n/a (T3 has no flattened wire) | `ui/src/remote/agentConversation.ts:78-79` parseToolResultOutput (producer: `src-tauri/src/agent_transcript.rs:334`) | The reviewer could not see where `← <tool> result` came from. | `ui/src/remote/agentConversation.ts` |
| **D46** | Index-Based Work-Row Keys Remounted Expanded Rows | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:436` | `ui/src/remote/agentConversation.ts:307-343`, `ui/src/remote/chat/MobileChatMessage.tsx:255-257` | Index-based keys remounted rows and reset local expanded state when earlier entries arrived. | `ui/src/remote/agentConversation.ts`, `ui/src/remote/chat/MobileChatMessage.tsx` |
| **D47** | Shimmer Test Name Promised Unchecked Assertions | Fixed | n/a (test-only) | `ui/src/remote/chat/MobileChatMessage.test.tsx:335-346` | Test name promised unchecked assertions on work-shimmer rows. | `ui/src/remote/chat/MobileChatMessage.test.tsx` |
| **D48** | Vacuous Historical-Duration Assertion and any-Casts | Fixed | n/a (test-only) | `ui/src/remote/agentConversation.test.ts:591-603` | Test contained vacuous historical duration assertions and loose any-casts. | `ui/src/remote/agentConversation.test.ts` |
| **D49** | Dead eval Branch in getToolVerb | Fixed | `apps/mobile/src/lib/threadActivity.ts:2300-2314` | `ui/src/remote/chat/MobileChatComponents.tsx:190-193` getToolVerb | Dead eval branch remained in getToolVerb after normalization refactors. | `ui/src/remote/chat/MobileChatComponents.tsx` |
| **D50** | Settled Turn Duration Preferred Tool-Time Sum Over Prompt Span | Fixed | `apps/mobile/src/lib/threadActivity.ts:1747-1752` (elapsed from turn start boundary to last entry) | `ui/src/remote/agentConversation.ts:178-184` resolveDuration | With no cached label (history reload), an idle turn fell back to the sum of card durationMs before the timestamp span, so a turn shown as "2m" while live could reload as "35s". | `ui/src/remote/agentConversation.ts`, `ui/src/remote/agentConversation.test.ts:591` |
| **D51** | Empty toolCalls Array Disabled Legacy Marker Handling | Fixed | n/a (T3 has no flattened wire) | `ui/src/remote/agentConversation.ts:70-72` cleanAssistantProse, `:323` processSpan guard | `toolCalls: []` plus "→ <tool>" lines leaked markers into prose and produced no work rows. | `ui/src/remote/agentConversation.ts`, `ui/src/remote/agentConversation.test.ts:618` |
| **D52** | Prose Work Row aria-label Hid Its Preview From Assistive Tech | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:771-772` (accessible name is the preview) | `ui/src/remote/chat/MobileChatComponents.tsx:341` ThinkingBlock | `aria-label="Message"` overrode the row content, so screen readers announced only "Message, button". | `ui/src/remote/chat/MobileChatComponents.tsx`, `ui/src/remote/chat/MobileChatMessage.test.tsx:426` |
| **D53** | Dead Binding and Tautological Assertion in Leak Test | Fixed | n/a (test-only) | `ui/src/remote/agentConversation.test.ts:633` | An unused `lastAssistant` binding and a `not.toContain("10m")` implied by the preceding exact assertion. | `ui/src/remote/agentConversation.test.ts` |
| **D54** | Conditional expect Could Silently Self-Skip | Fixed | n/a (test-only) | `ui/src/remote/agentConversation.test.ts:725` | `if (card && "toolName" in card) { expect(...) }` asserted nothing when the guard was false; the following toMatchObject already covers it. | `ui/src/remote/agentConversation.test.ts` |
| **D55** | Whitespace-Only Assistant Content Rendered an Empty Body | Fixed | `apps/mobile/src/features/threads/ThreadFeed.tsx:1698` (body renders only when `renderedText.trim().length > 0`) | `ui/src/remote/chat/MobileChatMessage.tsx:171,263` | The prose body was gated on raw `content` truthiness while the meta row used `hasProse`, so whitespace-only content produced an empty body without a meta row. | `ui/src/remote/chat/MobileChatMessage.tsx`, `ui/src/remote/chat/MobileChatMessage.test.tsx:286` |
| **D56** | Duplicate Approval-Fold Test Fixture | Fixed | n/a (test-only) | `ui/src/remote/chat/MobileChatMessage.test.tsx:361` | Two tests built the same approval fixture; the collapsed-fold one is removed and its unique assertions are merged into the document-order test. | `ui/src/remote/chat/MobileChatMessage.test.tsx` |
| **D57** | Text-Only Scroll Panes Were Not Keyboard-Scrollable | Fixed | `apps/mobile/src/features/threads/thread-work-log.tsx:1220-1227` (detail pane is a native ScrollView, scrollable by assistive tech) | `ui/src/remote/chat/MobileChatComponents.tsx:289,298,385` | The `max-h-60 overflow-y-auto` command pre, output pre and expanded thinking paragraph held only text, so keyboard users could not scroll them (axe scrollable-region-focusable). | `ui/src/remote/chat/MobileChatComponents.tsx`, `ui/src/remote/chat/MobileChatMessage.test.tsx:336-337` |

---

## Resolution & Empirical Evidence (D1–D49)

### D1: Turn Aggregation — Status: Fixed
* **T3 Reference:** `apps/mobile/src/lib/threadActivity.ts:1640-1740`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:149-322`
* **Resolution & Evidence:** `mapAgentConversation` now partitions raw daemon records into user-delineated `SpanGroup` spans. Within `processSpan`, all intermediate assistant message fragments, tool calls, and thinking blocks occurring between user prompts are accumulated into a single turn-level `workList`, resolving to a single terminal assistant message (`mapped.push` at lines 284–307). As measured in live after-evidence `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json`, assistant message bodies match user prompt boundaries exactly (baseline session: 95 bodies across 4 user messages; after (live capture of current session): 2 assistant message bodies across 2 user prompt boundaries, with `emptyBodies: 0`). Verified visually in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-collapsed.png`.

### D2: Tool Marker Stripping — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/ThreadFeed.tsx:1550-1610`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:37-43, 287-289`, `ui/src/remote/chat/MobileChatMessage.tsx:219`
* **Resolution & Evidence:** `cleanAssistantProse()` strips legacy tool tokens via `text.replace(/(^|\s)→ [A-Za-z0-9_.:-]+(?=\s|$)/g, "$1")` before text is rendered or assigned to content. Intermediate records with cleaned prose are converted to thinking items (`kind: "thinking"`) rather than leaking markdown arrows. Verified in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json` (baseline session: 86 bodies with arrows, 63 arrow-only bodies; after (live capture of current session): `toolMarkerLines: 0`, `rawToolOutput: 0`, `emptyBodies: 0`). In the valid after-capture, the only "→" characters in message bodies (`proseArrows: 2`, `proseArrowLines`) are genuine human prose explanations (e.g., `"spctl -a -t exec → accepted..."`, `"실행 확인 → 프로세스..."`), with zero CLI tool marker leaks. Visual proof of clean prose canvas is captured in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-collapsed.png`.

### D3: Legacy 927.5 Wire Tool Extraction — Status: Fixed
* **T3 Reference:** `apps/mobile/src/lib/threadActivity.ts:1400-1480`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:216-250, 272-282`
* **Resolution & Evidence:** In incoming assistant records without structured `toolCalls`, `matchAll(/(?:^|\s)→ ([A-Za-z0-9_.:-]+)(?=\s|$)/g)` extracts tool names into `ToolCallCardProps` with `status: "running"` and pushes them to `openCardsWithoutId`. When subsequent `role: "toolResult"` (or `isToolRole`) records arrive, `openCardsWithoutId.shift()` correlates each result with its triggering call, attaching stdout/stderr via `parseToolResultOutput()` and marking `status: "success"` or `"error"`. Live verification in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json` shows tool activities parsed into structured work rows (baseline session: 0 tool cards; after (live capture of current session): 52 expanded rows with `rowVerbCounts`: Ran 36, Updated 9, Thinking 5, Read 1, Fetched 1) with 0 raw tool output leaks (`rawToolOutput: 0`).

### D4: Work Fold Multiplicity — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:440-475`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:285-307`, `ui/src/remote/chat/MobileChatMessage.tsx:189-204`
* **Resolution & Evidence:** Ferryx now assigns a single `durationLabel` per assistant turn span instead of per daemon record. `MobileChatMessage` renders exactly one `<button data-testid="worked-for-toggle">` per assistant message, folding all encapsulated tool calls and thoughts behind a single disclosure chevron. In `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json`, toggle count is 1 per turn (baseline session: 87 toggles across 95 records; after (live capture of current session): exactly 2 toggles: `"Worked for 2m 14s"` and `"Worked for 8m 3s"`). Live screenshot evidence in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-collapsed.png` (collapsed) and `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png` (expanded).

### D5: Work Row Sizing & Typography — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:380-410`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatComponents.tsx:226-259, 298-323`, `ui/src/remote/chat/MobileChatMessage.tsx:160-163`
* **Resolution & Evidence:** `ToolCallCard` and `ThinkingBlock` now render as compact work rows with `min-h-[32px]` (matching T3's `THREAD_WORK_ROW_MIN_HEIGHT = 32`), `gap-px` (1px spacing), `w-6 h-6` container for 14px monochrome icons (`w-3.5 h-3.5`), and single-line `text-sm` leading-none truncated typography. In `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png`, the 52 work rows render in a compact 32px list without bulky card borders.

### D6: Work Group Scroll & Height Cap — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:490-540`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatMessage.tsx:142-166`
* **Resolution & Evidence:** All turn work rows are wrapped in `WorkRowsContainer`, which enforces `max-h-64` (256px, matching T3's `WORK_GROUP_MAX_HEIGHT = 256`), `overflow-y-auto`, and `scrollbar-thin`. This prevents multi-tool runs from creating unbounded page-length walls. Live behavior verified in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png` and `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json`.

### D7: Edge-Fade Gradients — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:690-740`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatMessage.tsx:142-165`
* **Resolution & Evidence:** `WorkRowsContainer` monitors scroll height via a `ResizeObserver` and applies top and bottom 12px linear gradient masks (`maskImage` and `WebkitMaskImage: linear-gradient(to bottom, transparent 0, black 12px, black calc(100% - 12px), transparent 100%)`) whenever rows exceed 8 items or container overflow is detected (`overflows`). This provides the exact visual equivalent of T3's 12px SVG linear gradient overlays on web canvas. Verified in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png`.

### D8: Work Row Semantic Icons — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:340-380`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatComponents.tsx:163-192, 233-241, 305-313`
* **Resolution & Evidence:** `getToolIcon()` and `getToolVerb()` provide semantic icon and label mapping: `Terminal` for bash/eval/exec ("Ran eval"), `Eye` for read/view ("Read"), `SquarePen` for edit/write ("Updated"), `Search` for grep/lsp ("Searched"), `Globe` for web/browser ("Fetched"), `Sparkles` for agent/task, and `Brain` for reasoning blocks ("Thinking"). Errors are rendered with rose/danger coloring (`text-[#ff6467]`). Visible across all 52 work rows in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png` and documented in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json` (`rowVerbCounts`: Ran 36, Updated 9, Thinking 5, Read 1, Fetched 1).

### D9: Tool Output Drawer Presentation — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:860-920`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatComponents.tsx:261-279, 326-333`
* **Resolution & Evidence:** Tapping any work row or thinking block toggles an indented output drawer (`ml-7 border-l border-[#191919] pl-3 py-1`). Monospace output and command text render with selectable text (`select-text`), 12px font (`text-[12px] font-mono text-[#838383]`), and a capped height of `max-h-60` (240px) with internal `overflow-y-auto`. Captured in live drawer screenshot `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-row-drawer.png`.

### D10: "Worked for" Duration Calculation — Status: Fixed
* **T3 Reference:** `apps/mobile/src/lib/threadActivity.ts:1740-1780`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:111-147, 178-183, 289-291`
* **Resolution & Evidence:** `resolveDuration()` derives turn duration by computing the total time elapsed across the span's collected timestamps (`Math.max(...timestamps) - Math.min(...timestamps)`) or summing known tool durations, formatted via `formatWorkedDuration()`. Sub-second adjacent record deltas no longer produce "Worked for 0s" or "Worked for 5s" micro-toggles. In the valid live capture, there are no micro-toggles: `toggleLabels` in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json` are `["Worked for 2m 14s", "Worked for 8m 3s"]` (baseline session had 87 per-record micro-toggles such as 5s, 9s, 14s).

### D11: Metadata Row (Timestamp / Copy) Multiplicity — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/ThreadFeed.tsx:1600-1620`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatMessage.tsx:168-187, 267`
* **Resolution & Evidence:** `metaRow` (containing formatted timestamp and copy button with feedback) is rendered strictly once at the footer of the assistant turn, guarded by `{hasProse && metaRow}`. Because records within a turn are coalesced, metadata multiplicity is eliminated (baseline session: 87 repetitive timestamp and copy rows across 95 records; after (live capture of current session): strictly 1 timestamp and copy button per completed assistant turn). Visual evidence in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-collapsed.png`.

### D12: Empty / Arrow-Only Message Suppression — Status: Fixed
* **T3 Reference:** `apps/mobile/src/lib/threadActivity.ts:1530-1550`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:37-43, 185-192, 284-307`, `ui/src/remote/chat/MobileChatMessage.tsx:219`
* **Resolution & Evidence:** `cleanAssistantProse()` strips arrow markers, and `processSpan()` checks for substantive prose length. Records containing only tool arrows yield 0 prose characters and are aggregated into `workList` without generating empty message bodies. `MobileChatMessage` additionally guards prose rendering with `{content && ...}`. In `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json`, `emptyBodies: 0` (baseline session: 63 arrow-only empty boxes; after (live capture of current session): 0 empty bodies).

### D13: Thinking Block Integration — Status: Fixed
* **T3 Reference:** `apps/mobile/src/lib/threadActivity.ts:1680-1710`
* **Ferryx Implementation:** `ui/src/remote/agentConversation.ts:232-237`, `ui/src/remote/chat/MobileChatComponents.tsx:288-336`, `ui/src/remote/chat/MobileChatMessage.tsx:206-215`
* **Resolution & Evidence:** Thinking content from `item.thinking` and intermediate assistant prose fragments are pushed to `workList` as `{ kind: "thinking", text }`. They render inside the turn's `WorkRowsContainer` under the single "Worked for" toggle as neutral 32px rows with a `Brain` icon, replacing the legacy isolated yellow callout boxes. As recorded in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-dump.json`, 5 thinking rows are integrated cleanly into the work group (`thinking: 5`, `rowVerbCounts.Thinking: 5`). Visual evidence in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/after-expanded.png`.

### D14: Live Follow & Shimmer Sweep — Status: Fixed
* **T3 Reference:** `apps/mobile/src/features/threads/thread-work-log.tsx:240-340`
* **Ferryx Implementation:** `ui/src/remote/chat/MobileChatComponents.tsx:214-250`
* **Resolution & Evidence:** When a tool call is in `status === "running"`, `ToolCallCard` injects CSS `@keyframes work-shimmer` animating a 200% background-size linear gradient (`linear-gradient(90deg, #838383 0%, #f5f5f5 50%, #838383 100%)`) with a 1.35s linear cycle, clipped to text via `-webkit-background-clip: text` on `.work-shimmer-text` with `motion-safe:animate-pulse`. This reproduces T3's 72px 1,350ms active shimmer sweep across running work rows.

### D15: Prose Arrows Mistaken for Tool Markers — Status: Fixed
- **Defect (found in orchestrator audit, round 1)**: the unanchored marker regex matched any `→ word` inside prose. "Open Account settings → Tenants." lost "Tenants." and produced a phantom `Tenants.` tool row. T3 renders tool activity only from structured activities (`apps/mobile/src/lib/threadActivity.ts`), so prose is never rewritten.
- **T3 reference**: `apps/mobile/src/features/threads/ThreadFeed.tsx:818` renders assistant text through `<Markdown>` unchanged; tool activity comes only from structured activities (`apps/mobile/src/lib/threadActivity.ts:426`), so prose arrows are never rewritten.
- **Fix**: the regex is now line-anchored: `LEGACY_TOOL_MARKER_REGEX = /^→ ([A-Za-z0-9_.:-]+)[ \t]*$/gm` (`ui/src/remote/agentConversation.ts:33`). Only whole marker lines are removed or extracted.
- **Evidence**: `agentConversation.test.ts` tests "keeps prose arrows unchanged" and "Settings → Tenants.\n→ bash" (the body keeps the arrow, and there is exactly one bash row).

### D16: Pending Approval / Activity Hidden Inside Collapsed Fold — Status: Fixed
- **Defect**: the approval card and activity indicator rendered inside the collapsed "Worked for" fold, so a pending approval was invisible. T3 keeps pending approvals outside the work log (`apps/mobile/src/features/threads/thread-work-log.tsx`).
- **T3 reference**: `apps/mobile/src/features/threads/ThreadDetailScreen.tsx:1034` renders `PendingApprovalCard` outside the work log, below the feed, so a pending approval is always visible.
- **Fix**: only `WorkRowsContainer` is gated by the fold (`ui/src/remote/chat/MobileChatMessage.tsx:252`). `ApprovalActionCard` (`:265`) and `ActivityIndicator` (`:271`) render unconditionally.
- **Evidence**: `MobileChatMessage.test.tsx:286` (approval visible while collapsed, with no work-row rendered).

### D17: Detail-less Rows Interactive; Duplicated Shimmer Style; Pulse + Shimmer — Status: Fixed
- **Defect**: every row was a button, even with nothing to expand. Each running row emitted its own `<style>` block, and running rows combined `animate-pulse` with the shimmer.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:749` takes `canExpand` from the row and `:778` sets `accessibilityState` only when it is true; the shimmer is one shared component, `ShimmeringWorkContent` (`:204-223`), with no per-row style block and no pulse.
- **Fix**: rows are gated by `canExpand` (`MobileChatComponents.tsx:215` for tools, `:319` for thinking), and detail-less rows render as a non-interactive `div` without `aria-expanded`. The shimmer CSS is defined once in `ui/src/index.css:238-251`, and `animate-pulse` was removed.
- **Evidence**: `MobileChatMessage.test.tsx` tests: a detail-less row is a DIV, a row with output expands, two running cards emit 0 `<style>` elements and both carry `.work-shimmer-text`, and short thinking text is non-interactive.

### D18: Duration Excluded the User Prompt — Status: Fixed
- **Defect**: "Worked for" was measured from the first assistant record, not from the user prompt. T3 measures from turn start.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1747-1750` measures elapsed time from the turn's `startedAt`, which is when the user prompt starts the turn (`computeElapsedMs`, `:1601`).
- **Fix**: the previous user record's timestamp is passed to `processSpan` and included in the span timestamps (`agentConversation.ts:193`, `:345`).
- **Evidence**: test "measures turn duration from user prompt timestamp" (prompt 0, done 60 gives "1m", not "30s").

### D19: Live Last Turn Duration Froze — Status: Fixed
- **Defect**: the `turnDurationsMap` and `prevById` caches froze the live turn's label at its first value.
- **T3 reference**: `apps/mobile/src/features/threads/floating-working-control.tsx:392-400` re-renders every 1,000 ms from `startedAt` to `Date.now()`, so the live duration keeps growing.
- **Fix**: `resolveDuration(..., isLastSpan)` (`agentConversation.ts:126`) bypasses and never writes the caches for the last span.
- **Evidence**: test "recomputes live last turn duration" (`0s` changes to `2m` across calls with a shared map).

### D20: Truncated Single-Line Thinking Not Expandable — Status: Fixed
- **Defect (found in orchestrator audit, round 3)**: collapsed thinking rows render the first line inside a `truncate` container, which fits about 45-55 characters at 430px. `canExpand` required a newline or more than 80 characters, so a single-line thought of 50-80 characters was cut off with an ellipsis and could not be opened. T3 lets every reasoning entry in the work log be opened (`apps/mobile/src/features/threads/thread-work-log.tsx`).
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1018-1023` (`workEntryCanExpand`) makes any entry with detail text expandable, and `apps/mobile/src/features/threads/thread-work-log.tsx:749` reads `row.canExpand`.
- **Fix**: `canExpand = trimmed.length > 0` (`ui/src/remote/chat/MobileChatComponents.tsx:319`). Only empty thinking text stays a non-interactive div.
- **Evidence**: in `MobileChatMessage.test.tsx`, empty text renders a DIV with no `aria-expanded`, and a 70-character single-line thought renders a button that expands to show the full text.

### D21: System Records Hoisted Above Turn Prose — Status: Fixed
- **Defect (found in code review round 5)**: system records inside an assistant turn span were emitted before the turn's final prose, breaking conversation chronology. T3 orders messages by creation in `apps/mobile/src/features/threads/thread-work-log.tsx` and the thread feed.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:426` orders every activity with `Arr.sort(activities, activityOrder)`, and `:1516-1532` ranks the lifecycle, so feed items follow creation order.
- **Fix**: non-turn records are collected with their ordinal (`ui/src/remote/agentConversation.ts:253`, `:260-262`) and inserted after the turn at the ordinal of its last prose record (`:392-395`).
- **Evidence**: `agentConversation.test.ts` test "maps assistant followed by system record to ids in ordinal order".

### D22: Error/Running States Conveyed by Color Only (A11y) — Status: Fixed
- **Defect (found in code review round 5)**: work rows signaled running or failed states solely through icon color or shimmer, leaving screen reader users without state feedback.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:772` sets `accessibilityLabel` to `"..., tool call failed"` for failed rows, and `:1029` announces `${summary.title}, ${summary.status}`; state is never color-only.
- **Fix**: `ToolCallCard` emits sr-only "Failed" and "Running" text (`ui/src/remote/chat/MobileChatComponents.tsx:250-251`).
- **Evidence**: `MobileChatMessage.test.tsx` asserts the error row text contains "Failed" and the success row does not.

### D23: Dead initiallyExpanded Prop — Status: Fixed
- **Defect (found in code review round 5)**: `initiallyExpanded` remained on `ToolCallCardProps` and in caller signatures after row-level default expansion was removed.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:778` and `:1028` derive `accessibilityState` from local `expanded` state gated by `canExpand`; there is no initially-expanded prop.
- **Fix**: removed from `ToolCallCardProps` (`ui/src/remote/chat/MobileChatComponents.tsx:160`), the component (`:203`), the mapper and all tests (grep for `initiallyExpanded` under `ui/src/remote` returns nothing).
- **Evidence**: tsc 0 (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/gate-tsc-r5.log`).

### D24: Live Last-Turn Duration Frozen When Span Has Multiple Timestamps — Status: Fixed
- **Defect (found in code review round 5)**: the live last turn duration froze once the span had at least two timestamps, ignoring ongoing wall clock time.
- **T3 reference**: `apps/mobile/src/features/threads/floating-working-control.tsx:389-416` shows a live duration from the turn's `startedAt` to `nowMs` (`formatWorkingDuration`), and `apps/mobile/src/lib/threadActivity.ts:1747-1757` uses `startedAt`/`completedAt` for finished turns.
- **Fix**: for the last span, `resolveDuration` measures from `activeTurnStartedAt` to wall clock (`agentConversation.ts:133`, `:168-188`).
- **Evidence**: test "updates live last turn duration from prompt timestamp when activeTurnStartedAt is set" (fake timers).

### D25: toolResult Without toolCallId Orphaned Id-Keyed Cards — Status: Fixed
- **Defect (found in code review round 5)**: incoming `toolResult` records lacking a `toolCallId` failed id lookup and orphaned open cards.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:529-532` reads `toolCallId` when present and `:740-751` keys the lifecycle row index by it; entries without an id still render as their own rows and are never dropped.
- **Fix**: an unmatched result fills the oldest still-open card, falling back through `openCardsWithoutId` (`agentConversation.ts:252`, `:276-277`, `:319`, `:333`, `:345`).
- **Evidence**: tests "falls back to oldest still-open card when toolResult has no toolCallId" and "... toolResult id isn't in openCalls ...".

### D26: CRLF Marker Lines Stripped but Not Extracted — Status: Fixed
- **Defect (found in code review round 5)**: CRLF-terminated marker lines were stripped by regex without matching the extraction pattern, dropping the tool call.
- **T3 reference**: n/a on the T3 side: T3 receives structured activities (`apps/mobile/src/lib/threadActivity.ts:104`), not text markers, so line endings never affect tool extraction. The fix makes the 927.5 text wire behave the same way.
- **Fix**: text is normalized with `rawText.replace(/\r\n/g, "\n")` before parsing (`agentConversation.ts:36`).
- **Evidence**: test "normalizes CRLF so '→ read\r\n→ read' gives empty content and two read cards".

### D27: Marker Lines Inside Fenced Code Treated as Tool Calls — Status: Fixed
- **Defect (found in code review round 5)**: lines starting with `→` inside markdown code fences were incorrectly extracted as tool markers.
- **T3 reference**: n/a on the T3 side: T3 renders assistant text only through `Markdown` (`apps/mobile/src/features/threads/ThreadFeed.tsx:65`), and tool activity never comes from prose, so fenced code is shown verbatim. The fix keeps fenced code verbatim.
- **Fix**: triple backtick fence tracking skips marker extraction inside fences (`agentConversation.ts:45-46`).
- **Evidence**: test "preserves fenced code blocks without stripping or extracting markers".

### D28: Frames Test Clicked Work Rows by Position — Status: Fixed
- **Defect (found in code review round 5)**: `remoteAppChatFrames.test.tsx` targeted work rows by brittle index positions that failed on reordered cards.
- **T3 reference**: n/a (test-only). T3's feed tests address rows by identity, not position (`apps/mobile/src/features/threads/thread-feed-live-follow.test.ts:18`).
- **Fix**: `remoteAppChatFrames.test.tsx:668` clicks every collapsed BUTTON row (`aria-expanded="false"`) and then asserts every tool output, independent of order.
- **Evidence**: vitest `src/remote` 36 files 539/539 (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/gate-vitest-r5.log`).

### D29: Turn Id Migrated as Prose Arrived, Remounting the Fold — Status: Fixed
- **Defect (found in code review round 5)**: the turn id shifted from intermediate record ordinals to the final record ordinal, remounting and collapsing open folds during streaming.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:436` keys work rows by the stable `row.id`, and `apps/mobile/src/lib/threadActivity.ts:54` carries a stable `turnId`, so a streaming turn never remounts.
- **Fix**: the turn id is `assistant-${span[0].ordinal}` (`agentConversation.ts:361`, `:378`), stable across streaming.
- **Evidence**: test "retains stable assistant turn id from first span record across progressive prose additions".

### D30: Matched Tool Result Dropped Reported Error Status — Status: Fixed
- **Defect (found in code review round 6)**: the matched branch dropped `item.status`, losing error status when `isError` was not set.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1023` builds the row state from the entry itself.
- **Fix**: the matched branch keeps `item.status` (`ui/src/remote/agentConversation.ts` processSpan) (`ui/src/remote/agentConversation.ts:295`).
- **Evidence**: test "matched toolResult with status error and no isError marks card error" (`ui/src/remote/agentConversation.test.ts:535`).

### D31: Matched Tool Result Dropped durationMs — Status: Fixed
- **Defect (found in code review round 6)**: the matched card did not copy `item.durationMs`, breaking the summed per-tool duration path.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1747-1757`.
- **Fix**: the matched card copies `item.durationMs`, so the summed per-tool duration path is live (`ui/src/remote/agentConversation.ts` processSpan) (`ui/src/remote/agentConversation.ts:296`).
- **Evidence**: the 90000 ms "1m 30s" test (`ui/src/remote/agentConversation.test.ts:562`).

### D32: Approval Card and Activity Rendered Above Assistant Prose — Status: Fixed
- **Defect (found in code review round 6)**: pending approval card and activity indicator rendered above assistant prose instead of after it.
- **T3 reference**: `apps/mobile/src/features/threads/ThreadDetailScreen.tsx:1034` renders `PendingApprovalCard` after the feed.
- **Fix**: both render after the prose body and stay outside the fold (`ui/src/remote/chat/MobileChatMessage.tsx`) (`ui/src/remote/chat/MobileChatMessage.tsx:263,333` (body at 263, approval at 333)).
- **Evidence**: the document-order test (`ui/src/remote/chat/MobileChatMessage.test.tsx:371`).

### D33: Dead Global-Flag Marker Regex Export — Status: Fixed
- **Defect (found in code review round 6)**: the unused `/gm` regex export was dead code and risked stateful regex reuse issues.
- **T3 reference**: n/a (T3 has no text marker parsing).
- **Fix**: the unused `/gm` export is removed (`ui/src/remote/agentConversation.ts`) (`ui/src/remote/agentConversation.ts:33` (export removed; the local `markerRegex` remains)).
- **Evidence**: tsc 0 (tsc 0).

### D34: Prompt Timestamp Parsed Twice — Status: Fixed
- **Defect (found in code review round 6)**: prompt timestamp was parsed redundantly instead of using the normalized value directly.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1601` (`computeElapsedMs` takes each timestamp once).
- **Fix**: resolveDuration uses the normalized value directly (`ui/src/remote/agentConversation.ts` resolveDuration) (`ui/src/remote/agentConversation.ts:167`).
- **Evidence**: the duration tests (`ui/src/remote/agentConversation.test.ts:344`).

### D35: Historical Turn Could Cache a Now-Relative Label — Status: Fixed
- **Defect (found in code review round 6)**: non-last spans evaluated the `activeTurnStartedAt` branch, allowing completed turns to freeze a now-relative duration.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1747-1749` uses `startedAt`/`completedAt` for finished turns.
- **Fix**: the non-last-span `activeTurnStartedAt` branch is removed (`ui/src/remote/agentConversation.ts` resolveDuration) (`ui/src/remote/agentConversation.ts:188`).
- **Evidence**: test with an untimestamped leading turn and `activeTurnStartedAt` (`ui/src/remote/agentConversation.test.ts:590`).

### D36: Result With an Id Consumed an Id-less Card — Status: Fixed
- **Defect (found in code review round 6)**: results with a `toolCallId` fell through to consume id-less open cards before checking id-keyed cards.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:740-751`.
- **Fix**: results with a `toolCallId` try id-keyed cards first (`ui/src/remote/agentConversation.ts` processSpan) (`ui/src/remote/agentConversation.ts:268`).
- **Evidence**: the mixed marker plus structured call test (`ui/src/remote/agentConversation.test.ts:428` and `:628`).

### D37: Duplicated, Untrimmed Content Computation — Status: Fixed
- **Defect (found in code review round 6)**: terminal assistant prose content was recomputed redundantly without standard prose cleaning.
- **T3 reference**: `apps/mobile/src/features/threads/ThreadFeed.tsx:818`.
- **Fix**: `content = cleanAssistantProse(proseRecord)` (`ui/src/remote/agentConversation.ts` processSpan) (`ui/src/remote/agentConversation.ts:370`).
- **Evidence**: the existing prose tests (the existing prose tests).

### D38: Folded Assistant Prose Labelled Thinking — Status: Fixed
- **Defect (found in code review round 6)**: intermediate assistant prose fragments folded into work items were mislabelled as "Thinking" with brain icons.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:1133-1140` uses `ThreadThinkingRow` only for reasoning.
- **Fix**: prose work items carry `source: "prose"` and render with a message icon, with no "Thinking" label (`ui/src/remote/chat/MobileChatComponents.tsx` ThinkingBlock) (`ui/src/remote/agentConversation.ts:317`, `ui/src/remote/chat/MobileChatComponents.tsx:310,316-325`).
- **Evidence**: the prose-row test (`ui/src/remote/chat/MobileChatMessage.test.tsx:399`).

### D39: Thinking Test Asserted a Nonexistent Threshold — Status: Fixed
- **Defect (found in code review round 6)**: thinking test checked a tautological length threshold rather than actual expandability contract.
- **T3 reference**: n/a (test-only).
- **Fix**: renamed, the tautological length assertion removed (`ui/src/remote/chat/MobileChatMessage.test.tsx`) (`ui/src/remote/chat/MobileChatMessage.test.tsx:350`).
- **Evidence**: vitest (vitest).

### D40: Shimmer Test Verified a Deletion Only — Status: Fixed
- **Defect (found in code review round 6)**: test checked only absence of pulse class without asserting the running screen-reader announcement.
- **T3 reference**: n/a (test-only).
- **Fix**: it now asserts the sr-only "Running" text (`ui/src/remote/chat/MobileChatMessage.test.tsx`) (`ui/src/remote/chat/MobileChatMessage.test.tsx:335`).
- **Evidence**: vitest (vitest).

### D41: Tests Pinned a Color Constant and a Dead Prop — Status: Fixed
- **Defect (found in code review round 6)**: tests pinned implementation-detail CSS classes and passed the removed `durationMs={450}` prop.
- **T3 reference**: n/a (test-only).
- **Fix**: the class assertion and `durationMs={450}` are removed (`ui/src/remote/chat/MobileChatMessage.test.tsx:40`).
- **Evidence**: vitest (vitest).

### D42: Redundant and Misnamed Mapper Tests — Status: Fixed
- **Defect (found in code review round 6)**: mapper test suite contained redundant test pairs and lacked a size assertion on duration cache maps.
- **T3 reference**: n/a (test-only).
- **Fix**: 2 pairs merged, and the cache test renamed with a `turnDurationsMap.size` assertion (`ui/src/remote/agentConversation.test.ts`) (`ui/src/remote/agentConversation.test.ts:344,428`).
- **Evidence**: vitest (vitest).

### D43: WorkRowsContainer Effect Deps — Status: n/a (rejected)
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:512-519` sizes the work group from measured content height (`contentHeight`), which updates as rows grow; Ferryx keeps `children` in the effect deps at `ui/src/remote/chat/MobileChatMessage.tsx:132` for the same reason.
- **Defect (found in code review round 6)**: proposal to remove `children` from effect dependencies in `WorkRowsContainer`.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx`.
- **Fix**: `children` stays in the deps because streamed text can grow existing rows without changing the row count (`ui/src/remote/chat/MobileChatMessage.tsx` WorkRowsContainer).
- **Evidence**: the ResizeObserver is on the max-h-64 container, whose box stops changing once clamped, so it would not fire.

### D44: Idle Last Turn Lost Its Settled Duration and Kept Running Cards — Status: Fixed
- **Defect (found in code review round 7)**: when the agent goes idle, RemoteApp clears activeTurnStartedAt, but the mapper still treated the last span as live, so it ignored the settled label in turnDurationsMap (the label snapped to "0s" on the 927.5 wire) and left unanswered cards running.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1747-1749` uses the finished turn's `completedAt`.
- **Fix**: `turnActive = isLastSpan && activeTurnStartedAt != null`; the cache lookup, cache write and running-to-success sweep all run whenever `!turnActive` (`ui/src/remote/agentConversation.ts` resolveDuration/processSpan).
- **Evidence**: the tests for the idle last turn keeping "1m 12s" and for the idle unanswered card being success versus running.

### D45: Empty-Output Placeholder Parsed Without a Named Producer — Status: Fixed (comment only)
- **Defect (found in code review round 7)**: the reviewer could not see where `← <tool> result` came from.
- **T3 reference**: n/a (T3 receives structured tool output).
- **Fix**: a comment in parseToolResultOutput names the producer, `src-tauri/src/agent_transcript.rs:334` `format!("← {tool} result")`.
- **Evidence**: the daemon test at `src-tauri/src/agent_transcript.rs:959`.

### D46: Index-Based Work-Row Keys Remounted Expanded Rows — Status: Fixed
- **Defect (found in code review round 7)**: index-based keys remounted rows and reset local expanded state when earlier entries arrived.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:436` keys rows by `row.id`.
- **Fix**: every work item carries a stable `workKey` (tool-<ordinal>-<k>, tool-<id>, result-<ordinal>, thinking-<ordinal>, prose-<ordinal>) and MobileChatMessage keys rows by it.
- **Evidence**: the test that workKeys are stable across a later record.

### D47: Shimmer Test Name Promised Unchecked Assertions — Status: Fixed
- **Defect (found in code review round 7)**: test name promised unchecked assertions on work-shimmer rows.
- **T3 reference**: n/a (test-only).
- **Fix**: renamed, and it now asserts the shimmer and the Running text on both rows.
- **Evidence**: vitest.

### D48: Vacuous Historical-Duration Assertion and any-Casts — Status: Fixed
- **Defect (found in code review round 7)**: test contained vacuous historical duration assertions and loose any-casts.
- **T3 reference**: n/a (test-only).
- **Fix**: exact "0s" assertion; the any-casts are replaced with typed narrowing.
- **Evidence**: vitest and tsc.

### D49: Dead eval Branch in getToolVerb — Status: Fixed
- **Defect (found in code review round 7)**: dead eval branch remained in getToolVerb after normalization refactors.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:2300-2314` derives the verb from the entry status.
- **Fix**: the branch is removed (`ui/src/remote/chat/MobileChatComponents.tsx` getToolVerb).
- **Evidence**: the existing "Ran eval" work-row tests.

### D50: Settled Turn Duration Preferred Tool-Time Sum Over Prompt Span — Status: Fixed
- **Defect (found in code review round 8)**: an idle turn without a cached label used the sum of card durationMs before the prompt-relative timestamp span.
- **T3 reference**: `apps/mobile/src/lib/threadActivity.ts:1747-1752` measures elapsed time from the turn's start boundary.
- **Fix**: `resolveDuration` (`ui/src/remote/agentConversation.ts:178-184`) now uses the timestamp span first, then the card sum, then "0s".
- **Evidence**: `agentConversation.test.ts:591` (2-minute span with a 5 s card reports "2m"); vitest 549/549.

### D51: Empty toolCalls Array Disabled Legacy Marker Handling — Status: Fixed
- **Defect (found in code review round 8)**: both the prose cleaner and the card builder keyed off `Array.isArray(item.toolCalls)`, so `[]` leaked markers and dropped cards.
- **T3 reference**: n/a (flattened wire only).
- **Fix**: `cleanAssistantProse` always strips markers (`agentConversation.ts:70-72`); card synthesis is gated on `item.toolCalls?.length` (`:323`).
- **Evidence**: `agentConversation.test.ts:618`; live r9 capture has 0 marker lines.

### D52: Prose Work Row aria-label Hid Its Preview From Assistive Tech — Status: Fixed
- **Defect (found in code review round 8)**: `aria-label="Message"` replaced the row's accessible name.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:771-772` names the row by its preview.
- **Fix**: the aria-label is removed from both branches; a visually hidden `Message: ` hint precedes the preview (`MobileChatComponents.tsx:341`).
- **Evidence**: `MobileChatMessage.test.tsx:426` (no aria-label, accessible name contains the first line); live DOM check `r9-dom-a11y-check.txt` (aria-label null, hint 1x1 clipped).

### D53: Dead Binding and Tautological Assertion in Leak Test — Status: Fixed
- **Defect (found in code review round 8)**: unused `lastAssistant` and a redundant `not.toContain("10m")`.
- **Fix**: both removed (`agentConversation.test.ts:633`).
- **Evidence**: tsc and vitest.

### D54: Conditional expect Could Silently Self-Skip — Status: Fixed
- **Defect (found in code review round 8)**: guarded `expect` blocks could pass without asserting.
- **Fix**: the guarded blocks are removed; the toMatchObject assertions remain (`agentConversation.test.ts:725`).
- **Evidence**: tsc and vitest.

### D55: Whitespace-Only Assistant Content Rendered an Empty Body — Status: Fixed
- **Defect (found in code review round 9)**: the body used `content` truthiness while the meta row used `hasProse`.
- **T3 reference**: `apps/mobile/src/features/threads/ThreadFeed.tsx:1698` renders the body only for non-blank text.
- **Fix**: the body is gated on `hasProse` (`MobileChatMessage.tsx:263`).
- **Evidence**: `MobileChatMessage.test.tsx:286` (whitespace-only content renders no body).

### D56: Duplicate Approval-Fold Test Fixture — Status: Fixed
- **Defect (found in code review round 9)**: two tests with the same fixture and overlapping assertions.
- **Fix**: one test removed; its three unique assertions merged into the document-order test (`MobileChatMessage.test.tsx:361`).
- **Evidence**: tsc and vitest.

### D57: Text-Only Scroll Panes Were Not Keyboard-Scrollable — Status: Fixed
- **Defect (found in code review round 10)**: the three `max-h-60 overflow-y-auto` text panes had no focusable element.
- **T3 reference**: `apps/mobile/src/features/threads/thread-work-log.tsx:1220-1227` renders the detail in a ScrollView.
- **Fix**: `tabIndex={0}` on the command pre, the output pre and the expanded thinking paragraph (`MobileChatComponents.tsx:289,298,385`).
- **Evidence**: `MobileChatMessage.test.tsx:336-337`.

Review coverage note: the round-9 and round-10 review prompts carried a truncated diff (3 of 6 file headers), so those rounds are not counted as full-diff reviews. From round 11 the diff is written by git to a file and the prompt builder refuses to spawn unless all six file headers are present.

Review round 5 finding #10 (unused lucide imports) was rejected: `noUnusedLocals` is enabled, tsc exits 0 and all four icons are referenced.

### Multi-Session Offline Validation — Status: Verified
* **Reference Evidence:** `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/offline-ferryx-mapping.json`
* **Resolution & Evidence:** Across 6 real Ferryx sessions (ranging from 25 to 136 work items each: `3d52fea3`: 114 items; `3f86cf0a`: 118 items; `6c0b074d`: 110 items; `b355b1db`: 136 items; `b7143d40`: 136 items; `d694c675`: 25 items), the normalizer mapping demonstrates consistent behavior:
  * `toolMarkerLines: 0` (zero leaked tool marker lines in message prose across all 6 sessions).
  * `rawToolOutputInBody: 0` (zero raw tool output leaking into assistant body text across all 6 sessions).
  * `turnsWithWorkButNoDuration: 0` (every turn containing work items successfully derives a duration label across all 6 sessions).

## After-Screenshots (final build, round 6)

Live capture on http://127.0.0.1:8899 at 430x900, app-served asset RemoteApp-CfXV4z-s.js (sha256 04c9da358abdd93c691b9c9bed01ebb66257a3ab686bc9b6223ea5d7d6b82d2e, identical in local dist, app bundle, 43821 and 8899 per `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/deploy-sha-r6.txt`), no workspace/session select clicks.

Results (from `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-dump.json`): 4 assistant bodies with 0 marker lines. 3 folds: "Worked for 2m 16s", "58m 40s", "3m 43s". Collapsed shows 0 work rows. Expanded shows 113 rows, all buttons: Ran 77, Read 8, Edited 9, Updated 5, and 3 folded prose rows (message icon, no "Thinking" label). The tool drawer and the prose-row expansion are both open.

Offline GET remap of the 6 ferryx sessions with history (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/offline-ferryx-mapping-r6.json`): toolMarkerLinesInBodies 0, rawToolOutputInBody 0, turnsWithWorkButNoDuration 0, against 447 raw marker records in the input.

| Item | After-evidence mapping |
|---|---|
| D1, D2, D3, D4, D11, D12, D15 | `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-collapsed.png` (one fold per turn, clean prose, one meta row, no arrow lines) |
| D5, D6, D7, D8, D14, D17, D21 | `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-expanded.png` |
| D9 | `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-row-drawer.png` (monospace output drawer under a Ran row) |
| D13, D20 | `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-thinking-open.png` (italic full thinking text) |
| D10, D18, D19, D24 | Fold labels in `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-collapsed.png`, plus the duration unit tests (time is not visually verifiable beyond the label) |
| D16 | Not visual in this session (no pending approval exists); covered by MobileChatMessage.test.tsx:286 |
| D22 | sr-only text is invisible by design; covered by the MobileChatMessage.test.tsx assertions |
| D23, D25, D26, D27, D28, D29 | Not visual (code/test-structure changes); covered by the named tests and `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/offline-ferryx-mapping-r6.json` (all ferryx sessions: toolMarkerLinesInBodies 0, rawToolOutputInBody 0, turnsWithWorkButNoDuration 0, against 447 raw marker records in the input) |
| D32, D38 | `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-thinking-open.png` and `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r6-expanded.png` (folded prose shows as a message-icon row without "Thinking"; approval order has no live instance in this session and is covered by `MobileChatMessage.test.tsx:371`) |
| D30, D31, D33, D34, D35, D36, D37 | not visual (mapper logic); covered by the named tests and `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/offline-ferryx-mapping-r6.json` |
| D39, D40, D41, D42 | not visual (test-only); vitest 543/543 (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/gate-vitest-r6.log`) |
| D43 | n/a (rejected, no change) |
| D44, D45, D46, D47, D48, D49 | Not visual (mapper, key and test changes); covered by the named tests (vitest src/remote 546/546, `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/gate-vitest-r7.log`). Round-7 build re-captured live: `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r7-collapsed.png`, `r7-expanded.png`, `r7-row-drawer.png`, `r7-dump.json` (RemoteApp-CfUwRSIb.js sha256 608f37320c49e2c32b4d554139ee7bafa9436afdea09b51dea8792b9ccccfcc4 in local dist, app bundle and 8899 per `deploy-sha-r7.txt`; 0 marker lines; fold "Worked for 4m 35s" settled on an idle turn; collapsed 0 rows, expanded 19 button rows: Ran 11, Read 5, Updated 3) |
| D50, D51, D52, D53, D54 | Round-9 build RemoteApp-BSCuRfTv.js (sha256 aeee6c040441762dee358baad60ac844fafc622286ab8aa4b46664ddfc9d8202 per `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/deploy-sha-r9.txt`): `.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r9-collapsed.png`, `r9-expanded.png`, `r9-thinking-open.png`, `r9-dump.json` (0 marker lines; folds 2m 20s / 5m 51s / 10m 27s; expanded 99 button rows), `r9-dom-a11y-check.txt` (prose row aria-label null, Message hint 1x1 clipped); D53/D54 test-only, vitest 549/549 (`gate-vitest-r9.log`) |
| D55, D56 | D55 is covered by `MobileChatMessage.test.tsx:286` and the round-10 live capture (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r10-dump.json`, `r10-collapsed.png`, `r10-expanded.png`); D56 is test-only, vitest `gate-vitest-r10.log` |
| D57 | Keyboard focusability is not visible in a screenshot; covered by `MobileChatMessage.test.tsx:336-337` and the round-11 live DOM check (`.omo/evidence/ulw/01a0d650-db7a-7817-a21b-7be552a81e89/G009/r11-dom-a11y-check.txt`) |

---

## Action Plan for Implementation

Because the frontend is served statically by the running daemon (`remote/server.rs:2889-2915`), deploying this fix requires **zero daemon restarts, zero PTY interruptions, and zero session loss**.

1. **Phase 1: Wire 927.5 Normalizer & Turn Merger in `agentConversation.ts`:**
   * Rewrite `mapAgentConversation` to accumulate messages into turn groups delineated by `role === "user"`.
   * Implement regex parser for `/(?:^|\n|\s)→\s*([a-zA-Z0-9_-]+)/g` to extract legacy tool calls from `assistant.text` and strip them from prose.
   * Pair following `toolResult` records with their preceding tool calls.
   * Calculate turn duration across the entire turn span (`startBoundary` to `terminalTimestamp`).
   * Suppress records that contain zero prose after marker stripping.
2. **Phase 2: T3 Work Log Components in `MobileChatComponents.tsx`:**
   * Refactor `ToolCallCard` into a compact 32px work row (`min-h-8`) with 14px text and SF/Lucide symbol mapping.
   * Wrap multiple work rows in a `WorkGroupContainer` with `max-h-64` (256px), inner scrolling, and top/bottom edge-fade gradients.
   * Move tool output into an expandable nested drawer (`max-h-60`, 12px monospace text).
3. **Phase 3: Turn Header & Metadata Cleanliness in `MobileChatMessage.tsx`:**
   * Render single "Worked for <duration>" fold at the top of the assistant turn.
   * Render timestamp and Copy action strictly once at the bottom of the assistant turn.
4. **Phase 4: Bundle & Deploy:**
   * Re-bundle web assets (`bun run --cwd ui build`).
   * Copy to `/Applications/Ferryx.app/Contents/Resources/ui/dist/`.
   * Verify live on mobile browser (`http://127.0.0.1:8899/`).

---

## Follow-ups (out of scope)

* **"Scroll to latest" Floating Pill Overlap:** The floating "Scroll to latest" pill in `ui/src/remote/chat/MobileChatWorkspace.tsx:277` overlaps the prose at 430px wide. T3 has no such pill. That file carries another session's uncommitted work, so it was not changed here.
