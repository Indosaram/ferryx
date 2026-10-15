/**
 * Owning-host file staging and preview for the Herdr reference chat (plan task 11).
 *
 * Ported from 'devswha/herdr-web-ui' @
 * '54e5a1f67090cb09552d182e7e30dd0ecc314918' (MIT, 'docs/chat/HERDR_LICENSE').
 * Upstream anchors, read at the pinned revision:
 *
 * | Upstream | What this module ports |
 * |---|---|
 * | 'server/paste.ts' (savePaneImage, PasteImageError) | the file is staged on the host that
 *   owns the pane and reaches the program as a PATH, never as browser bytes |
 * | 'server/conversation.ts' file and image parts | a staged file belongs to the same
 *   conversation as typed text, so it needs no second transport |
 *
 * Frozen contract: 'docs/chat/herdr-port-contract.md' section 5 (Files - owner-host, bounded).
 * The Rust twin is 'src-tauri/src/remote/reference_chat/files.rs'; a rule added here must be
 * added there in the same change.
 *
 * ## What this module is, and is not
 *
 * * Staging happens on the owning host. What crosses the wire is the frozen
 *   'ReferenceFileStagePayload'; what comes back is the frozen 'ReferenceFileReceipt' - an
 *   opaque 'AttachmentReceipt' plus the editable '@path ' mention. No browser-local path ever
 *   crosses the wire in either direction.
 * * The mention is TEXT the user can still edit, and it reaches the pane through the ordered
 *   submit lane (task 7) as an ordinary 'ReferenceSubmitPayload' whose 'attachmentIds' is EMPTY.
 *   There is no managed turn, no managed child and no second transport in this file.
 * * Bounds are the frozen scoped ones ('ATTACHMENT_MAX_FILE_BYTES',
 *   'ATTACHMENT_MAX_FILES_PER_TURN', 'ATTACHMENT_MAX_TURN_BYTES',
 *   'ATTACHMENT_UNREFERENCED_TTL_MS'). Nothing here re-declares them.
 * * A refused file stages nothing; a failed send deletes nothing and keeps the draft. Deletion
 *   is always explicit ('deleteReferenceChatFile').
 *
 * ## Reuse seam (W2)
 *
 * The owner-safe rules here are the rules W2 applies in
 * 'ui/src/remote/chat/remoteAttachmentAdapter.ts' together with the owning host's
 * 'remote/attachment_api.rs': a name with no separator, parent or control component; a mention
 * path that is relative and free of parent components; containment proven against a
 * host-resolved real path, so a symlink that leaves the root is refused; and owner-private
 * staging. W2's chunked upload transport, its cancellation route and its receipt discipline are
 * reused through the injected 'ReferenceFileStagingTransport' below, so the integration owner
 * wires the ported adapter in without this module importing a file that does not exist in this
 * worktree yet. Nothing here duplicates the transport.
 *
 * This module is pure helpers plus one transport seam: no React, no DOM beyond an optional
 * 'File'-shaped source, and no storage.
 */
import {
  ATTACHMENT_MAX_FILE_BYTES,
  ATTACHMENT_MAX_FILES_PER_TURN,
  ATTACHMENT_MAX_TURN_BYTES,
  ATTACHMENT_UNREFERENCED_TTL_MS,
  type AttachmentMediaType,
  type AttachmentReceipt,
  type MutationEnvelope,
  type ScopeErrorCode,
  type TargetRef,
} from "../../lib/scopedContracts";
import {
  referenceChatRoute,
  referenceMentionFor,
  type ReferenceFileReceipt,
  type ReferenceFileStagePayload,
  type ReferenceSubmitOrigin,
  type ReferenceSubmitPayload,
  type ReferenceTargetRef,
} from "./referenceTypes";

/** Route suffix for the file routes, under the frozen reference-chat prefix. */
export const REFERENCE_FILE_ROUTE_SUFFIX = "files";

/** Longest accepted staged file name, in bytes (the scoped attachment rule). */
export const REFERENCE_FILE_MAX_NAME_BYTES = 255;

/** The frozen media types a staged file may declare, as the scoped attachment contract has them. */
export const REFERENCE_ALLOWED_MEDIA_TYPES: readonly AttachmentMediaType[] = [
  "image/png",
  "image/jpeg",
  "image/webp",
  "text/plain",
  "application/pdf",
];

