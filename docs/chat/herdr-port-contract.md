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
| Revision | **2** — task 27 restores the pinned standalone `skill` part (§4) and binds the two upstream parser files it reads (§7). Revision 1 is everything else. |

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
* Voice/microphone stays user-deferred and visibly unavailable; no fake voice pass.
