# omo history family fixtures (plan task 19)

Provenance for `src-tauri/src/remote/reference_chat/history_omo.rs`.

| Item | Value |
|---|---|
| Upstream reference | `devswha/herdr-web-ui` |
| Pinned revision | `54e5a1f67090cb09552d182e7e30dd0ecc314918` |
| Upstream license | MIT, `Copyright (c) 2026 devswha` — `docs/chat/HERDR_LICENSE` |
| Frozen contract | `docs/chat/herdr-port-contract.md` |
| Ferryx base | `19e99a149bddec45ef29e825c948ce587f380591` (branch `work/herdr-reference-chat-w1`) |
| Lane | task 19 — the `omo` native history family (`omo-transcript`) |

## Pinned sources read for this lane (SHA-256 over the fetched UTF-8 bytes)

| Upstream path | SHA-256 |
|---|---|
| `server/transcript-records.ts` | `e6a50c9b1e6523ed25e027d0c26493c5c5f4f573bfd22a04e03528e9e943099f` |
| `server/skill-activity.ts` | `dbd94c28a03d375404872c38e7d0c5aaf6e9b3ec9b245e874488634bdb105207` |
| `server/tool-output.ts` | `b3d15563d25244fc69246648570f2fbff2cd3ae2f0532e22b21c2cc6eb76685b` |
| `server/omo.ts` | `6b47806bd834930f0d8bd8ef9b4f1533fbe5585322352b1796bdf4582ed55edf` |
| `server/conversation.ts` | `c02267560aafdaf4a07a8c440f2c2d34d7379f0b4711acc934131145e7b5159d` |
| `shared/protocol.ts` | `418c140e1559ed14f19fcee641a69b99e29d77cc2011db347f66ef5b241059ea` |

`server/conversation.ts` is the dispatch proof: its header states that omo "writes omp's
session shape, so `parseOmpTranscript` reads it", and `parseTurns` routes `omo-transcript`
through the non-Claude/non-Codex arm with `toolImages: false` and the caller's
`taskTitles`. This lane therefore ports `parseOmpTranscript` and its OmO-specific
dependencies, not a second, separately-invented omo parser.

## Fixture files

| File | What it pins |
|---|---|
| `session-authentic.jsonl` | one omo session with every record variant this lane handles, in order |
| `session-partial.jsonl` | torn tail line, non-JSON text, non-object lines, whitespace-only prompt, string `content` |
| `session-context-clear.jsonl` | a `context_clear` reset that empties the conversation before it |
| `session-stop-error.jsonl` | a failed assistant message (`stopReason: "error"` + `errorMessage`) |

Fixtures are read through `include_str!`, so the bytes the tests assert against are bound at
compile time. They are shaped from the pinned record grammar; they are not copies of a real
user transcript and carry no personal data.

## Record-variant inventory (every variant named by the pin, and where it lands)

