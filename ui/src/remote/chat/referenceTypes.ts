/**
 * Frozen Herdr reference-chat contract types (task 1).
 *
 * Ported from `devswha/herdr-web-ui` @ `54e5a1f67090cb09552d182e7e30dd0ecc314918`
 * (MIT, `docs/chat/HERDR_LICENSE`). The upstream shapes are `shared/protocol.ts`
 * (`ConversationTurn`, `ConversationPart`, `InteractivePrompt`, `PromptAnswer`),
 * `server/conversation.ts` (`RecognizedConversation`, `ConversationPage`, cursor
 * semantics) and `src/lib/compose.ts` (submit byte shaping).
 *
 * This module is types plus pure helpers: no fetch, no DOM, no storage.
 *
 * Frozen contract: `docs/chat/herdr-port-contract.md`. The Rust twin is
 * `src-tauri/src/remote/reference_chat/types.rs`; a wire field added here must be
 * added there in the same change.
 *
 * Identity, error, result and delivery-receipt types are **reused** from
 * `scopedContracts`, never duplicated here.
 */
import type {
  AttachmentMediaType,
  AttachmentReceipt,
  DeliveryStage,
  TargetRef,
} from "../../lib/scopedContracts";

/** Route prefix for the reference-chat surface. Task 13 registers the routes. */
export const REFERENCE_CHAT_ROUTE_PREFIX = "/api/v1/reference-chat";

/**
 * The reference's own copy for a pane whose turns are same-pane output rather than a
 * native transcript. Kept byte-exact: it is the disclosure the user reads.
 */
export const REFERENCE_SCROLLBACK_DISCLOSURE =
  "Conversation unavailable — show terminal output";

/** The reference's composer cap (`compose.ts`: `MAX_COMPOSER_CHARS`). */
export const REFERENCE_SUBMIT_MAX_CHARS = 20_000;

/** The error code meaning the mutation may have happened and cannot be confirmed. */
export const REFERENCE_OUTCOME_UNKNOWN_CODE = "OPERATION_OUTCOME_UNKNOWN";

/** A reference-chat target: the scoped `TargetRef` plus the provider session where known. */
export interface ReferenceTargetRef {
  readonly target: TargetRef;
  /**
   * The provider/native session identity where a reader identified one. Absent means
   * *unknown* — never permission to read another session's file.
   */
  readonly providerSessionId?: string | null;
}

/** Where a conversation page's turns came from. Kebab-case, matching the upstream readers. */
export type ReferenceHistorySource =
  | "claude-transcript"
  | "codex-transcript"
  | "omp-transcript"
  | "omo-transcript"
  | "gjc-transcript"
  | "pi-transcript"
  | "scrollback";

/** How honestly the page's turns are sourced. */
export type ReferenceHistoryAvailability = "native" | "scrollback" | "notStarted";

/** Which provider-native reader a lane implements. `unavailable` is a boundary, not a denial. */
export type ReferenceNativeHistoryKind =
  | "claude"
  | "codex"
  | "omp"
  | "omo"
  | "gjc"
  | "pi"
  | "unavailable";

export type ReferenceTurnRole = "user" | "assistant";

/** `runtime` marks a turn the agent's runtime put in the user's seat; nobody typed it. */
export type ReferenceTurnSource = "typed" | "runtime";

export type ReferencePartKind =
  | "text"
  | "thinking"
  | "skill"
  | "tool"
  | "image"
  | "compact"
  | "notice"
  | "taskResult";

export type ReferenceTextPhase = "commentary" | "finalAnswer";

export type ReferenceSkillEvidence = "invocation" | "instructions";
export type ReferenceSkillStatus = "requested" | "loaded" | "failed";

export interface ReferenceSkillActivity {
  readonly name: string;
  readonly evidence: ReferenceSkillEvidence;
  readonly status: ReferenceSkillStatus;
  readonly path?: string | null;
}

export type ReferenceTaskStatus = "completed" | "failed" | "cancelled";

export interface ReferenceTaskResult {
  readonly id: string;
  readonly title: string;
  readonly agent?: string | null;
  readonly model?: string | null;
  readonly status: ReferenceTaskStatus;
  readonly durationMs?: number | null;
  readonly turns?: number | null;
  readonly toolCalls?: number | null;
  readonly tokens?: number | null;
  readonly result: string;
  readonly resultCut?: boolean | null;
}

