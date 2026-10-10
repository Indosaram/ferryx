# Claude history family — fixture manifest and record inventory

Owner: task 17 (`src-tauri/src/remote/reference_chat/history_claude.rs`).
Pinned upstream: `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
`docs/chat/HERDR_LICENSE`). Hashes: `docs/chat/herdr-port-contract.md` §7 —
`server/conversation.ts` = `c02267560aafdaf4a07a8c440f2c2d34d7379f0b4711acc934131145e7b5159d`.

These fixtures are **authored**, not captured: no live Claude session produced them, so they
are a record-variant inventory, not provenance evidence for QA-02. Task 14's producer owns
sanitized capture of real transcripts with original raw hashes.

## Parser/record variant inventory

Every named record variant of the pinned Claude reader is listed with the fixture that
exercises it and the behavior it must produce. `parseClaudeTranscript`
(`server/conversation.ts:117-226`) is the only Claude parser upstream; `isContextClear`
(`server/transcript-records.ts:15-20`), `toolSummary` (ibid. `:153-163`), `trimOutput`
(`server/tool-output.ts`), `invokedSkill` (`server/skill-activity.ts`) and the `TURN_MARK`
(`server/conversation.ts:425-429`) / `opensTurn` (`:440-455`) helpers are the adjacent rules
it depends on. No variant is skipped without a reason in the table below.

| Upstream variant | Where | Fixture / test | Required behavior |
|---|---|---|---|
| `isCommandEntry` prefixes `<command-`, `<local-command`, `<task-` | `conversation.ts:73-75` | `branch.jsonl` L12/L13, `partial.jsonl` L16 | dropped as a user turn, never rendered as prose |
| `unwrapPastes` matching id pair | `:81-91` | `queued.jsonl` L6 | tags removed, visible text kept |
| `unwrapPastes` mismatched ids / id > 64 / non-word id | `:83-86` | `queued.jsonl` L9, unit test | left exactly as typed |
| `unwrapPastes` leading/trailing newline trim (only when changed) | `:90` | `queued.jsonl` L10 | `"  \n body "` |
| torn tail line (`JSON.parse` throws) | `:132-137` | `normal.jsonl` L20, `partial.jsonl` L8 | skipped, never repaired or fabricated |
| non-object line (`42`, `null`, bare string, array) | `:138` | `partial.jsonl` L4-L7 | skipped |
| `isMeta` truthy entry | `:138` | `normal.jsonl` L19, `partial.jsonl` L19 | skipped entirely |
| `/clear` local-command envelope | `isContextClear` `:15-20` | `branch.jsonl` L4 | turns **and** pending tool results reset |
| `/clear` quoted in prose | `:19` | `branch.jsonl` L9 | ordinary user text, no reset |
| `isCompactSummary` with array content | `:142-147` | `branch.jsonl` L14 | `compact` part joined with `\n`, user seat |
| `isCompactSummary` with non-string, non-array content | `:142-147` | `partial.jsonl` L20 | `compact` part with empty text |
| `attachment` / `queued_command` / `commandMode: prompt` / `origin.kind: human` | `:149-157` | `queued.jsonl` L2 | user turn with the queued prompt |
| queued command from `origin.kind: agent` | `:154` | `queued.jsonl` L3 | ignored |
| queued command with `commandMode: system` | `:154` | `queued.jsonl` L4 | ignored |
| queued prompt that is a `<task-` envelope | `:155` | `queued.jsonl` L5 | ignored |
| queued prompt that is blank | `:155` | `queued.jsonl` L7 | ignored |
| other attachment kinds (`file`) | `:149` | `queued.jsonl` L8 | ignored |
| `user` entry with string content | `:159-163` | `normal.jsonl` L2, `queued.jsonl` L1 | user turn |
| `user` entry with array content, text blocks only | `:166-173` | `normal.jsonl` L13, `partial.jsonl` L16 | blocks joined with `\n`, command blocks excluded |
| `user` entry with array content, empty after filtering | `:172` | `partial.jsonl` L19 | no turn (no fabricated empty prompt) |
| `tool_result` answering a pending `tool_use` | `:175-183` | `normal.jsonl` L5/L7/L9/L11 | output folded into the tool part, by id |
| `tool_result` for an unknown id | `:177-178` | `normal.jsonl` L16, `partial.jsonl` L11 | ignored, no new turn |
| `tool_result` with `is_error: true` | `:181` | `normal.jsonl` L7 | `error: true` on the tool part |
| `tool_result` with `is_error` of another type (`"true"`) | `:181` | `partial.jsonl` L13 | not an error (strict `=== true`) |
| `tool_result` content as an array of text parts | `:103-108` | `assistant.jsonl` L11 | parts concatenated without separator |
| `tool_result` answering a `tool_use` with no `id` (key `""`) | `:218` | `assistant.jsonl` L10 | folds into that tool part |
| `tool_result` for a `Skill` tool | `:182` | `normal.jsonl` L11, `assistant.jsonl` L11 | skill status `loaded` / `failed` |
| pasted base64 image of a shown type | `:186-191` | `normal.jsonl` L13 | `image` part, ref `<uuid>:<index>`, before the text |
| image of an unshown type (`image/svg+xml`) | `:110`, `:189` | `partial.jsonl` L14 | ignored |
| image with `source.type: url` | `:189` | `partial.jsonl` L15 | ignored |
| image on an entry with no string `uuid` | `:186` | unit test | no image part (no fabricated ref) |
| `assistant` entry, adjacent entries merged into one turn | `:120-125`, `:197` | `normal.jsonl` L3-L12, `assistant.jsonl` L1-L13 | one assistant turn, `startedAt` from the first entry |
| `assistant` entry whose content array adds no part still advances `end_ts` | `:197-198` | `normal.jsonl` L21 (empty array, after a user turn) | its own empty turn is dropped; it never rewrites an earlier turn's `endedAt` |
| `end_ts` = last assistant activity | `:198` | `normal.jsonl` L3 (09:00:02 → 09:00:22) | never the next user's timestamp |
| `text` block, non-empty | `:202-203` | `normal.jsonl` L3 | `text` part |
| `text` block, empty string | `:202` | `assistant.jsonl` L2 | no part |
| `thinking` block via `thinking` field | `:204-207` | `normal.jsonl` L3 | `thinking` part |
| `thinking` block via `text` fallback | `:205` | `assistant.jsonl` L1 | `thinking` part |
| `thinking` block, empty | `:206` | `assistant.jsonl` L3 | no part |
| `tool_use` with object input | `:205-217` | `normal.jsonl` L4 | tool part with pretty input |
| `tool_use` with non-object input | `:208` | `assistant.jsonl` L5, `partial.jsonl` L17 | input collapses to `{}` |
| `tool_use` without `name` | `:205` | `assistant.jsonl` L6 | no part |
| unsupported blocks (`redacted_thinking`, `server_tool_use`) | `:220` | `partial.jsonl` L10, `assistant.jsonl` L4 | ignored |
| `assistant` entry whose content is not an array | `:197` | `assistant.jsonl` L12 | ignored |
| `toolSummary` `task` call titles (`task_summary` then `description`) | `transcript-records.ts:155-159` | unit test (`claude_tool_summary("task", …)`) | `"Port omp history · Port claude history"` |
| `toolSummary`'s `name === "task"` gate is lowercase | `:155` | `normal.jsonl` L8 (tool named `Task`) | the gate does **not** fire: summary is the tool's name `"Task"`, never the titles |
| `toolSummary` field order `command`→`file_path`→`notebook_path`→`path`→`pattern`→`description`→`url` | `:161-162` | `normal.jsonl` L4/L6, `assistant.jsonl` L5 | first present field, cut at 120 |
| `toolSummary` fallback to the tool name | `:162` | `assistant.jsonl` L6 | `"Write"` |
| `invokedSkill` on `Skill` with a valid label | `skill-activity.ts` | `normal.jsonl` L10 | skill part `{invocation, requested}` |
| `invokedSkill` label trimmed / rejecting control chars | ibid. | `assistant.jsonl` L7/L8 | `"padded"` accepted (the payload keeps its spaces), `"a\nb"` rejected |
| `trimOutput` cut at 4000 with `output_ref`/`output_size` | `tool-output.ts:14-21` | unit test | first 4000 chars + `\n… trimmed` |
| `trimOutput` whole-output tools at 16000 | `:10-11` | unit test | `create_goal`/`update_goal`/`get_goal` |
| `MAX_TURNS` = 100, `slice(-maxTurns)` | `conversation.ts:117`, `:225` | unit test | newest kept; `0` keeps all (`slice(-0)`) |
| `TURN_MARK` `"user"` + `opensTurn` for page starts | `:427`, `:449-452` | unit test | exported for task 3's pager |

## Deliberate divergences from the pinned reader

1. `isSidechain` entries are **not** filtered, because the pinned parser does not filter them
   either: it has no `isSidechain` check anywhere. Ported verbatim. The fixture
   (`branch.jsonl` L11) pins the behavior instead of hiding it; whether a sidechain belongs in
   the page is a product decision owned outside this lane.
2. The reference's `/tree` branch projection (`piAbandonedTurns` / `piBranchSegments`) is
   **pi-family only** — `server/conversation.ts` applies it in the pi path and nowhere in
   `parseClaudeTranscript`. Claude records carry `parentUuid`, but the pinned Claude reader
   never walks that graph, so this lane does not invent one: `abandoned` stays `None`.
3. Length limits count characters, not UTF-16 code units (`String#length`): a cut at 4000 must
   not split a character, and no fixture exercises an astral-plane boundary.
4. A non-string `text` inside a result array or a summary block is read as its JSON text; the
   reference's `String()` coercion of objects (`"[object Object]"`) has no protocol meaning.
5. `tool_use.input` is serialized with the reference's own `JSON.stringify(input, null, 2)`
   (insertion order, `JSON.stringify` escaping) rather than `serde_json`'s pretty printer, so a
   ported page's bytes match the reference's. An object input therefore keeps its record order.
   No fixture separates the two: every input in these fixtures happens to be in alphabetical key
   order, so a sorted-map implementation would pass them too — the divergence is pinned by the
   unit test on the writer, not by a fixture.
6. A `Skill` invocation is a `tool` part (`summary` = skill name, `skill` = the evidence),
   exactly as the reference emits it: the claude reader attaches `invokedSkill` to the call and
   emits no standalone `skill` part. The contract does carry one for the readers that do —
   `docs/chat/herdr-port-contract.md` section 4, revision 2.
