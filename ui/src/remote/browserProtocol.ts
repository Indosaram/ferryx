/**
 * Remote Browser Screencast Protocol - Wire Codec & Type Definitions
 * Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.1, §4.2)
 */

export const PROTOCOL_KIND = 0x62; // 'b' in ASCII
export const PROTOCOL_VERSION = 1;
export const OPCODE_FRAME = 1;
export const FORMAT_JPEG = 1;
export const FORMAT_PNG = 2;

export const MAX_METADATA_BYTES = 4096; // 4KiB
export const MAX_FRAME_BYTES = 2 * 1024 * 1024; // 2MiB
export const MAX_EDGE_PIXELS = 2048;
export const MAX_IMAGE_PIXELS = 4_000_000; // 4MP cap

export type BrowserImageFormat = "jpeg" | "png";

export interface BrowserCaptureRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BrowserFrameMetadata {
  offsetTop: number;
  pageScaleFactor: number;
  deviceWidth: number;
  deviceHeight: number;
  imageWidth: number;
  imageHeight: number;
  scrollOffsetX: number;
  scrollOffsetY: number;
  timestamp: number;
  // Ferryx required extension fields (§4.2)
  streamId: number;
  browserInstanceId: string;
  browserServiceEpoch: string; // u64 decimal string
  desktopEpoch: string; // u64 decimal string
  documentGeneration: string; // u64 decimal string
  viewportRevision: string; // u64 decimal string
  captureRect: BrowserCaptureRect;
  geometrySource: "wkSnapshot";
}

export interface BrowserFrame {
  format: BrowserImageFormat;
  seq: number;
  metadata: BrowserFrameMetadata;
  imageBytes: Uint8Array;
}

export type DecodedBrowserFrame = BrowserFrame;

const ALLOWED_METADATA_KEYS = new Set([
  "offsetTop",
  "pageScaleFactor",
  "deviceWidth",
  "deviceHeight",
  "imageWidth",
  "imageHeight",
  "scrollOffsetX",
  "scrollOffsetY",
  "timestamp",
  "streamId",
  "browserInstanceId",
  "browserServiceEpoch",
  "desktopEpoch",
  "documentGeneration",
  "viewportRevision",
  "captureRect",
  "geometrySource",
]);

const ALLOWED_RECT_KEYS = new Set(["x", "y", "width", "height"]);

const U64_DECIMAL_REGEX = /^\d+$/;

function isFiniteNumber(val: unknown): val is number {
  return typeof val === "number" && Number.isFinite(val);
}

function isU64DecimalString(val: unknown): val is string {
  return typeof val === "string" && U64_DECIMAL_REGEX.test(val);
}

function parseJpegDimensions(bytes: Uint8Array): { width: number; height: number } | null {
  if (bytes.length < 4 || bytes[0] !== 0xff || bytes[1] !== 0xd8) {
    return null;
  }
  let offset = 2;
  while (offset < bytes.length) {
    if (bytes[offset] !== 0xff) {
      offset++;
      continue;
    }
    while (offset < bytes.length && bytes[offset] === 0xff) {
      offset++;
    }
    if (offset >= bytes.length) break;
    const marker = bytes[offset++];
    // Standalone markers without length
    if (marker === 0xd8 || marker === 0xd9 || (marker >= 0xd0 && marker <= 0xd7)) {
      continue;
    }
    // Stop at Start of Scan (SOS)
    if (marker === 0xda) break;

    if (offset + 2 > bytes.length) break;
    const len = (bytes[offset] << 8) | bytes[offset + 1];

    // SOF markers: SOF0 (0xC0), SOF1 (0xC1), SOF2 (0xC2), etc.
    const isSof =
      (marker >= 0xc0 && marker <= 0xc3) ||
      (marker >= 0xc5 && marker <= 0xc7) ||
      (marker >= 0xc9 && marker <= 0xcb) ||
      (marker >= 0xcd && marker <= 0xcf);

    if (isSof) {
      if (offset + len <= bytes.length && len >= 7) {
        const height = (bytes[offset + 3] << 8) | bytes[offset + 4];
        const width = (bytes[offset + 5] << 8) | bytes[offset + 6];
        return { width, height };
      }
      break;
    }
    offset += len;
  }
  return null;
}

