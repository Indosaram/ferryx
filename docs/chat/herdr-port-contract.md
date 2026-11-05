# Herdr reference chat port contract (frozen)

Status: **frozen for authoring**. This document plus the DTO sources it names are the
only shared contract for every Herdr reference-chat task. No task may add, rename or
re-shape a wire field without a new revision of this file.

| Item | Value |
|---|---|
| Upstream reference | `devswha/herdr-web-ui` |
| Pinned revision | `54e5a1f67090cb09552d182e7e30dd0ecc314918` |
| Upstream license | MIT, `Copyright (c) 2026 devswha` — verbatim copy at [`docs/chat/HERDR_LICENSE`](./HERDR_LICENSE) |
| Ferryx base | `19e99a149bddec45ef29e825c948ce587f380591` (branch `work/herdr-reference-chat-w1`) |
| Rust DTO source | [`src-tauri/src/remote/reference_chat/types.rs`](../../src-tauri/src/remote/reference_chat/types.rs) |
| TS DTO source | [`ui/src/remote/chat/referenceTypes.ts`](../../ui/src/remote/chat/referenceTypes.ts) |
| Reused Ferryx contracts | `src-tauri/src/scoped_contracts.rs`, `ui/src/lib/scopedContracts.ts` |
| Plan | `.omo/plans/herdr-reference-chat-parity.md` (task 1) |
| Revision | **3** — additive, negotiated. Revision 2 remains valid. A rev2 client keeps polling full pages; a rev2 host never receives delta, control, payload, or continuation requests from a rev3 client. Wire shapes are §10. What the current host fills, and what is still pending, is §11. |

Everything in this file that is not a *port obligation* is a *port boundary*: a fact
about the reference that later tasks must not silently exceed.

---

## 1. What is ported, and what is deliberately not

The reference serves a **chat lens over the pane's own program**. The pane's PTY stays
the input path; the transcript stores are read-only.

| Reference behavior | Ported into | Boundary |
|---|---|---|
| Six native transcript readers (`claude-transcript`, `codex-transcript`, `omp-transcript`, `omo-transcript`, `gjc-transcript`, `pi-transcript`) | tasks 2, 17–21 | Only these six are native readers. `gjc` writes omp's record shape. |
| Non-native panes | task 5 presentation, task 12 wiring | Same-pane scrollback with an explicit disclosure label. Never invented assistant turns, never a different session's file. |
| Prompt detectors (Claude, Codex, OMP, OMO, Pi) | tasks 9, 22–26 | `gjc` has **no named detector** in the reference allowlist. |
| Original-pane submit / Stop | tasks 7, 12 | Submit is a per-pane serialized transaction; Stop is provider-routed and may be a typed refusal. |
| Owner-host files | task 11 | The path is delivered to the agent as editable `@path` text. |
| Drafts / held messages | task 8 | ACK settles the sent prefix only; held rows never auto-send. |

**24-row action matrix.** Every Ferryx registry entry keeps generic original-pane access
(submit, Stop, terminal). Native readers and structured detectors are advertised only
where the reference actually has them. This matrix is the normative coverage record.

| Registry ID | Native reader | Detector set | Original PTY submit/Stop/files |
|---|---|---|---|
| claude | claude-transcript | claude | required |
| codex | codex-transcript | codex | required |
| omo | omo-transcript | omo | required |
| omp | omp-transcript | omp | required |
| pi | pi-transcript | pi | required |
| gjc | gjc-transcript | none (fallback card only) | required |
| opencode | none | none | required |
| prime-agent | none | none | required |
| antigravity | none | none | required |
| mimo-code | none | none | required |
| droid | none | none | required |
| grok | none | none | required |
| devin | none | none | required |
| kimi | none | none | required |
| copilot | none | none | required |
| cursor | none | none | required |
| cursor-agent | none | none | required |
| aider | none | none | required |
| crush | none | none | required |
| cline | none | none | required |
| hermes | none | none | required |
| goose | none | none | required |
| codebuff | none | none | required |
| rovo | none | none | required |
| openclaw | none | none | required |

"none" is a **boundary**, not a permanent denial: `NativeHistoryKind::Unavailable` means
*this agent has no native reader today*. A later task that verifies a real reader adds it
here with evidence; it never guesses a transcript path from the agent's label.

The table above is the original Herdr six. `ReferenceNativeHistoryKind` now also has
native readers for copilot, antigravity, kimi, grok, cline, opencode, cursor, and crush.
That inventory, and what those readers actually emit, is §11. This table is not the
rev3 support matrix.

---

## 2. Target identity

`TargetRef` is **reused verbatim** from the frozen scoped contracts
(`scoped_contracts.rs` / `scopedContracts.ts`):

```
hostId, ownerId, epoch (canonical decimal u64 string), backendSessionId
```

`epoch` is a **string** on the wire in both languages: a u64 above 2^53 must not round
through a JS `number`. The Rust `Epoch` type refuses a non-canonical decimal encoding.

The reference chat contract adds exactly one optional refinement:

```
providerSessionId?: string | null
```

* It is the provider/native session identity **where the reference reader identified
  one**; `null`/absent means *unknown*, never "use another session".
* A **visual leaf ID is never a target.** The target is owning host + backend session +
  daemon incarnation + provider/native session identity where known.

### Reads vs mutations

| Request | Target binding |
|---|---|
| `GET /history`, `GET /screen`, `GET /prompt` | query `target` + optional `epoch`, `providerSessionId` |
| `POST /submit`, `POST /stop`, `POST /answer`, `POST /files`, `DELETE /files/{id}` | body `{ target, requestId, payload }` |

Routes (registered by task 13, **not** by this task):

