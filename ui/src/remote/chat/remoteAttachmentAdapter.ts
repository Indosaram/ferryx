import type {
  AttachmentMediaType,
  AttachmentReceipt,
  TargetRef,
} from "../../lib/scopedContracts";
import { ATTACHMENT_MAX_FILE_BYTES } from "../../lib/scopedContracts";
import { safeRandomUUID } from "../../lib/uuid";
import { remoteApiUrl } from "../remoteClient";

export interface AttachmentUploadChunkRequest {
  readonly target: TargetRef;
  readonly attachmentId: string;
  readonly fileName: string;
  readonly mediaType: AttachmentMediaType;
  readonly chunkIndex: number;
  readonly totalChunks: number;
  readonly offset: number;
  readonly totalBytes: number;
  readonly data: string;
}

export interface AttachmentUploadChunkResponse {
  readonly ok: boolean;
  readonly data?: AttachmentReceipt;
  readonly chunkIndex?: number;
  readonly error?: {
    readonly code: string;
    readonly message: string;
  };
  readonly requestId?: string;
  readonly path?: unknown;
  readonly remotePath?: unknown;
  readonly localPath?: unknown;
  readonly filePath?: unknown;
}

export interface AttachmentCancelRequest {
  readonly target: TargetRef;
  readonly attachmentId: string;
}

export interface AttachmentCancelResponse {
  readonly ok: boolean;
  readonly cleaned?: boolean;
  readonly error?: {
    readonly code: string;
    readonly message: string;
  };
}

export type AttachmentErrorKind =
  | "VALIDATION"
  | "UPLOAD_FAILED"
  | "RECEIPT_MISMATCH"
  | "PATH_LEAK_DETECTED"
  | "CANCELLED";

export class AttachmentValidationError extends Error {
  readonly kind = "VALIDATION" as const;
  constructor(message: string) {
    super(message);
    this.name = "AttachmentValidationError";
  }
}

export class AttachmentUploadError extends Error {
  readonly kind = "UPLOAD_FAILED" as const;
  readonly status?: number;
  readonly code?: string;
  constructor(message: string, status?: number, code?: string) {
    super(message);
    this.name = "AttachmentUploadError";
    this.status = status;
    this.code = code;
  }
}

export class AttachmentReceiptMismatchError extends Error {
  readonly kind = "RECEIPT_MISMATCH" as const;
  constructor(message: string) {
    super(message);
    this.name = "AttachmentReceiptMismatchError";
  }
}

export class AttachmentCancellationError extends Error {
  readonly kind = "CANCELLED" as const;
  readonly serverStateCleaned: boolean;
  constructor(message: string, serverStateCleaned: boolean) {
    super(message);
    this.name = "AttachmentCancellationError";
    this.serverStateCleaned = serverStateCleaned;
  }
}

export const ALLOWED_ATTACHMENT_MEDIA_TYPES: readonly AttachmentMediaType[] = [
  "image/png",
  "image/jpeg",
  "image/webp",
  "text/plain",
  "application/pdf",
] as const;

export const DEFAULT_ATTACHMENT_CHUNK_BYTES = 32 * 1024;
export const DEFAULT_ATTACHMENT_CANCEL_TIMEOUT_MS = 5000;

export function validateAttachmentFileName(rawName: string): string {
  const trimmed = rawName.trim();
  if (trimmed.length === 0) {
    throw new AttachmentValidationError("Attachment file name must not be empty");
  }

  const encodedBytes = new TextEncoder().encode(trimmed);
  if (encodedBytes.length > 255) {
    throw new AttachmentValidationError(
      `Attachment file name exceeds 255 bytes (got ${encodedBytes.length} bytes)`
    );
  }

  if (trimmed.includes("/") || trimmed.includes("\\")) {
    throw new AttachmentValidationError("Attachment file name must not contain path separators");
  }

  if (trimmed === "." || trimmed === "..") {
    throw new AttachmentValidationError("Attachment file name must not be a relative directory component");
  }

  for (let i = 0; i < trimmed.length; i++) {
    const code = trimmed.charCodeAt(i);
    if (code < 32 || (code >= 127 && code <= 159)) {
      throw new AttachmentValidationError("Attachment file name contains invalid control characters");
    }
  }

  return trimmed;
}

export function validateAttachmentMediaType(type: string, fileName: string): AttachmentMediaType {
  const normalized = type.toLowerCase().trim();
  if ((ALLOWED_ATTACHMENT_MEDIA_TYPES as readonly string[]).includes(normalized)) {
    return normalized as AttachmentMediaType;
  }

  const extMatch = fileName.match(/\.([a-zA-Z0-9]+)$/);
  const ext = extMatch ? extMatch[1].toLowerCase() : "";
  switch (ext) {
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "webp":
      return "image/webp";
    case "txt":
    case "log":
      return "text/plain";
    case "pdf":
      return "application/pdf";
    default:
      throw new AttachmentValidationError(
        `Unsupported media type "${type || ext}". Allowed types: ${ALLOWED_ATTACHMENT_MEDIA_TYPES.join(", ")}`
      );
  }
}

