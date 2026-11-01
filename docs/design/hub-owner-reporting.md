# Hub owner reporting: owner report rules, one outbound gate, commitments

Date: 2026-10-30. Status: approved for implementation; integration and acceptance checks ongoing.
Scope: `crates/ferryx-hub`. Incorporates `docs/research/harness2-pstack-adoption-2026-10-30.md`
(lesson ledger A1/A2, owner control A3, verifier rules A4, alert-only caps A5).
Evidence: `ferryx-wt/.evidence/report-gate-20261030/` (`findings.md`, `slack-audit-182.jsonl`,
`replay-expected.jsonl`, `score-replay.ts`).

## 1. Problem

The owner's top complaint, in his words: "이메일, 카톡, 문자를 jev로 판단해서 나한테 보내라고했는데
이거 자체가 제대로 안되고있고 이거 보내지 마 라고 한 게 메모리에 안들어가고 적용이 안되는게 제일 큰문제".

Measured causes:

- `inbox.rs` classifies every mail, kakao and imessage item with
  `judge.choice(Purpose::InboxClass, text, TRIAGE_INSTRUCTIONS, TRIAGE_OPTIONS)`.
  `TRIAGE_INSTRUCTIONS` (`inbox.rs:159`) is a compile-time constant. No code in the crate reads
  any owner memory, so nothing the owner says can change a classification.
- At 2026-10-30 00:14 KST the owner wrote "쓸데없는 메일 알림 하는거 하지마". The bot's memory repo
  recorded it (`human.md:34`, commit `bc5f3d5`). After that, five mails were still classified
  `owner_must_act` and delivered: Google security alert, GitLab sign-in from a new location,
  GitLab token expiry, Namecheap add funds, and "Sign in to Ferryx" (a magic link the bot's own
  work triggered). The hub's `memories` table has 0 rows. No table holds report feedback.
- The owner later clarified the rule: "보내지 말라는게아니고 사용자가 이거는 이제그만보내 라고 하면
  기억을 하라는거임 기본값은 당연히 보내느거지". The default stays "send". An objection mutes only
  the kind of alert he objected to, from then on.

The seven audit defects (182 Slack messages, 24h):

| # | Defect | Evidence |
|---|---|---|
| D1 | No salience step: duplicates, raw English, empty kakao alerts, already-blocked job cards | labels: 20 duplicate, 49 internal, 11 empty, 3 stale out of 151 bot rows |
| D2 | Promises are not tracked, and known outages are not reported | 20 promises, 7 never fulfilled (rows 17, 85, 87, 157, 165, 180, 181); Freelancer logged out since 10/28 with no report |
| D3 | Cron and report agents cannot see owner directives | each `cron-*` agent has its own memory; `cron-omon-content-intel-publish-ready` has none |
| D4 | Directives conflict | `agreements.md:22` bans build-in-public; the 10/30 correction in `boundaries.md:11` reverses it; both are active |
| D5 | A rerun is reported as successful from the exit code alone | `cron_remediate.rs:697`; #omo-home rows 56 and 63 say "verified rerun succeeded" for Freemoa |
| D6 | Internal progress text leaks to the owner | 49 rows labelled `suppress_internal` |
| D7 | Fixes are not in the installed build | `1c5ef853` on main; installed app is 2026.1029.1 |

## 2. Decision: base branch

Build on `merge/sup-20261030`, after it is merged to `main`.

That branch already has what this design needs and must not duplicate:

- `supervisor/memory.rs`: owner-authored teach, correct and forget through mhc, with
  `once:`/`이번만:` lines never learned, and a refusal to learn worker or supervisor text;
- `supervisor/request.rs`: one service behind MCP, daemon, desktop, CLI and remote HTTP;
- `supervisor/receipts.rs` and the `judge_calls` table for provenance;
- the research doc's lesson ledger is specified against it.

Building on `main` would mean reimplementing owner memory provenance and the service surface,
then reconciling two versions at merge.

Order of work:

1. Merge `merge/sup-20261030` to `main` (separate work, gated as usual).
2. Lanes L1 to L5 below. New migrations are numbered after the branch's last one,
   `0027_supervisor_cutover.sql`.

