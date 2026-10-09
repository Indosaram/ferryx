import type {
  AttachmentReceipt,
  ChatDraft,
  DeliveryReceipt,
  DeliveryStage,
  TargetRef,
} from "../../lib/scopedContracts";
import type { Callback, ChatService, Question } from "../../features/ferryx/chat/ManagedChat";
import { safeRandomUUID } from "../../lib/uuid";
import { remoteApiUrl } from "../remoteClient";
import {
  stageRemoteAttachment,
  type RemoteAttachmentAdapterOptions,
} from "./remoteAttachmentAdapter";

export interface RemoteManagedChatServiceOptions {
  readonly baseUrl: string;
  readonly token: () => string | null;
  readonly fetchFn?: typeof fetch;
}

export interface ChatSendRequestPayload {
  readonly requestId: string;
  readonly target: TargetRef;
  readonly draft: ChatDraft;
}

export interface ChatReplyRequestPayload {
  readonly requestId: string;
  readonly target: TargetRef;
  readonly callbackId: string | number;
  readonly threadId: string;
  readonly turnId: string;
  readonly callbackIncarnation?: number;
  readonly kind: "approval" | "question";
  readonly result: unknown;
}

export interface ChatStopRequestPayload {
  readonly requestId: string;
  readonly target: TargetRef;
}

export interface ChatStartRequestPayload {
  readonly requestId: string;
  readonly target: TargetRef;
  readonly provider: "codex";
}

export interface ChatStartResult {
  readonly target: TargetRef;
  readonly provider: "codex" | string;
  readonly threadId: string;
}

export interface ResultPreviewTokenRequestPayload {
  readonly target: TargetRef;
  readonly fileId: string;
}

export interface ResultFileListEntry {
  readonly fileId: string;
  readonly displayName: string;
}

export interface ResultPreviewTokenResponsePayload {
  readonly ok: boolean;
  readonly token?: string;
  readonly expiresAt?: number;
  readonly error?: {
    readonly code: string;
    readonly message: string;
  };
}

export interface LiveCallbackQuestion {
  readonly id: string;
  readonly question: string;
  readonly required?: boolean;
  readonly isSecret?: boolean;
  readonly options?: { readonly label: string; readonly description: string }[] | null;
}

export type RemoteManagedQuestion = Question & { readonly required?: boolean };

export interface LiveCallbackSummaryDto {
  readonly callbackId: string;
  readonly threadId: string;
  readonly turnId: string;
  readonly callbackIncarnation?: number;
  readonly target?: TargetRef;
  readonly kind: "approval" | "question";
  readonly text?: string | null;
  readonly questions?: LiveCallbackQuestion[] | null;
}

export interface RemoteManagedCallback extends Callback {
  readonly callbackIncarnation?: number;
  readonly target?: TargetRef;
  readonly questions?: RemoteManagedQuestion[];
}

export interface ScopeErrorPayload {
  readonly code: string;
  readonly message: string;
  readonly retryable?: boolean;
  readonly details?: unknown;
}

export type ScopeResult<T> =
  | { readonly ok: true; readonly data: T; readonly requestId: string }
  | { readonly ok: false; readonly error: ScopeErrorPayload; readonly requestId: string };

export class ManagedChatRemoteError extends Error {
  readonly code: string;
  readonly status?: number;
  readonly retryable: boolean;

  constructor(message: string, code = "MANAGED_CHAT_ERROR", status?: number, retryable = false) {
    super(message);
    this.name = "ManagedChatRemoteError";
    this.code = code;
    this.status = status;
    this.retryable = retryable;
  }
}

export class StaleManagedChatCallbackError extends ManagedChatRemoteError {
  constructor(message: string, status?: number, retryable = false) {
    super(message, "STALE_CALLBACK", status, retryable);
    this.name = "StaleManagedChatCallbackError";
  }
}