/** The bounds a turn's staged files respect; every field is a frozen scoped constant. */
export interface ReferenceFileLimits {
  readonly maxFileBytes: number;
  readonly maxFilesPerTurn: number;
  readonly maxTurnBytes: number;
  readonly unreferencedTtlMs: number;
}

export const REFERENCE_FILE_LIMITS: ReferenceFileLimits = {
  maxFileBytes: ATTACHMENT_MAX_FILE_BYTES,
  maxFilesPerTurn: ATTACHMENT_MAX_FILES_PER_TURN,
  maxTurnBytes: ATTACHMENT_MAX_TURN_BYTES,
  unreferencedTtlMs: ATTACHMENT_UNREFERENCED_TTL_MS,
};

/**
 * A typed refusal from this lane, carrying a frozen scoped code rather than a message a caller
 * would have to pattern-match.
 */
export class ReferenceFileError extends Error {
  readonly code: ScopeErrorCode;
  readonly retryable: boolean;

  constructor(code: ScopeErrorCode, message: string, retryable = false) {
    super(message);
    this.name = "ReferenceFileError";
    this.code = code;
    this.retryable = retryable;
  }
}

function referenceIsSpace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13;
}

const REFERENCE_BACKSLASH = String.fromCharCode(92);

/**
 * A staged file's name, or the refusal that keeps it out of the staging root.
 *
 * Whitespace is refused (the Rust twin refuses it too): an '@path ' mention ends at the first
 * space, so a name carrying one could be staged on the host but never mentioned.
 */
export function validateReferenceFileName(rawName: string): string {
  const trimmed = rawName.trim();
  if (trimmed.length === 0) {
    throw new ReferenceFileError("INVALID_REQUEST", "A staged file name must not be empty");
  }
  if (new TextEncoder().encode(trimmed).length > REFERENCE_FILE_MAX_NAME_BYTES) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file name must not exceed " + REFERENCE_FILE_MAX_NAME_BYTES + " bytes",
    );
  }
  if (
    trimmed.includes("/") ||
    trimmed.includes(REFERENCE_BACKSLASH) ||
    // a drive-relative 'a:x.png' leaves the staging root when the host joins it on Windows
    trimmed.includes(":")
  ) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file name must not contain a path separator",
    );
  }
  if (trimmed === "." || trimmed === "..") {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file name must not be a relative directory component",
    );
  }
  for (let index = 0; index < trimmed.length; index += 1) {
    const code = trimmed.charCodeAt(index);
    if (code < 32 || (code >= 127 && code <= 159)) {
      throw new ReferenceFileError(
        "INVALID_REQUEST",
        "A staged file name must not contain a control character",
      );
    }
  }
  if (/\s/.test(trimmed)) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file name must not contain whitespace",
    );
  }
  return trimmed;
}

/**
 * The name a browser file is staged under: its own name with every whitespace run replaced by
 * '_', so a desktop screenshot ('Screenshot 2026-10-13 at 10.00.00.png') stays attachable
 * instead of being refused by the whitespace rule above.
 */
export function referenceStagedFileName(rawName: string): string {
  return validateReferenceFileName(rawName.trim().replace(/\s+/g, "_").replace(/:/g, "_"));
}

/**
 * Is this mention path safe to hand to a program as a relative path?
 *
 * Refuses an absolute path, a Windows drive prefix, a home-relative path, a backslash
 * separator, a control byte, a parent or current-directory component, and an empty or
 * whitespace-padded path.
 */
export function referenceMentionPathIsSafe(relativePath: string): boolean {
  if (relativePath.length === 0 || relativePath.trim() !== relativePath) return false;
  for (let index = 0; index < relativePath.length; index += 1) {
    const code = relativePath.charCodeAt(index);
    if (code < 32 || (code >= 127 && code <= 159)) return false;
  }
  if (
    relativePath.startsWith("/") ||
    relativePath.includes(REFERENCE_BACKSLASH) ||
    relativePath.startsWith("~")
  ) {
    return false;
  }
  const first = relativePath.charAt(0);
  if (relativePath.charAt(1) === ":" && /[A-Za-z]/.test(first)) return false;
  const segments = relativePath.split("/");
  return segments.every(
    (segment) => segment.length > 0 && segment !== "." && segment !== "..",
  );
}

