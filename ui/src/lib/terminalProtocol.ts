export const GHOSTTY_SNAPSHOT_MAGIC = "GHOSTSNP";
export const GHOSTTY_SNAPSHOT_FORMAT_VERSION = 1;
export const GHOSTTY_CLEAN_REVISION = "6a508fd5e34c7e222c052a6d00bb3891ff3feace";
export const ENGINE_IDENTIFIER = "ghostty-vt";
export const MAX_SNAPSHOT_CHUNK_BYTES = 64 * 1024;
export const MAX_COMPRESSED_PAYLOAD_BYTES = 4 * 1024 * 1024;
export const MAX_DECODED_BYTES = 32 * 1024 * 1024;
export const MAX_PASTE_BYTES = 512 * 1024;
export const MAX_RESIZE_COLS = 1000;
export const MAX_RESIZE_ROWS = 1000;
export const GHOSTTY_ENVELOPE_LEN = 10;

const CRC32C_TABLE = new Uint32Array(256);
for (let i = 0; i < 256; i++) {
  let crc = i >>> 0;
  for (let j = 0; j < 8; j++) {
    if ((crc & 1) !== 0) {
      crc = (crc >>> 1) ^ 0x82f63b78;
    } else {
      crc = crc >>> 1;
    }
  }
  CRC32C_TABLE[i] = crc >>> 0;
}

export function crc32c(data: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < data.length; i++) {
    const index = (crc ^ data[i]) & 0xff;
    crc = (crc >>> 8) ^ CRC32C_TABLE[index];
  }
  return (crc ^ 0xffffffff) >>> 0;
}

export function encodeBase64(bytes: Uint8Array): string {
  if (typeof Buffer !== "undefined") {
    return Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength).toString("base64");
  }
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return globalThis.btoa(binary);
}