## 3. Owner report rules

### Two stores, kept separate

- **mhc owner memory** (`memory.rs teach`): the owner's own words, for recall by every agent.
  It is fuzzy: recall can miss a rule, so it cannot enforce one.
- **`owner_report_rules`** (new hub table): the deterministic form of the same objection.
  It is applied by code, before and after the judge, on every send path.

Each rule row stores the mhc receipt of the owner text it came from. Undoing the rule also
forgets that receipt, so the two stores never disagree.

### Capture

An objection is recognised when the owner, as the authenticated owner identity:

- replies to a delivered alert, or reacts to it with a mute reaction (Slack `:no_bell:`); or
- writes, in DM or a thread, text the judge classifies as an objection. This is one judge call:
  `Purpose::ReportFeedback`, with options `mute_kind`, `send_kind`, `not_feedback`.

Words such as "그만 보내", "이건 이제 보내지 마", "필요없어" and "이런 거 하지마" are examples for the
judge, not regexes.

The rule's scope comes from the alert being objected to, never from guessing:

| field | value |
|---|---|
| `source` | `mail`, `kakao`, `imessage`, `cron`, `github`, `ci`, `report` |
| `sender_key` | normalised sender address, domain or room id (optional) |
| `topic_key` | stable non-inbox topic; inbox item IDs are excluded when sender or subject identifies the kind |
| `pattern` | a subject stem taken from the alert, with digits and ids stripped (optional) |
| `effect` | `mute` (never send), `digest` (daily digest only), `send` (always send immediately) |

With no alert to point at ("쓸데없는 메일 알림 하지마"), the judge proposes a scope from the last 24h
of delivered alerts. The bot shows the matching alerts in Korean and asks once. The rule is
stored only after he confirms. This is the one case where the bot asks.

A `once:` or `이번만:` objection mutes only the alert it was attached to and stores no rule.

Feedback on a delivered inbox alert must match the next item of that kind, not only the
already-delivered item. The triage and outbound gate both normalise the sender and subject
stem before matching, including display-name addresses and changing numeric codes. Other
subjects from the same sender retain the default-send policy.

### Confirmation and undo

Delivery compares an applicable durable rule with the applicable memory directive.
A conflicting directive overrides the rule only when its source date proves it was
given later than the rule's recorded timestamp. Projection refresh and judge-call
timestamps are not owner-decision timestamps. Date-only directives use the earliest
possible instant for that date (UTC+14); overlapping dates, undated directives and
unparseable timestamps retain the explicit scoped rule rather than inventing order.
This limitation does not affect later scoped feedback, which records an exact time
and supersedes earlier matching rules. Unrelated scopes retain their own decisions.

After storing a rule the bot replies once, in Korean, in the same thread, for example:
"앞으로 GitLab 새 위치 로그인 메일은 보내지 않습니다. 다시 받으시려면 '이건 다시 보내'라고 해 주세요."

- `send_kind` ("이건 다시 보내") closes the matching rule (`superseded_by`) and records a new
  `send` rule.
- `/omo-rules` lists the active rules and closes one by number.

### Supersession

When rules overlap, the newest matching rule wins (`created_at`, then insertion order).
An older rule is closed with `superseded_by` only when the new rule covers its entire scope.
Broader rules remain active for unaffected items when a narrower exception is added. Two
different subjects from one sender never erase each other's rules. Closed rows remain for history.
When a new rule closes an older rule that has a different `effect`, the confirmation says so in
one line: "이전 규칙 '…'은 이 규칙으로 대체됐습니다."

### Application

- **Before the judge:** a matching `mute` rule closes the item as `muted` with no model call.
  The item is still logged in `report_suppressions`.
- **After the judge:** the class is the model's. A `digest` rule moves an owner item to the
  digest. A `send` rule forces immediate delivery even when the model says `fyi`.

### Judge instructions are assembled, not constant

`TRIAGE_INSTRUCTIONS` becomes the fixed base text. `inbox::triage_instructions(pool)` appends:

- the active owner report directives, at most 12 lines, from the directive projection
  (section 6);
- the active `owner_report_rules`, rendered as one line each;
- up to 6 of the owner's labelled examples: alerts he muted or asked to keep, with their
  decision;