/** The path an '@path ' mention names, or null when the text is not a plain mention. */
export function referenceMentionPathOf(mentionText: string): string | null {
  if (mentionText.length < 2 || mentionText.charAt(0) !== "@") return null;
  let end = mentionText.length;
  while (end > 1 && referenceIsSpace(mentionText.charCodeAt(end - 1))) end -= 1;
  if (end <= 1) return null;
  const path = mentionText.slice(1, end);
  for (let index = 0; index < path.length; index += 1) {
    if (referenceIsSpace(path.charCodeAt(index))) return null;
  }
  return path.length === 0 ? null : path;
}

function referenceNormalizeForCompare(path: string): string {
  let normalized = path.split(REFERENCE_BACKSLASH).join("/");
  while (normalized.length > 1 && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  return normalized;
}

/**
 * Does a host-resolved real path still live inside the root it was resolved against?
 *
 * The compare is segment-wise ('/root/wt2' is not inside '/root/wt'), which is what makes this
 * the fence for a symlink that left the root: the owning host resolves the symlink first, then
 * this question is asked about the real path.
 */
export function referenceMentionPathWithinRoot(root: string, resolvedPath: string): boolean {
  const normalizedRoot = referenceNormalizeForCompare(root);
  const normalizedResolved = referenceNormalizeForCompare(resolvedPath);
  if (normalizedRoot.length === 0 || normalizedResolved.length === 0) return false;
  if (normalizedResolved.split("/").includes("..")) return false;
  if (normalizedRoot === "/") return normalizedResolved.startsWith("/");
  return (
    normalizedResolved === normalizedRoot || normalizedResolved.startsWith(normalizedRoot + "/")
  );
}

/** Refuse a file over the frozen per-file limit. */
export function validateReferenceFileSize(
  sizeBytes: number,
  limits: ReferenceFileLimits = REFERENCE_FILE_LIMITS,
): void {
  if (sizeBytes > limits.maxFileBytes) {
    throw new ReferenceFileError(
      "PAYLOAD_TOO_LARGE",
      "Staged file of " + sizeBytes + " bytes exceeds the " + limits.maxFileBytes + " byte limit",
    );
  }
}

/** Refuse a file that would take the turn past its file count or its byte budget. */
export function validateReferenceTurnBounds(
  fileCount: number,
  turnBytes: number,
  sizeBytes: number,
  limits: ReferenceFileLimits = REFERENCE_FILE_LIMITS,
): void {
  if (fileCount + 1 > limits.maxFilesPerTurn) {
    throw new ReferenceFileError(
      "PAYLOAD_TOO_LARGE",
      "A turn may reference at most " + limits.maxFilesPerTurn + " staged files",
    );
  }
  if (turnBytes + sizeBytes > limits.maxTurnBytes) {
    throw new ReferenceFileError(
      "PAYLOAD_TOO_LARGE",
      "Staged files for a turn must stay within " + limits.maxTurnBytes + " bytes",
    );
  }
}

/** The frozen media type for a file, or the refusal for an unsupported one. */
export function referenceMediaTypeFor(mediaType: string, fileName: string): AttachmentMediaType {
  const normalized = mediaType.toLowerCase().trim();
  if ((REFERENCE_ALLOWED_MEDIA_TYPES as readonly string[]).includes(normalized)) {
    return normalized as AttachmentMediaType;
  }
  const dot = fileName.lastIndexOf(".");
  const extension = dot >= 0 ? fileName.slice(dot + 1).toLowerCase() : "";
  switch (extension) {
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
      throw new ReferenceFileError(
        "INVALID_REQUEST",
        "Unsupported media type for a staged file: " + (mediaType.length > 0 ? mediaType : extension),
      );
  }
}

/** Append a mention to a draft, adding the separating space only when one is needed. */
export function referenceAppendMention(current: string, path: string): string {
  const mention = referenceMentionFor(path);
  if (current.length === 0 || referenceIsSpace(current.charCodeAt(current.length - 1))) {
    return current + mention;
  }
  return current + " " + mention;
}

/** Insert a mention at the caret, whitespace-aware, leaving the caret after the mention. */
export function referenceMentionInsertion(
  current: string,
  path: string,
  caret: number,
): { readonly text: string; readonly caret: number } {
  const safeCaret = Math.max(0, Math.min(caret, current.length));
  const before = current.slice(0, safeCaret);
  const after = current.slice(safeCaret);
  const needsSpace = before.length > 0 && !referenceIsSpace(before.charCodeAt(before.length - 1));
  const inserted = (needsSpace ? " " : "") + referenceMentionFor(path);
  return { text: before + inserted + after, caret: before.length + inserted.length };
}

/** Base64 for a staged payload's bytes (the frozen 'contentBase64' field). */
export function referenceBase64(bytes: Uint8Array): string {
  const maybeBuffer = (
    globalThis as unknown as {
      Buffer?: { from(input: Uint8Array): { toString(encoding: string): string } };
    }
  ).Buffer;
  if (maybeBuffer !== undefined) {
    return maybeBuffer.from(bytes).toString("base64");
  }
  let binary = "";
  const chunkSize = 8192;
  for (let index = 0; index < bytes.byteLength; index += chunkSize) {
    const slice = bytes.subarray(index, Math.min(index + chunkSize, bytes.byteLength));
    binary += String.fromCharCode.apply(null, Array.from(slice));
  }
  return btoa(binary);
}

const SHA256_K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

/**
 * FIPS 180-4 SHA-256 in plain JavaScript, for origins without WebCrypto: a remote client opened
 * over plain HTTP (an overlay or LAN address) is not a secure context, so `crypto.subtle` is absent.
 */
function sha256Portable(bytes: Uint8Array): Uint8Array {
  const bitLength = bytes.byteLength * 8;
  const padded = new Uint8Array(Math.ceil((bytes.byteLength + 9) / 64) * 64);
  padded.set(bytes);
  padded[bytes.byteLength] = 0x80;
  const tail = new DataView(padded.buffer);
  tail.setUint32(padded.byteLength - 8, Math.floor(bitLength / 0x1_0000_0000));
  tail.setUint32(padded.byteLength - 4, bitLength >>> 0);
  const h = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);
  const w = new Uint32Array(64);
  const rotr = (x: number, n: number): number => (x >>> n) | (x << (32 - n));
  for (let offset = 0; offset < padded.byteLength; offset += 64) {
    for (let i = 0; i < 16; i += 1) w[i] = tail.getUint32(offset + i * 4);
    for (let i = 16; i < 64; i += 1) {
      const s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, hh] = h;
    for (let i = 0; i < 64; i += 1) {
      const t1 = (hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + ((e & f) ^ (~e & g)) + SHA256_K[i] + w[i]) >>> 0;
      const t2 = ((rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22)) + ((a & b) ^ (a & c) ^ (b & c))) >>> 0;
      hh = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    h[0] = (h[0] + a) >>> 0;
    h[1] = (h[1] + b) >>> 0;
    h[2] = (h[2] + c) >>> 0;
    h[3] = (h[3] + d) >>> 0;
    h[4] = (h[4] + e) >>> 0;
    h[5] = (h[5] + f) >>> 0;
    h[6] = (h[6] + g) >>> 0;
    h[7] = (h[7] + hh) >>> 0;
  }
  const out = new Uint8Array(32);
  const view = new DataView(out.buffer);
  h.forEach((word, index) => view.setUint32(index * 4, word));
  return out;
}