/** An opaque image reference, fetched through the owning host. */
export interface ReferenceImageRef {
  readonly mediaType: string;
  readonly ref: string;
}

/** One part of a turn: the upstream `ConversationPart` union, ported field-for-field. */
export type ReferencePart =
  | { readonly kind: "text"; readonly text: string; readonly phase?: ReferenceTextPhase | null }
  | { readonly kind: "thinking"; readonly text: string }
  /**
   * A skill the transcript recorded on its own, beside the text it was invoked with
   * (`transcript-records.ts:259`, `codex.ts:241,275`). Drawn by the turn's skill list, never
   * inside the inline part list (`ChatView.tsx:322`).
   */
  | { readonly kind: "skill"; readonly skill: ReferenceSkillActivity }
  | {
      readonly kind: "tool";
      readonly name: string;
      readonly summary: string;
      readonly input: string;
      readonly output: string;
      readonly error?: boolean | null;
      readonly skill?: ReferenceSkillActivity | null;
      /** Set when `output` was cut, so the whole output can be fetched on demand. */
      readonly outputRef?: string | null;
      readonly outputSize?: number | null;
      readonly images?: readonly ReferenceImageRef[];
    }
  | { readonly kind: "image"; readonly mediaType: string; readonly ref: string }
  | { readonly kind: "compact"; readonly text: string }
  | { readonly kind: "notice"; readonly text: string; readonly source?: string | null }
  | { readonly kind: "taskResult"; readonly tasks: readonly ReferenceTaskResult[] };

/** Turns a `/tree` walked away from; disclosed rather than dropped in silence. */
export interface ReferenceAbandonedBranch {
  readonly count: number;
  readonly branches: number;
  readonly summary?: string | null;
}

export interface ReferenceTurn {
  readonly role: ReferenceTurnRole;
  readonly startedAt?: string | null;
  /** Last recorded assistant activity — never the next user's timestamp. */
  readonly endedAt?: string | null;
  readonly source?: ReferenceTurnSource | null;
  readonly parts: readonly ReferencePart[];
  readonly abandoned?: ReferenceAbandonedBranch | null;
}

/** A position in a transcript stream: which stream, and where inside it. */
export interface ReferenceHistoryCursor {
  /** Identity of the transcript the cursor was minted against. */
  readonly streamId: string;
  /** Byte offset inside that stream. */
  readonly offset: number;
}

export interface ReferenceHistoryPage {
  readonly source: ReferenceHistorySource;
  readonly availability: ReferenceHistoryAvailability;
  readonly turns: readonly ReferenceTurn[];
  /** The page before this one; absent at the conversation's beginning or for scrollback. */
  readonly cursor?: ReferenceHistoryCursor | null;
  readonly hasMore: boolean;
  /** Changes whenever the answer could; a mismatch means the response is discarded. */
  readonly generation: string;
  /** Required when `availability` is not `native`. */
  readonly unavailableReason?: string | null;
}

/** A bounded, VT-aware snapshot of the original pane. Reading never emits input. */
export interface ReferenceScreenSnapshot {
  readonly revision: string;
  readonly text: string;
  readonly truncated: boolean;
  /** Replay gap: the reader could not reconstruct the history it was asked for. */
  readonly gap: boolean;
  readonly cols: number;
  readonly rows: number;
}

export type ReferenceSubmitOrigin = "chat" | "terminal";

export interface ReferenceSubmitPayload {
  readonly text: string;
  readonly attachmentIds: readonly string[];
  readonly origin: ReferenceSubmitOrigin;
}

/**
 * What the caller observed about the pane's ability to stop. `providerInterrupt` sends the
 * pane's own interrupt key; `shellSignal` is the explicit terminal's deliberate signal;
 * `refused` means the capability is unknown for this target.
 */
export type ReferenceStopCapability = "providerInterrupt" | "shellSignal" | "refused";

export interface ReferenceStopPayload {
  readonly capability: ReferenceStopCapability;
}

export type ReferencePromptKind = "question" | "approval" | "plan" | "menu";

export interface ReferencePromptOption {
  readonly label: string;
  readonly description?: string | null;
}

export interface ReferencePromptStep {
  readonly label: string;
  readonly answered: boolean;
  readonly current: boolean;
}