export function uint8ArrayToBase64(bytes: Uint8Array): string {
  if (typeof Buffer !== "undefined") {
    return Buffer.from(bytes).toString("base64");
  }
  let binary = "";
  const len = bytes.byteLength;
  const chunkSize = 8192;
  for (let i = 0; i < len; i += chunkSize) {
    const sub = bytes.subarray(i, Math.min(i + chunkSize, len));
    binary += String.fromCharCode.apply(null, sub as unknown as number[]);
  }
  return btoa(binary);
}

export async function computeSha256Hex(buffer: ArrayBuffer): Promise<string> {
  const hashBuffer = await crypto.subtle.digest("SHA-256", buffer);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
}

export interface RemoteAttachmentAdapterOptions {
  readonly baseUrl: string;
  readonly token: string;
  readonly chunkSize?: number;
  readonly cancelTimeoutMs?: number;
  readonly fetchFn?: typeof fetch;
}

export async function cancelRemoteAttachment(
  options: RemoteAttachmentAdapterOptions,
  target: TargetRef,
  attachmentId: string
): Promise<boolean> {
  const fetchImpl = options.fetchFn ?? fetch;
  const url = remoteApiUrl(options.baseUrl, "/api/v1/chat/attachments/cancel");
  const payload: AttachmentCancelRequest = {
    target,
    attachmentId,
  };

  const timeoutMs = options.cancelTimeoutMs ?? DEFAULT_ATTACHMENT_CANCEL_TIMEOUT_MS;
  const abortController = new AbortController();
  const timer = setTimeout(() => abortController.abort(), timeoutMs);

  try {
    const response = await fetchImpl(url, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${options.token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
      signal: abortController.signal,
    });

    if (!response.ok) {
      return false;
    }

    const data: unknown = await response.json();
    if (data && typeof data === "object") {
      const resp = data as AttachmentCancelResponse;
      return resp.ok === true && resp.cleaned === true;
    }
    return false;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

function parseAndValidateReceipt(
  body: unknown,
  expectedTarget: TargetRef,
  expectedAttachmentId: string,
  expectedSizeBytes: number,
  expectedSha256: string,
  expectedMediaType: AttachmentMediaType
): AttachmentReceipt {
  if (!body || typeof body !== "object") {
    throw new AttachmentReceiptMismatchError("Server response is not a valid JSON object");
  }

  const record = body as Record<string, unknown>;

  const forbiddenPathKeys = ["path", "remotePath", "localPath", "filePath", "directory"];
  for (const key of forbiddenPathKeys) {
    if (key in record && record[key] !== undefined && record[key] !== null) {
      throw new AttachmentReceiptMismatchError(
        `Security violation: server response contained raw host path in "${key}"`
      );
    }
  }

  if (record.ok !== true || !record.data || typeof record.data !== "object") {
    throw new AttachmentReceiptMismatchError(
      "Server response envelope must be { ok: true, data: AttachmentReceipt }"
    );
  }

  const receiptObj = record.data as Record<string, unknown>;

  for (const key of forbiddenPathKeys) {
    if (key in receiptObj && receiptObj[key] !== undefined && receiptObj[key] !== null) {
      throw new AttachmentReceiptMismatchError(
        `Security violation: attachment receipt contained raw host path in "${key}"`
      );
    }
  }

  const hostId = receiptObj.hostId;
  const attachmentId = receiptObj.attachmentId;
  const sha256 = receiptObj.sha256;
  const sizeBytes = receiptObj.sizeBytes;
  const mediaType = receiptObj.mediaType;

  if (typeof hostId !== "string" || hostId !== expectedTarget.hostId) {
    throw new AttachmentReceiptMismatchError(
      `Receipt hostId mismatch: expected "${expectedTarget.hostId}", got "${String(hostId)}"`
    );
  }

  if (typeof attachmentId !== "string" || attachmentId !== expectedAttachmentId) {
    throw new AttachmentReceiptMismatchError(
      `Receipt attachmentId mismatch: expected "${expectedAttachmentId}", got "${String(attachmentId)}"`
    );
  }

  if (typeof sizeBytes !== "number" || sizeBytes !== expectedSizeBytes) {
    throw new AttachmentReceiptMismatchError(
      `Receipt sizeBytes mismatch: expected ${expectedSizeBytes}, got ${String(sizeBytes)}`
    );
  }

  if (typeof sha256 !== "string" || sha256.toLowerCase() !== expectedSha256.toLowerCase()) {
    throw new AttachmentReceiptMismatchError(
      `Receipt sha256 checksum mismatch: expected "${expectedSha256}", got "${String(sha256)}"`
    );
  }

  if (typeof mediaType !== "string" || mediaType !== expectedMediaType) {
    throw new AttachmentReceiptMismatchError(
      `Receipt mediaType mismatch: expected "${expectedMediaType}", got "${String(mediaType)}"`
    );
  }

  return {
    hostId,
    attachmentId,
    sha256: sha256.toLowerCase(),
    sizeBytes,
    mediaType: mediaType as AttachmentMediaType,
  };
}

export async function stageRemoteAttachment(
  options: RemoteAttachmentAdapterOptions,
  target: TargetRef,
  file: File,
  signal: AbortSignal,
  customAttachmentId?: string
): Promise<AttachmentReceipt> {
  const fetchImpl = options.fetchFn ?? fetch;
  const chunkSize = options.chunkSize ?? DEFAULT_ATTACHMENT_CHUNK_BYTES;

  if (file.size > ATTACHMENT_MAX_FILE_BYTES) {
    throw new AttachmentValidationError(
      `Attachment size ${file.size} bytes exceeds maximum allowed limit of ${ATTACHMENT_MAX_FILE_BYTES} bytes (10 MiB)`
    );
  }

  const fileName = validateAttachmentFileName(file.name);
  const mediaType = validateAttachmentMediaType(file.type, fileName);
  const attachmentId = customAttachmentId ?? safeRandomUUID();

  const arrayBuffer = await file.arrayBuffer();
  if (arrayBuffer.byteLength !== file.size) {
    throw new AttachmentValidationError(
      `Read byte count mismatch: file reports ${file.size} bytes but buffer has ${arrayBuffer.byteLength} bytes`
    );
  }

  const localSha256 = await computeSha256Hex(arrayBuffer);
  const fileBytes = new Uint8Array(arrayBuffer);
  const totalBytes = fileBytes.byteLength;
  const totalChunks = totalBytes === 0 ? 1 : Math.ceil(totalBytes / chunkSize);

  const uploadUrl = remoteApiUrl(options.baseUrl, "/api/v1/chat/attachments/upload");

  let finalReceipt: AttachmentReceipt | null = null;

  for (let chunkIndex = 0; chunkIndex < totalChunks; chunkIndex++) {
    if (signal.aborted) {
      const cleaned = await cancelRemoteAttachment(options, target, attachmentId);
      throw new AttachmentCancellationError(
        cleaned
          ? "Attachment upload cancelled; server staging state cleaned successfully"
          : "Attachment upload cancelled; server staging state cleanup could not be acknowledged",
        cleaned
      );
    }

    const offset = chunkIndex * chunkSize;
    const end = Math.min(offset + chunkSize, totalBytes);
    const chunkSlice = fileBytes.subarray(offset, end);
    const base64Data = uint8ArrayToBase64(chunkSlice);

    const chunkPayload: AttachmentUploadChunkRequest = {
      target,
      attachmentId,
      fileName,
      mediaType,
      chunkIndex,
      totalChunks,
      offset,
      totalBytes,
      data: base64Data,
    };

    let response: Response;
    try {
      response = await fetchImpl(uploadUrl, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${options.token}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify(chunkPayload),
        signal,
      });
    } catch (networkError) {
      if (signal.aborted) {
        const cleaned = await cancelRemoteAttachment(options, target, attachmentId);
        throw new AttachmentCancellationError(
          cleaned
            ? "Attachment upload aborted; server staging state cleaned successfully"
            : "Attachment upload aborted; server staging state cleanup could not be acknowledged",
          cleaned
        );
      }
      throw new AttachmentUploadError(
        `Failed to upload chunk ${chunkIndex + 1}/${totalChunks}: ${
          networkError instanceof Error ? networkError.message : String(networkError)
        }`
      );
    }

    if (!response.ok) {
      let errorCode = `HTTP_${response.status}`;
      let errorMessage = `Upload failed with HTTP status ${response.status}`;
      try {
        const errJson: unknown = await response.json();
        if (errJson && typeof errJson === "object") {
          const errObj = errJson as Record<string, unknown>;
          if (errObj.error && typeof errObj.error === "object") {
            const inner = errObj.error as Record<string, unknown>;
            if (typeof inner.code === "string") errorCode = inner.code;
            if (typeof inner.message === "string") errorMessage = inner.message;
          } else if (typeof errObj.message === "string") {
            errorMessage = errObj.message;
          }
        }
      } catch {}
      throw new AttachmentUploadError(errorMessage, response.status, errorCode);
    }

    if (chunkIndex === totalChunks - 1) {
      let responseBody: unknown;
      try {
        responseBody = await response.json();
      } catch {
        throw new AttachmentReceiptMismatchError("Server response is not valid JSON");
      }

      finalReceipt = parseAndValidateReceipt(
        responseBody,
        target,
        attachmentId,
        totalBytes,
        localSha256,
        mediaType
      );
    }
  }

  if (!finalReceipt) {
    throw new AttachmentUploadError("Upload completed but no final receipt was acquired from server");
  }

  return finalReceipt;
}

export function createRemoteAttachmentStage(
  options: RemoteAttachmentAdapterOptions
): (target: TargetRef, file: File, signal: AbortSignal) => Promise<AttachmentReceipt> {
  return (target: TargetRef, file: File, signal: AbortSignal) =>
    stageRemoteAttachment(options, target, file, signal);
}