- the report lessons block (section 8), at most 8 lines.

All appended text goes through the existing `fence`. The assembled text is stored on the
`judge_calls` row, so any decision can be traced to the rules it saw.

## 4. One outbound owner-report gate

New module `report_gate.rs`. Every owner-facing send calls `report_gate::submit(pool, Report)`
instead of posting directly. `Report` carries:

- `source` and `topic_key`;
- `salience`: `action` (owner must act), `decision` (owner must choose), `result` (asked-for
  outcome), `fyi`, or `internal`;
- `body` in Korean;
- an optional `thread`, `due_at` and `origin_ref`.

### Send paths rerouted

| path | today | change |
|---|---|---|
| `owner_alert.rs` `notify_owner` | posts every obligation | builds a `Report` and submits it |
| `owner_alert.rs` `sweep` | re-includes the same body | submits by `topic_key`; the gate dedupes |
| `cron_remediate.rs` `owner_message` | English "needs you" | Korean `Report` (section 7) |
| `inbox_cards.rs` (card post, ~348) | posts every card | submits; stale cards are dropped (below) |
| `mail_brief.rs`, `inbox_watch.rs` | "📬 …" lines | submit via `owner_alert` |
| `chat/turn_status.rs` (~182) | posts progress lines | `internal`, never sent to the owner DM |
| `chat/turn_approval.rs` (~229) | approval prompts | `decision`, always sent |
| `lanes/kibitzer.rs` wake output | free text | `fyi` or `action` per its envelope |
| `chat/slack.rs` `post_message`, `chat/slack_web.rs` | raw transport | reachable only from the gate and from direct replies to an owner message |

A direct reply is a requested result, not background FYI. Its intermediate and final reports
share the owner-turn identity. Before admission, a Jev comparison against the latest admitted
report in that same destination marks pure rewording as the existing state; the normal gate
then records a duplicate. New actions, decisions, outcomes, blockers, corrections and evidence
that changes the conclusion remain separate reports. Missing or capped judgment preserves
the candidate. Cancelled reports and other destinations never supply comparison context.
There is no elapsed-time window. Commitment-fulfilling results retain their explicit delivery.

Turn phase is context, not a salience verdict. Interim and final candidates are classified
with the originating request; a meaningful interim action or result is not discarded as
progress. Successful action, decision and result judgments survive the prose leak heuristic.
Classification metadata is persisted with the queued report for delivery-policy rechecks;
the originating request text is not persisted in that metadata. An internal final answer
to an explicit owner request is suppressed only after a report of that same destination
and turn has a non-cancelled `chat_outbox.delivered_at` receipt. Admission in `report_topics`
does not establish delivery. Without that receipt the real final answer remains eligible,
subject to owner rules, empty-content, stale and duplicate checks.

### Rules, in order

1. **Owner report rules** (section 3).
2. **Leak filter:** drop the report as `internal` when the body is a progress narration
   ("확인하겠습니다", "진행 중", tool names, stack traces, raw logs) or is mostly non-Korean. The
   check is code plus one small judge call (`Purpose::ReportLeak`) only for borderline cases.
   Raw output is never sent; a result report summarises it in Korean.
3. **Empty filter:** drop bodies with no text after removing links and images (the 7 bare
   kakao images, rows 131 to 137).
4. **Stale filter:** drop cards whose item is already closed or known blocked (a BOOST-only
   Wishket job, a Preferred-Freelancer-only listing). Uses `inbox_decisions.status` and the
   job-board's eligibility flag.
5. **Topic dedupe:** a report whose `topic_key` was already delivered is suppressed as
   `duplicate` until its state changes (a new `state_hash`). There is no time window: the
   same thing is said once, and said again only when something about it changed.