export function validateTargetRef(target: TargetRef): TargetRef {
  if (!target || typeof target !== "object") {
    throw new ManagedChatRemoteError("TargetRef must be a non-null object", "INVALID_TARGET");
  }
  const { hostId, ownerId, epoch, backendSessionId } = target;
  if (typeof hostId !== "string" || hostId.trim().length === 0) {
    throw new ManagedChatRemoteError("TargetRef.hostId must be a non-empty string", "INVALID_TARGET");
  }
  if (typeof ownerId !== "string" || ownerId.trim().length === 0) {
    throw new ManagedChatRemoteError("TargetRef.ownerId must be a non-empty string", "INVALID_TARGET");
  }
  if (typeof epoch !== "string" || !/^(0|[1-9][0-9]*)$/.test(epoch)) {
    throw new ManagedChatRemoteError("TargetRef.epoch must be a canonical decimal u64 string", "INVALID_TARGET");
  }
  if (typeof backendSessionId !== "string" || backendSessionId.trim().length === 0) {
    throw new ManagedChatRemoteError("TargetRef.backendSessionId must be a non-empty string", "INVALID_TARGET");
  }

  return Object.freeze({
    hostId: hostId.trim(),
    ownerId: ownerId.trim(),
    epoch: epoch.trim(),
    backendSessionId: backendSessionId.trim(),
  });
}

export class RemoteManagedChatService implements ChatService {
  private readonly baseUrl: string;
  private readonly getToken: () => string | null;
  private readonly fetchImpl: typeof fetch;

  constructor(options: RemoteManagedChatServiceOptions) {
    this.baseUrl = options.baseUrl;
    this.getToken = options.token;
    this.fetchImpl = options.fetchFn ?? fetch.bind(globalThis);
  }

  private authHeaders(): HeadersInit {
    const token = this.getToken();
    if (!token) {
      throw new ManagedChatRemoteError("Authentication token is required for managed chat service", "UNAUTHORIZED", 401);
    }
    return {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };
  }

  async send(target: TargetRef, draft: ChatDraft, requestId: string): Promise<DeliveryReceipt> {
    const frozenTarget = validateTargetRef(target);
    const boundRequestId = requestId && requestId.trim().length > 0 ? requestId.trim() : safeRandomUUID();

    if (!draft || typeof draft !== "object") {
      throw new ManagedChatRemoteError("ChatDraft must be a non-null object", "INVALID_DRAFT");
    }

    const payload: ChatSendRequestPayload = {
      requestId: boundRequestId,
      target: frozenTarget,
      draft: {
        text: draft.text ?? "",
        attachments: Array.isArray(draft.attachments) ? draft.attachments.map(a => Object.freeze({ ...a })) : [],
      },
    };

    const url = remoteApiUrl(this.baseUrl, "/api/v1/chat/send");
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: this.authHeaders(),
        body: JSON.stringify(payload),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to deliver message via managed chat service: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ManagedChatRemoteError(
        `Chat send response was not valid JSON (HTTP ${response.status})`,
        "INVALID_RESPONSE",
        response.status
      );
    }

