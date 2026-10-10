# pi prompt family — fixture manifest and branch inventory

Owner: task 26 (`src-tauri/src/remote/reference_chat/prompt_pi.rs`).
Pinned upstream: `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
`docs/chat/HERDR_LICENSE`). Hashes: `docs/chat/herdr-port-contract.md` §7 —
`server/prompt.ts` = `083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58` (re-verified by
fetch in this authoring session), `src/lib/promptAnswer.ts` =
`044fc3872f67d760899f05860a8935c66cf5b2d69bdb8725dd4f3e9bb1928b5b`.

These fixtures are **authored, not captured**: no live pi process produced them. They are the
pinned reference's own pi captures, lifted byte-for-byte out of `server/prompt.test.ts` (the two
`describe` blocks at `:1661` and `:1741`), so they carry the reference's real pi 0.87.1 screens
without claiming a run of this port. They are a record-variant inventory, not provenance evidence
for QA-02/QA-05: task 14's producer owns sanitized capture of a real pane with its original hash.

## Where these bytes come from

`server/prompt.test.ts` builds every pi screen from four constants, all lifted verbatim:

| Upstream | Value |
|---|---|
| `FOOTER` (`:1662`) | `\n────────────────────────────────────────\n/tmp/app\n0.0%/215k (auto)                                        some-model • medium\n` |
| `piScreen(body)` (`:1663`) | `────────────────────────────────────────\n\n${body}\n────────────────────────────────────────${FOOTER}` |
| `MENU_HINT` (`:1664`) | ` ↑↓ navigate  enter select  escape/ctrl+c cancel` |
| `MODEL_HINT` (`:1743`) | ` Enter to select · Ctrl+S to set as default · Escape/Ctrl+C to cancel` |

Each fixture below names the pinned test it came from. Where a screen is a `piScreen(...)` call
argument, the fixture file is the **interpolated result**, byte for byte; where the test writes the
screen as its own template (`wide`, `narrow`, `login`, and the two `cut` screens) the fixture is
that template with `${MODEL_HINT}` / `${FOOTER}` substituted.

## Branch inventory

Every named pi branch of the pinned `server/prompt.ts`, with the fixture that exercises it and the
behavior it must produce. No variant is skipped without a reason; the reasons are in the last
section.

| Upstream branch | Where | Fixture / test | Required behavior |
|---|---|---|---|
| `Responder` union, pi's members (`pi-question`, `pi-confirm`, `pi-input`, `pi-model`) | `:102-105` | all dialog + model fixtures | the four responders this lane can produce; `PiResponder` names them |
| `parsePrompt` pi branch: candidates `[parsePiModel, parsePiDialog, ...omo()]`, first non-null that is tail-active | `:1941-1947`, `:1952` | `model-*`, `dialog-*` | model is tried before dialog; each candidate is gated by the tail check |
| `parseInteractivePrompt` agent allowlist | `:2469` | `detector_claims_only_pi_and_the_aliases_are_the_same_functions` | any agent other than `pi` yields `None` |
| `promptTailIsActive` → `pi-model` uses `hintAtEnd(shown, PI_MODEL_HINT_AT_END_RE, 2, 3)` | `:1668` | `model-wide`, `model-narrow`, `model-narrow-buried` | the catalogue's hint must be within 2 footer lines of the bottom |
| `promptTailIsActive` → `pi-question` / `pi-confirm` / `pi-input` use `hintAtEnd(..., 2, 4)` over the menu hint **or** the input hint | `:1669-1671` | `dialog-select`, `dialog-input`, `dialog-login-narrow`, `dialog-*-buried` | a dialog whose wrapped hint sits over the footer is still offered; one whose hint is buried is not |
| `hintAtEnd` window: `end` walks the last `footerLines+1` shown lines, `size` 1..=span, joined with `" "` | `:1727-1736` | `dialog-login-narrow` (span 2 at `end = len-3`), `model-narrow` (span 2 at `end = len-3`) | a hint split across two lines matches; the anchor is at the **end** of the join |
| `hintAtEnd` is anchored (`new RegExp(source + "$")`) | `:1696-1697`, `:1712` | `dialog-answered-buried` | the footer's own lines complete a join that merely *starts* with the hint's words — that must not match |
| `PI_MENU_HINT_RE` | `:1687` | `dialog-select`, `dialog-confirm` | `↑↓ navigate  enter select  escape/ctrl+c cancel` |
| `PI_INPUT_HINT_RE` | `:1688` | `dialog-input` | `enter submit  escape/ctrl+c cancel` |
| `PI_INPUT_LINE_RE` | `:1699` | `dialog-input` | every row of the block must be a `›`/`>`/`❯` line |
| `PI_ROW_RE` (cursor, optional tick, label) | `:1701` | `dialog-select`, `model-wide`, `model-narrow` | group 2 is the label; the tick is not part of it |
| `PI_WRAPPED_REST_RE` (a row's rest, one column in, no cursor char) | `:1757` | `dialog-select-wrapped-narrow`, `model-narrow` | a continuation line is joined onto the row it belongs to, not read as its own option |
| `piDialogRows` reads upward and joins the rest it met first | `:1758-1783` | `dialog-select-wrapped-narrow` | `['Keep it on the staging environment for now and wait for review', 'Deploy to production', 'Cancel']` |
| `piDialogRows` prepends lines one column in with **no** row over them as cursor-less rows | `:1776` | `palette-two-columns` (the `/model` first line) | the leftover stands as a row and is refused by the label rules below |
| `piDialogRows` title run: max 8 lines, stops at a divider or at a blank after a non-blank | `:1777-1782` | `dialog-select`, `dialog-confirm`, `dialog-confirm-wrapped` | the dialog's own words, title first |
| pi-input: `title = body ?? ""`, `question = title ?? ""`, one option `Type your answer`, `custom_option_index: 0` | `:1892-1896` | `dialog-input` | `question == "Branch name?"`, `title == ""`, `custom_option_index == Some(0)` |
| every block row must be a single label (no `\s{2,}`) | `:1908` | `palette-two-columns` | a two-column palette is refused: those rows are slash commands |
| at least 2 rows | `:1909` | `dialog-answered-buried` (1 row), `dialog-empty` | one row is not a menu |
| cursor rule: `moved ? cursor < 0 : cursor !== 0` refuses | `:1912-1913` | `dialog-select`, `dialog-select-cursor-moved` | the card is offered only with the cursor on row 0; `moved` reads where the cursor stands |
| Yes/No ⇒ `kind: approval` (`^yes\b` then `^no\b` on the first two labels) | `:1915-1917` | `dialog-confirm`, `dialog-select` (not a confirm) | `Clear session?` is an approval; `Allow dangerous command?` is a question |
| confirm title/body: `title = block.title[0]`, `question = body ?? title` | `:1919-1921` | `dialog-confirm-wrapped` | title `Delete the branch?`, question the wrapped message joined whole |
| question title/body: `title: ""`, `question: block.title.join(" ")` | `:1919-1921` | `dialog-select`, `dialog-select-wrapped-narrow` | the dialog's lines joined with one space |
| `rejectWithEscapeIndex: null` for every pi prompt | `:1922-1924` | `dialog-confirm` | "No" is **pressed**, never Escape |
| `PI_MODEL_HINT_RE` (no `↑↓ navigate`) | `:1710` | `model-wide`, `model-narrow` | `/model` is found by its own hint, so a menu reader never takes it |
| `PI_MODEL_HINT_AT_END_RE` | `:1712` | `model-narrow` | the wrapped hint at the end matches |
| `PI_MODEL_FILTER_RE` anchors the catalogue | `:1738`, `:1853` | `model-wide`, `model-narrow` | the last `>`/`›`/`❯` line above the hint; the catalogue is what sits under it |
| `piModelRows` skips blanks after the filter line | `:1804-1805` | `model-narrow` | the blank between the filter line and the rows is not a row |
| `piModelRows` joins a wrapped tail at column 0-1 back onto its row | `:1812-1819` | `model-narrow` | `vllm-flash/Qwen3.8-Flash-Next [lwsa-platform] · default` reads whole |
| a row must carry a provider bracket | `:1826-1831` | `model-wide` (notes present), `model-narrow` (`Model Name:` / `Refreshing model catalogs…`) | pi's own notes are never options |
| a row must be indented `^ {2,}` or carry the cursor | `:1827` | `model-wide` (`Could not refresh llama.cpp; showing cached models.` at column 0 ends the list) | the run ends at the first line that is not a row |
| a row cut mid-bracket voids the whole reading | `:1826-1828`, `:1835` | `model-wide-cut-mid-name` | the rows already collected are not offered |
| any row still missing its provider voids the whole reading | `:1843` | `model-half-name-cut` | joining a cut bracket would invent a model |
| at least 2 rows and a cursor row | `:1844` | `model-wide-one-row`, `model-wide-no-cursor` | one model is nothing to choose between; no cursor is nowhere to navigate from |
| the current model's tick names the card | `:1740`, `:1863-1866` | `model-wide`, `model-narrow`, `model-wide-cursor-moved` | `Select model (currently …)`; with no tick, `Select model` |
| `PI_MODEL_DEFAULT_RE` strips ` · default` from the current name | `:1741` | `model-wide`, `model-narrow` | `… vllm/Qwen/Qwen3.8-27B [lwsa-platform])`, never ` · default)` |
| `answerKeys` custom_text: `pi-input` is in the no-pre-Enter list and pushes `ctrl+k`, `ctrl+u` | `:2011`, `:2015` | `dialog-input` | `[ctrl+k, ctrl+u, text, enter]` — the line is emptied after the cursor and before the answer |
| `answerKeys` custom_text on a menu refuses (`customMenuIndex === null`) | `:2007` | `dialog-select` | `This prompt does not accept a custom answer.` |
| `answerKeys` option_indices: `!multi_select || length === 0` refuses, and pi has no `multiSteps` | `:2024`, `:2044` | `dialog-select` | `This prompt requires one or more selections.` |
| `answerKeys` option_index: range, custom-row and multi-select guards | `:2048-2050` | `dialog-select` (`option(3)`), `dialog-input` (`option(0)`) | `A valid option index is required.` |
| `answerKeys` option_index: `navigationKeys(index - selectedIndex)` then Enter | `:1987-1989`, `:2052` | `model-wide`, `model-wide-cursor-moved`, `dialog-select-wrapped-narrow`, `dialog-select-cursor-moved` | one move per row, `up` above and `down` below, then `enter` |
| `answerKeys` exactly-one-shape guard | `:1997-1998` | `answer_refuses_two_or_zero_shapes_and_out_of_range` | `Exactly one answer is required.` |
| `finishPrompt` id: `sha256(JSON.stringify({agent, …input, …hashed})).slice(0,12)` | `:228-242` | `dialog_select_reads_pi_options_in_order_and_pins_the_upstream_id` | the 12-hex content hash; cursor movement is not in the hash input |

## Expected ids

The port re-implements `finishPrompt`'s hash input byte for byte — `JSON.stringify` key order and
escaping — so the ids match upstream's. The expected values below were derived by running the
pinned hash expression (`JSON.stringify` + `sha256` + `slice(0,12)`) over the exact objects the
pinned readers build for these fixtures, and the port's own writer was checked to produce the same
bytes for every pi shape before the values were written down. The `@<row>` suffix is this lane's
cursor carrier (see the divergences below); `pi_prompt_content_id` returns the upstream part.

| Fixture | id | asserted in |
|---|---|---|
| `dialog-select.txt` | `b91f20ed7e62@0` | `dialog_select_reads_pi_options_in_order_and_pins_the_upstream_id` |
| `dialog-select-other-label.txt` | `13e14ed07883@0` | `dialog_select_other_label_is_another_asking` |
| `dialog-confirm.txt` | `cb9d4c8c3f45@0` | `dialog_confirm_reads_as_an_approval_and_no_is_pressed` |
| `dialog-confirm-wrapped.txt` | `f9b284b9824e@0` | `dialog_confirm_wrapped_message_reads_whole` |
| `dialog-input.txt` | `36052ff443a2@0` | `dialog_input_takes_text_after_emptying_the_line` |
| `dialog-select-wrapped-narrow.txt` | `7f2fd4f1e5b2@0` | `dialog_select_wrapped_narrow_joins_a_wrapped_row` |
| `model-wide.txt` | `d70050f3655a@0` | `model_wide_reads_the_catalogue_and_names_the_model_answering_now` |
| `model-narrow.txt` | `0f162d0db226@0` | not asserted (the narrow capture's question is checked instead) |
| `dialog-login-narrow.txt` | `02038d31f8ed@0` | not asserted |

## Fixture index

| Fixture | Pinned source | Screen |
|---|---|---|
| `dialog-select.txt` | `prompt.test.ts:1667` `select` | an extension's `select`, cursor on row 0 |
| `dialog-select-cursor-moved.txt` | `prompt.test.ts:1717` | the same, cursor moved to row 1 by hand |
| `dialog-select-other-label.txt` | `prompt.test.ts:1731` | the same with `Block` → `Block and say why` |
| `dialog-select-footer-time-changed.txt` | `prompt.test.ts:1735` | the same with the footer's clock at `12.3%` |
| `dialog-select-wrapped-narrow.txt` | `prompt.test.ts:1700` | a 46-column pane wrapping a row |
| `dialog-select-wrapped-narrow-second.txt` | `prompt.test.ts:1705` | the same, cursor on the unwrapped row |
| `dialog-confirm.txt` | `prompt.test.ts:1678` | `Clear session?` with a message line |
| `dialog-confirm-wrapped.txt` | `prompt.test.ts:1710` | `Delete the branch?`, message wrapped |
| `dialog-input.txt` | `prompt.test.ts:2770` `input("Branch name?")` | a dialog that wants text |
| `dialog-login-narrow.txt` | `prompt.test.ts:1866` `login` | `/login` at 46 columns, hint wrapped over the footer |
| `dialog-login-narrow-buried.txt` | `prompt.test.ts:1892` | `login + "Some later output\nand more\n"` |
| `dialog-answered-buried.txt` | `prompt.test.ts:1719` | an answered dialog, hint far above the bottom |
| `dialog-empty.txt` | `prompt.test.ts:1722` | `piScreen("")` — pi's own prompt |
| `model-wide.txt` | `prompt.test.ts:1744` `wide` | the catalogue at 140 columns |
| `model-wide-cursor-moved.txt` | `prompt.test.ts:1769` | the same, cursor on the model not in use |
| `model-wide-cut-mid-name.txt` | `prompt.test.ts:1777` `cut` | a row cut inside its provider bracket |
| `model-wide-one-row.txt` | `prompt.test.ts:1848` | the same filtered to one model |
| `model-wide-no-cursor.txt` | `prompt.test.ts:1857` | the same with the cursor on no row |
| `model-narrow.txt` | `prompt.test.ts:1800` `narrow` | the catalogue at 46 columns |
| `model-narrow-buried.txt` | `prompt.test.ts:1839` | `narrow + "Some later output\nand more\n"` |
| `model-half-name-cut.txt` | `prompt.test.ts:1844` `cut` | a bracket cut mid-word |
| `palette-two-columns.txt` | `prompt.test.ts:1724` | the slash palette, two columns |
| `tree-navigator.txt` | `prompt.test.ts:1726` | `/tree`, whose hint is `↑/↓ move` |

## Deliberate divergences from the pinned reader

1. **The cursor travels in the prompt id.** The frozen `ReferencePrompt` carries no cursor index,
   and this lane may not reshape the shared DTO, but pi's answers navigate from where pi drew the
   cursor (`/model` starts on the model in use). Upstream keeps the position in a `WeakMap` beside
   the public prompt — a side channel a pure `(prompt, answer) -> keys` planner does not have. The
   id is therefore `<upstream 12-hex content hash>@<row>`. The prefix is byte-identical to
   upstream's, so content staleness is unchanged; a cursor move now changes the suffix, which makes
   an open card stale instead of silently navigating from a stale row. That refuses rather than
   misfires — the same direction upstream's `aimed()` takes when the live row is not the one the
   moves were for. `pi_prompt_cursor` / `pi_prompt_content_id` read the two parts.
2. **The OmO fallback is not in this lane.** Upstream's pi branch ends with `...omo()`: a pane
   herdr names `pi` can be showing an OmO form. That fallback belongs to the omo lane (task 25);
   this module returns `None` where upstream would fall through, and the dispatcher (task 9)
   composes the two. `detector_claims_only_pi_and_the_aliases_are_the_same_functions` pins that
   this lane claims `pi` alone and never another registry id.
3. **`PI_WRAPPED_REST_RE` is rewritten without lookahead.** Upstream writes
   `^ (?![\u2192\u276f\u279c])\S`; the Rust regex crate has no lookahead. The lookahead's `\S`
   tests the very character it then consumes, so `^ [^\s→❯➜]` is the exact equivalent — not an
   approximation.
4. **The hash input is written by hand.** `serde_json`'s map is key-sorted by default, which would
   change the hashed bytes and therefore every id. The port emits `JSON.stringify`'s own key order
   and escaping instead. `pi_hash_input` is the only writer, and the pinned ids in the table above
   are what pins it.
5. **`/tree` is not read, on purpose.** Its hint says `↑/↓ move`, and answering it from the chat
   would move the session's branch, which the chat has no way to undo by clicking. Upstream leaves
   it alone for the same reason; `tree-navigator.txt` pins the refusal.
6. **The id-literal assertions are pinned, not derived at test time.** The five exact ids asserted
   in the tests are literals from the table above. If the port's writer ever drifts from
   `JSON.stringify`, those assertions fail loudly rather than silently producing new ids.

## What these fixtures do not prove

* **No live pi.** Every screen is the reference's own capture; nothing here is evidence that pi
  0.87.1 (or any pi) still draws these bytes. QA-05 needs the real pane.
* **No ANSI-laden capture.** All pinned screens are plain text, so the ANSI-stripping path
  (`cleanLine`) is exercised only through the shared reader's ported rule, never by a fixture with
  real escape sequences. Upstream's own pi captures have the same property.
* **No `/scoped-models`, and only one `moved` screen.** `/scoped-models` opens the same widget as
  `/login`, which is covered; the `moved` rule is pinned by the one hand-moved dialog and the
  moved catalogue, not by a live answer's intermediate draws (that is the route's, task 9/13).
* **No case-variant hints.** The pinned captures use pi's exact casing; `(?i)` is ported from
  upstream but not separated by a fixture.
* **No bordered (`│`) screen.** `cleanLine`'s box-border strip is ported for the shared reader; no
  pi capture needs it.

## Deferred command

Authored, **never executed** in this session (the run is deferred by explicit instruction,
`.omo/ulw-execute/herdr-reference-chat-parity-execution.md`):

```
cargo test --manifest-path src-tauri/Cargo.toml --lib reference_chat::prompt_pi
```