/** SHA-256 of a staged payload's bytes, as the receipt reports it. */
export async function referenceSha256Hex(bytes: Uint8Array): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  let digest: Uint8Array;
  if (subtle === undefined) {
    digest = sha256Portable(bytes);
  } else {
    const copy = new Uint8Array(bytes.byteLength);
    copy.set(bytes);
    digest = new Uint8Array(await subtle.digest("SHA-256", copy));
  }
  return Array.from(digest)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

/** The frozen stage payload for bytes the browser is about to hand to the owning host. */
export function referenceFileStagePayload(
  name: string,
  mediaType: AttachmentMediaType,
  bytes: Uint8Array,
): ReferenceFileStagePayload {
  return {
    name,
    mediaType,
    sizeBytes: bytes.byteLength,
    contentBase64: referenceBase64(bytes),
  };
}

/**
 * The transport this lane reuses: the ported W2 attachment adapter satisfies it.
 *
 * 'stage' returns the raw response body so this module - not the transport - decides whether the
 * receipt is trustworthy.
 */
export interface ReferenceFileStagingTransport {
  readonly stage: (
    target: TargetRef,
    payload: ReferenceFileStagePayload,
    signal: AbortSignal,
  ) => Promise<unknown>;
  readonly cancel: (target: TargetRef, attachmentId: string) => Promise<boolean>;
  readonly remove: (target: TargetRef, attachmentId: string) => Promise<boolean>;
}