function parsePngDimensions(bytes: Uint8Array): { width: number; height: number } | null {
  if (bytes.length < 24) return null;
  // Check PNG signature: 89 50 4E 47 0D 0A 1A 0A
  if (
    bytes[0] !== 0x89 ||
    bytes[1] !== 0x50 ||
    bytes[2] !== 0x4e ||
    bytes[3] !== 0x47 ||
    bytes[4] !== 0x0d ||
    bytes[5] !== 0x0a ||
    bytes[6] !== 0x1a ||
    bytes[7] !== 0x0a
  ) {
    return null;
  }
  // Check IHDR length (13) and type "IHDR" (0x49, 0x48, 0x44, 0x52)
  if (
    bytes[12] !== 0x49 ||
    bytes[13] !== 0x48 ||
    bytes[14] !== 0x44 ||
    bytes[15] !== 0x52
  ) {
    return null;
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = view.getUint32(16, false); // Big-Endian
  const height = view.getUint32(20, false);
  return { width, height };
}

export function validateMetadata(raw: unknown): BrowserFrameMetadata {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("Frame metadata must be a JSON object");
  }
  const obj = raw as Record<string, unknown>;

  // Deny unknown fields
  for (const key of Object.keys(obj)) {
    if (!ALLOWED_METADATA_KEYS.has(key)) {
      throw new Error(`Frame metadata contains unknown field: ${key}`);
    }
  }

  // Base fields validation
  if (!isFiniteNumber(obj.offsetTop)) throw new Error("Frame metadata offsetTop must be a finite number");
  if (!isFiniteNumber(obj.pageScaleFactor) || obj.pageScaleFactor <= 0) {
    throw new Error("Frame metadata pageScaleFactor must be a positive finite number");
  }
  if (!isFiniteNumber(obj.deviceWidth) || obj.deviceWidth <= 0) {
    throw new Error("Frame metadata deviceWidth must be a positive finite number");
  }
  if (!isFiniteNumber(obj.deviceHeight) || obj.deviceHeight <= 0) {
    throw new Error("Frame metadata deviceHeight must be a positive finite number");
  }
  if (!isFiniteNumber(obj.imageWidth) || !Number.isInteger(obj.imageWidth) || obj.imageWidth <= 0) {
    throw new Error("Frame metadata imageWidth must be a positive integer");
  }
  if (!isFiniteNumber(obj.imageHeight) || !Number.isInteger(obj.imageHeight) || obj.imageHeight <= 0) {
    throw new Error("Frame metadata imageHeight must be a positive integer");
  }
  if (!isFiniteNumber(obj.scrollOffsetX)) throw new Error("Frame metadata scrollOffsetX must be a finite number");
  if (!isFiniteNumber(obj.scrollOffsetY)) throw new Error("Frame metadata scrollOffsetY must be a finite number");
  if (!isFiniteNumber(obj.timestamp)) throw new Error("Frame metadata timestamp must be a finite number");

  // Edge and pixel checks
  if (obj.imageWidth > MAX_EDGE_PIXELS || obj.imageHeight > MAX_EDGE_PIXELS) {
    throw new Error(`Frame image dimension exceeds 2048px limit (${obj.imageWidth}x${obj.imageHeight})`);
  }
  if (obj.imageWidth * obj.imageHeight > MAX_IMAGE_PIXELS) {
    throw new Error(`Frame image exceeds 4MP limit (${obj.imageWidth * obj.imageHeight} pixels)`);
  }

  // Required Ferryx extension fields (§4.2)
  if (!isFiniteNumber(obj.streamId) || !Number.isInteger(obj.streamId) || obj.streamId <= 0) {
    throw new Error("Required extension streamId must be a positive integer");
  }
  if (typeof obj.browserInstanceId !== "string" || !obj.browserInstanceId) {
    throw new Error("Required extension browserInstanceId must be a non-empty string");
  }
  if (!isU64DecimalString(obj.browserServiceEpoch)) {
    throw new Error("Required extension browserServiceEpoch must be a u64 decimal string");
  }
  if (!isU64DecimalString(obj.desktopEpoch)) {
    throw new Error("Required extension desktopEpoch must be a u64 decimal string");
  }
  if (!isU64DecimalString(obj.documentGeneration)) {
    throw new Error("Required extension documentGeneration must be a u64 decimal string");
  }
  if (!isU64DecimalString(obj.viewportRevision)) {
    throw new Error("Required extension viewportRevision must be a u64 decimal string");
  }
  if (obj.geometrySource !== "wkSnapshot") {
    throw new Error("Required extension geometrySource must be 'wkSnapshot'");
  }

  // captureRect validation
  if (!obj.captureRect || typeof obj.captureRect !== "object" || Array.isArray(obj.captureRect)) {
    throw new Error("Required extension captureRect must be an object");
  }
  const rect = obj.captureRect as Record<string, unknown>;
  for (const k of Object.keys(rect)) {
    if (!ALLOWED_RECT_KEYS.has(k)) {
      throw new Error(`captureRect contains unknown field: ${k}`);
    }
  }
  if (!isFiniteNumber(rect.x) || !isFiniteNumber(rect.y) || !isFiniteNumber(rect.width) || !isFiniteNumber(rect.height)) {
    throw new Error("captureRect dimensions must be finite numbers");
  }

  return {
    offsetTop: obj.offsetTop,
    pageScaleFactor: obj.pageScaleFactor,
    deviceWidth: obj.deviceWidth,
    deviceHeight: obj.deviceHeight,
    imageWidth: obj.imageWidth,
    imageHeight: obj.imageHeight,
    scrollOffsetX: obj.scrollOffsetX,
    scrollOffsetY: obj.scrollOffsetY,
    timestamp: obj.timestamp,
    streamId: obj.streamId,
    browserInstanceId: obj.browserInstanceId,
    browserServiceEpoch: obj.browserServiceEpoch,
    desktopEpoch: obj.desktopEpoch,
    documentGeneration: obj.documentGeneration,
    viewportRevision: obj.viewportRevision,
    captureRect: {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    },
    geometrySource: "wkSnapshot",
  };
}

