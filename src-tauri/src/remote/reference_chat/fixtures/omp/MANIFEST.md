# omp fixture manifest (plan task 18)

Reference pin: `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918` (MIT,
`docs/chat/HERDR_LICENSE`). Line numbers below are that revision's.

Every named omp record/parser variant the pin handles is listed here with the fixture that
exercises it. A variant with no fixture would be an unexplained gap, so there is none.

| # | Variant (upstream) | Upstream anchor | Fixture |
|---|---|---|---|
| 1 | A prompt recorded as a plain `content` string | `transcript-records.ts:247-249` | `record-aliases.jsonl` (r1) |
| 2 | A prompt recorded as text blocks, joined by newlines | `transcript-records.ts:250-253` | `basic-session.jsonl` (m1), `task-results.jsonl` (t1) |
| 3 | An image-only user part is not a turn | `transcript-records.ts:254` | `partial-and-malformed.jsonl` (p6) |
| 4 | `context_clear` restarts the conversation | `transcript-records.ts:12-20`, `:204-205` | `compaction-and-clear.jsonl` (cc1) |
| 5 | A compaction entry becomes a card | `transcript-records.ts:220-226` | `compaction-and-clear.jsonl` (k1) |
| 6 | A compaction with a blank summary produces nothing | `transcript-records.ts:221-224` | `compaction-and-clear.jsonl` (k4) |
| 7 | A compaction with no summary produces nothing | `transcript-records.ts:221-223` | `compaction-and-clear.jsonl` (k5) |
| 8 | A `custom_message` notice in the user's seat, envelope stripped | `transcript-records.ts:85-91` | `task-results.jsonl` (n1) |
| 9 | A `omo-senpi:wake` task result, one row per task | `transcript-records.ts:118-150` | `task-results.jsonl` (w1) |
| 10 | Task status mapping: completed / failed | `transcript-records.ts:139` | `task-results.jsonl` (st-1, st-2) |
| 11 | Task titles from the `task` call's own result | `transcript-records.ts:101-110` | `task-results.jsonl` (t3) |
| 12 | `display: false` on the entry suppresses the record | `transcript-records.ts:26` | `partial-and-malformed.jsonl` (p2) |
| 13 | Assistant text blocks | `transcript-records.ts:269-270` | `basic-session.jsonl` (m2) |
| 14 | An empty text block is dropped | `transcript-records.ts:269` | `partial-and-malformed.jsonl` (p3) |
| 15 | Thinking, spelled `thinking` | `transcript-records.ts:271-273` | `basic-session.jsonl` (m2) |
| 16 | Thinking, spelled `text` | `transcript-records.ts:272` | `partial-and-malformed.jsonl` (p3) |
| 17 | A tool call: `name` / `arguments` | `transcript-records.ts:274-286` | `basic-session.jsonl` (c1, c2) |
| 18 | A tool call: `toolName` / `toolInput` / `toolCallId` | `transcript-records.ts:30` | `record-aliases.jsonl` (r2) |
| 19 | Summary from the call's own `intent` | `transcript-records.ts:276` | `basic-session.jsonl` (c1) |
| 20 | Summary from `toolSummary`'s first named argument | `transcript-records.ts:153-163` | `basic-session.jsonl` (c2) |
| 21 | Summary from a `task` call's task summaries, joined | `transcript-records.ts:155-159` | `task-results.jsonl` (c9) |
| 22 | A call with no id is shown but never adopts a result | `transcript-records.ts:285` | `partial-and-malformed.jsonl` (p3) |
| 23 | A call whose `arguments` is not an object | `transcript-records.ts:275` | `partial-and-malformed.jsonl` (c4) |
| 24 | An unsupported part type is ignored | `transcript-records.ts:287` | `partial-and-malformed.jsonl` (p3) |
| 25 | A tool result adopting its call, matched by id | `transcript-records.ts:229-243` | `basic-session.jsonl` (m3, m7) |
| 26 | A tool result whose content is a plain string | `transcript-records.ts:9-10` | `partial-and-malformed.jsonl` (p4) |
| 27 | A tool result's `error` flag | `transcript-records.ts:50` | `basic-session.jsonl` (m7) |
| 28 | A result for a call this page never saw is ignored | `transcript-records.ts:232` | `partial-and-malformed.jsonl` (p5) |
| 29 | A tool result block: `output` / `result` spellings, read only inside `content` | `transcript-records.ts:31`, `:33-35` | `record-aliases.jsonl` (r5) |
| 30 | Output cut at the page limit, with a fetch ref and size | `tool-output.ts:14-20` | `history_omp.rs` test (constructed) |
| 31 | A goal call's output kept whole up to the longer limit | `tool-output.ts:10-11` | `history_omp.rs` test (constructed) |
| 32 | `stopReason: "stop"` ends its turn | `transcript-records.ts:182,296` | `settled-turn-boundary.jsonl` (b2) |
| 33 | Assistant messages merge while the turn is open | `transcript-records.ts:184-190` | `basic-session.jsonl` (m2+m4) |
| 34 | `stopReason: "error"` adds the message's error text | `transcript-records.ts:293-295` | `partial-and-malformed.jsonl` (p7) |
| 35 | An empty `errorMessage` adds nothing | `transcript-records.ts:293` | `partial-and-malformed.jsonl` (p8) |
| 36 | A message with no content adds nothing | `transcript-records.ts:227-228` | `partial-and-malformed.jsonl` (p9) |
| 37 | A torn tail line while omp appends is skipped | `transcript-records.ts:198-202` | `partial-and-malformed.jsonl` (last line) |
| 38 | A non-JSON, `null`, array or scalar line is skipped | `transcript-records.ts:198-203` | `partial-and-malformed.jsonl` |
| 39 | Turns with no parts are dropped | `transcript-records.ts:300` | `partial-and-malformed.jsonl` (p6, p8, p9) |
| 40 | `maxTurns` keeps the newest turns | `transcript-records.ts:300` | `history_omp.rs` test (fixture 1) |
| 41 | A skill-invocation envelope: request kept, skill chipped | `skill-activity.ts:41-59` | `skill-invocation.jsonl` (s1) |
| 42 | The legacy `<skill name location>` envelope | `skill-activity.ts:60-62` | `skill-invocation.jsonl` (s3) |
| 43 | Prose that merely mentions the tag is the user's text | `skill-activity.ts:41-63` | `skill-invocation.jsonl` (s4) |
| 44 | `model_change` / `thinking_level_change` are not turns | `conversation-metadata.ts:64-66` | `basic-session.jsonl` (mc1, tl1) |
| 45 | A message-level-only `result` is never read; a message-level `callId` still names the call | `transcript-records.ts:27`, `:35` | `record-aliases.jsonl` (r3 -> `""`, r2 -> `callId` mapped) |

## Deliberate non-port, with its reason

* **`toolImages` stays off** (`conversation.ts:571-574`): only `pi-transcript` passes
  `toolImages: true`, so an omp tool part never carries `images` refs. The pi lane (task 21)
  owns that option.
* **`invokedSkill` is not called** (`conversation.ts:214`): the pinned `parseOmpTranscript`
  does not call it.
* **`abandoned` branches stay `null`**: the entry-tree projection belongs to pi
  (`pi-tree.ts`, task 21). omp writes a log.
* **`ReferenceTurn.source` stays `null`**: the pinned `parseOmpTranscript` sets no turn
  source. `ReferenceTurnSource::Runtime` is left to the integration owner rather than
  guessed here.
* **The `{ kind: "skill" }` part** (`transcript-records.ts:259`) ships: the request is the
  user's text and each skill the prompt invoked rides it as its own `ReferencePart::Skill`,
  in the order the envelope named them.

## Provenance of the fixtures

Fixtures are hand-authored to the record shapes the pinned parser reads — they are **not**
captured from a live omp session, and they are labelled as synthetic. Task 14's producer is
what captures authentic sanitized transcripts with their source hash; these files exercise
the parser's branches until that runs.