/** The minimal 'File'-shaped source a staging call reads; a DOM File satisfies it. */
export interface ReferenceFileSource {
  readonly name: string;
  readonly type: string;
  readonly size: number;
  readonly arrayBuffer: () => Promise<ArrayBuffer>;
}

export interface ReferenceFileReceiptExpectation {
  readonly target: TargetRef;
  readonly displayName: string;
  readonly mediaType: AttachmentMediaType;
  readonly sizeBytes: number;
  readonly sha256: string;
  /** The owning worktree root and the host-resolved real path, when the caller knows them. */
  readonly root?: string;
  readonly resolvedPath?: string;
}

const REFERENCE_FORBIDDEN_PATH_KEYS = [
  "path",
  "remotePath",
  "localPath",
  "filePath",
  "absolutePath",
  "directory",
];

function assertNoHostPath(value: unknown, where: string): void {
  if (value === null || typeof value !== "object") return;
  const record = value as Record<string, unknown>;
  for (const key of REFERENCE_FORBIDDEN_PATH_KEYS) {
    if (!Object.prototype.hasOwnProperty.call(record, key)) continue;
    const raw = record[key];
    if (raw === undefined || raw === null) continue;
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file response carried a raw host path in " + key + " (" + where + ")",
    );
  }
}

/**
 * Validate a stage response against what was uploaded, and refuse anything this lane cannot
 * prove: a foreign host, a different digest, size or media type, a raw host path, a mention that
 * is not a safe relative path, or a real path that left the owning root.
 */
export function parseReferenceFileReceipt(
  raw: unknown,
  expected: ReferenceFileReceiptExpectation,
): ReferenceFileReceipt {
  if (raw === null || typeof raw !== "object") {
    throw new ReferenceFileError("INVALID_REQUEST", "A staged file response must be an object");
  }
  const body = raw as Record<string, unknown>;
  assertNoHostPath(body, "envelope");

  const receiptRaw = body.receipt;
  if (receiptRaw === null || typeof receiptRaw !== "object") {
    throw new ReferenceFileError("INVALID_REQUEST", "A staged file response must carry a receipt");
  }
  assertNoHostPath(receiptRaw, "receipt");
  const receipt = receiptRaw as Record<string, unknown>;

  const hostId = receipt.hostId;
  const attachmentId = receipt.attachmentId;
  const sha256 = receipt.sha256;
  const sizeBytes = receipt.sizeBytes;
  const mediaType = receipt.mediaType;
  const displayName = body.displayName;
  const mentionText = body.mentionText;

  if (typeof hostId !== "string" || hostId !== expected.target.hostId) {
    throw new ReferenceFileError(
      "FORBIDDEN",
      "A staged file receipt must belong to the owning host " + expected.target.hostId,
    );
  }
  if (typeof attachmentId !== "string" || attachmentId.length === 0) {
    throw new ReferenceFileError("INVALID_REQUEST", "A staged file receipt must carry an id");
  }
  if (typeof sha256 !== "string" || sha256.toLowerCase() !== expected.sha256.toLowerCase()) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file receipt must carry the digest of the bytes that were uploaded",
    );
  }
  if (typeof sizeBytes !== "number" || sizeBytes !== expected.sizeBytes) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file receipt must carry the size of the bytes that were uploaded",
    );
  }
  if (typeof mediaType !== "string" || mediaType !== expected.mediaType) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file receipt must carry the media type that was uploaded",
    );
  }
  if (typeof displayName !== "string" || displayName !== expected.displayName) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file receipt must name the file that was uploaded",
    );
  }
  if (typeof mentionText !== "string") {
    throw new ReferenceFileError("INVALID_REQUEST", "A staged file receipt must carry a mention");
  }

  const path = referenceMentionPathOf(mentionText);
  if (path === null || !referenceMentionPathIsSafe(path)) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file mention must name a safe relative path",
    );
  }
  if (path !== expected.displayName && !path.endsWith("/" + expected.displayName)) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file mention must name the file that was uploaded",
    );
  }
  if (
    expected.root !== undefined &&
    expected.resolvedPath !== undefined &&
    !referenceMentionPathWithinRoot(expected.root, expected.resolvedPath)
  ) {
    throw new ReferenceFileError(
      "FORBIDDEN",
      "A staged file resolved outside the owning worktree root",
    );
  }

  const staged: AttachmentReceipt = {
    hostId,
    attachmentId,
    sha256: sha256.toLowerCase(),
    sizeBytes,
    mediaType: mediaType as AttachmentMediaType,
  };
  return { receipt: staged, displayName, mentionText };
}

