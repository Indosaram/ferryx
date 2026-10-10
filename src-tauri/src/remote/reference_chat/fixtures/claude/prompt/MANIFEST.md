# Claude prompt family — fixture manifest and responder inventory

Owner: task 22 (`src-tauri/src/remote/reference_chat/prompt_claude.rs`).
Pinned upstream: `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
`docs/chat/HERDR_LICENSE`). Hash: `docs/chat/herdr-port-contract.md` §7 —
`server/prompt.ts` = `083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58`.

Provenance of these fixtures:

* **Captured** — the screen is copied byte-for-byte out of the pinned
  `server/prompt.test.ts`, which records it as captured from a live `[CC]` pane
  (`2.1.280`–`2.1.290`). These are the reference's own captures, not re-typed.
* **Authored** — the screen is written for this port to exercise a branch the pinned test
  covers with an inline, parameterized template. No live session produced it.
* **Authored (composed)** — `answered-plan.screen` is the `plan.screen` card, transcribed from
  that pinned capture, with the output that buries it appended. The appended bytes are written
  for this port; no live session produced them.

Neither kind is QA-02/05 provenance: task 14's producer owns sanitized capture of real
transcripts and screens with original raw hashes.

## Responder inventory

Every named claude responder of the pinned `Responder` union (`server/prompt.ts:88-96`) is
listed with its parser, its fixture and the behavior it must produce. The claude family is
`parsePrompt`'s `agent === "claude"` arm (`:1944-1946`):
`[parseClaudeQuestion, parseClaudeSubmit, parseClaudeApproval, parseClaudeConfirm,
parseClaudeModel, ...omo()]`. The omo arm is task 25's family, not this lane's. No branch
below is skipped without a reason.

| Responder | Parser (pinned) | Fixture / test | Required behavior |
|---|---|---|---|
| `claude-question` | `parseClaudeQuestion` `:473-510` | `question.screen` | numbered rows, `Chat about this` last, `Type something.` before it; chip title |
| `claude-question` (tabs) | `claudeTabs` `:527-539`, `claudeQuestionText` `:541-552` | `question-tabs.screen` | bar above the question; title `Route · 1 of 2`; cursor on row 1 |
| `claude-question` (multi-select) | `:499-510` | `question-tabs-multiselect.screen` | `[ ]` rows read as multi-select; `custom_option_index: null`; `→` leaves the choice |
| `claude-question` (wrapped question, cut-off bar) | `claudeTabs` `:531` (`CLAUDE_TABS_RE`), `nearestQuestion` fallback | `question-tabs-wrapped.screen` | question joined back over seven lines; bar cut at `✔ Su` still read; title `Author` |
| `claude-question` (narrow pane, wrapped hint) | `wrapped()` `:161-167` | `question-narrow.screen` | hint read across the wrap; `│`-prefixed body stripped by `cleanLine` |
| `claude-question` (option preview) | `withoutPreview` `:446-471` | `question-preview.screen` | preview box cut off at its own column; `n to add notes` line dropped; no typed row |
| `claude-question` (over the task list) | `withoutClaudeTasks` `:1575-1603` | `question-tasks.screen` | task list under the panel; `promptTailIsActive` still finds the hint at the end |
| `claude-submit` | `parseClaudeSubmit` `:554-572` | `submit.screen` | `kind: menu`, title `Review your answers`, no typed row; tail = last row `2. Cancel` |
| `claude-plan` | `parseClaudeApproval` plan arm `:1113-1132` | `plan.screen` | `kind: plan`, title `Ready to code?`, `custom_option_index` = `Tell Claude what to change`; typed answer ends `shift+tab` |
| `claude-plan` (tail) | `promptTailIsActive` `:1624` | `answered-plan.screen` | the plan's own tail is buried under later output: **no card** |
| `claude-approval` (marker form) | `parseClaudeApproval` `:1134-1177` | `approval-command.screen` | panel opened by `This command requires approval`; title from `nearestQuestion`; options joined over wrapped lines |
| `claude-approval` (no marker, panel rule) | `:1148-1172` | `approval-command-wrapped.screen`, `approval-edit-wrapped.screen`, `approval-under-text.screen` | panel = lines under the first rule after the tool call; a rule in Claude's own table is not the panel; `Tip:` lines dropped |
| `claude-approval` (MCP call) | `:1153` (`callIndex` regex) | `approval-mcp.screen` | `● server - tool (MCP)(…)` is a call: the rule under it opens the panel |
| `claude-confirm` | `parseClaudeConfirm` `:1192-1244` | `trust-menu.screen`, `confirm-narrow.screen` | unnumbered rows, exactly one `❯`; panel's first line titles it; the `?` sentence asks |
| `claude-confirm` (ambiguous) | `:1207-1226` (`wrappedFrom`, `unsure`) | `confirm-ambiguous.screen` | a row wider than every line off the rows says nothing of the pane's width: **no card** |
| `claude-model` | `parseClaudeModel` `:1531-1563`, `listRows` `:1332-1375`, `listNames` `:1315-1329` | `model.screen`, `model-families.screen`, `model-phone.screen` | window rows with `↑`/`↓`, `… +N models` counted, `✔` = in use, `s`-only option steps |
| `claude-model` (waits, unreadable) | `claudeModelListWaits` `:1524-1529` | unit test (no fixture) | a list holding the screen's end gets no fallback card |
| `claude` suggestion | `parseClaudeSuggestion` `:2377-2401` | unit test (ANSI in code, no fixture) | the grey text in the empty input box; `null` while text is typed, on Claude's tip, or off the live box |
| tail check | `promptTailIsActive` `:1617-1624` | `answered.screen` | an answered menu above later output is **not** an open one |
| `omo-*` | `parseOmoQuestion` / `parseOmoTyping` / `parseOmoReview` / `parseOmoPending` | task 25 (`prompt_omo.rs`) | the claude arm appends `omo()` upstream; this lane must not claim it |

## Deliberate divergences from the pinned reader

1. **Prompt id bytes.** Upstream mints `sha256(JSON.stringify({agent, kind, title, question,
   body, options, multi_select, custom_option_index})).slice(0,12)` with JavaScript
   `JSON.stringify` escaping. This port builds the same canonical string and hashes it with
   `sha2`, so the id is stable for a card and changes when the card's text changes. It is
   **not** byte-equal to upstream's for text holding an astral character (a JS lone surrogate
   escapes as `\uXXXX`, this port emits the character). The id is opaque to every consumer;
   only its stability and its sensitivity to the card's text are contractual.
2. **Display width.** `Bun.stringWidth` measures grapheme clusters (a joined emoji is two
   columns). This port sums per-character widths, so a joined emoji counts its parts. Every
   fixture here is aligned in ASCII or a single wide character, where the two agree.
3. **Length arithmetic** in `claudeTabs`/`confirm`/`listRows` counts Unicode scalar values,
   where upstream counts UTF-16 code units. The comparisons are between lengths measured the
   same way on both sides, so only an astral character can change a comparison's outcome.
4. **`ReferenceAnswerPlanner`.** The frozen alias carries only the public card
   (`&ReferencePrompt`); the cursor and responder upstream keeps in `parsedByPublicPrompt`
   (a `WeakMap`) are not recoverable from it. This lane exposes the parsed card as
   [`ClaudePrompt`] and plans from it (`plan_claude_answer`), which is that map's Rust
   equivalent. No lossy inverse is shipped.
5. The claude family reads **only** `agent == "claude"`. Upstream also routes `""` (an
   unlabeled pane) into `omo()`, which is task 25's family; claiming it here would advertise
   support this lane does not port.
6. `sgrRuns`'s two-character escape alternative is written exactly as upstream writes it
   (`[@-Z\\-_]`). JavaScript reads the trailing `-_` as an ascending range rather
   than two literal characters, and so does this port; the alternative only skips stray
   two-byte escapes, and every fixture here reaches `sgrRuns` through a CSI.