export function decodeFrame(input: ArrayBuffer | Uint8Array): BrowserFrame {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (bytes.byteLength < 16) {
    throw new Error(`Frame buffer too short: expected at least 16 bytes, got ${bytes.byteLength}`);
  }
  if (bytes.byteLength > MAX_FRAME_BYTES) {
    throw new Error(`Frame buffer exceeds 2MiB limit: got ${bytes.byteLength} bytes`);
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const kind = view.getUint8(0);
  const version = view.getUint8(1);
  const opcode = view.getUint8(2);
  const formatCode = view.getUint8(3);
  const seq = view.getUint32(4, true); // Little-Endian
  const metadataByteLength = view.getUint32(8, true); // Little-Endian
  const reserved = view.getUint32(12, true); // Little-Endian

  if (kind !== PROTOCOL_KIND) {
    throw new Error(`Invalid kind: expected 0x62, got 0x${kind.toString(16)}`);
  }
  if (version !== PROTOCOL_VERSION) {
    throw new Error(`Invalid version: expected 1, got ${version}`);
  }
  if (opcode !== OPCODE_FRAME) {
    throw new Error(`Invalid opcode: expected 1 (Frame), got ${opcode}`);
  }
  if (formatCode !== FORMAT_JPEG && formatCode !== FORMAT_PNG) {
    throw new Error(`Invalid format: expected 1 (jpeg) or 2 (png), got ${formatCode}`);
  }
  if (reserved !== 0) {
    throw new Error(`Reserved field must be 0, got ${reserved}`);
  }

  if (metadataByteLength > MAX_METADATA_BYTES) {
    throw new Error(`Metadata length exceeds 4KiB: got ${metadataByteLength} bytes`);
  }

  const minTotal = 16 + metadataByteLength;
  if (bytes.byteLength < minTotal) {
    throw new Error(`Truncated frame: expected at least ${minTotal} bytes, got ${bytes.byteLength}`);
  }

  const imageByteLength = bytes.byteLength - minTotal;
  if (imageByteLength === 0) {
    throw new Error("Frame contains empty image payload");
  }

  // Parse metadata
  const metaBytes = bytes.subarray(16, 16 + metadataByteLength);
  let rawMeta: unknown;
  try {
    const jsonStr = new TextDecoder("utf-8", { fatal: true }).decode(metaBytes);
    rawMeta = JSON.parse(jsonStr);
  } catch (err) {
    throw new Error(`Failed to parse frame metadata JSON: ${err instanceof Error ? err.message : String(err)}`);
  }

  const metadata = validateMetadata(rawMeta);

  const imageBytes = bytes.subarray(16 + metadataByteLength);
  const format: BrowserImageFormat = formatCode === FORMAT_JPEG ? "jpeg" : "png";

  // Validate image header & dimensions against metadata
  if (format === "jpeg") {
    if (imageBytes.length < 2 || imageBytes[0] !== 0xff || imageBytes[1] !== 0xd8) {
      throw new Error("Invalid JPEG magic header: expected SOI marker 0xFF 0xD8");
    }
    const dims = parseJpegDimensions(imageBytes);
    if (dims && (dims.width !== metadata.imageWidth || dims.height !== metadata.imageHeight)) {
      throw new Error(
        `JPEG image dimension mismatch: header has ${dims.width}x${dims.height}, metadata specifies ${metadata.imageWidth}x${metadata.imageHeight}`,
      );
    }
  } else {
    // PNG
    if (imageBytes.length < 8) {
      throw new Error("Invalid PNG signature: image payload too short");
    }
    const dims = parsePngDimensions(imageBytes);
    if (!dims) {
      throw new Error("Invalid PNG signature or IHDR chunk");
    }
    if (dims.width !== metadata.imageWidth || dims.height !== metadata.imageHeight) {
      throw new Error(
        `PNG image dimension mismatch: header has ${dims.width}x${dims.height}, metadata specifies ${metadata.imageWidth}x${metadata.imageHeight}`,
      );
    }
  }

  return {
    format,
    seq,
    metadata,
    imageBytes,
  };
}

export function encodeFrame(frame: BrowserFrame): Uint8Array {
  const metadata = validateMetadata(frame.metadata);
  const metaJson = JSON.stringify(metadata);
  const metaBytes = new TextEncoder().encode(metaJson);

  if (metaBytes.byteLength > MAX_METADATA_BYTES) {
    throw new Error(`Metadata length exceeds 4KiB limit: ${metaBytes.byteLength} bytes`);
  }
  if (frame.imageBytes.byteLength === 0) {
    throw new Error("Image payload cannot be empty");
  }

  const totalLength = 16 + metaBytes.byteLength + frame.imageBytes.byteLength;
  if (totalLength > MAX_FRAME_BYTES) {
    throw new Error(`Total frame length exceeds 2MiB limit: ${totalLength} bytes`);
  }

  const output = new Uint8Array(totalLength);
  const view = new DataView(output.buffer);

  view.setUint8(0, PROTOCOL_KIND);
  view.setUint8(1, PROTOCOL_VERSION);
  view.setUint8(2, OPCODE_FRAME);
  view.setUint8(3, frame.format === "jpeg" ? FORMAT_JPEG : FORMAT_PNG);
  view.setUint32(4, frame.seq, true);
  view.setUint32(8, metaBytes.byteLength, true);
  view.setUint32(12, 0, true);

  output.set(metaBytes, 16);
  output.set(frame.imageBytes, 16 + metaBytes.byteLength);

  return output;
}

// ---------------------------------------------------------------------------
// JSON Message Protocol Types (§4.1)
// ---------------------------------------------------------------------------

export interface BrowserSubscribeOptions {
  format: BrowserImageFormat;
  quality?: number;
  intervalMs?: number;
  maxEdge?: number;
}

export interface BrowserHelloMessage {
  type: "browserHello";
  browserId: string;
  browserInstanceId: string;
  browserServiceEpoch: string; // u64 decimal string
  desktopEpoch: string; // u64 decimal string
  protocolVersion: number;
  supportedCommands: string[];
  capabilities?: Record<string, unknown>;
}

export interface BrowserSubscribeMessage {
  type: "browserSubscribe";
  requestId: string;
  viewerInstanceId: string;
  options: BrowserSubscribeOptions;
}

export interface BrowserSubscribedMessage {
  type: "browserSubscribed";
  requestId: string;
  subscriptionId: string;
  streamId: number;
  browserId: string;
  browserInstanceId: string;
  browserServiceEpoch: string; // u64 decimal string
  desktopEpoch: string; // u64 decimal string
  documentGeneration: string; // u64 decimal string
  options: BrowserSubscribeOptions;
}

export interface BrowserFrameAckMessage {
  type: "browserFrameAck";
  streamId: number;
  seq: number;
}

export interface BrowserHeartbeatMessage {
  type: "browserHeartbeat";
  requestId?: string;
  leaseEpoch?: string;
  subscriptionId?: string;
}

export interface BrowserPongMessage {
  type: "browserPong";
  requestId?: string;
  timestamp?: number;
}

export interface BrowserDriverClaimMessage {
  type: "browserDriverClaim";
  requestId: string;
  subscriptionId: string;
  browserId: string;
}

export interface BrowserDriverClaimedMessage {
  type: "browserDriverClaimed";
  requestId: string;
  leaseEpoch: string;
  expiresAt: number;
}

export interface BrowserDriverChangedMessage {
  type: "browserDriverChanged";
  leaseEpoch: string | null;
  isDriver: boolean;
}

export interface BrowserDriverReleaseMessage {
  type: "browserDriverRelease";
  requestId: string;
  leaseEpoch: string;
}

export interface BrowserDriverReleasedMessage {
  type: "browserDriverReleased";
  requestId: string;
  leaseEpoch?: string;
}

export interface ServerBrowserDriverRevoked {
  type: "browserDriverRevoked";
  reason?: string;
  leaseEpoch: string;
}

export type BrowserDriverRevokedMessage = ServerBrowserDriverRevoked;

export type BrowserCommandName =
  | "navigate"
  | "back"
  | "forward"
  | "reload"
  | "click"
  | "fill"
  | "keypress"
  | "eval"
  | "wait"
  | "getState"
  | "snapshot";

export interface BrowserCommandMessage {
  type: "browserCommand";
  requestId: string;
  requestSeq: string; // u64 decimal string
  browserId: string;
  leaseEpoch: string;
  browserInstanceId: string;
  desktopEpoch: string; // u64 decimal string
  documentGeneration: string; // u64 decimal string
  command: BrowserCommandName | string;
  params?: Record<string, unknown>;
}

export interface BrowserResultMessage {
  type: "browserResult";
  requestId: string;
  result?: unknown;
}

export interface BrowserErrorMessage {
  type: "browserError";
  requestId?: string;
  code: string;
  message: string;
  retryable: boolean;
  retryAfterMs?: number;
}

export interface BrowserStateMessage {
  type: "browserState";
  browserId: string;
  url?: string;
  title?: string;
  documentGeneration: string; // u64 decimal string
  viewportRevision: string; // u64 decimal string
  loading: boolean;
  paused: boolean;
  pauseReason?: string | null;
  snapshotId?: string;
  mapRevision?: string;
}

export interface BrowserUnsubscribeMessage {
  type: "browserUnsubscribe";
  requestId: string;
  subscriptionId: string;
}

export interface BrowserUnsubscribedMessage {
  type: "browserUnsubscribed";
  requestId: string;
  subscriptionId: string;
}

export interface BrowserSnapshotClientMessage {
  type: "browserSnapshot";
  requestId: string;
  browserId: string;
}

export interface BrowserSnapshotServerMessage {
  type: "browserSnapshot";
  requestId: string;
  snapshotId: string;
  mapRevision: string; // u64 decimal string
  root?: unknown;
  elements?: unknown[];
}

export interface BrowserPauseMessage {
  type: "browserPause";
  browserId: string;
  streamId: number;
}

export interface BrowserResumeMessage {
  type: "browserResume";
  browserId: string;
  streamId: number;
}

export type ClientBrowserSnapshot = BrowserSnapshotClientMessage;
export type ServerBrowserSnapshot = BrowserSnapshotServerMessage;
export type BrowserSnapshotMessage = BrowserSnapshotServerMessage;
export type ClientBrowserPause = BrowserPauseMessage;
export type ClientBrowserResume = BrowserResumeMessage;

export type ServerMessage =
  | BrowserHelloMessage
  | BrowserSubscribedMessage
  | BrowserPongMessage
  | BrowserDriverClaimedMessage
  | BrowserDriverChangedMessage
  | BrowserDriverReleasedMessage
  | ServerBrowserDriverRevoked
  | BrowserResultMessage
  | BrowserErrorMessage
  | BrowserStateMessage
  | BrowserUnsubscribedMessage
  | BrowserSnapshotServerMessage;

export type ClientMessage =
  | BrowserSubscribeMessage
  | BrowserFrameAckMessage
  | BrowserHeartbeatMessage
  | BrowserDriverClaimMessage
  | BrowserDriverReleaseMessage
  | BrowserCommandMessage
  | BrowserUnsubscribeMessage
  | BrowserSnapshotClientMessage
  | BrowserPauseMessage
  | BrowserResumeMessage;

const SERVER_MESSAGE_ALLOWED_KEYS: Record<string, Set<string>> = {
  browserHello: new Set([
    "type",
    "browserId",
    "browserInstanceId",
    "browserServiceEpoch",
    "desktopEpoch",
    "protocolVersion",
    "supportedCommands",
    "capabilities",
  ]),
  browserSubscribed: new Set([
    "type",
    "requestId",
    "subscriptionId",
    "streamId",
    "browserId",
    "browserInstanceId",
    "browserServiceEpoch",
    "desktopEpoch",
    "documentGeneration",
    "options",
  ]),
  browserPong: new Set(["type", "requestId", "timestamp"]),
  browserDriverClaimed: new Set(["type", "requestId", "leaseEpoch", "expiresAt"]),
  browserDriverChanged: new Set(["type", "leaseEpoch", "isDriver"]),
  browserDriverReleased: new Set(["type", "requestId", "leaseEpoch"]),
  browserDriverRevoked: new Set(["type", "reason", "leaseEpoch"]),
  browserResult: new Set(["type", "requestId", "result"]),
  browserError: new Set(["type", "requestId", "code", "message", "retryable", "retryAfterMs"]),
  browserState: new Set([
    "type",
    "browserId",
    "url",
    "title",
    "documentGeneration",
    "viewportRevision",
    "loading",
    "paused",
    "pauseReason",
    "snapshotId",
    "mapRevision",
  ]),
  browserUnsubscribed: new Set(["type", "requestId", "subscriptionId"]),
  browserSnapshot: new Set([
    "type",
    "requestId",
    "snapshotId",
    "mapRevision",
    "root",
    "elements",
  ]),
};

const OPTIONS_ALLOWED_KEYS = new Set(["format", "quality", "intervalMs", "maxEdge"]);

function validateOptions(options: unknown): BrowserSubscribeOptions {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new Error("Subscribe options must be an object");
  }
  const obj = options as Record<string, unknown>;
  for (const k of Object.keys(obj)) {
    if (!OPTIONS_ALLOWED_KEYS.has(k)) {
      throw new Error(`Subscribe options contains unknown field: ${k}`);
    }
  }
  if (obj.format !== "jpeg" && obj.format !== "png") {
    throw new Error("Subscribe option format must be 'jpeg' or 'png'");
  }
  if (obj.quality !== undefined && (!isFiniteNumber(obj.quality) || obj.quality < 1 || obj.quality > 100)) {
    throw new Error("Subscribe option quality must be a finite number 1..100");
  }
  if (obj.intervalMs !== undefined && (!isFiniteNumber(obj.intervalMs) || obj.intervalMs <= 0)) {
    throw new Error("Subscribe option intervalMs must be a positive finite number");
  }
  if (obj.maxEdge !== undefined && (!isFiniteNumber(obj.maxEdge) || obj.maxEdge <= 0)) {
    throw new Error("Subscribe option maxEdge must be a positive finite number");
  }
  return obj as unknown as BrowserSubscribeOptions;
}