```
GET    /api/v1/reference-chat/{sessionId}/history?limit&cursor
GET    /api/v1/reference-chat/{sessionId}/screen
GET    /api/v1/reference-chat/{sessionId}/prompt
POST   /api/v1/reference-chat/{sessionId}/submit
POST   /api/v1/reference-chat/{sessionId}/stop
POST   /api/v1/reference-chat/{sessionId}/answer
POST   /api/v1/reference-chat/{sessionId}/files
GET    /api/v1/reference-chat/{sessionId}/files/{fileId}
DELETE /api/v1/reference-chat/{sessionId}/files/{fileId}
```

---

## 3. Errors and results — reuse, never duplicate

This contract introduces **no new error enum and no new result envelope**. It reuses:

* `ScopeErrorCode` / `ScopeError` / `ScopeResult` — `scoped_contracts.rs`, `scopedContracts.ts`
* `DeliveryStage` / `DeliveryReceipt` — the staged → accepted → providerRead ladder
* `MutationEnvelope<P>` — `{ requestId, target?, params }`

### `ScopeErrorCode` extension — REQUIRED, but owned outside task 1

The reference contract needs one new variant on the frozen `ScopeErrorCode` enum in **both**
languages:

```
"OPERATION_OUTCOME_UNKNOWN"
```

* TypeScript: a new member of the `ScopeErrorCode` union in `ui/src/lib/scopedContracts.ts`.
* Rust: `ScopeErrorCode::OperationOutcomeUnknown` in `src-tauri/src/scoped_contracts.rs`,
  which must carry an explicit `#[serde(rename = "OPERATION_OUTCOME_UNKNOWN")]` — the enum
  uses `SCREAMING_SNAKE_CASE`, which would otherwise emit `OPERATION_OUTCOME_UNKNOW_N`.

It is a **pure widening**: no existing variant, order or wire string changes. It exists so that
"accepted but not confirmed" is a *typed state*, not a boolean.

**Task 1 does not make this edit.** Its file scope excludes both `scoped_contracts.rs` and
`scopedContracts.ts`, so the variant is recorded here as an integration need for the owner of
those files. Until it lands, the wire value is still named by the constants task 1 does ship
(`REFERENCE_OUTCOME_UNKNOWN_CODE` in `types.rs`, `REFERENCE_OUTCOME_UNKNOWN_CODE` in
`referenceTypes.ts`) and tested by `reference_is_outcome_unknown` /
`referenceIsOutcomeUnknown`, so no lane has to hardcode the string.

### Delivery ladder (unchanged, and its exact meaning)

| Stage | Means | Does **not** mean |
|---|---|---|
| `staged` | the request was authorized and the payload validated | any byte reached the pane |
| `accepted` | the writer accepted the bytes | the provider consumed them |
| `providerRead` | a **matching native observation** confirms consumption | available for generic sources |

For a source with no native reader the ladder **ends at `accepted`**. Claiming
`providerRead` there is a contract violation.

### Accept-then-unknown

A mutation whose outcome cannot be determined (writer deadline, transport loss after the
write was dispatched) is answered with `ScopeResult` **failure** carrying
`code = OPERATION_OUTCOME_UNKNOWN`, `retryable = false`. The caller holds the pending
record; it never auto-replays. Duplicate `requestId` with an identical payload fingerprint
returns the recorded state; a conflicting payload fails with `REQUEST_CONFLICT`.

---

## 4. History

### Types

```ts
type ReferenceHistorySource = "claude-transcript" | "codex-transcript" | "omp-transcript"
  | "omo-transcript" | "gjc-transcript" | "pi-transcript" | "scrollback";

type ReferenceHistoryAvailability = "native" | "scrollback" | "notStarted";

type ReferencePartKind = "text" | "thinking" | "skill" | "tool" | "image" | "compact" | "notice" | "taskResult";
type ReferenceTurnRole = "user" | "assistant";
type ReferenceTurnSource = "typed" | "runtime";   // runtime = a turn nobody typed
```

* `native` — an exact native file was resolved for this target.
* `scrollback` — **explicit** same-pane output. It is a legitimate primary result, not a
  failure, and must be labelled as such in the UI.
* `notStarted` — the pane's agent holds a session it has not written yet: a conversation
  with zero turns, **not** a missing one.

An exact-lookup failure (auth, identity mismatch, ambiguous owner, missing file) is a
`ScopeResult` **error**, never permission to read a different file or session.

### Cursor

```ts
interface ReferenceHistoryCursor {
  streamId: string;   // identity of the transcript the cursor was minted against
  offset: number;     // byte offset inside that stream
}
```

* `streamId` must be re-checked against the live stream identity on every page.
* A cursor minted against another stream is refused with `TARGET_EXPIRED` (never silently
  re-anchored).
* `scrollback` has **no older cursor** — `cursor` is `null` and `hasMore` is `false`.

### Page and envelope

```ts
interface ReferenceHistoryPage {
  source: ReferenceHistorySource;
  availability: ReferenceHistoryAvailability;
  turns: ReferenceTurn[];
  cursor: ReferenceHistoryCursor | null;  // page before this one
  hasMore: boolean;
  generation: string;                     // changes when the answer could
  unavailableReason: string | null;       // required when availability !== "native"
}
```

`generation` is the frontend's fence (task 4): a response whose generation is not the
requested one is discarded, never painted.

