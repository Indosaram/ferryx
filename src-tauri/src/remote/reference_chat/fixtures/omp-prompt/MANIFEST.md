# omp prompt fixture manifest (plan task 24)

Reference pin: `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
`docs/chat/HERDR_LICENSE`). Line numbers below are that revision's `server/prompt.ts` unless
another file is named.

Every named omp responder branch the pin handles is listed here with the fixture that exercises
it. A variant with no fixture would be an unexplained gap, so there is none. Fixture files are
raw screens (ANSI already stripped, `\n` separated) under this directory.

## Branches

| # | Branch (upstream) | Upstream anchor | Fixture | What it must produce |
|---|---|---|---|---|
| 1 | The omp dispatch gate: only `agent === "omp"` reaches the omp candidates | `:1941-1943` | `not-omp-hint.txt` | `None` for every other agent id (`claude`, `codex`, `omo`, `pi`, `gjc`, `""`) |
| 2 | `parseOmpQuestion`: the single-select hint is found on one line | `:263-267`, `:13` | `question-single.txt` | `kind: question`, `title: "Question"` |
| 3 | The hint is matched across a wrap (a line and the two after it) | `:163-166`, `:265` | `question-wrapped-hint.txt` | the menu is still found when the hint wraps over two lines |
| 4 | `parseOmpQuestion`: the multi-select hint (`space/enter toggle`) | `:14`, `:274` | `question-multi.txt` | `multi_select: true`, `title: "Multiple choice"` |
| 5 | `findMenuDividers` + `parseBorderMenu`: rows between the last two dividers above the hint | `:177-200` | `question-single.txt`, `question-multi.txt`, `approval.txt` | one option per non-blank non-divider row |
| 6 | Row markers stripped: cursor `[❯›>]`, checked `[☑☒✓]`/`[○●◉◯☐]` | `:11`, `:185-188` | `question-single.txt`, `question-multi.txt` | labels carry no marker; `☑` rows are the checked ones |
| 7 | `(Recommended)` is stripped from an option's label | `:283` | `question-single.txt` | label `crates/core/src/lib.rs` |
| 8 | The cursor row is read off the menu (`selectedIndex`) | `:277-282` | `question-single.txt` (row 0), `question-single-moved.txt` (row 2) | the answer's moves are counted from the shown row |
| 9 | Checked rows become `checkedOptionIndices`, one per **option** row | `:283` | `question-multi.txt` | checked indices `[0, 2]` |
| 10 | `custom_option_index` is the last option index (single) or `null` (multi) | `:283` | `question-single.txt`, `question-multi.txt` | `Some(2)` / `None` |
| 11 | The `Other (type your own)` row is **required** for a question to exist | `:280-281` | `question-no-custom-row.txt` | `None` |
| 12 | The question text comes from `nearestQuestion` above the menu | `:167-176`, `:280` | `question-single.txt` | `question: "Which file should I open?"` |
| 13 | `nearestQuestion` walks back at most 14 lines and skips blanks/dividers | `:167-176` | `question-single.txt` | the title line is found past the blank line and the opening divider |
| 14 | The question's tail check: the hint must still end the screen | `:1613` | `question-stale.txt` | `None` |
| 15 | `parseOmpApproval`: the `Allow tool:` header | `:1092-1094` | `approval.txt` | `kind: approval`, `title`/`question` = the cleaned header |
| 16 | `parseOmpApproval`: **exactly two** `Approve`/`Deny` rows, one cursor | `:1096-1100` | `approval.txt`, `approval-three-rows.txt` | a prompt / `None` |
| 17 | `parseOmpApproval`: `body` is the lines between the header and the first row | `:1104` | `approval.txt` | `body: "Command: git push --force origin main"` |
| 18 | The approval's tail check (a row or `esc … cancel` still at the end) | `:1621` | `approval-stale.txt` | `None` |
| 19 | `finishPrompt`: `id` = first 12 hex of sha256 over the pinned payload, cursor excluded | `:228-242` | `question-single.txt` vs `question-single-moved.txt` vs a text-edited screen | equal for a moved cursor, different for changed text |
| 20 | `answerKeys`: a single option → the moves from the cursor, then `enter` | `:2054` | `question-single.txt` | `down,down,enter` (row 2) / `enter` (row 0) |
| 21 | `answerKeys`: custom text → moves, `enter`, the text, `enter` | `:2032-2050` | `question-single.txt` | `down,down,down,enter,"crates/core/src/lib.rs",enter` |
| 22 | `answerKeys`: multi-select → toggles with `space`, then `tab`, `enter` | `:2037-2041` | `question-multi.txt` | `down,space,tab,enter` for `[0]`; `tab,enter` for `[0, 2]` |
| 23 | `answerKeys` refusals: an unparsed prompt, zero/two answer shapes, an out-of-range index, a custom answer on a multi-select | `:1997-2010`, `:2020-2030`, `:2052-2056` | constructed prompts in `prompt_omp.rs` tests | a typed error, never a guessed key |

## Deliberate non-port, with its reason

* **`queued`, `steps` and `fallback` stay unset** for an omp prompt. The pinned omp branches
  set none of them (`:277-286`, `:1102-1110`); `queued` belongs to Codex's rollout queue,
  `steps` to omo's multi-question form, `fallback` to `parseFallbackPrompt` (`:2105+`).
* **`answerKeys` ends at `:2056`.** The plan text cites `:2064-2077` as an omp branch of
  `answerKeys`; those lines are the `parseFallbackPrompt` constant block (`ASKED_RE`,
  `YES_NO_RE`, `ARROWS_RE`, `MENU_HINT_RE`, `INPUT_FIELD_RE`, `HINT_LINE_RE`,
  `MENU_WRAP_LINES`, `NOT_PROMPT_TEXT_RE`) — the last-resort card for a pane no reader knows,
  reached through the `fallback-menu`/`fallback-keys` responders, never through omp. omp's own
  `answerKeys` paths are `:2032-2050` (custom text), `:2034-2041` (multiple choice) and
  `:2054` (a single option), all ported above.
* **No `optionSteps`, `customSteps`, `multiSteps` or `rowKey`.** Those are set by the omo, pi
  and model-list branches; the omp branches leave all four undefined, so `answerKeys` takes
  its generic path for them.
* **No `omoAsk` / `omoOpen` input.** `parsePrompt` only consults omo's form readers for the
  `claude`/`pi`/`omo`/`""` agents (`:1944-1953`), never for `omp`.
* **No model list.** `parseOmpModel` does not exist upstream: omp's model picker is not a
  named branch, so this lane does not claim one.
* **No `suggestion`.** That read is Claude-only (`:2477+`).
* **The answer planner needs internal state.** Upstream keeps `selectedIndex`,
  `checkedOptionIndices`, `customMenuIndex` and the responder in a `WeakMap` beside the public
  prompt (`:133`, read back at `:1996-1997`) and refuses an answer for a prompt it did not
  parse. Ferryx's public `ReferencePrompt` crosses the wire, so this lane keeps a bounded
  registry keyed by prompt id. The route re-reads the screen and re-detects immediately before
  answering (`docs/chat/herdr-port-contract.md` §5), which repopulates the registry.

## Provenance of the fixtures

These screens are hand-authored to the shapes the pinned parser reads — they are **not**
captured from a live omp session, and they are labelled as synthetic. Task 14's producer is
what captures authentic sanitized screens with their source hash; these files exercise the
parser's branches until that runs.