export function parseServerMessage(jsonString: string): ServerMessage {
  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonString);
  } catch (err) {
    throw new Error(`Malformed JSON in server message: ${err instanceof Error ? err.message : String(err)}`);
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("Server message must be a JSON object");
  }

  const obj = parsed as Record<string, unknown>;
  const type = obj.type;
  if (typeof type !== "string") {
    throw new Error("Server message missing type discriminator");
  }

  const allowedKeys = SERVER_MESSAGE_ALLOWED_KEYS[type];
  if (!allowedKeys) {
    throw new Error(`Unrecognized server message type: ${type}`);
  }

  // Deny unknown fields on the message object
  for (const key of Object.keys(obj)) {
    if (!allowedKeys.has(key)) {
      throw new Error(`Server message '${type}' contains unknown field: ${key}`);
    }
  }

  // Specific message validation
  switch (type) {
    case "browserHello": {
      if (typeof obj.browserId !== "string" || !obj.browserId) throw new Error("browserHello: invalid browserId");
      if (typeof obj.browserInstanceId !== "string" || !obj.browserInstanceId) {
        throw new Error("browserHello: invalid browserInstanceId");
      }
      if (!isU64DecimalString(obj.browserServiceEpoch)) {
        throw new Error("browserHello: browserServiceEpoch must be a u64 decimal string");
      }
      if (!isU64DecimalString(obj.desktopEpoch)) {
        throw new Error("browserHello: desktopEpoch must be a u64 decimal string");
      }
      if (!isFiniteNumber(obj.protocolVersion)) {
        throw new Error("browserHello: protocolVersion must be a finite number");
      }
      if (!Array.isArray(obj.supportedCommands)) {
        throw new Error("browserHello: supportedCommands must be an array");
      }
      return obj as unknown as BrowserHelloMessage;
    }
    case "browserSubscribed": {
      if (typeof obj.requestId !== "string") throw new Error("browserSubscribed: invalid requestId");
      if (typeof obj.subscriptionId !== "string") throw new Error("browserSubscribed: invalid subscriptionId");
      if (!isFiniteNumber(obj.streamId)) throw new Error("browserSubscribed: streamId must be a number");
      if (typeof obj.browserId !== "string") throw new Error("browserSubscribed: invalid browserId");
      if (typeof obj.browserInstanceId !== "string") throw new Error("browserSubscribed: invalid browserInstanceId");
      if (!isU64DecimalString(obj.browserServiceEpoch)) {
        throw new Error("browserSubscribed: browserServiceEpoch must be a u64 decimal string");
      }
      if (!isU64DecimalString(obj.desktopEpoch)) {
        throw new Error("browserSubscribed: desktopEpoch must be a u64 decimal string");
      }
      if (!isU64DecimalString(obj.documentGeneration)) {
        throw new Error("browserSubscribed: documentGeneration must be a u64 decimal string");
      }
      validateOptions(obj.options);
      return obj as unknown as BrowserSubscribedMessage;
    }
    case "browserPong": {
      if (obj.requestId !== undefined && typeof obj.requestId !== "string") {
        throw new Error("browserPong: requestId must be a string");
      }
      if (obj.timestamp !== undefined && !isFiniteNumber(obj.timestamp)) {
        throw new Error("browserPong: timestamp must be a finite number");
      }
      return obj as unknown as BrowserPongMessage;
    }
    case "browserDriverClaimed": {
      if (typeof obj.requestId !== "string") throw new Error("browserDriverClaimed: invalid requestId");
      if (typeof obj.leaseEpoch !== "string") throw new Error("browserDriverClaimed: invalid leaseEpoch");
      if (!isFiniteNumber(obj.expiresAt)) throw new Error("browserDriverClaimed: expiresAt must be a number");
      return obj as unknown as BrowserDriverClaimedMessage;
    }
    case "browserDriverChanged": {
      if (obj.leaseEpoch !== null && typeof obj.leaseEpoch !== "string") {
        throw new Error("browserDriverChanged: leaseEpoch must be string or null");
      }
      if (typeof obj.isDriver !== "boolean") throw new Error("browserDriverChanged: isDriver must be boolean");
      return obj as unknown as BrowserDriverChangedMessage;
    }
    case "browserDriverReleased": {
      if (typeof obj.requestId !== "string") throw new Error("browserDriverReleased: invalid requestId");
      return obj as unknown as BrowserDriverReleasedMessage;
    }
    case "browserDriverRevoked": {
      if (typeof obj.leaseEpoch !== "string") throw new Error("browserDriverRevoked: invalid leaseEpoch");
      if (obj.reason !== undefined && typeof obj.reason !== "string") {
        throw new Error("browserDriverRevoked: reason must be a string");
      }
      return obj as unknown as ServerBrowserDriverRevoked;
    }
    case "browserResult": {
      if (typeof obj.requestId !== "string") throw new Error("browserResult: invalid requestId");
      return obj as unknown as BrowserResultMessage;
    }
    case "browserError": {
      if (typeof obj.code !== "string") throw new Error("browserError: code must be a string");
      if (typeof obj.message !== "string") throw new Error("browserError: message must be a string");
      if (typeof obj.retryable !== "boolean") throw new Error("browserError: retryable must be a boolean");
      return obj as unknown as BrowserErrorMessage;
    }
    case "browserState": {
      if (typeof obj.browserId !== "string") throw new Error("browserState: invalid browserId");
      if (!isU64DecimalString(obj.documentGeneration)) {
        throw new Error("browserState: documentGeneration must be a u64 decimal string");
      }
      if (!isU64DecimalString(obj.viewportRevision)) {
        throw new Error("browserState: viewportRevision must be a u64 decimal string");
      }
      if (typeof obj.loading !== "boolean") throw new Error("browserState: loading must be boolean");
      if (typeof obj.paused !== "boolean") throw new Error("browserState: paused must be boolean");
      if (obj.snapshotId !== undefined && typeof obj.snapshotId !== "string") {
        throw new Error("browserState: snapshotId must be a string");
      }
      if (obj.mapRevision !== undefined && typeof obj.mapRevision !== "string") {
        throw new Error("browserState: mapRevision must be a string");
      }
      return obj as unknown as BrowserStateMessage;
    }
    case "browserUnsubscribed": {
      if (typeof obj.requestId !== "string") throw new Error("browserUnsubscribed: invalid requestId");
      if (typeof obj.subscriptionId !== "string") throw new Error("browserUnsubscribed: invalid subscriptionId");
      return obj as unknown as BrowserUnsubscribedMessage;
    }
    case "browserSnapshot": {
      if (typeof obj.requestId !== "string") throw new Error("browserSnapshot: invalid requestId");
      if (typeof obj.snapshotId !== "string") throw new Error("browserSnapshot: invalid snapshotId");
      if (typeof obj.mapRevision === "number" && Number.isFinite(obj.mapRevision) && obj.mapRevision >= 0) {
        obj.mapRevision = Math.trunc(obj.mapRevision).toString();
      }
      if (!isU64DecimalString(obj.mapRevision)) {
        throw new Error("browserSnapshot: mapRevision must be a u64 decimal string");
      }
      if (obj.elements !== undefined && !Array.isArray(obj.elements)) {
        throw new Error("browserSnapshot: elements must be an array");
      }
      return obj as unknown as BrowserSnapshotServerMessage;
    }
    default:
      throw new Error(`Unhandled message type: ${type}`);
  }
}