/** A question queued while the agent keeps working. */
export type ReferencePromptQueueState = "collapsed" | "open";

/** A prompt detected on the original pane's current screen. */
export interface ReferencePrompt {
  readonly id: string;
  readonly agent: string;
  readonly kind: ReferencePromptKind;
  readonly title: string;
  readonly question: string;
  readonly body?: string | null;
  readonly options: readonly ReferencePromptOption[];
  readonly multiSelect: boolean;
  /** Index of the "type your own answer" option, when the menu has one. */
  readonly customOptionIndex?: number | null;
  readonly queued?: ReferencePromptQueueState | null;
  readonly steps?: readonly ReferencePromptStep[];
  /** The last-resort card for a blocked pane no reader knows. */
  readonly fallback?: boolean | null;
}

/** The user's answer: exactly one of the three shapes. */
export interface ReferencePromptAnswer {
  readonly optionIndex?: number | null;
  readonly optionIndices?: readonly number[] | null;
  readonly customText?: string | null;
}

export interface ReferencePromptAnswerPayload {
  readonly promptId: string;
  /** The screen revision the card was rendered from. */
  readonly screenRevision: string;
  readonly answer: ReferencePromptAnswer;
}

/** One step of a key sequence that answers a prompt. */
export interface ReferenceKeyStep {
  readonly keys?: readonly string[];
  readonly text?: string | null;
}

/** `POST /files` payload: a bounded file staged on the owning host. */
export interface ReferenceFileStagePayload {
  readonly name: string;
  readonly mediaType: AttachmentMediaType;
  readonly sizeBytes: number;
  readonly contentBase64: string;
}

/** A staged file, as the chat refers to it. Reuses the scoped `AttachmentReceipt`. */
export interface ReferenceFileReceipt {
  readonly receipt: AttachmentReceipt;
  readonly displayName: string;
  readonly mentionText: string;
}

/** The stable identity of a target, for draft keys and request fencing. */
export function referenceTargetKey(target: ReferenceTargetRef): string {
  const { hostId, ownerId, epoch, backendSessionId } = target.target;
  return `${hostId}|${ownerId}|${epoch}|${backendSessionId}`;
}

/** Same pane, same daemon incarnation? A provider session difference does not move the pane. */
export function sameReferenceTarget(a: ReferenceTargetRef, b: ReferenceTargetRef): boolean {
  return referenceTargetKey(a) === referenceTargetKey(b);
}

/** A cursor only names a position while its stream is still the live stream. */
export function referenceCursorMatches(
  cursor: ReferenceHistoryCursor,
  streamId: string,
): boolean {
  return cursor.streamId === streamId;
}

export function referenceCursorKey(cursor: ReferenceHistoryCursor): string {
  return `${cursor.streamId}:${cursor.offset}`;
}

export function referenceHistoryIsNative(page: ReferenceHistoryPage): boolean {
  return page.availability === "native";
}

/**
 * The disclosure the UI must show, or `null` when nothing needs disclosing.
 *
 * A native page discloses nothing. Scrollback is same-pane output and a not-started session
 * is a conversation with no turns yet; both are labelled rather than passed off as native.
 */
export function referenceHistoryDisclosure(page: ReferenceHistoryPage): string | null {
  switch (page.availability) {
    case "native":
      return null;
    case "scrollback":
      return REFERENCE_SCROLLBACK_DISCLOSURE;
    case "notStarted":
      return "This session has not written a turn yet.";
  }
}

/**
 * May the delivery ladder reach `providerRead` for this page? Only a native page bound to an
 * identified provider session can be matched against a native observation.
 */
export function referenceCanReachProviderRead(
  page: ReferenceHistoryPage,
  target: ReferenceTargetRef,
): boolean {
  return referenceHistoryIsNative(page) && referenceTargetHasProviderSession(target);
}

export function referenceTargetHasProviderSession(target: ReferenceTargetRef): boolean {
  const id = target.providerSessionId;
  return typeof id === "string" && id.trim().length > 0;
}

/** Where a delivery stage sits on the ladder: staged < accepted < providerRead. */
export function referenceStageRank(stage: DeliveryStage): 0 | 1 | 2 {
  switch (stage) {
    case "staged":
      return 0;
    case "accepted":
      return 1;
    case "providerRead":
      return 2;
  }
}