export function decodeBase64(data: string): Uint8Array {
  if (typeof Buffer !== "undefined") {
    const buf = Buffer.from(data, "base64");
    return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  }
  const binary = globalThis.atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function toUint8Array(input: Uint8Array | string): Uint8Array {
  if (typeof input === "string") {
    return decodeBase64(input);
  }
  return input;
}

export type EngineCompatibility = {
  readonly engine: string;
  readonly sourceRevision: string;
  readonly snapshotVersion: number;
};

export function createDefaultEngineCompatibility(): EngineCompatibility {
  return {
    engine: ENGINE_IDENTIFIER,
    sourceRevision: GHOSTTY_CLEAN_REVISION,
    snapshotVersion: GHOSTTY_SNAPSHOT_FORMAT_VERSION,
  };
}

export function validateEngineCompatibility(compat: EngineCompatibility): void {
  if (compat.engine !== ENGINE_IDENTIFIER) {
    throw new Error(`Incompatible terminal engine: expected '${ENGINE_IDENTIFIER}', got '${compat.engine}'`);
  }
  if (compat.snapshotVersion !== GHOSTTY_SNAPSHOT_FORMAT_VERSION) {
    throw new Error(
      `Unsupported Ghostty snapshot version: expected ${GHOSTTY_SNAPSHOT_FORMAT_VERSION}, got ${compat.snapshotVersion}`,
    );
  }
  if (compat.sourceRevision.toLowerCase() !== GHOSTTY_CLEAN_REVISION.toLowerCase()) {
    throw new Error(
      `Incompatible engine source revision: expected '${GHOSTTY_CLEAN_REVISION}', got '${compat.sourceRevision}'`,
    );
  }
}

export function validateGhosttySnapshotEnvelope(bytes: Uint8Array): number {
  if (bytes.length < GHOSTTY_ENVELOPE_LEN) {
    throw new Error(`Snapshot envelope too short: expected at least ${GHOSTTY_ENVELOPE_LEN} bytes, got ${bytes.length}`);
  }

  const magic = new TextDecoder("ascii").decode(bytes.subarray(0, 8));
  if (magic !== GHOSTTY_SNAPSHOT_MAGIC) {
    throw new Error(`Invalid Ghostty snapshot magic bytes: expected '${GHOSTTY_SNAPSHOT_MAGIC}', got '${magic}'`);
  }

  const version = bytes[8] | (bytes[9] << 8);
  if (version !== GHOSTTY_SNAPSHOT_FORMAT_VERSION) {
    throw new Error(
      `Unsupported Ghostty snapshot version: expected ${GHOSTTY_SNAPSHOT_FORMAT_VERSION}, got ${version}`,
    );
  }

  return version;
}

export type SnapshotMetadata = {
  readonly snapshotId: string;
  readonly generation: number;
  readonly streamEpoch: number;
  readonly chunkSequence: number;
  readonly engineCompat: EngineCompatibility;
  readonly totalCompressedBytes: number;
  readonly totalDecodedBytes: number;
  readonly totalChunks: number;
  readonly hasUnfinishedContinuation: boolean;
  readonly historyRowsPrimary?: number;
  readonly historyRowsAlternate?: number;
};

export function validateMetadataPreAllocation(metadata: SnapshotMetadata): void {
  if (!metadata.snapshotId || metadata.snapshotId.trim() === "") {
    throw new Error("Snapshot ID cannot be blank");
  }

  validateEngineCompatibility(metadata.engineCompat);

  if (metadata.totalChunks <= 0 || !Number.isInteger(metadata.totalChunks)) {
    throw new Error("Total chunks must be a positive integer");
  }

  if (metadata.totalCompressedBytes > MAX_COMPRESSED_PAYLOAD_BYTES) {
    throw new Error(
      `Total compressed payload size (${metadata.totalCompressedBytes}) exceeds limit (${MAX_COMPRESSED_PAYLOAD_BYTES})`,
    );
  }

  if (metadata.totalDecodedBytes > MAX_DECODED_BYTES) {
    throw new Error(
      `Declared decoded size (${metadata.totalDecodedBytes}) exceeds 32MiB safety cap (${MAX_DECODED_BYTES})`,
    );
  }

  const maxPossibleChunks = Math.min(
    Math.max(metadata.totalCompressedBytes, 1),
    MAX_COMPRESSED_PAYLOAD_BYTES,
  );
  if (metadata.totalChunks > maxPossibleChunks) {
    throw new Error(
      `Declared chunk count (${metadata.totalChunks}) exceeds possible chunks for payload (${maxPossibleChunks})`,
    );
  }
}

export type SnapshotAbortReason =
  | {
      readonly reason: "staleGeneration";
      readonly activeGeneration: number;
      readonly rejectedGeneration: number;
    }
  | {
      readonly reason: "chunkSequenceMismatch";
      readonly expectedSequence: number;
      readonly actualSequence: number;
    }
  | {
      readonly reason: "limitExceeded";
      readonly limit: string;
      readonly actual: number;
      readonly max: number;
    }
  | {
      readonly reason: "checksumMismatch";
      readonly expected: number;
      readonly computed: number;
    }
  | {
      readonly reason: "incompatibleEngine";
      readonly detail: string;
    }
  | {
      readonly reason: "clientCancelled";
    }
  | {
      readonly reason: "unknownSession";
    };

export type SnapshotTransferBeginMessage = {
  readonly type: "begin";
  readonly metadata: SnapshotMetadata;
};

export type SnapshotTransferChunkMessage = {
  readonly type: "chunk";
  readonly snapshotId: string;
  readonly generation: number;
  readonly chunkIndex: number;
  readonly chunkBytes: Uint8Array | string;
  readonly chunkCrc32c: number;
};

export type SnapshotTransferCommitMessage = {
  readonly type: "commit";
  readonly snapshotId: string;
  readonly generation: number;
  readonly totalChunks: number;
  readonly totalCompressedBytes: number;
  readonly aggregateCrc32c: number;
};

export type SnapshotTransferAbortMessage = {
  readonly type: "abort";
  readonly snapshotId: string;
  readonly generation: number;
  readonly reason: SnapshotAbortReason;
};

export type SnapshotTransferMessage =
  | SnapshotTransferBeginMessage
  | SnapshotTransferChunkMessage
  | SnapshotTransferCommitMessage
  | SnapshotTransferAbortMessage;

export class SnapshotTransferAssembler {
  private currentGen = 0;
  private activeMeta: SnapshotMetadata | null = null;
  private readonly receivedChunks = new Map<number, Uint8Array>();
  private receivedByteCount = 0;

  public get currentGeneration(): number {
    return this.currentGen;
  }

  public get activeMetadata(): SnapshotMetadata | null {
    return this.activeMeta;
  }

  public reset(): void {
    this.activeMeta = null;
    this.receivedChunks.clear();
    this.receivedByteCount = 0;
  }

  public handleBegin(metadata: SnapshotMetadata): void {
    validateMetadataPreAllocation(metadata);

    if (metadata.generation < this.currentGen) {
      throw new Error(
        `Stale generation: active generation ${this.currentGen} >= incoming ${metadata.generation}`,
      );
    }

    this.currentGen = metadata.generation;
    this.activeMeta = metadata;
    this.receivedChunks.clear();
    this.receivedByteCount = 0;
  }

  public handleChunk(
    snapshotId: string,
    generation: number,
    chunkIndex: number,
    chunkBytesInput: Uint8Array | string,
    chunkCrc32c: number,
  ): void {
    const chunkBytes = toUint8Array(chunkBytesInput);

    if (chunkBytes.length > MAX_SNAPSHOT_CHUNK_BYTES) {
      throw new Error(
        `Chunk bytes (${chunkBytes.length}) exceeds maximum upper bound (${MAX_SNAPSHOT_CHUNK_BYTES})`,
      );
    }

    if (generation < this.currentGen) {
      throw new Error(
        `Stale generation: active generation ${this.currentGen} >= incoming ${generation}`,
      );
    }

    if (!this.activeMeta) {
      throw new Error("No active snapshot transfer in progress");
    }

    if (generation !== this.activeMeta.generation) {
      throw new Error(
        `Generation mismatch: active ${this.activeMeta.generation} !== chunk ${generation}`,
      );
    }

    if (snapshotId !== this.activeMeta.snapshotId) {
      throw new Error(
        `Transfer snapshot ID mismatch: expected '${this.activeMeta.snapshotId}', got '${snapshotId}'`,
      );
    }

    if (chunkIndex >= this.activeMeta.totalChunks) {
      throw new Error(
        `Chunk index ${chunkIndex} out of bounds for total chunks ${this.activeMeta.totalChunks}`,
      );
    }

    const computedCrc = crc32c(chunkBytes);
    if (computedCrc !== chunkCrc32c) {
      throw new Error(
        `Chunk CRC32C mismatch for chunk ${chunkIndex}: expected ${chunkCrc32c}, computed ${computedCrc}`,
      );
    }

    const existing = this.receivedChunks.get(chunkIndex);
    if (existing) {
      if (existing.length !== chunkBytes.length) {
        throw new Error(`Duplicate chunk index ${chunkIndex} received with mismatched length`);
      }
      for (let i = 0; i < existing.length; i++) {
        if (existing[i] !== chunkBytes[i]) {
          throw new Error(`Duplicate chunk index ${chunkIndex} received with mismatched content`);
        }
      }
      return;
    }

    const newTotal = this.receivedByteCount + chunkBytes.length;
    if (newTotal > this.activeMeta.totalCompressedBytes) {
      throw new Error(
        `Total chunk bytes (${newTotal}) exceeds declared payload size (${this.activeMeta.totalCompressedBytes})`,
      );
    }

    this.receivedByteCount = newTotal;
    this.receivedChunks.set(chunkIndex, chunkBytes);
  }

  public handleCommit(
    snapshotId: string,
    generation: number,
    totalChunks: number,
    totalCompressedBytes: number,
    aggregateCrc32c: number,
  ): Uint8Array {
    if (generation < this.currentGen) {
      throw new Error(
        `Stale generation: active generation ${this.currentGen} >= incoming ${generation}`,
      );
    }

    if (!this.activeMeta) {
      throw new Error("No active snapshot transfer in progress");
    }

    if (generation !== this.activeMeta.generation) {
      throw new Error(
        `Generation mismatch: active ${this.activeMeta.generation} !== commit ${generation}`,
      );
    }

    if (snapshotId !== this.activeMeta.snapshotId) {
      throw new Error(
        `Transfer snapshot ID mismatch: expected '${this.activeMeta.snapshotId}', got '${snapshotId}'`,
      );
    }

    if (totalChunks !== this.activeMeta.totalChunks || this.receivedChunks.size !== totalChunks) {
      throw new Error(
        `Missing chunks at commit: received ${this.receivedChunks.size} of ${totalChunks}`,
      );
    }

    if (totalCompressedBytes !== this.receivedByteCount) {
      throw new Error(
        `Assembled snapshot byte length mismatch: declared ${totalCompressedBytes}, got ${this.receivedByteCount}`,
      );
    }

    const assembled = new Uint8Array(this.receivedByteCount);
    let offset = 0;
    for (let i = 0; i < totalChunks; i++) {
      const chunk = this.receivedChunks.get(i);
      if (!chunk) {
        throw new Error(`Missing chunk index ${i} during assembly`);
      }
      assembled.set(chunk, offset);
      offset += chunk.length;
    }

    const computedAggregateCrc = crc32c(assembled);
    if (computedAggregateCrc !== aggregateCrc32c) {
      throw new Error(
        `Aggregate CRC32C mismatch: expected ${aggregateCrc32c}, computed ${computedAggregateCrc}`,
      );
    }

    validateGhosttySnapshotEnvelope(assembled);

    this.reset();
    return assembled;
  }

  public handleAbort(snapshotId: string, generation: number, reason: SnapshotAbortReason): void {
    if (generation < this.currentGen) {
      throw new Error(
        `Stale generation: active generation ${this.currentGen} >= incoming ${generation}`,
      );
    }

    if (
      this.activeMeta &&
      this.activeMeta.generation === generation &&
      this.activeMeta.snapshotId === snapshotId
    ) {
      this.reset();
      throw new Error(`Snapshot transfer was aborted: reason=${reason.reason}`);
    }

    this.reset();
  }
}

export function chunkSnapshotPayload(
  payload: Uint8Array,
  snapshotId: string,
  generation: number,
): {
  readonly metadata: SnapshotMetadata;
  readonly chunks: readonly SnapshotTransferChunkMessage[];
  readonly commit: SnapshotTransferCommitMessage;
} {
  if (payload.length > MAX_COMPRESSED_PAYLOAD_BYTES) {
    throw new Error(
      `Total compressed payload size (${payload.length}) exceeds limit (${MAX_COMPRESSED_PAYLOAD_BYTES})`,
    );
  }

  validateGhosttySnapshotEnvelope(payload);

  const chunkList: SnapshotTransferChunkMessage[] = [];
  let chunkIndex = 0;
  let offset = 0;

  while (offset < payload.length) {
    const end = Math.min(offset + MAX_SNAPSHOT_CHUNK_BYTES, payload.length);
    const slice = payload.subarray(offset, end);
    const chunkCrc = crc32c(slice);

    chunkList.push({
      type: "chunk",
      snapshotId,
      generation,
      chunkIndex,
      chunkBytes: slice,
      chunkCrc32c: chunkCrc,
    });

    chunkIndex++;
    offset = end;
  }

  const totalChunks = Math.max(chunkList.length, 1);

  const metadata: SnapshotMetadata = {
    snapshotId,
    generation,
    streamEpoch: 1,
    chunkSequence: 1,
    engineCompat: createDefaultEngineCompatibility(),
    totalCompressedBytes: payload.length,
    totalDecodedBytes: payload.length,
    totalChunks,
    hasUnfinishedContinuation: true,
  };

  const aggregateCrc = crc32c(payload);

  const commit: SnapshotTransferCommitMessage = {
    type: "commit",
    snapshotId,
    generation,
    totalChunks,
    totalCompressedBytes: payload.length,
    aggregateCrc32c: aggregateCrc,
  };

  return {
    metadata,
    chunks: chunkList,
    commit,
  };
}

export type InputMessage = {
  readonly inputEpoch: number;
  readonly inputSeq: number;
  readonly data: Uint8Array | string;
};

export function validateInputMessage(msg: InputMessage): void {
  if (msg.inputSeq <= 0 || !Number.isInteger(msg.inputSeq)) {
    throw new Error(`Invalid input sequence number ${msg.inputSeq}: must be positive`);
  }

  const bytes = toUint8Array(msg.data);
  if (bytes.length > MAX_SNAPSHOT_CHUNK_BYTES) {
    throw new Error(
      `Input payload size (${bytes.length}) exceeds chunk limit (${MAX_SNAPSHOT_CHUNK_BYTES})`,
    );
  }
}

export type PasteMessage = {
  readonly inputEpoch: number;
  readonly pasteId: string;
  readonly pasteSeq: number;
  readonly bracketed: boolean;
  readonly payload: Uint8Array | string;
};

export function validatePasteMessage(msg: PasteMessage): void {
  if (!msg.pasteId || msg.pasteId.trim() === "") {
    throw new Error("Paste ID cannot be blank");
  }

  const bytes = toUint8Array(msg.payload);
  if (bytes.length > MAX_PASTE_BYTES) {
    throw new Error(
      `Paste payload size (${bytes.length}) exceeds 512KiB staging cap (${MAX_PASTE_BYTES})`,
    );
  }
}

const BRACKETED_PASTE_START = new Uint8Array([0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]);
const BRACKETED_PASTE_END = new Uint8Array([0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]);

export function formatBracketedPaste(payload: Uint8Array | string): Uint8Array {
  const bytes = toUint8Array(payload);
  const out = new Uint8Array(BRACKETED_PASTE_START.length + bytes.length + BRACKETED_PASTE_END.length);
  out.set(BRACKETED_PASTE_START, 0);
  out.set(bytes, BRACKETED_PASTE_START.length);
  out.set(BRACKETED_PASTE_END, BRACKETED_PASTE_START.length + bytes.length);
  return out;
}

export type ResizeLeaseMessage = {
  readonly sessionId: string;
  readonly leaseId: string;
  readonly leaseEpoch: number;
  readonly chunkSequence: number;
  readonly cols: number;
  readonly rows: number;
  readonly signatureOrToken: string;
  readonly expiresAtUnixMillis: number;
};

export function validateResizeLeaseMessage(
  msg: ResizeLeaseMessage,
  currentTimeMillis: number,
  expectedToken?: string,
): void {
  if (!msg.sessionId || msg.sessionId.trim() === "") {
    throw new Error("Session ID cannot be blank");
  }

  if (!msg.leaseId || msg.leaseId.trim() === "") {
    throw new Error("Lease ID cannot be blank");
  }

  if (
    msg.cols <= 0 ||
    msg.cols > MAX_RESIZE_COLS ||
    msg.rows <= 0 ||
    msg.rows > MAX_RESIZE_ROWS ||
    !Number.isInteger(msg.cols) ||
    !Number.isInteger(msg.rows)
  ) {
    throw new Error(`Invalid terminal dimensions: cols=${msg.cols}, rows=${msg.rows}`);
  }

  if (!msg.signatureOrToken || msg.signatureOrToken.trim() === "") {
    throw new Error("Resize lease signature or token cannot be blank");
  }

  if (currentTimeMillis > msg.expiresAtUnixMillis) {
    throw new Error(
      `Resize lease expired: expired_at=${msg.expiresAtUnixMillis}, current_time=${currentTimeMillis}`,
    );
  }

  if (expectedToken !== undefined && msg.signatureOrToken !== expectedToken) {
    throw new Error("Resize lease token rejected as unauthorized");
  }
}