export function reconcileMapRevision(value: unknown): string | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (U64_DECIMAL_REGEX.test(trimmed)) {
      return trimmed;
    }
    return trimmed;
  }
  if (typeof value === "number" && Number.isFinite(value) && value >= 0) {
    return Math.trunc(value).toString();
  }
  if (typeof value === "bigint" && value >= 0n) {
    return value.toString();
  }
  return undefined;
}

export function serializeClientMessage(message: ClientMessage): string {
  // Validate client message doesn't have undefined fields and follows camelCase
  if (!message || typeof message !== "object") {
    throw new Error("Client message must be an object");
  }
  return JSON.stringify(message);
}

// ---------------------------------------------------------------------------
// R4-8: point-click fence (§4.5)
// ---------------------------------------------------------------------------

/** The exact frame currently painted in the viewport. Point clicks require one. */
export interface DisplayedFrameRef {
  seq: number;
  metadata: BrowserFrameMetadata;
}

export interface PointClickInput {
  u: number;
  v: number;
  frame: DisplayedFrameRef | null | undefined;
  /** Highest frame sequence already confirmed as presented on this stream. */
  lastAckedSeq?: number | null;
}

export interface PointClickParams {
  u: number;
  v: number;
  streamId: number;
  sequenceNumber: number;
  documentGeneration: string;
  viewportRevision: string;
  browserInstanceId: string;
  captureRect: BrowserCaptureRect;
  geometrySource: "wkSnapshot";
  x: number;
  y: number;
}