| Upstream variant / named behavior | Pinned anchor | Ported as | Fixture line |
|---|---|---|---|
| `session` header (`type`, `cwd`, `id`, `timestamp`) | `omo.ts` `candidate()` | `omo_session_header` | `session-authentic.jsonl` 1 |
| `message` with string `content` | `piMessage` | `omo_pi_message` | `session-partial.jsonl` 3, 7 |
| `message` with block-array `content` | `piMessage` | `omo_pi_message` | `session-authentic.jsonl` 3 |
| `entry.display === false` / `message.display === false` | `piMessage` | `omo_pi_message` → `None` | `session-authentic.jsonl` 12 |
| `toolCall` spellings `toolName`/`name`, `toolCallId`/`id`/`callId`, `toolInput`/`input`/`arguments` | `piMessage` | `omo_pi_message` | `session-authentic.jsonl` 3, 5 |
| `toolResult` spellings `toolCallId`/`callId`/`id`, `output`/`content`/`result`, `isError` | `piMessage`, `piResults` | `omo_pi_message`, `omo_pi_results` | `session-authentic.jsonl` 4, 6 |
| image block in a result (`mimeType`/`media_type`, `data`) | `piResults`, `piImageOf` | `omo_pi_results` (+ `ReferenceImageRef` when `tool_images`) | inline test |
| `custom` + `customType: "context_clear"` | `isContextClear` | `is_omo_context_clear` | `session-context-clear.jsonl` 4 |
| `custom_message` the runtime displays | `piNotice` | `omo_pi_notice` | `session-authentic.jsonl` 8 |
| `<system-notice>` envelope | `piNotice` | `strip_system_notice` | `session-authentic.jsonl` 8 |
| `custom_message` + `omo-senpi:wake` + `senpi-task.completion` | `omoTaskResults` | `omo_task_results` | `session-authentic.jsonl` 7 |
| task fields `task_id`, `status`, `name`, `agent_type`/`category`/`subagent_type`, `resolved_model.display`/`model`, `duration_ms`, `run_stats.*`, `tokens`, `final_response`, `error` | `omoTaskResults` | `omo_task_results` | `session-authentic.jsonl` 7 |
| `toolName: "task"` `details.items[].task_id` + `task_summary`/`description` | `omoTaskTitles` | `omo_task_titles` | `session-authentic.jsonl` 6 |
| `compaction` entry with `summary` | `parseOmpTranscript` | compact part | `session-authentic.jsonl` 9 |
| `stopReason: "stop"` settles the turn | `parseOmpTranscript` | `settled` | `session-authentic.jsonl` 10, 11 |
| `stopReason: "error"` + `errorMessage` | `parseOmpTranscript` | error text part | `session-stop-error.jsonl` 2 |
| `intent` on a tool call outranks the derived summary | `parseOmpTranscript` | `omo_tool_summary` call site | `session-authentic.jsonl` 5 |
| `toolSummary` argument precedence and the `task` batch join | `toolSummary` | `omo_tool_summary` | inline test |
| `trimOutput` page limit + `output_ref`/`output_size` | `tool-output.ts` | `trim_omo_tool_output` | inline test |
| `WHOLE_OUTPUT_TOOLS` / `WHOLE_OUTPUT_CHARS` | `tool-output.ts` | `OMO_WHOLE_OUTPUT_TOOLS`, `OMO_WHOLE_OUTPUT_CHARS` | inline test |
| `OMO_TASK_RESULT_MAX` cut + `result_cut` | `transcript-records.ts` | `OMO_TASK_RESULT_MAX_CHARS` | inline test |
| `MAX_TURNS` | `transcript-records.ts` | `OMO_MAX_TURNS` | inline test |
| `TURN_MARK["omo-transcript"]` and `opensTurn` | `conversation.ts` | `OMO_TURN_MARK`, `omo_line_opens_turn` | inline test |
| `skillInvocationPrompt` (chained + `<user-request>` + legacy `<skill>`) | `skill-activity.ts` | `omo_skill_invocation_prompt` | inline test |
| `loadedSkill`, `skillDocument`, the strict `label` | `skill-activity.ts` | `omo_loaded_skill`, `omo_skill_document`, `skill_label` | inline test |
| `isOmoProcess` (program-only, `--extension`, `--` prompt boundary, Windows words, PATH lists) | `omo.ts` | `is_omo_process` | inline test |
| `omoSessionFolder` | `omo.ts` | `omo_session_folder` | inline test |

No named omo variant is silently skipped.

## Deliberately out of this lane (owned elsewhere, not skipped)

| Upstream behavior | Why it is not here | Owner |
|---|---|---|
| `omoCandidates`, `selectOmoTranscript`, `heldSessionIds`, `heldRuntime`, `holderStartedAt`, `earliestStart`, `omoAgentDir`, `processEnviron`, `omoTranscriptsOfCwd`, `omoPanes`, `omoSessionForPane`, `omoTranscriptForPane` | file/session acquisition and inference, not record parsing | task 3 resolver (`history.rs`) |
| `labelOmoPanes`, `paneRunsOmo` | pane-label rewriting during a snapshot read | task 3 / integration |
| `conversation.ts` paging, cursors, `history_id`, ETag `version`, live/settled scans, `pageBefore`, `newestPage` | page machinery over a byte stream | task 2 dispatcher |
| `omp-transcript` and `gjc-transcript` registry ids | same record shape, different lanes | tasks 18 and 20 |

`parse_omo_history` refuses every `ReferenceNativeHistoryKind` except `Omo`, including
`Unavailable` — an empty success there would be read as "this session has no turns".

## The skill part

Upstream `parseOmpTranscript` pushes a user turn's parts as
`[text(asked), { kind: "skill", skill }]`, and `skillInvocationPrompt` is what turns a
`/skill:name` prompt into the request the person typed plus one chip per loaded skill.

Task 27 added the `skill` variant to `ReferencePart` (contract revision 2), so this lane emits
both: `parse_omo_transcript` renders the request as the user's text and pushes one
`ReferencePart::Skill` per skill the envelope named, in the order it named them.
`omo_skill_invocation_prompt` still exports the whole invocation (skills + request) for a
caller that wants it alone.

## Deferred commands (not run; the execution override forbids intermediate checks)

```
cargo test --manifest-path src-tauri/Cargo.toml --lib reference_chat::history_omo
```

Must be run by the post-merge verification wave together with the module registration of
`pub mod history_omo;` in `src-tauri/src/remote/reference_chat/mod.rs` (integration owner,
task 13 / wave-1 registrar).
