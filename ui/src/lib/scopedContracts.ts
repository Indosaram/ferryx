/** Scoped contracts only: transport adapters must parse untrusted JSON. */
export type Epoch = string;
export interface TargetRef {
  readonly hostId: string;
  readonly ownerId: string;
  /** Canonical decimal u64, never a JavaScript number. */
  readonly epoch: Epoch;
  readonly backendSessionId: string;
}

export type RunTarget = { readonly kind: "local" } | { readonly kind: "ssh"; readonly hostId: string };
export type ScopeCapability = "scopeControlV1" | "sshHelperV1" | "managedCodexV1" | "captureV1";
export type InventoryCompleteness = "complete" | "partial" | "unknown";
export interface InventorySnapshot<T> {
  readonly revision: number;
  readonly items: readonly T[];
  readonly completeness: InventoryCompleteness;
  readonly unavailableHosts: readonly string[];
}
export type CanonicalProvider = "codex" | "claude";
export type TransitionKind = "waiting" | "working" | "idle" | "taskComplete" | "stopped" | "removed";
export type TransitionSource =
  | { readonly kind: "provider"; readonly provider: CanonicalProvider; readonly requestId: string | null }
  | { readonly kind: "terminalDetection"; readonly detector: string }
  | { readonly kind: "lifecycle" };
export interface InventoryTransition {
  readonly revision: number;
  readonly target: TargetRef;
  readonly kind: TransitionKind;
  readonly source: TransitionSource;
}
export interface ConversationClaimKey {
  readonly hostId: string;
  readonly provider: CanonicalProvider;
  readonly conversationId: string;
}
export type ConversationOwner =
  | { readonly kind: "native"; readonly target: TargetRef }
  | { readonly kind: "managed"; readonly target: TargetRef }
  | { readonly kind: "provisional"; readonly requestId: string };
export interface ControlLease {
  readonly target: TargetRef;
  readonly deviceId: string;
  readonly leaseId: string;
  readonly expiresAtMs: number;
}
export interface MutationEnvelope<P> {
  readonly requestId: string;
  readonly target?: TargetRef;
  readonly params: P;
}
export type ScopeErrorCode = "INVALID_REQUEST" | "UNAUTHORIZED" | "FORBIDDEN" | "NOT_FOUND"
  | "TARGET_EXPIRED" | "CONTROL_CONFLICT" | "REQUEST_CONFLICT" | "PROVIDER_OWNED"
  | "UNSUPPORTED" | "TIMEOUT" | "INVENTORY_INCOMPLETE" | "PAYLOAD_TOO_LARGE" | "CAPTURE_UNSUPPORTED";
export type JsonValue = null | boolean | number | string | readonly JsonValue[] | { readonly [key: string]: JsonValue };
export interface ScopeError<D = JsonValue> {
  readonly code: ScopeErrorCode;
  readonly message: string;
  readonly retryable: boolean;
  readonly details: D;
}
export type ScopeResult<T, D = JsonValue> =
  | { readonly ok: true; readonly data: T; readonly requestId: string }
  | { readonly ok: false; readonly error: ScopeError<D>; readonly requestId: string };
export interface ScopeEvent<T> {
  readonly sequence: number;
  readonly target: TargetRef;
  readonly revision: number;
  readonly type: string;
  readonly data: T;
}
export type EventReplay<T, S> =
  | { readonly kind: "events"; readonly events: readonly ScopeEvent<T>[]; readonly afterSequence: number }
  | { readonly kind: "gap"; readonly snapshot: InventorySnapshot<S>; readonly afterSequence: number };
export const ATTACHMENT_MAX_FILE_BYTES = 10 * 1024 * 1024;
export const ATTACHMENT_MAX_FILES_PER_TURN = 4;
export const ATTACHMENT_MAX_TURN_BYTES = 20 * 1024 * 1024;
export const ATTACHMENT_UNREFERENCED_TTL_MS = 24 * 60 * 60 * 1000;
export type AttachmentMediaType = "image/png" | "image/jpeg" | "image/webp" | "text/plain" | "application/pdf";
export interface AttachmentReceipt {
  readonly hostId: string;
  readonly attachmentId: string;
  readonly sha256: string;
  readonly sizeBytes: number;
  readonly mediaType: AttachmentMediaType;
}
export interface ChatDraft {
  readonly text: string;
  readonly attachments: readonly AttachmentReceipt[];
}
export interface ConfirmedDraft {
  readonly target: TargetRef;
  readonly browserGeneration: Epoch;
  readonly attachments: readonly AttachmentReceipt[];
  readonly note: string;
}
export type DeliveryStage = "staged" | "accepted" | "providerRead";
export interface DeliveryReceipt {
  readonly requestId: string;
  readonly target: TargetRef;
  readonly stage: DeliveryStage;
}