export interface ReferenceFileStagingDeps {
  readonly transport: ReferenceFileStagingTransport;
  readonly limits?: ReferenceFileLimits;
  /** The owning worktree root and the host-resolved real path, when the caller knows them. */
  readonly root?: string;
  readonly resolvedPath?: string;
  /** SHA-256 of the staged bytes; the WebCrypto implementation unless a caller supplies one. */
  readonly digest?: (bytes: Uint8Array) => Promise<string>;
}

/** How much the turn already carries, for the frozen turn bounds. */
export interface ReferenceFileTurnUsage {
  readonly fileCount: number;
  readonly turnBytes: number;
}

const REFERENCE_EMPTY_TURN_USAGE: ReferenceFileTurnUsage = { fileCount: 0, turnBytes: 0 };

/**
 * Stage a file on the owning host and return the receipt the chat refers to.
 *
 * The bytes are read once, validated against the frozen bounds, and handed to the injected
 * transport; nothing is written locally and no path is invented here.
 */
export async function stageReferenceChatFile(
  deps: ReferenceFileStagingDeps,
  target: ReferenceTargetRef,
  file: ReferenceFileSource,
  signal: AbortSignal,
  usage: ReferenceFileTurnUsage = REFERENCE_EMPTY_TURN_USAGE,
): Promise<ReferenceFileReceipt> {
  const limits = deps.limits ?? REFERENCE_FILE_LIMITS;
  const name = referenceStagedFileName(file.name);
  const mediaType = referenceMediaTypeFor(file.type, name);
  validateReferenceFileSize(file.size, limits);
  validateReferenceTurnBounds(usage.fileCount, usage.turnBytes, file.size, limits);

  const buffer = await file.arrayBuffer();
  const bytes = new Uint8Array(buffer);
  if (bytes.byteLength !== file.size) {
    throw new ReferenceFileError(
      "INVALID_REQUEST",
      "A staged file's read byte count must match the size it reported",
    );
  }

  const digest = deps.digest ?? referenceSha256Hex;
  const sha256 = (await digest(bytes)).toLowerCase();
  const payload = referenceFileStagePayload(name, mediaType, bytes);
  const raw = await deps.transport.stage(target.target, payload, signal);

  try {
    return parseReferenceFileReceipt(raw, {
      target: target.target,
      displayName: name,
      mediaType,
      sizeBytes: file.size,
      sha256,
      root: deps.root,
      resolvedPath: deps.resolvedPath,
    });
  } catch (error) {
    // The host already staged bytes under this id; a receipt this lane refuses must not leave
    // them orphaned on the host, counting against the turn's file limit until the TTL.
    const stagedId = referenceStagedAttachmentId(raw);
    if (stagedId !== null) {
      await deps.transport.cancel(target.target, stagedId).catch(() => false);
    }
    throw error;
  }
}

/** The attachment id a raw stage response names, when it names one at all. */
function referenceStagedAttachmentId(raw: unknown): string | null {
  if (raw === null || typeof raw !== "object") return null;
  const receipt = (raw as Record<string, unknown>).receipt;
  if (receipt === null || typeof receipt !== "object") return null;
  const id = (receipt as Record<string, unknown>).attachmentId;
  return typeof id === "string" && id.length > 0 ? id : null;
}

/**
 * Read the GET /files answer: the files the owning host still holds staged, unsent, for this
 * target. A stage whose response the client never saw (an aborted upload, a reload) is rehydrated
 * from here so it can be released. An entry this lane cannot prove - a foreign host, a raw host
 * path, a mention that is not a safe relative path naming its own file - is dropped.
 */