6. **Salience routing:** `action`, `decision` and `result` go to the owner DM immediately;
   `fyi` goes to the daily digest (#omo-home, one message per KST day); `internal` is never
   sent.

Every decision writes one `report_suppressions` row (kept or dropped, with the rule that
decided). This is the suppression log, and the replay reads it.

Delivery admission, the retry outbox row and the owner-feedback reference commit in one
SQLite write transaction. Inbox alert obligations join that transaction. A failed insert
must not consume the topic, and concurrent admissions of one state queue only one message.

The daily digest is queued at 18:00 KST or the next delivery sweep. It carries actual
content and quoted source text, not topic identifiers. Up to 20 pending topics are included;
remaining and later topics carry over to the next day. The latest state replaces earlier
pending content for a topic. Owner rules are checked again immediately before queuing, so
a mute received after deferral still applies. The outbox retries transport failures without
creating another digest for the same KST day. Each included topic retains a feedback reference.

Outbox retries recheck the latest owner rules immediately before transport. Muted messages
receive a cancellation timestamp, not a delivered timestamp, and their obligations become
suppressed. A pending digest removes only the muted parts and keeps the remaining content
and feedback references. A new digest rule defers an admitted immediate report without
losing its body. All outbox retry entry points use this same check.

External report admission accepts the producer's optional Slack blocks and thread. Blocks
commit with the gate decision and outbox, and retries preserve button action IDs rather than
replacing a card with plain text. An admission response is not a Slack delivery receipt;
producers that need a channel and message timestamp must obtain the actual transport receipt
before recording a posted card.

## 5. Commitment ledger and known outages

### Commitments

Every bot message in an owner conversation passes through `commitments::detect`: one small
judge call (`Purpose::Commitment`). It returns `none` or `{what, due}`, where `due` comes from
the text or defaults to 2h.

A detected commitment becomes a `commitments` row (`open`) with its thread. A later bot message
in the same thread that the judge marks as delivering it closes the row (`fulfilled`).

The sweeper runs every 10 minutes:

- An overdue row gets exactly one follow-up in the thread, in Korean, stating plainly what is
  still undone and why. It also emits a `lane_events` wake to the owning lane so the work
  resumes.
- A row the bot cannot do any more is closed `abandoned` with one honest line.
- The owner can close a row by replying.

The overdue state transition, report-gate decision, retry outbox row, feedback reference and
lane wake commit in one write transaction per commitment. Queue failure leaves the promise
open for the next sweep. Concurrent sweeps admit one follow-up. Delivery uses the stored
Slack channel and thread, and a transport failure leaves the outbox retry responsible for it.
The follow-up states the missed deadline without making another untracked promise.

### Known outages

A failing job or login gets an `outages` row keyed by `(subject, cause)`, for example
Freelancer logged out or a cron needing a login.

- It is reported once, as `action`, with the one thing the owner must do.
- It is reported again only when its state changes: recovered, or a new cause.
- Later failures of the same outage update `last_seen`. They produce no message.

## 6. Owner-directive projection into agents and cron prompts

New module `owner_directives.rs` builds one projection from the owner's memory:

- **Sources:** the bot's `system/agreements.md`, `system/boundaries.md` and `system/human.md`
  (INSTRUCTION lines), plus active `owner_report_rules`.
- **Parsing:** each directive is parsed with its date from the line's metadata
  (`since:`/`last:` or `[YYYY-MM-DD]`).
- **Supersession:** directives on the same subject are grouped by one judge call per new
  directive (`Purpose::DirectiveConflict`). A newer directive wins. A boundary beats an
  agreement of the same date. A superseded directive is excluded and listed in a "superseded"
  appendix for audit.
- **Conflict display:** a newly found conflict is sent once as `decision` ("이전 지시 '…'와 새 지시
  '…'가 충돌합니다. 새 지시를 따릅니다."), so the owner can correct it.
- **Injection:** `cron_exec.rs` prepends the projection (capped at 40 lines, fenced) to every
  cron prompt. The same block goes into every hub agent's session, including the lead,
  kibitzer and judge fallback, through the agent bootstrap. Each `cron-*` workspace keeps its
  own memory for its own facts but no longer needs its own copy of owner directives.
- **Freshness:** the projection is rebuilt when the memory repo's HEAD changes. Its hash is
  stored on each cron run, so a run shows which directives it saw.

This resolves D4 for the build-in-public case: the 10/30 boundary supersedes the 10/29
agreement.

## 7. Honest cron remediation

`cron_remediate.rs` stops treating exit code 0 as success. A rerun is reported as succeeded
only when the job's own output proves work happened:

- a new `cron_runs` row with a non-empty result;
- no failure marker in the output;
- for publishing jobs, the published item id or URL.

Before the runtime marks a problem fixed, a separate CronOutcome judgment checks
that the output describes the intended work actually completing. Login requests,
skipped work and a bare success/ok are not completion evidence. Only a nonfallback,
noncapped completed judgment closes the problem; unavailable judgment leaves it
unverified. The stored run receipt remains the evidence for that decision.

Otherwise the outcome is `unverified` or `failed`. The outcome taxonomy, with owner text in
Korean:

| outcome | owner text |
|---|---|
| verified | "재실행했고 결과를 확인했습니다: <요약>" (`result`, only if he asked or it had failed loudly) |
| unverified | "재실행은 끝났지만 결과를 확인하지 못했습니다: <무엇이 없는지>" (`action`) |
| needs owner | "<할 일 한 줄> 해 주셔야 다시 돌 수 있습니다" (`action`, via outages) |
| failed | "재실행도 실패했습니다: <원인 한 줄>" (`action`) |

The English "needs you" text (`owner_message`, line 126) is removed. `approval_instruction` is
rendered in Korean.

## 8. Lesson ledger for reporting quality (research A1 to A5)

The research doc's `supervisor_lessons` ledger is built as specified. It is also used for
report quality, with `domain = 'report'`:

- **Writing.** When the owner objects to a delivered report, or a commitment goes overdue, one
  `Purpose::LessonReflect` call writes 0 to 1 lessons. Example: "GitHub CI 실패 메일은 사장님 DM에
  보내지 않는다". Each lesson's `evidence_ref` names the objected alert or the overdue commitment
  row. A lesson without a resolvable ref is dropped. Lessons are never written to mhc owner
  memory; that would break `memory.rs`'s owner-only rule.
- **Using.** At most 8 lines, ordered `effective` first, then at most 2 `candidate`s, fenced.
  They go into the inbox judge instructions (section 3) and the gate's leak and salience calls.
- **Measuring (A2).** Owner reactions are the probe. A report kept under lesson L that he then
  objects to counts `hurt`. 7 days without objection to reports of L's topic counts `helped`.
  Promotion and retirement use the doc's thresholds. The doc and the UI say "observed".
- **A3 surfaces.** The supervisor service gains `lessons_list`, `lesson_retire`, `lesson_pin`,
  `report_rules_list` and `report_rule_close`. Slack gets `/omo-lessons` and `/omo-rules`.
  Lesson text cannot be edited; the owner retires a lesson or teaches memory.
- **A4.** The verifier proof ladder applies to remediation (section 7): a "verified" rerun needs
  a level-4 check, which is a command over the job's real output.
- **A5.** Token and wall caps alert through the gate as `action`, deduped by topic.
- **Not adopted.** Everything in research section 4 stays out: installing Harness², live
  Propose-Probe-Compose, lessons editing guardrails, `poteto-mode`, bulk skills.

## 9. Defect-to-mechanism map

| defect | mechanism |
|---|---|
| Top complaint (jev triage ignores "보내지 마") | section 3: rules applied before and after the judge; instructions assembled from directives, rules and examples; mhc teach for recall |
| D1 no salience | section 4 gate: leak, empty, stale, dedupe, salience routing, digest |
| D2 promises and outages | section 5 commitments and sweeper; outages reported once and on change |
| D3 agents blind to directives | section 6 one projection injected into every cron and agent |
| D4 conflicts | section 6 supersession, newest wins, conflict shown once |
| D5 false success | section 7 output-proven success, A4 ladder |
| D6 internal leaks | section 4 leak filter; `turn_status` is `internal` |
| D7 not deployed | release flow: version bump and maho-mac build after merge; install only on owner approval |

## 10. Data model

All new tables are created in migration `0028_owner_reporting.sql`.

| table | columns |
|---|---|
| `owner_report_rules` | id, source, sender_key, topic_key, pattern, effect (`mute`, `digest`, `send`), owner_words, origin_alert_ref, mhc_receipt, created_at, superseded_by, closed_at |
| `report_suppressions` | id, report_id, source, topic_key, salience, decision (`sent`, `digest`, `muted`, `duplicate`, `internal`, `empty`, `stale`), rule_ref, state_hash, created_at |
| `report_topics` | topic_key, last_state_hash, last_sent_at, salience |
| `commitments` | id, thread_ref, message_ref, what, due_at, state (`open`, `fulfilled`, `followed_up`, `abandoned`), fulfilled_by, followed_up_at, lane_key |
| `outages` | id, subject, cause, state (`active`, `recovered`), first_seen, last_seen, reported_at, last_reported_state |
| `owner_directive_projection` | hash, built_at, body, superseded_json |
| `supervisor_lessons` | as the research doc, plus `domain` (`goal`, `report`) |

Changed tables:

- `delivery_obligations` gains `report_id`, so old rows map to gate decisions.
- `judge_calls` gains `instructions_hash`.

## 11. Verification

**Unit tests** (hub):

- `report_rules::a_mute_rule_skips_the_judge`
- `report_rules::send_kind_supersedes_mute`
- `report_rules::once_objection_stores_no_rule`
- `inbox::instructions_include_active_rules_and_are_fenced`
- `report_gate::duplicate_topic_is_suppressed_until_state_changes`
- `report_gate::progress_narration_is_internal`
- `report_gate::empty_kakao_image_is_dropped`
- `report_gate::stale_card_is_dropped`
- `report_gate::fyi_goes_to_one_daily_digest`
- `commitments::overdue_promise_gets_one_follow_up_and_a_lane_wake`
- `outages::repeated_failure_reports_once_and_again_on_recovery`
- `owner_directives::newer_boundary_supersedes_older_agreement`
- `owner_directives::cron_prompt_contains_projection`
- `cron_remediate::exit_zero_without_output_is_unverified`
- `cron_remediate::owner_text_is_korean`
- `lessons::lesson_without_evidence_is_dropped`
- `lessons::injection_caps_at_eight_lines`

Each test is seen failing first against the current code.

**Incident replay** (`report_rules::incident_2026_10_30`), using the real 5 mail rows:

1. Directive "쓸데없는 메일 알림 하는거 하지마", confirmed with the CI-mail scope: CI and automated
   mail are muted.
2. The Google security alert is still delivered (default send).
3. The owner says "이건 이제 그만 보내" on the GitLab sign-in alert: the next GitLab new-location
   mail is muted. Google and Namecheap are still delivered.
4. "이건 다시 보내" restores GitLab.

**182-row replay:** a `report_gate` replay binary feeds the audited bot rows as candidate
reports and writes `{i, outcome}` JSONL. `score-replay.ts` passes when there are no missing
rows, no expected-suppress row is kept, and no expected-keep row is lost. Mismatches between
digest and suppress are listed, not failed.

The owner confirmed Ferryx-only scope: the 21 GeekNews/Instinct posts are explicitly
reported as `external_bot`, checked against the original sender, and excluded from
Ferryx acceptance. They are not counted as successful gate decisions. Missing Ferryx
producer evidence still fails. External producers using the Ferryx bot remain in scope.

**Commitment DB replay:** the 20 promise labels include two Instinct promises
(157 and 165), outside Ferryx visibility and excluded under the confirmed scope.
Each of the five unfulfilled Ferryx promises (17, 85, 87, 180 and 181) gets one
follow-up. Fulfilled promises get no subsequent follow-up; a promise fulfilled
after its deadline may already have received one, recorded separately.

**Gates:** `cargo test -p ferryx-hub` and `cargo clippy -p ferryx-hub --all-targets -- -D warnings`
on a remote host picked by `scripts/pick-build-host.sh`, never this Mac.

## 12. Implementation lanes

| lane | writes | depends on | tests |
|---|---|---|---|
| L1 schema + rules | `migrations/0028_owner_reporting.sql`, `src/report_rules.rs` (new), `src/inbox.rs` | sup branch in main | `report_rules::*`, `inbox::instructions_*`, incident replay |
| L2 gate | `src/report_gate.rs` (new), `src/owner_alert.rs`, `src/inbox_cards.rs`, `src/mail_brief.rs`, `src/inbox_watch.rs`, `src/chat/turn_status.rs`, `src/chat/turn_approval.rs`, `src/chat/slack.rs`, `src/chat/slack_web.rs`, `src/bin/report_replay.rs` (new) | L1 (tables) | `report_gate::*`, 182-row replay |
| L3 commitments + outages | `src/commitments.rs` (new), `src/outages.rs` (new), `src/chat/slack_turn.rs` | L1 | `commitments::*`, `outages::*`, commitment DB replay |
| L4 directives + cron | `src/owner_directives.rs` (new), `src/cron_exec.rs`, `src/cron_remediate.rs`, `src/cron_remediate/tests.rs`, `src/lanes/kibitzer.rs` | L1 | `owner_directives::*`, `cron_remediate::*` |
| L5 lessons + surfaces | `src/supervisor/lessons.rs` (new), `src/supervisor/request.rs`, Slack command registration file | L1, L2 | `lessons::*`, service and command tests |

`src/lib.rs` module registration is done once by L1 for every new module, so the lanes do not
collide on it.

## Generic owner

The hub is a product anyone can run, not one owner's bot. Every setting is listed in
[`docs/hub/CONFIGURATION.md`](../hub/CONFIGURATION.md).

- **Locale.** Owner-facing built-in texts, day boundaries, quiet hours and time labels come
  from `crate::locale` (`FERRYX_HUB_OWNER_LANGUAGE` / `FERRYX_HUB_OWNER_TIMEZONE`, or
  `ownerLanguage` / `ownerTimezone` in the hub config file). Built-in texts have a Korean and an
  English branch (`locale::text` / `pick`), and any other language gets English. Model prompts
  are written in English and ask for output "in {language name}". Daily caps, the usage
  digest's "yesterday", kibitzer and session quiet hours, and inbox filter stats all use the
  owner's local day. No code adds a fixed +9h offset, and no text hardcodes a "KST" label:
  labels come from `locale.format`, which appends the zone abbreviation.
- **No personal defaults.** There is no built-in Slack owner id, background model, requester
  name or personal URL. With no owner configured, the hub logs a config error and runs without
  one: there is no owner DM lead and nobody may press buttons. It never guesses a person.
  `OMON_BACKGROUND_MODEL` unset means background turns use the default model. Remaining
  hardcoded values outside this lane are listed as known gaps in CONFIGURATION.md
  (`OMON_PIRATETALK_URL` fallback, quota URL).
- **Incident ids.** A cron job script receives `FERRYX_CRON_RUN_ID`. When it reports a failure
  through the control socket it passes that id as `incident`, so the hub's own remediation of
  the same run is recognised as one incident and the owner hears about it once, not once from
  the script and again from remediation.

## 13. Open owner decisions

Decided by the owner on 2026-10-30:

1. **Build on the supervisor branch:** proceed as designed; `merge/sup-20261030` is merged into
   the work branch and reaches `main` with it.
2. **Bot-caused mail:** no special case. It is sent by default and muted only by his rules.
3. **Dedupe:** no time windows (rule 5): one message per topic, again only on a state change.

## 14. Follow-ups outside this crate

- **outsourcing-autopilot** (`~/code/project/outsourcing-autopilot/src/slack.ts`) posts job
  cards, "⏳ 접수했습니다" acks, results and collection errors straight to Slack with the hub
  bot token, so those posts skip the gate. The hub now exposes a control-socket `report` op
  (topic, salience, body, state, thread and blocks) and a `report_status` receipt query.
  Producer migration remains required acceptance work; an admission is not a Slack receipt.
  The source is not a git repository. Preserve a patch artifact and keep deployment separate
  from the Ferryx release; this does not make the bypass an acceptable final state.
  Versioned adapters are in `scripts/hub-report-adapters/autopilot.patch` and
  `scripts/hub-report-adapters/content-intel.patch`; their README describes ordering
  and receipt semantics. Both patches pass apply-check against the current producer
  sources. They are not applied automatically by building or installing Ferryx.
- **Release:** build unsigned on another Mac, never the owner's MacBook. The owner permits
  reaching maho-mac through the Ferryx relay when needed. Install and daemon handover
  wait for his approval; a paid builder requires separate authorization.