/**
 * Builds fenced point-click params from the committed displayed frame.
 *
 * The displayed-frame metadata is REQUIRED: without it the daemon cannot tell
 * which pixels the user actually clicked. Frames older than the last frame the
 * viewer acknowledged are rejected, and the click point is derived from the real
 * captureRect geometry rather than a guessed viewport.
 */
export function buildPointClickParams(input: PointClickInput): PointClickParams {
  const { u, v, frame } = input;
  if (!frame || !frame.metadata) {
    throw new Error("Point click requires a committed displayed frame");
  }
  if (!isFiniteNumber(u) || !isFiniteNumber(v) || u < 0 || u > 1 || v < 0 || v > 1) {
    throw new Error("Point click coordinates must be finite and within [0, 1]");
  }
  if (!isFiniteNumber(frame.seq) || !Number.isInteger(frame.seq) || frame.seq < 0) {
    throw new Error("Point click requires a displayed frame with an integer sequence");
  }

  const lastAckedSeq = input.lastAckedSeq;
  if (isFiniteNumber(lastAckedSeq) && frame.seq < lastAckedSeq) {
    throw new Error(
      `Point click references a stale frame: seq ${frame.seq} precedes acknowledged seq ${lastAckedSeq}`,
    );
  }

  const meta = frame.metadata;
  const rect = meta.captureRect;
  if (!rect || rect.width <= 0 || rect.height <= 0) {
    throw new Error("Point click requires displayed frame capture geometry");
  }

  return {
    u,
    v,
    streamId: meta.streamId,
    sequenceNumber: frame.seq,
    documentGeneration: meta.documentGeneration,
    viewportRevision: meta.viewportRevision,
    browserInstanceId: meta.browserInstanceId,
    captureRect: rect,
    geometrySource: "wkSnapshot",
    x: rect.x + u * rect.width,
    y: rect.y + v * rect.height,
  };
}