`ReferenceTurn` carries `role`, `parts`, `startedAt`, `endedAt` (last recorded assistant
activity — never the next user's timestamp), and the optional
`abandoned: ReferenceAbandonedBranch | null` disclosure for turns a `/tree` walked away
from. `ReferencePart` is a discriminated union on `kind`; every variant is a real shape
from the pinned upstream protocol, not a placeholder.

### The standalone skill part (revision 2)

```ts
{ kind: "skill"; skill: ReferenceSkillActivity }
```

`ReferenceSkillActivity` is the pinned `SkillActivity` — `{ name, evidence, status, path? }` —
where `evidence` is `"invocation" | "instructions"` and `status` is
`"requested" | "loaded" | "failed"`. Evidence of skill activity, never a claim that the
skill's workflow completed.

It is emitted **only** where the pinned readers emit it:

| Site | What the pinned reader does |
|---|---|
| `server/transcript-records.ts:259` (omp, omo, gjc, pi) | the user turn's parts are `[text(asked), …one skill per skill the prompt invoked]`, in the order the runtime named them |
| `server/codex.ts:241` (`item_completed`) | a completed read whose skill no tool part carries becomes its own part; a read that **is** on a tool part updates that part's `skill.status` instead |
| `server/codex.ts:275` (a user `response_item`) | Codex's explicitly selected skill becomes its own part, unless a part with the same name and path already exists |

The claude reader emits **no** standalone part: `invokedSkill` attaches the skill to the
`Skill` tool call it came from, which is `ReferencePart::Tool::skill`. A skill on a tool call
is never duplicated as a part.

**Drawn by the turn's skill list, not by the inline part list.** The reference filters
`kind !== "skill"` out of the parts it renders in order (`src/components/ChatView.tsx:322`)
and lists a turn's skills from `turnSkills(parts)`, which reads a tool's `skill` and a
standalone `skill` part alike, keyed `${evidence}:${path ?? name}`
(`src/lib/skillActivity.ts`). Ferryx mirrors that split: `referenceTurnSkills` reads both,
and no part view draws a skill chip inline.

No skill is ever fabricated. A pane with no native reader is `scrollback`, whose turns are
same-pane output with no native parts at all, so it can never carry a skill chip.


---

## 5. Screen, submit, Stop, answer, files

```ts
interface ReferenceScreenSnapshot {
  revision: string;          // changes whenever the visible screen changes
  text: string;              // VT-aware, segmented, bounded
  truncated: boolean;
  gap: boolean;              // replay gap: the reader could not reconstruct history
  cols: number; rows: number;
}

interface ReferenceSubmitPayload {
  text: string;
  attachmentIds: readonly string[];
  origin: "chat" | "terminal";
}

interface ReferenceStopPayload {
  capability: ReferenceStopCapability;   // what the caller observed on the screen
}

type ReferenceStopCapability = "providerInterrupt" | "shellSignal" | "refused";

interface ReferencePromptAnswerPayload {
  promptId: string;
  screenRevision: string;    // the revision the card was rendered from
  answer: ReferencePromptAnswer;
}

interface ReferenceFileStagePayload {
  name: string;
  mediaType: AttachmentMediaType;
  sizeBytes: number;
  contentBase64: string;
}
```

### Submit — ordered transaction

* Per-pane **serialized**: a Stop tapped right after Send must not land between the text
  and its Enter. Submit, Stop and prompt answers share one ordering.
* Reauthorization happens **before** mutation.
* Payload shaping is normative from the pinned `src/lib/compose.ts`:
  trim trailing CR/LF, normalize CRLF, convert internal LF to CR, bracket only when the
  pane's own bracketed-paste mode is on, and submit Enter **separately**.
* `MAX_COMPOSER_CHARS = 20_000` is retained, subject to the existing transport byte limit:
  multibyte text is rejected (`PAYLOAD_TOO_LARGE`) **before** writing if the encoded frame
  would exceed it.

### Stop — provider-routed, honestly labelled

`providerInterrupt` — the reference sends `Escape` for a TUI it knows handles it.
`shellSignal` — the explicit-terminal path (`Ctrl-C`) stays a separate, deliberate action.
`refused` — the capability is unknown for this target: the request fails with
`UNSUPPORTED`. **A killing signal is never substituted for unknown behavior.**

### Answer — single use, fresh screen

* `promptId` + `screenRevision` bind the answer to the exact card the user saw.
* The current screen is re-read **immediately before** the serialized answer keys.
* A changed screen fails with `REQUEST_CONFLICT` (`staleScreen`); a replayed answer fails
  the same way. The answer is single-use.

### Files — owner-host, bounded

* Staging happens on the **owning host**; `ReferenceFileReceipt.attachmentId` is the
  opaque id from the existing `AttachmentReceipt` (`hostId`, `attachmentId`, `sha256`,
  `sizeBytes`, `mediaType`). No browser-local path ever crosses the wire.
* The staged file becomes an editable `@path ` mention with whitespace-aware insertion.
* Limits reuse `ATTACHMENT_MAX_FILE_BYTES` / `ATTACHMENT_MAX_FILES_PER_TURN` /
  `ATTACHMENT_MAX_TURN_BYTES` / `ATTACHMENT_UNREFERENCED_TTL_MS` from `scopedContracts`.
* Deletion is explicit (`DELETE /files/{fileId}`); a failed send never deletes the draft.

---

## 6. Public signatures for independent lanes

These are the **frozen entry points**. Provider families (tasks 17–26), the dispatchers
(tasks 2, 9) and the client (task 4) all consume the types above through these names. A
lane implements its own file; it does not change another lane's signature.

### Rust — `src-tauri/src/remote/reference_chat/types.rs` (shipped by task 1)

Constants: `REFERENCE_CHAT_ROUTE_PREFIX`, `REFERENCE_SCROLLBACK_DISCLOSURE`,
`REFERENCE_SUBMIT_MAX_CHARS`, `REFERENCE_NATIVE_HISTORY_KINDS`,
`REFERENCE_OUTCOME_UNKNOWN_CODE`.

Lane obligation types (the aliases a family lane's function must satisfy):

```rust
pub type ReferenceHistoryParser =
    fn(kind: ReferenceNativeHistoryKind, text: &str) -> Result<Vec<ReferenceTurn>, String>;
pub type ReferencePromptDetector = fn(agent: &str, screen: &str) -> Option<ReferencePrompt>;
pub type ReferenceAnswerPlanner =
    fn(prompt: &ReferencePrompt, answer: &ReferencePromptAnswer)
        -> Result<Vec<ReferenceKeyStep>, String>;
```

Shared helpers task 1 ships and every lane uses instead of re-deriving them:

```rust
impl ReferenceTargetRef {
    pub fn without_provider_session(target: TargetRef) -> Self;
    pub fn with_provider_session(target: TargetRef, provider_session_id: impl Into<String>) -> Self;
    pub fn has_provider_session(&self) -> bool;
}
impl ReferenceHistorySource {
    pub fn native_kind(self) -> ReferenceNativeHistoryKind;
    pub fn as_str(self) -> &'static str;
}
impl ReferenceNativeHistoryKind {
    pub fn from_registry_id(registry_id: &str) -> Self;
    pub fn is_native(self) -> bool;
    pub fn as_str(self) -> &'static str;
}
impl ReferenceHistoryCursor {
    pub fn matches_stream(&self, stream_id: &str) -> bool;
}
impl ReferenceHistoryPage {
    pub fn is_native(&self) -> bool;
    pub fn disclosure(&self) -> Option<&'static str>;
    pub fn can_reach_provider_read(&self, target: &ReferenceTargetRef) -> bool;
}
impl ReferencePart {
    pub fn kind(&self) -> ReferencePartKind;
    pub fn is_inline_text(&self) -> bool;
}
impl ReferenceScreenSnapshot { pub fn is_answerable(&self) -> bool; }
impl ReferenceStopCapability { pub fn is_refusal(self) -> bool; }
impl ReferencePrompt {
    pub fn selectable_indices(&self) -> Vec<usize>;
    pub fn needs_confirmation(&self, answer: &ReferencePromptAnswer) -> bool;
}
impl ReferencePromptAnswer {
    pub fn variant_count(&self) -> usize;
    pub fn is_single_choice(&self) -> bool;
}
impl ReferenceKeyStep {
    pub fn typed(text: impl Into<String>) -> Self;
    pub fn keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self;
    pub fn is_effective(&self) -> bool;
}
impl ReferenceFileReceipt { pub fn mention_for(path: &str) -> String; }
pub fn reference_stage_rank(stage: DeliveryStage) -> u8;
pub fn reference_stage_at_least(stage: DeliveryStage, floor: DeliveryStage) -> bool;
pub fn reference_chat_route(session_id: &str, suffix: &str) -> String;
pub fn reference_is_outcome_unknown(code: &str) -> bool;
pub fn reference_draft_key(target: &ReferenceTargetRef) -> String;
```

Signatures the later lanes must provide, satisfying the aliases above:

```rust
// History dispatcher (task 2) and families (tasks 17-21):
pub fn dispatch_reference_history(kind: ReferenceNativeHistoryKind, bytes: &[u8])
    -> Result<Vec<ReferenceTurn>, String>;

// Screen lane (task 6):
pub fn snapshot_reference_screen(
    mirror: &mut crate::remote::mirror::RemoteTerminalMirror,
    segments: Option<&[Vec<u8>]>,
) -> Result<ReferenceScreenSnapshot, String>;

// Input lane (task 7):
pub fn shape_reference_submit(text: &str, bracketed_paste: bool) -> Result<String, ScopeErrorCode>;
pub fn stop_keys_for(capability: ReferenceStopCapability) -> Result<&'static [u8], ScopeErrorCode>;

// Prompt dispatcher (task 9) and detectors (tasks 22-26):
pub fn detect_reference_prompt(agent: &str, screen: &str) -> Option<ReferencePrompt>;
pub fn reference_answer_keys(
    prompt: &ReferencePrompt,
    answer: &ReferencePromptAnswer,
) -> Result<Vec<ReferenceKeyStep>, ScopeErrorCode>;

// Files lane (task 11):
pub fn stage_reference_file(payload: &ReferenceFileStagePayload)
    -> Result<ReferenceFileReceipt, ScopeErrorCode>;
```

### TypeScript — `ui/src/remote/chat/referenceTypes.ts` (shipped by task 1)

```ts
export const REFERENCE_CHAT_ROUTE_PREFIX: string;
export const REFERENCE_SCROLLBACK_DISCLOSURE: string;
export const REFERENCE_SUBMIT_MAX_CHARS: number;
export const REFERENCE_OUTCOME_UNKNOWN_CODE: string;

export function referenceTargetKey(target: ReferenceTargetRef): string;
export function sameReferenceTarget(a: ReferenceTargetRef, b: ReferenceTargetRef): boolean;
export function referenceTargetHasProviderSession(target: ReferenceTargetRef): boolean;
export function referenceDraftKey(target: ReferenceTargetRef): string;
export function referenceCursorMatches(cursor: ReferenceHistoryCursor, streamId: string): boolean;
export function referenceCursorKey(cursor: ReferenceHistoryCursor): string;
export function referenceHistoryIsNative(page: ReferenceHistoryPage): boolean;
export function referenceHistoryDisclosure(page: ReferenceHistoryPage): string | null;
export function referenceCanReachProviderRead(
  page: ReferenceHistoryPage, target: ReferenceTargetRef): boolean;
export function referenceStageRank(stage: DeliveryStage): 0 | 1 | 2;
export function referenceStageAtLeast(stage: DeliveryStage, floor: DeliveryStage): boolean;
export function referenceStopIsRefusal(capability: ReferenceStopCapability): boolean;
export function referenceIsOutcomeUnknown(code: string): boolean;
export function referenceAnswerVariantCount(answer: ReferencePromptAnswer): number;
export function referenceAnswerIsSingleChoice(answer: ReferencePromptAnswer): boolean;
export function referenceSelectableIndices(prompt: ReferencePrompt): readonly number[];
export function referencePromptNeedsConfirmation(
  prompt: ReferencePrompt, answer: ReferencePromptAnswer): boolean;
export function referencePartIsInlineText(part: ReferencePart): boolean;
export function referenceKeyStepIsEffective(step: ReferenceKeyStep): boolean;
export function referenceMentionFor(path: string): string;
export function referenceNativeKindFromRegistryId(registryId: string): ReferenceNativeHistoryKind;
export function referenceNativeKindIsNative(kind: ReferenceNativeHistoryKind): boolean;
export function referenceNativeKindOfSource(source: ReferenceHistorySource): ReferenceNativeHistoryKind;
export function referenceChatRoute(sessionId: string, suffix?: string): string;
```

Wire-name rule for both languages: `ReferenceHistorySource` and `ReferenceNativeHistoryKind`
serialize **kebab-case** (`claude-transcript`, `unavailable`); every other enum serializes
camelCase (`notStarted`, `providerInterrupt`, `taskResult`). `ReferenceTargetRef` is the
scoped `TargetRef` flattened with one optional `providerSessionId`.

---

## 7. Provenance hashes

Every file below was read at the pinned revision: the first block during task 1 authoring,
the two task-27 rows during the revision-2 authoring. SHA-256 is over the exact bytes read
(UTF-8).

### Upstream reference (pin `54e5a1f6…`)

| Upstream path | SHA-256 |
|---|---|
| `server/conversation.ts` | `c02267560aafdaf4a07a8c440f2c2d34d7379f0b4711acc934131145e7b5159d` |
| `server/prompt.ts` | `083a74a29015258f7c1e11016c4f520cf265fb2ca89013feede71c2e030dba58` |
| `server/transcript-records.ts` | `e6a50c9b1e6523ed25e027d0c26493c5c5f4f573bfd22a04e03528e9e943099f` |
| `server/codex.ts` | `eedfd59a54ccf0935d0fc0a658ceb457af8953e9a9dc99e60a181d45c659114a` |
| `server/index.ts` | `66c11aee5dad62811b78ddb30e37c3dab446c10aca3072faa3e8087928993655` |
| `server/paste.ts` | `7d32165407c842725c8ce6c015fa5ab5c8c1d4d8f77df5c0f35752955711d023` |
| `shared/protocol.ts` | `418c140e1559ed14f19fcee641a69b99e29d77cc2011db347f66ef5b241059ea` |
| `src/lib/compose.ts` | `37037676e4511c94f279c031c9456966be8b30c5f04b3fdcfa19e186aa12ccde` |
| `src/lib/promptAnswer.ts` | `044fc3872f67d760899f05860a8935c66cf5b2d69bdb8725dd4f3e9bb1928b5b` |
| `src/lib/skillActivity.ts` | `312864f34fffa694475b275475572eb2c7f9bfc145dcbad86113156181460387` |
| `src/components/ChatView.tsx` | `17a76e86c069d9209dcb11531e99d2c9fc55e21fc73ca03b69c37d4f9fa5d8c1` |
| `LICENSE` | `adf73f123ae2cb21845c459d25df12b8c7988fa258bf606a543fd1f4b22ff4ab` |

### Ferryx port sources

| Path | SHA-256 |
|---|---|
| `ui/src/remote/RemoteApp.tsx` (main) | `6ff624d7ae9c78b7aef6921a9e52ec9be8ee9a6bd9156467ab777f8e6b43e433` |
| `ui/src/remote/agentConversation.ts` (main) | `dd9c3335db7c3fe79947750842dd4208d4ce43ee0a83319cc0244f52f67da800` |
| `src-tauri/src/remote/server.rs` (main) | `2612ae1cfe5027082e9b8d796c12f771a338e6d41e69e105eefc960bbd8716fa` |
| `src-tauri/src/agent_transcript.rs` (main) | `54b776e009de8ff910117bd692b14eafbdd9c919215aeb912577f383b34596c8` |
| `ui/src/remote/agentConversation.ts` (W1) | `8e229b46a46f9e4cf3fb22187ccd2f3962775c0bc61fe72c4474e7ab56a84e50` |
| `src-tauri/src/remote/managed_chat_lifecycle.rs` (W2) | `046cb181032553cb0ab6a273b4618ba63dce7ca842cd54a90189f48f6379bd07` |
| `scripts/qa/herdr-mobile-acceptance.ps1` (W3) | `b0613774c9dc564d14c3d8cf131afa90826821249415a91daaff968034964d2c` |
| `ui/src/lib/scopedContracts.ts` (w1) | `71729a71480cc21876a2e0270e58bc70237303c25fed497c5435651bb3edc80a` |
| `src-tauri/src/scoped_contracts.rs` (w1) | `a08d87df7844c4799b5f5c6ea25dc263561f7d8cb1c135b8b5cba7f040f096e1` |

These hashes bind **selected decisive reads**, not every file of any tree, and no
executable binary. Task 16 binds the final candidate binary/UI artifacts.

---

## 8. Task 1 deliverables and remaining integration needs

Shipped by this task (all authored, **none executed** — the run is deferred by explicit
instruction):

| Path | Contents |
|---|---|
| `docs/chat/herdr-port-contract.md` | this document |
| `docs/chat/HERDR_LICENSE` | upstream MIT license, verbatim |
| `src-tauri/src/remote/reference_chat/types.rs` | frozen Rust DTOs, helpers, lane signatures, inline `#[cfg(test)]` tests |
| `src-tauri/src/remote/reference_chat/mod.rs` | module root; registers `types` only |
| `src-tauri/src/remote/mod.rs` | one line: `pub mod reference_chat;` |
| `ui/src/remote/chat/referenceTypes.ts` | frozen TS DTOs and helpers |
| `ui/src/remote/chat/referenceTypes.test.ts` | adjacent Vitest source |

Remaining integration needs, each owned by the task that owns the file:

1. `ScopeErrorCode::OperationOutcomeUnknown` / `"OPERATION_OUTCOME_UNKNOWN"` added to
   `src-tauri/src/scoped_contracts.rs` and `ui/src/lib/scopedContracts.ts` (see section 3).
   The rename attribute is mandatory; without it the acronym serializes wrong.
2. Every later lane's `pub mod <name>;` line added to
   `src-tauri/src/remote/reference_chat/mod.rs` by the integration owner, in the same change
   that adds the file.
3. Route registration (task 13) — task 1 declares the prefix and the route builder only.
4. Task 16 runs the deferred commands: `cargo test --manifest-path src-tauri/Cargo.toml --lib
   reference_chat` and `bun run --cwd ui test -- src/remote/chat/referenceTypes.test.ts`.

## 9. Non-goals and prohibitions

* No new provider RPC framework, no resident sidecar, no provider-selected managed child.
* No Codex-first phase: all reference-supported providers integrate together.
* No silent fallback after a failed managed request; no transplant of W2's managed-only
  send into ordinary sessions.
* No blanket control-router mount; no hidden terminal that changes geometry to scrape
  prompts.
* No user desktop manipulation, no production daemon restart, no unrecorded-process
  cleanup.
* Dictation is a client probe of `SpeechRecognition` / `webkitSpeechRecognition`
  (`ui/src/remote/chat/referenceDictation.ts`). The mic is hidden when the constructor
  is missing or not constructible. It does not add a native audio permission, a native
  speech engine, or a hosted transcription service, and it never auto-sends.

---

## 10. Revision 3 — additive wire contract

Rev2 pages, routes, and clients stay valid. Rev3 fields are optional on the wire.
When they are absent, a rev3 encoder omits them, so a rev2 page is byte-identical to
the revision-2 document. A rev2 client does not send `after`, does not ask for a
payload or continuation, and keeps the existing three-second poll plus reconnect read.
A rev2 host does not advertise `referenceHistoryDeltaV1` or `terminalContinueV1` and
does not emit `referenceHistoryChanged`, a delta, or a composer document. Correctness
never depends on the advisory frame.

The shapes below are the wire contract. §11 records which of them the current host
fills. A rev2 read (no `after`) still returns a bare `ReferenceHistoryPage`, not a
`{ mode }` envelope.

### 10.1 Identity and metadata

Optional on the wire. A rev3 native-aligned reader fills them only from provider
records. Unaligned turns omit ids and must not be used as a delta anchor.

```ts
ReferenceTurn.id?: string
ReferenceTurn.meta?: {
  provider: ReferenceNativeHistoryKind
  model?: string
  usage?: {
    inputTokens?: number
    outputTokens?: number
    cacheReadTokens?: number
    cacheWriteTokens?: number
  }
}
ReferencePart.id?: string
ReferencePart.Tool.callId?: string
ReferencePart.Tool.status?: "running" | "completed" | "failed" | "cancelled" | "unverifiable"
```

Ids are generation-scoped. A generation reset invalidates every prior id. Tool results
fold into the part with the same native call id. Orphan results stay explicit. Turn
ids are attached only when parsed turns align with source segments.

A recorded native cancel, abort, or interrupt is `cancelled`. It is not `failed`.
`unverifiable` remains unfinished work after restart or reopen, never a synonym for
cancel. Page assembly keeps reader-provided turn `meta` and does not add `meta` to a
turn the reader left without it, so those rev2 bytes stay unchanged.

### 10.2 Advisory history-change frame

```ts
{
  type: "referenceHistoryChanged"
  target: ReferenceTargetRef   // flattened wire target
  changeSeq: string
}
```

The frame contains no path, stream content, model, or transcript payload. It means
only that the owning host observed a possibly relevant storage-metadata change.

### 10.3 Replacement delta

Capability `referenceHistoryDeltaV1`. The existing history route accepts this query
object as `after`. `cursor` together with `after` is `INVALID_REQUEST`
(`REFERENCE_HISTORY_CURSOR_AND_AFTER`). A foreign stream or provider rotation remains
`TARGET_EXPIRED`.

```ts
after: {
  streamId: string
  generation: string
  anchorTurnId: string
  previousTurnFingerprint: string | null
}
```

```ts
{ mode: "delta", source, generation, replaceFromTurnId, turns, nextAnchor, tailOpen }
{ mode: "reset", reason, page }
```

`reason` is one of `generationChanged`, `anchorOutsideWindow`, `anchorNotTurnStart`,
`anchorPrevMismatch`, `tooFarBehind`, `unalignedTurns`, `notNative`,
`unsupportedProjection`. A reset carries a complete newest page. `nextAnchor` has the
same shape as `after`.

### 10.4 Composer capabilities

Optional `composer` on the native history page. Absent hides every composer control.

```ts
ReferenceComposerCapabilities {
  models: {
    current: string | null
    choices: { id: string, label: string, efforts: string[] }[]
    apply: "command" | "agent-picker"
  } | null
  options: {
    id: string
    label: string
    values: string[]
    current: string | null
    apply: "command" | "agent-picker" | "start-only"
  }[]
  slashCommands: { name: string, description: string | null }[]
  followUp: { queue: boolean, steer: boolean }
}
```

### 10.5 Work visualization

`ReferenceTaskStatus` includes `running`, `cancelled`, and `unverifiable` on the wire.
No current reader emits `running` or `unverifiable`. OmO and OMP map a recorded
`cancelled` / `canceled` / `aborted` to `cancelled`, `completed` to `completed`, and
any other task word to `failed`. That last mapping is not an unverifiable rule; see
§11. `parentCallId` is set only when an OmO or OMP task record has a parent-call key.
`id` remains the task id.

```ts
ReferencePayloadRef { ref: string, byteLength: number, digest: string, truncated: boolean }
{ kind: "diff", id?: string, patch: ReferencePayloadRef }
```

`ref` is opaque. It is not a host path. Tool `outputRef` / `outputSize` stay the rev2
fields. Rev3 may also set `outputPayload` to a `ReferencePayloadRef`. Readers that see
a native patch body emit a `diff` part. They do not set `outputPayload`. The payload
route is not registered (§11).

### 10.6 Continuation startup

Capability `terminalContinueV1`. The client never sends argv.

```ts
startup: {
  kind: "agentContinue"
  agentType: string          // registry agent id
  providerSession: AgentProviderSession
  mode: "fork" | "resume"
}
```

`AgentProviderSession` is the existing daemon session object (`key`, `id`, optional
`transcriptPath`, optional `launcher`). On the machine create boundary only `key` and
`id` are accepted. The owning host builds argv and does not stop the source pane (§11).

---

## 11. Landed rev3 behavior

Read against the worktree sources named below, not against the adoption plan. An item
in §11.7 is a wire field, route, or behavior that is not implemented.

### 11.1 Negotiation

Rev2 remains valid. Omitting `after` returns the rev2 page object. Optional rev3 fields
serialize only when set, so an unfilled turn keeps its rev2 bytes.

A rev3 client may send `after` on the existing history route. The value is one JSON
object in the `after` query string (`streamId`, `generation`, `anchorTurnId`,
`previousTurnFingerprint`). It is not a set of repeated query keys. `cursor` together
with `after` is `INVALID_REQUEST` before the read. A foreign `after.streamId` is
`TARGET_EXPIRED`. Provider rotation on the pane is the route's existing
`TARGET_EXPIRED`.

The host re-reads the newest page of the same bounded window and runs
`reference_history_negotiate`. It does not parse an unchecked tail. A delta replaces
from the oldest open turn at or before the client's anchor, or from the anchor when
none of those turns is open, and resends that turn whole plus every later turn.
`tailOpen` is whether the newest turn is open. `nextAnchor` is the oldest open turn,
or the newest turn when none is open.

Reset check order, then `tooFarBehind` if the replacement exceeds 1000 turns or 4 MiB:

| Reason | When |
|---|---|
| `notNative` | availability is not native |
| `unsupportedProjection` | pi, cursor, opencode, crush, cline |
| `unalignedTurns` | an otherwise alignable read has a turn with no id |
| `generationChanged` | `after.generation` is not the page generation |
| `anchorOutsideWindow` | anchor id is absent and the page has `hasMore`, an older cursor, or `unavailableReason` |
| `anchorNotTurnStart` | anchor id is absent on a complete newest page |
| `anchorPrevMismatch` | `previousTurnFingerprint` does not match the turn before the anchor (`null` only when the anchor is first) |
| `tooFarBehind` | the replacement is over the turn or byte cap |

A reset includes the complete newest page. No partial timeline is returned.

Capability `referenceHistoryDeltaV1` is on the daemon capability list and on the
machine gateway document when the terminal is ready. A Mirror Control device sees
it too: that device can already read history, and the desktop pairing code the
phone exchanges is Mirror. It does not see machine powers. A View device on either scope is told `referenceHistoryDeltaV1` and `referencePayloadV1` when the terminal is ready, and is not told composer control. The relay forwards history GETs.
It does not rewrite frames. A missed frame changes nothing: the client still polls and
re-reads.

### 11.2 Identity, generation, open set

Generation for a native window read is `{streamId}:inc-{n}`. `n` comes from a
process-wide counter (`next_reference_history_incarnation`, starts at 1). A new
tracker, a first observation, a provider-session change, a prefix rewrite, and a
truncation each take the next value. Eviction of the least-recently observed stream
(cap 1024) also takes a new value, so `inc-1` is not reused. An append, including
growth of the newest open turn and a window whose head slid forward past the read cap,
keeps the token. Ids are generation-scoped; a new incarnation invalidates them.

`attach_reference_history_identity` attaches ids only when the user-turn count equals
the number of turn-opening records. Otherwise it clears turn and part ids and delta is
unavailable for that read. `callId` is left as the parser recorded it.

| Provider | Turn id | Part id |
|---|---|---|
| claude | opening user record `uuid`, else `{generation}:{offset}:{local}` | native `callId` if stamped, else `{generation}:{offset}:{local}:{partIndex}` |
| codex | opening user record `payload.turn_id`, else derived as above | same |
| copilot, omo, omp, gjc | opening user record `id`, else derived | same |
| antigravity, kimi, grok | derived only; no native opening id is read | same |
| pi, cursor, opencode, crush, cline | omitted | omitted |
| unavailable / scrollback | omitted | omitted |

Assistant turns after the opening user record are derived even when the provider has a
native opening id. The assembler does not overwrite a part id the parser already set,
and it does not invent `meta`.

A turn is open when it has an unanswered call (`running`, or a `callId` with empty
output that is not an error and not `completed` / `failed` / `cancelled` /
`unverifiable`), or commentary that never became a final answer, or — only if it is
the newest turn — assistant text or thinking that may still grow. A submitted user turn is
never open. The open set is every such turn, not only the tail.

### 11.3 Advisory frame

`referenceHistoryChanged` is `{ type, target, changeSeq }`. `changeSeq` is a decimal
counter on that host's watch registry. The frame has no path, stream content, model,
or transcript. The watch stats the resolved transcript, or a `.db` file plus its
`-wal` sibling, for length, mtime, and inode. Content is not read. Stats go through
the blocking boundary.

| Bound | Value |
|---|---|
| registrations | 64; a new read evicts the soonest expiry |
| TTL | 60s, refreshed by a history read on this host |
| coalesce | 1s per target; a change inside the window stays pending |
| poll | 1s |

A paired owner emits its own frame. This gateway does not watch a foreign host or a
paired session, and it drops a watch when the stream is scrollback or not started.
Relay forwarding stays opaque.

### 11.4 Composer

`ReferenceHistoryPage.composer` is set only after a successful probe. A miss, timeout,
oversize, parse failure, or custom launch omits the document. The history read still
succeeds. Other providers are not probed.

Probes are fixed argv on the owning host, 15s, stdout cap 256 KiB. The client does
not supply a program or a template. A launch program is probed only when its basename
is the provider CLI and that file can be stat'd. A wrapper, a relative custom command,
or a CLI that cannot be resolved disables discovery. Nothing is guessed.

| Provider | Probe | Apply |
|---|---|---|
| claude | `claude -p` stream-json `control_request` / `list_models` | command: host types `/model {valueId}` or `/effort {valueId}` |
| codex | `codex` app-server `model/list` | `agent-picker`: host types `/model` and answers the native menu. Never `/model {valueId}`. |
| grok | `grok models`; header `Available models:` is required | command, same `/model` and `/effort` lines |

Effort choices are the discovered levels of the model the probe marked current. The
published effort `current` is always null; the host does not read the pane's selected
effort. Model `current` is only the probe's own marker. Cache key is owning host,
provider, pane place, and the verified CLI path, mtime, and size. Place is the pane
cwd, or the host home when cwd is unknown. TTL is 5 minutes. A different CLI file
does not reuse the catalog.

`POST /api/v1/reference-chat/{sessionId}/control` body is flat:
`{ requestId, target, registryId, controlId, valueId }`, plus optional
`providerSessionId`. Control ids are `model` and `effort`. The value must be one token
in the cached document. Applies share the per-pane input queue.

| Outcome | Wire | Retry |
|---|---|---|
| confirmed screen delta | `{ outcome: "applied" }` | no |
| provider rejection text, unknown id, ledger full, or a picker row that is not offered | `{ outcome: "rejected", code, message }` | no; a new user action is a new request id |
| write, screen, or an unobserved Codex picker | `{ outcome: "unknown", code: "OPERATION_OUTCOME_UNKNOWN" }` | no; the same request id replays and does not type again |
| another apply in flight | `CONTROL_CONFLICT` | not ledgered |

`followUp.queue` is true for Claude, Codex, and Grok, whose idle submit route already
types into the pane. `followUp.steer` is false: that submit, posted while a turn is
running, is not a documented mid-turn steer. `slashCommands` is filled only from a
Claude stream-json init `slash_commands` record the probe saw. Codex and Grok stay
empty. There is no hardcoded catalog.

`referenceComposerControlV1` is on that document for a Machine Control device and
for a Mirror Control device when the terminal is ready. A View device, Mirror or
Machine, is told `referenceHistoryDeltaV1` and `referencePayloadV1` when the
terminal is ready, and is not told composer control. `referenceComposerControlV1` is not on
the daemon `GetCapabilities` list. The relay allowlist
includes `POST .../control`.

Dictation is client-only. Partial and final text insert at the caret. A target change,
unmount, terminal mode, or `discard` drops a late result. OS keyboard dictation remains
the baseline.

### 11.5 Work visualization

Tool `status` is set only from a recorded word: `running` (also `in_progress`,
`pending`, `started`, `executing`), `completed`, `failed`, `cancelled` (also
`canceled`, `aborted`, `interrupted`). An unknown word is omitted. A result settles an
open call to `completed` or `failed` and does not overwrite `cancelled`.

Turn `meta.model` and `meta.usage` are attached only from provider fields the reader
found. Absent counts stay absent. Page assembly does not add `meta`.

A native patch body becomes a `diff` part. The ref is `payload:diff:{sha256}` with
`byteLength` and `truncated` (body longer than 8 KiB). The patch text is not inlined
in the part. A tool name alone is not a diff. An oversized patch tool input is replaced
with the fixed omission string, not a reconstructed git diff.

`parentCallId` is read from OmO and OMP task records (`parent_call_id`,
`parentCallId`, `parent_tool_use_id`, `parentToolUseId`, `parent_tool_call_id`,
`parentToolCallId`). Other readers do not set it.

### 11.6 Continuation

Startup `kind: "agentContinue"` carries `agentType`, `providerSession`, and
`mode: "fork" | "resume"`. Unknown keys, including any client `argv` or `args`, are
`INVALID_REQUEST` on machine create. The host builds argv in
`resolve_agent_continue_plan`. Extra keys on a local startup object are not copied
into that argv.

| Agent | Fork | Resume |
|---|---|---|
| claude | `claude --resume ID --fork-session` | `claude --resume ID` |
| codex | `codex fork ID` | `codex resume ID` |
| any other verified resume builder | not offered; `AGENT_CONTINUE_UNSUPPORTED` | that builder's existing argv (`grok --resume ID`, and the other resume plans in `shell.rs`) |

Resume is refused while any pane owning that provider session is running. Fork is
refused while a pane owning it is running and working. An exited source is admitted
for both. Neither check stops or signals the source. The new session is a different
pane. The source pane, its draft, and its held rows are not rebound.

| Code | HTTP | When |
|---|---|---|
| `AGENT_CONTINUE_UNSUPPORTED` | 422 | capability absent, fork of a non-claude/codex agent, machine agent not in the resume admission list (copilot is in that list but has no cwd proof, so it is not admitted), machine `providerSession.transcriptPath` present, or terminal not ready |
| `AGENT_CONTINUE_CONFLICT` | 409 | live claim for resume, or working source for fork |

`terminalContinueV1` is on the daemon capability list and on the machine gateway
document for a Machine-scope Control device when the terminal is ready. A Mirror
device is not told it and must not send `agentContinue`: creating a pane stays
Machine scope. An unknown create outcome is not retried automatically.

### 11.7 Pending

* **Payload route.** No `GET` under `/api/v1/reference-chat/...` serves a payload ref.
  The relay allowlist has no payload suffix. Diff refs are minted and cannot be
  fetched. Tool `outputPayload` is never set; `outputRef` remains the rev2 field and
  is not this route.
* **Task `running` and `unverifiable`.** Both are legal wire values. No reader emits
  them. OmO and OMP still map an unrecognized task word to `failed`.
* **Follow-up steer.** `followUp.steer` stays false. No provider path here delivers
  typed input as a mid-turn steer. Queue is advertised for Claude, Codex, and Grok.
* **Slash catalog.** Only Claude init `slash_commands` seen by the probe. Other
  providers stay empty.