    return this.parseAndValidateDeliveryReceipt(body, boundRequestId, frozenTarget, response.status);
  }

  private parseAndValidateDeliveryReceipt(
    body: unknown,
    expectedRequestId: string,
    expectedTarget: TargetRef,
    httpStatus: number
  ): DeliveryReceipt {
    if (!body || typeof body !== "object") {
      throw new ManagedChatRemoteError("Invalid receipt response envelope", "INVALID_RECEIPT", httpStatus);
    }

    const record = body as Record<string, unknown>;

    if (record.ok === false) {
      const err = record.error as ScopeErrorPayload | undefined;
      const code = typeof err?.code === "string" ? err.code : "CHAT_SEND_REJECTED";
      const message = typeof err?.message === "string" ? err.message : "Chat delivery rejected by provider";
      const retryable = Boolean(err?.retryable);
      throw new ManagedChatRemoteError(message, code, httpStatus, retryable);
    }

    if (record.ok !== true || !record.data || typeof record.data !== "object") {
      throw new ManagedChatRemoteError("Canonical ScopeResult envelope expected with { ok: true, data }", "INVALID_RECEIPT", httpStatus);
    }

    const receiptData = record.data as Record<string, unknown>;

    const receiptRequestId = receiptData.requestId;
    if (typeof receiptRequestId !== "string" || receiptRequestId !== expectedRequestId) {
      throw new ManagedChatRemoteError(
        `Receipt requestId mismatch: expected "${expectedRequestId}", got "${String(receiptRequestId)}"`,
        "RECEIPT_REQUEST_MISMATCH",
        httpStatus
      );
    }

    const receiptTarget = receiptData.target as Record<string, unknown> | undefined;
    if (!receiptTarget || typeof receiptTarget !== "object") {
      throw new ManagedChatRemoteError("Receipt target is missing or invalid", "RECEIPT_TARGET_MISMATCH", httpStatus);
    }

    if (
      receiptTarget.hostId !== expectedTarget.hostId ||
      receiptTarget.ownerId !== expectedTarget.ownerId ||
      String(receiptTarget.epoch) !== expectedTarget.epoch ||
      receiptTarget.backendSessionId !== expectedTarget.backendSessionId
    ) {
      throw new ManagedChatRemoteError(
        `Receipt target does not match request target: expected (${expectedTarget.hostId}:${expectedTarget.epoch}:${expectedTarget.backendSessionId}), got (${String(receiptTarget.hostId)}:${String(receiptTarget.epoch)}:${String(receiptTarget.backendSessionId)})`,
        "RECEIPT_TARGET_MISMATCH",
        httpStatus
      );
    }

    const stage = receiptData.stage as DeliveryStage;
    if (stage !== "staged" && stage !== "accepted" && stage !== "providerRead") {
      throw new ManagedChatRemoteError(`Invalid delivery stage "${String(stage)}"`, "INVALID_STAGE", httpStatus);
    }

    return Object.freeze({
      requestId: expectedRequestId,
      target: expectedTarget,
      stage,
    });
  }

  async stage(target: TargetRef, file: File, signal: AbortSignal, attachmentId?: string): Promise<AttachmentReceipt> {
    const frozenTarget = validateTargetRef(target);
    const token = this.getToken();
    if (!token) {
      throw new ManagedChatRemoteError("Authentication token is required to stage attachments", "UNAUTHORIZED", 401);
    }

    const adapterOptions: RemoteAttachmentAdapterOptions = {
      baseUrl: this.baseUrl,
      token,
      fetchFn: this.fetchImpl,
    };

    return stageRemoteAttachment(adapterOptions, frozenTarget, file, signal, attachmentId);
  }

  async reply(target: TargetRef, callback: Callback, result: unknown): Promise<void> {
    const frozenTarget = validateTargetRef(target);
    if (!callback || typeof callback !== "object") {
      throw new ManagedChatRemoteError("Callback must be a non-null object", "INVALID_CALLBACK");
    }
    const managedCallback = callback as RemoteManagedCallback;
    if (managedCallback.target) {
      const callbackTarget = validateTargetRef(managedCallback.target);
      if (
        callbackTarget.hostId !== frozenTarget.hostId ||
        callbackTarget.ownerId !== frozenTarget.ownerId ||
        callbackTarget.epoch !== frozenTarget.epoch ||
        callbackTarget.backendSessionId !== frozenTarget.backendSessionId
      ) {
        throw new ManagedChatRemoteError("Callback target does not match reply target", "CALLBACK_TARGET_MISMATCH");
      }
    }
    if (!Number.isSafeInteger(managedCallback.callbackIncarnation) || (managedCallback.callbackIncarnation ?? 0) < 1) {
      throw new ManagedChatRemoteError("Callback incarnation is missing or invalid", "INVALID_CALLBACK_INCARNATION");
    }

    const boundRequestId = safeRandomUUID();
    const payload: ChatReplyRequestPayload = {
      requestId: boundRequestId,
      target: frozenTarget,
      callbackId: callback.id,
      threadId: callback.threadId,
      turnId: callback.turnId,
      callbackIncarnation: managedCallback.callbackIncarnation,
      kind: callback.kind,
      result,
    };

    const url = remoteApiUrl(this.baseUrl, "/api/v1/chat/reply");
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: this.authHeaders(),
        body: JSON.stringify(payload),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to submit callback reply: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ManagedChatRemoteError(
        `Callback reply response was not valid JSON (HTTP ${response.status})`,
        "INVALID_RESPONSE",
        response.status
      );
    }

    if (!body || typeof body !== "object") {
      throw new ManagedChatRemoteError("Invalid callback reply response envelope", "INVALID_RESPONSE", response.status);
    }

    const record = body as ScopeResult<{ readonly resolved: boolean }>;
    if (record.ok === false) {
      const err = record.error;
      if (
        err.code === "STALE_CALLBACK" ||
        (err.code === "TARGET_EXPIRED" && err.message.includes("incarnation is stale"))
      ) {
        throw new StaleManagedChatCallbackError(err.message, response.status, Boolean(err.retryable));
      }
      throw new ManagedChatRemoteError(err.message, err.code, response.status, Boolean(err.retryable));
    }
  }

  async stop(target: TargetRef): Promise<void> {
    const frozenTarget = validateTargetRef(target);
    const boundRequestId = safeRandomUUID();
    const payload: ChatStopRequestPayload = {
      requestId: boundRequestId,
      target: frozenTarget,
    };

    const url = remoteApiUrl(this.baseUrl, "/api/v1/chat/stop");
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: this.authHeaders(),
        body: JSON.stringify(payload),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to stop agent execution: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ManagedChatRemoteError(
        `Agent stop response was not valid JSON (HTTP ${response.status})`,
        "INVALID_RESPONSE",
        response.status
      );
    }

    if (!body || typeof body !== "object") {
      throw new ManagedChatRemoteError("Invalid agent stop response envelope", "INVALID_RESPONSE", response.status);
    }

    const record = body as ScopeResult<{ readonly stopped: boolean }>;
    if (record.ok === false) {
      const err = record.error;
      throw new ManagedChatRemoteError(err.message, err.code, response.status, Boolean(err.retryable));
    }
  }

  async start(
    target: TargetRef,
    provider: "codex" = "codex",
    requestId?: string
  ): Promise<ChatStartResult> {
    const frozenTarget = validateTargetRef(target);
    const boundRequestId = requestId ?? safeRandomUUID();
    const payload: ChatStartRequestPayload = {
      requestId: boundRequestId,
      target: frozenTarget,
      provider,
    };

    const url = remoteApiUrl(this.baseUrl, "/api/v1/chat/start");
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: this.authHeaders(),
        body: JSON.stringify(payload),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to start managed agent: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ManagedChatRemoteError(
        `Agent start response was not valid JSON (HTTP ${response.status})`,
        "INVALID_RESPONSE",
        response.status
      );
    }

    if (!body || typeof body !== "object") {
      throw new ManagedChatRemoteError("Invalid agent start response envelope", "INVALID_RESPONSE", response.status);
    }

    const record = body as ScopeResult<ChatStartResult>;
    if (record.ok === false) {
      const err = record.error;
      throw new ManagedChatRemoteError(err.message, err.code, response.status, Boolean(err.retryable));
    }

    if (record.ok !== true || !record.data || typeof record.data !== "object") {
      throw new ManagedChatRemoteError("Canonical ScopeResult envelope expected with { ok: true, data }", "INVALID_RESPONSE", response.status);
    }

    const startData = record.data as Partial<ChatStartResult>;
    if (!startData.threadId || typeof startData.threadId !== "string") {
      throw new ManagedChatRemoteError("Provider did not return a valid managed threadId", "INVALID_THREAD_ID", response.status);
    }

    return Object.freeze({
      target: frozenTarget,
      provider: startData.provider || "codex",
      threadId: startData.threadId,
    });
  }

  async fetchCallbacks(backendSessionId: string, threadId?: string): Promise<RemoteManagedCallback[]> {
    if (!backendSessionId || backendSessionId.trim().length === 0) {
      return [];
    }

    const params = new URLSearchParams();
    params.set("backendSessionId", backendSessionId.trim());
    if (threadId && threadId.trim().length > 0) {
      params.set("threadId", threadId.trim());
    }

    const url = remoteApiUrl(this.baseUrl, `/api/v1/chat/callbacks?${params.toString()}`);
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "GET",
        headers: this.authHeaders(),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to query active callbacks: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    if (!response.ok) {
      let code = `HTTP_${response.status}`;
      let message = `Failed to query callbacks (HTTP ${response.status})`;
      try {
        const errJson: unknown = await response.json();
        if (errJson && typeof errJson === "object") {
          const rec = errJson as Record<string, unknown>;
          if (rec.ok === false && rec.error && typeof rec.error === "object") {
            const inner = rec.error as ScopeErrorPayload;
            code = inner.code;
            message = inner.message;
          }
        }
      } catch {}
      throw new ManagedChatRemoteError(message, code, response.status);
    }

    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new ManagedChatRemoteError("Callbacks response was not valid JSON", "INVALID_RESPONSE", response.status);
    }

    const record = body as ScopeResult<LiveCallbackSummaryDto[]>;
    if (record.ok === false) {
      const err = record.error;
      throw new ManagedChatRemoteError(err.message, err.code, response.status, Boolean(err.retryable));
    }

    if (!Array.isArray(record.data)) {
      return [];
    }

    return record.data.map(parseLiveCallback);
  }

  async fetchResultFiles(target: TargetRef): Promise<ResultFileListEntry[]> {
    const frozenTarget = validateTargetRef(target);
    const response = await this.fetchImpl(remoteApiUrl(this.baseUrl, "/api/v1/files/results/list"), {
      method: "POST",
      headers: this.authHeaders(),
      body: JSON.stringify({ target: frozenTarget }),
    });
    if (!response.ok) {
      throw new ManagedChatRemoteError(`Result file listing failed with HTTP ${response.status}`, `HTTP_${response.status}`, response.status);
    }
    const body: unknown = await response.json();
    const record = body as { ok?: unknown; files?: unknown };
    if (record?.ok !== true || !Array.isArray(record.files)) {
      throw new ManagedChatRemoteError("Invalid result file listing response", "INVALID_RESPONSE", response.status);
    }
    return record.files.flatMap((item): ResultFileListEntry[] => {
      if (!item || typeof item !== "object") return [];
      const entry = item as Record<string, unknown>;
      return typeof entry.fileId === "string" && typeof entry.displayName === "string"
        ? [{ fileId: entry.fileId, displayName: entry.displayName }]
        : [];
    });
  }

  async openResultPreview(target: TargetRef, fileId: string): Promise<string> {
    const frozenTarget = validateTargetRef(target);
    if (!fileId || typeof fileId !== "string") {
      throw new ManagedChatRemoteError("Result file identifier is required", "INVALID_FILE_ID");
    }

    const payload: ResultPreviewTokenRequestPayload = {
      target: frozenTarget,
      fileId,
    };

    const url = remoteApiUrl(this.baseUrl, "/api/v1/files/preview/token");
    let response: Response;
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: this.authHeaders(),
        body: JSON.stringify(payload),
      });
    } catch (netErr) {
      throw new ManagedChatRemoteError(
        `Failed to request result file preview: ${netErr instanceof Error ? netErr.message : String(netErr)}`,
        "NETWORK_ERROR"
      );
    }

    if (!response.ok) {
      let code = `HTTP_${response.status}`;
      let message = `Preview token request failed with HTTP ${response.status}`;
      try {
        const errJson: unknown = await response.json();
        if (errJson && typeof errJson === "object") {
          const errObj = errJson as Record<string, unknown>;
          if (errObj.error && typeof errObj.error === "object") {
            const inner = errObj.error as Record<string, unknown>;
            if (typeof inner.code === "string") code = inner.code;
            if (typeof inner.message === "string") message = inner.message;
          } else if (typeof errObj.message === "string") {
            message = errObj.message;
          }
        }
      } catch {}
      throw new ManagedChatRemoteError(message, code, response.status);
    }

    const data: ResultPreviewTokenResponsePayload = await response.json();
    if (!data || !data.ok || !data.token) {
      throw new ManagedChatRemoteError(
        data?.error?.message ?? "Invalid preview token response from gateway",
        data?.error?.code ?? "INVALID_PREVIEW_TOKEN"
      );
    }

    return remoteApiUrl(this.baseUrl, `/api/v1/files/preview/${encodeURIComponent(data.token)}`);
  }
}

export function parseLiveCallback(summary: LiveCallbackSummaryDto): RemoteManagedCallback {
  return {
    id: summary.callbackId,
    threadId: summary.threadId,
    turnId: summary.turnId,
    callbackIncarnation: summary.callbackIncarnation,
    target: summary.target ? validateTargetRef(summary.target) : undefined,
    kind: summary.kind,
    text: summary.text ?? "",
    questions: Array.isArray(summary.questions)
      ? summary.questions.map((q): RemoteManagedQuestion => ({
          id: q.id,
          question: q.question,
          required: q.required,
          isSecret: q.isSecret,
          options: q.options,
        }))
      : undefined,
  };
}

export function createRemoteManagedChatService(
  options: RemoteManagedChatServiceOptions
): RemoteManagedChatService {
  return new RemoteManagedChatService(options);
}
