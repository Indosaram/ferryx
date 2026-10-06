# OmO prompt family — fixture manifest and branch inventory

Owner: task 25 (`src-tauri/src/remote/reference_chat/prompt_omo.rs`).
Pinned upstream: devswha/herdr-web-ui @ 54e5a1f67090cb09552d182e7e30dd0ecc314918 (MIT,
docs/chat/HERDR_LICENSE). Hashes: docs/chat/herdr-port-contract.md §7 —
server/prompt.ts = 083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58.

These fixtures are **authored**, not captured: no live OmO session produced them, so they are a
branch inventory, not provenance evidence for QA-05. Task 14's producer owns sanitized capture of
real screens and session files.

## Pinned sources read for this lane (SHA-256 over the fetched UTF-8 bytes)

| Upstream path | SHA-256 |
|---|---|
| server/prompt.ts | 083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58 |
| server/omo-ask.ts | 76c3b35fc5d39cad81ae8afdc99d2187ce3ac95bdf06ff30e9b296f3c6ba5743 |

server/omo-ask.ts is not in the contract's §7 table (task 1 hashed the files the shared types
needed); its hash is recorded here because this lane ports it.

## Fixture files

| File | What it pins |
|---|---|
| form-tabbed.screen | the whole form: Ask user title, tab bar, question, numbered options with a wrapped description, the own-answer row, Submit (1/2 answered), the options hint |
| form-cut.screen | the same form with its tab bar scrolled off the pane (a cut form), the question wrapped mid-word |
| form-multi.screen | a multiple choice: the hint says toggle, one row already checked |
| form-typing.screen | the typed answer's row opened in the terminal (the typing hint) |
| form-review.screen | the Submit tab: Review your answers, a row per answer, the comment field |
| form-widget-opened.screen | the form the folded widget opens with the route's key |
| widget-pending.screen | the folded widget over OmO's input box for a question asked without waiting |
| form-stale.screen | the tabbed form answered, with the input box and typed text drawn under it |
| form-shell-quoted.screen | the form printed in a shell, with a shell prompt after the footer |
| form-no-rule.screen | the form with no footer rule under its hint |
| form-two-rules.screen | the form with two rules under its hint |
| form-box-under.screen | the form with an input box drawn under its footer |
| form-long-footer.screen | the form with more footer lines than the reference allows |
| widget-no-box.screen | the widget with its input box gone |
| widget-far-box.screen | the widget with its input box more than 60 lines below its hint |
| form-ambiguous.screen | a cut form whose rows match two open calls of the same shape |
| asks-two-questions.jsonl | a waiting call of two questions (the form's own text) |
| asks-cut-three-options.jsonl | a call whose current question has three options (the cut form shows two) |
| asks-multi.jsonl | a call whose question is a multiple choice |
| asks-folded.jsonl | a call that does not wait, accepted with a pending tool result |
| asks-ambiguous.jsonl | two open calls of the same shape |
| asks-none.jsonl | a session with no ask call at all |
| asks-lifecycle.jsonl | every way a call opens or closes: tool result, error result, answer frame, settlement record, a non-ask tool call |

## Branch inventory (every named omo branch of the pin, and where it lands)

| Upstream branch | Pinned anchor | Ported as | Fixture / test |
|---|---|---|---|
| Ask user title + tab bar + Submit tab (whole or wrapped) | prompt.ts:589-605 | omo_form | form-tabbed, form-cut, form-review |
| a tab cut with an ellipsis | :661-665 | same_header | asks-cut-three-options (header match) |
| the session's call is the form on screen (same tabs) | :668-670 | ask_on_screen | form-tabbed, form-multi |
| tab steps named by the call | :673-675 | omo_steps | form-tabbed (2 tabs) |
| card title by position among several questions | :678-680 | omo_title | form-tabbed, form-review |
| keys from the cursor to a row, cursor off screen | :686-688 | omo_walk | form-multi (checked row), answer plan |
| lines a narrow pane wrapped, joined (CJK inside a word) | :699-713 | join_wrapped | form-cut (question + description), form-tabbed (description) |
| the pane's width, from its own rules | :716-718 | screen_width | form-cut, form-tabbed |
| "Submit (n/m answered)" | :721-724 | omo_answered_count | form-review, form-tabbed |
| question view: numbered rows, wrapped labels, indented descriptions, the own-answer row, a cut form | :750-776 | omo_question_view | form-tabbed, form-cut, form-multi |
| the question of a cut-off form, matched by its rows | :782-788 | asked_question | form-cut (unique), form-ambiguous (two → refused) |
| where the form stands when its tabs are out of view | :794-801 | cut_steps | form-cut, widget-pending |
| omo-question: option pick, typed answer, multiple choice | :814-881 | parse_omo_question | form-tabbed, form-cut, form-multi |
| omo-question refuses a reviewing form, and a screen-only form without the call | :819, :844 | parse_omo_question | form-review (refused), form-cut without a session (refused) |
| omo-question requires the call's question to be the one shown (multiSelect, row count, question text, labels) | :837-839 | parse_omo_question | asks-cut-three-options vs form-cut |
| omo-typing: the typed answer's row opened | :893-934 | parse_omo_typing | form-typing |
| omo-review: Submit tab, a row per answer, the comment field, the notice | :954-1021 | parse_omo_review | form-review |
| omo-review rows named by the call's headers | :981-983 | parse_omo_review | form-review |
| omo-pending: the widget over the input box | :1038-1069 | parse_omo_pending | widget-pending |
| omo-pending matches one non-waiting call by header and question | :1049-1051 | parse_omo_pending | widget-pending, asks-folded |
| the candidate chain (question, typing, review, then pending) | :1931-1952 | omo_card_for_screen | every card test |
| with open calls, only the call the screen matches answers (exactly one) | :1934 | omo_card_for_screen | form-ambiguous (refused), form-cut |
| an omo form is live only with OmO's own footer under its hint (rule, at most 3 footer lines, no input box) | :1626-1647 | prompt_tail_is_active | form-stale, form-shell-quoted, form-no-rule, form-two-rules, form-box-under, form-long-footer |
| the widget is live over the input box (rule, box, at most 3 footer lines, box within 60 lines) | :1648-1664 | prompt_tail_is_active | widget-pending, widget-no-box, widget-far-box |
| the agent names that read an omo form (omo, empty, claude, pi) | :1941-1950 | omo_card_for_screen + omo_form_is_trusted | agent routing test |
| a pane herdr names claude is omo's only when it is blocked | :2476 | omo_form_is_trusted | trust test |
| the form marker that is worth a session read | :2451 | omo_form_on_screen | marker test |
| the route's key that opens the folded form | :67-81 (KEY.openQueue) | REFERENCE_OMO_OPEN_FORM_KEY | pending plan test |
| answerKeys: exactly one answer shape | :1998-1999 | omo_answer_plan | refusal test |
| answerKeys: a typed answer, refused for a menu without one | :2001-2005 | omo_answer_plan | form-tabbed, form-review, form-typing (refused) |
| answerKeys: multiple selections, toggled from the checked rows | :2023-2045 | omo_answer_plan | form-multi |
| answerKeys: an option index, refused for the custom row and for a multi-select | :2048-2051 | omo_answer_plan | refusal test |
| answerKeys: the card's own option steps, and an option that answers with Escape | :2052-2054 | omo_answer_plan | form-tabbed, form-typing |
| answerKeys: the card's own typed-answer steps | :2005 | omo_answer_plan | form-tabbed, form-review |
| answerKeys: the card's own multi-select steps | :2030 | omo_answer_plan | form-multi |
| answerKeys: a prompt not produced by the parser is refused | :1996-1997 | omo_card_for_prompt | refusal test |
| the card id (sha256 over the card's own fields, 12 hex) | :228-242 | omo_prompt_id | the folded widget and its opened form share one id |
| the body cap | :241 | OMO_PROMPT_BODY_MAX_CHARS | manifest only (12 000) |
| the answer route opens the folded form, then verifies call and question again | :2674-2699 | REFERENCE_OMO_OPEN_FORM_KEY + the pending card's empty plan | pending plan test |
| a recorded ask_user_question call and its waitForAnswer | omo-ask.ts waits | omo_ask_waits, omo_ask_of | asks-folded (wait false), asks-two-questions (wait true) |
| request_user_input is the same tool | omo-ask.ts OMO_ASK_TOOLS | omo_ask_tool | tool test |
| a call whose arguments are not the shape OmO asks with | omo-ask.ts omoAskOf | omo_ask_of | asks-lifecycle |
| a waiting call closes at its tool result | omo-ask.ts | omo_asks_after | asks-lifecycle |
| a non-waiting call closes only when settled (settlement record, answer frame, error result) | omo-ask.ts | omo_asks_after | asks-lifecycle, asks-folded |
| an accepted pending result keeps the call open | omo-ask.ts | omo_asks_after | asks-folded, asks-lifecycle |
| assistant narration is not evidence of an answer | omo-ask.ts | omo_asks_after | asks-lifecycle |
| incomplete calls are not open | omo-ask.ts | omo_asks_after | asks-lifecycle |
| open calls newest first, the newest is the pending one | omo-ask.ts, prompt.ts:644-658 | open_omo_asks, pending_omo_ask | asks-lifecycle |
| the records that can open or close a call | prompt.ts:636-637 | omo_ask_record_interesting | lifecycle test |

No named omo branch is silently skipped.

## Deliberately out of this lane (owned elsewhere, not skipped)

| Upstream behavior | Why it is not here | Owner |
|---|---|---|
| reading a pane's session file and choosing which omo transcript belongs to it (omoTranscriptForPane, omoCandidates, heldSessionIds) | file/session acquisition, not prompt detection | task 3 resolver (history.rs) |
| the answer route's serialized write, the settle waits, the asking lifecycle, the per-pane answer turns | the route and its writer ordering | task 9 (prompts.rs) |
| Codex's queue, the model lists, and Claude's, OMP's and Pi's own dialogs and fallback card | other provider families | tasks 22, 23, 24, 26 and task 9 |
| the live screen read and its screen revision | the read-only VT snapshot lane | task 6 (screen.rs) |

## Port boundaries (recorded, not skipped)

* **NFKC**: upstream normalizes with full NFKC before comparing screen text with the call's
  (comparable, prompt.ts:2523). This lane folds the compatibility range a pane's own text uses
  (fullwidth ASCII, the ideographic space) and leaves the rest alone, because the full
  normalization table is not a dependency of this crate and this lane may not add one.
* **Caps**: the body cap and the ellipsis comparison count characters, not UTF-16 units.
* **The answer registry**: upstream keys the parse's plan off the public prompt object in a
  WeakMap. This lane keeps the same shape in a bounded process-local registry keyed by the card
  id, because the frozen planner signature (contract §6) takes only the prompt and the answer.
  The route re-reads the screen and re-detects before planning, so the entry is as fresh as the
  last detection.
* **The folded widget's plan**: its option and typed plans are empty on purpose — the route
  opens the form with the route's own key, verifies the call and question again, and plans from
  the opened form (prompt.ts:2674-2702). The card is validation-only, as upstream states.
* **The footer cap**: a footer line is refused when it is longer than 200 characters, so a
  quoted transcript line cannot pass as OmO's footer.

## Deferred commands (not run; the execution override forbids intermediate checks)

    cargo test --manifest-path src-tauri/Cargo.toml --lib reference_chat::prompt_omo

Must be run by the post-merge verification wave together with the module registration of
pub mod prompt_omo; in src-tauri/src/remote/reference_chat/mod.rs (integration owner).