export function parseReferenceStagedFileList(
  raw: unknown,
  target: ReferenceTargetRef,
): ReferenceFileReceipt[] {
  if (raw === null || typeof raw !== "object") return [];
  const files = (raw as Record<string, unknown>).files;
  if (!Array.isArray(files)) return [];
  const out: ReferenceFileReceipt[] = [];
  for (const entry of files) {
    try {
      const receipt = (entry as Record<string, unknown> | null)?.receipt as
        | Record<string, unknown>
        | undefined;
      if (receipt === undefined || receipt === null || typeof receipt !== "object") continue;
      const sha256 = receipt.sha256;
      const sizeBytes = receipt.sizeBytes;
      const mediaType = receipt.mediaType;
      const displayName = (entry as Record<string, unknown>).displayName;
      if (
        typeof sha256 !== "string" ||
        typeof sizeBytes !== "number" ||
        typeof mediaType !== "string" ||
        typeof displayName !== "string"
      ) {
        continue;
      }
      out.push(
        parseReferenceFileReceipt(entry, {
          target: target.target,
          displayName,
          mediaType: mediaType as AttachmentMediaType,
          sizeBytes,
          sha256,
        }),
      );
    } catch {
      // Not provable as this target's own staged file: never surfaced.
    }
  }
  return out;
}

/** The outcome of an explicit cleanup: the id it acted on, and whether the host acknowledged it. */
export interface ReferenceFileCleanupReceipt {
  readonly attachmentId: string;
  readonly cleaned: boolean;
}

/** Cancel a staged upload's host-side state; a caller reports an unacknowledged cleanup. */
export async function cancelReferenceChatFile(
  deps: ReferenceFileStagingDeps,
  target: ReferenceTargetRef,
  receipt: ReferenceFileReceipt,
): Promise<ReferenceFileCleanupReceipt> {
  const cleaned = await deps.transport.cancel(target.target, receipt.receipt.attachmentId);
  return { attachmentId: receipt.receipt.attachmentId, cleaned };
}

/** Delete a staged file. The only deletion path, and it is always explicit. */
export async function deleteReferenceChatFile(
  deps: ReferenceFileStagingDeps,
  target: ReferenceTargetRef,
  receipt: ReferenceFileReceipt,
): Promise<ReferenceFileCleanupReceipt> {
  const cleaned = await deps.transport.remove(target.target, receipt.receipt.attachmentId);
  return { attachmentId: receipt.receipt.attachmentId, cleaned };
}

/**
 * The submit a set of staged files produces: the mentions as EDITABLE TEXT, no managed
 * attachment. 'attachmentIds' is deliberately empty - the file is already on the owning host,
 * and only the path has to reach the program, as typed text through the ordered submit lane.
 */
export function planReferenceFileSend(
  draftText: string,
  receipts: readonly ReferenceFileReceipt[],
  origin: ReferenceSubmitOrigin = "chat",
): ReferenceSubmitPayload {
  let text = draftText;
  for (const receipt of receipts) {
    const path = referenceMentionPathOf(receipt.mentionText);
    if (path === null || !referenceMentionPathIsSafe(path)) {
      throw new ReferenceFileError(
        "INVALID_REQUEST",
        "A staged file's mention must name a safe relative path before it can be sent",
      );
    }
    text = referenceAppendMention(text, path);
  }
  return { text, attachmentIds: [], origin };
}

/** What a failed send leaves behind: the draft, the receipts, and no deletion. */
export interface ReferenceFileSendFailure {
  readonly text: string;
  readonly attachmentIds: readonly string[];
  readonly error: ReferenceFileError;
  readonly deleted: false;
  readonly receipts: readonly ReferenceFileReceipt[];
}

/**
 * Record a failed send. The draft text survives untouched, the staged files stay staged, and no
 * managed attachment id is ever minted: a retry is the user's explicit choice.
 */
export function referenceFileSendFailure(
  draftText: string,
  receipts: readonly ReferenceFileReceipt[],
  error: ReferenceFileError,
): ReferenceFileSendFailure {
  return { text: draftText, attachmentIds: [], error, deleted: false, receipts };
}

/** The route that stages and lists this target's files (task 13 registers it). */
export function referenceFilesRoute(sessionId: string): string {
  return referenceChatRoute(sessionId, REFERENCE_FILE_ROUTE_SUFFIX);
}

/** The route that previews or deletes one staged file. */
export function referenceFilePreviewRoute(sessionId: string, attachmentId: string): string {
  return referenceChatRoute(sessionId, REFERENCE_FILE_ROUTE_SUFFIX + "/" + attachmentId);
}

/** The frozen mutation envelope for a file request: the target travels with the request id. */
export function referenceFileMutationEnvelope<P>(
  requestId: string,
  target: ReferenceTargetRef,
  params: P,
): MutationEnvelope<P> {
  return { requestId, target: target.target, params };
}