// ---------------------------------------------------------------------------
// R4-5: authoritative sharing-state publication (§6.1)
// ---------------------------------------------------------------------------

/** Window event channel carrying authoritative daemon sharing state. */
export const REMOTE_BROWSER_SHARING_EVENT = "remote-browser-sharing";
/** Tauri event name emitted by the desktop shell for the same state. */
export const REMOTE_BROWSER_SHARING_TAURI_EVENT = "browser-remote-sharing";

export type SharingDriverStatus = "idle" | "viewing" | "driving";

export interface RemoteBrowserSharingState {
  isSharing: boolean;
  activeSessionsCount: number;
  driverStatus: SharingDriverStatus;
  driverDeviceId: string | null;
}

function isSharingDriverStatus(value: unknown): value is SharingDriverStatus {
  return value === "idle" || value === "viewing" || value === "driving";
}

/**
 * Normalizes an authoritative sharing payload from the daemon/desktop shell.
 * Returns `null` for payloads that carry no sharing signal at all.
 */
export function parseSharingState(raw: unknown): RemoteBrowserSharingState | null {
  if (typeof raw === "boolean") {
    return {
      isSharing: raw,
      activeSessionsCount: 0,
      driverStatus: raw ? "viewing" : "idle",
      driverDeviceId: null,
    };
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return null;
  }

  const obj = raw as Record<string, unknown>;
  const activeSessionsCount =
    isFiniteNumber(obj.activeSessionsCount) && obj.activeSessionsCount >= 0
      ? Math.trunc(obj.activeSessionsCount)
      : 0;
  const isSharing =
    typeof obj.isSharing === "boolean"
      ? obj.isSharing
      : typeof obj.active === "boolean"
        ? obj.active
        : activeSessionsCount > 0;

  const driverStatus: SharingDriverStatus = isSharingDriverStatus(obj.driverStatus)
    ? obj.driverStatus
    : isSharing
      ? "viewing"
      : "idle";

  return {
    isSharing,
    activeSessionsCount,
    driverStatus,
    driverDeviceId: typeof obj.driverDeviceId === "string" ? obj.driverDeviceId : null,
  };
}

/** Publishes authoritative sharing state to every in-page listener. */
export function emitSharingState(state: RemoteBrowserSharingState): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent(REMOTE_BROWSER_SHARING_EVENT, { detail: { ...state } }),
  );
}