/** Has the delivery reached at least `floor`? */
export function referenceStageAtLeast(stage: DeliveryStage, floor: DeliveryStage): boolean {
  return referenceStageRank(stage) >= referenceStageRank(floor);
}

/** A refusal is a typed answer, not a failure to try harder. */
export function referenceStopIsRefusal(capability: ReferenceStopCapability): boolean {
  return capability === "refused";
}

/** Does this wire error code mean the outcome is unknown? */
export function referenceIsOutcomeUnknown(code: string): boolean {
  return code === REFERENCE_OUTCOME_UNKNOWN_CODE;
}

/** How many of the three mutually exclusive answer shapes were set. */
export function referenceAnswerVariantCount(answer: ReferencePromptAnswer): number {
  let count = 0;
  if (answer.optionIndex != null) count += 1;
  if (answer.optionIndices != null) count += 1;
  if (answer.customText != null) count += 1;
  return count;
}

/** Exactly one answer shape? Zero is empty; two or more is ambiguous and must be refused. */
export function referenceAnswerIsSingleChoice(answer: ReferencePromptAnswer): boolean {
  return referenceAnswerVariantCount(answer) === 1;
}

/** The option indices a typed number may pick, in the order the agent shows them. */
export function referenceSelectableIndices(prompt: ReferencePrompt): readonly number[] {
  return prompt.options
    .map((_, index) => index)
    .filter((index) => index !== prompt.customOptionIndex);
}

/**
 * Does an answer to this prompt need an explicit confirm before it goes out? A typed message
 * picking an approval's, a plan's or a menu's option could act on a stray "yes" or "1".
 */
export function referencePromptNeedsConfirmation(
  prompt: ReferencePrompt,
  answer: ReferencePromptAnswer,
): boolean {
  const risky = prompt.kind === "approval" || prompt.kind === "plan" || prompt.kind === "menu";
  return risky && answer.optionIndex != null;
}

export function referencePartIsInlineText(part: ReferencePart): boolean {
  return part.kind === "text" || part.kind === "thinking" || part.kind === "compact";
}

/** Does this key step do anything at all? */
export function referenceKeyStepIsEffective(step: ReferenceKeyStep): boolean {
  const hasKeys = (step.keys?.length ?? 0) > 0;
  return hasKeys || (typeof step.text === "string" && step.text.length > 0);
}

/** The `@path ` mention for a staged file: plain text the user can still edit before sending. */
export function referenceMentionFor(path: string): string {
  return `@${path} `;
}

/** The durable draft key for a target, owner- and epoch-scoped. */
export function referenceDraftKey(target: ReferenceTargetRef): string {
  return referenceTargetKey(target);
}

/**
 * Map a registry id to its native reader.
 *
 * This is the only label→reader mapping the contract allows, keyed on the registry id the
 * inventory publishes — never on a terminal title. An OmO pane whose label flips between
 * `pi` and `claude` is routed by process/session evidence, so the caller confirms `omo`.
 */
export function referenceNativeKindFromRegistryId(registryId: string): ReferenceNativeHistoryKind {
  switch (registryId.trim().toLowerCase()) {
    case "claude":
      return "claude";
    case "codex":
      return "codex";
    case "omp":
      return "omp";
    case "omo":
      return "omo";
    case "gjc":
      return "gjc";
    case "pi":
      return "pi";
    default:
      return "unavailable";
  }
}

export function referenceNativeKindIsNative(kind: ReferenceNativeHistoryKind): boolean {
  return kind !== "unavailable";
}

/** The native reader a history source belongs to. */
export function referenceNativeKindOfSource(
  source: ReferenceHistorySource,
): ReferenceNativeHistoryKind {
  return source === "scrollback" ? "unavailable" : referenceNativeKindFromRegistryId(
    source.replace(/-transcript$/, ""),
  );
}

/** The reference-chat route for one session, e.g. `/api/v1/reference-chat/abc/history`. */
export function referenceChatRoute(sessionId: string, suffix = ""): string {
  const trimmed = suffix.replace(/^\/+|\/+$/g, "");
  return trimmed.length === 0
    ? `${REFERENCE_CHAT_ROUTE_PREFIX}/${sessionId}`
    : `${REFERENCE_CHAT_ROUTE_PREFIX}/${sessionId}/${trimmed}`;
}
