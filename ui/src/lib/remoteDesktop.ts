import { useEffect, useState } from "react";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toIpcError } from "./tauri";
import type { StructuredIpcError } from "./types";

/** Remote desktop (ferryx-rd) bridge. Every command exists only in builds with the
 * `remote-desktop` feature except `cmd_rd_supported`, which callers check first. */

export const RD_FRAME_KIND_VIDEO = 0x01;
export const RD_FRAME_KIND_STATUS = 0x02;
export const RD_FRAME_KIND_CURSOR = 0x03;
/**
 * Native presentation notification. The canonical wire format is EXACTLY 9 bytes, little endian,
 * with no JSON and no image payload:
 *
 * ```text
 * byte 0        kind = 0x04
 * bytes 1..4    u32 LE width
 * bytes 5..8    u32 LE height
 * ```
 *
 * It reuses the `RD_FRAME_KIND_VIDEO` header convention so the host emits the same u32 pair.
 * Wave 3 Todo 7 must write exactly these 9 bytes; anything shorter is rejected by
 * `parseRdFrameMessage`, and trailing bytes are ignored.
 */
export const RD_FRAME_KIND_NATIVE_PRESENTED = 0x04;
/** kind(1) + width(4) + height(4) + frameId(8). */
export const RD_VIDEO_HEADER_BYTES = 17;

export type RdConnectionState = "connecting" | "connected" | "reconnecting" | "ended";

export type RdVideoFrame = {
  kind: "video";
  width: number;
  height: number;
  frameId: number;
  jpeg: Uint8Array;
};

export type RdStatusFrame = {
  kind: "status";
  state: RdConnectionState;
  reason: string | null;
};

export type RdCursorFrame = {
  kind: "cursor";
  x: number;
  y: number;
  cursorType: number;
  visible: boolean;
  shapeId: number | null;
  rgbaBase64: string | null;
  width: number | null;
  height: number | null;
  hotspotX: number | null;
  hotspotY: number | null;
};

export type RdNativePresentedFrame = { kind: "nativePresented"; width: number; height: number };

export type RdFrameMessage = RdVideoFrame | RdStatusFrame | RdCursorFrame | RdNativePresentedFrame;

export type RdPlayoutMode = "immediate" | "latest_wins" | "adaptive";
export type RdScaleMode = "fit" | "native100" | "scale75" | "scale50";
export type RdLatencyStats = { frames: number; p50Us: number; p95Us: number; p99Us: number; maxUs: number };
export type RdSessionStats = {
  rdSessionId: string;
  targetFps: number;
  presentFps: number;
  droppedFrames: number;
  codec: string;
  hwAccel: string;
  presentBackend: string;
  transportMode: string;
  playoutMode: string;
  frameWidth: number;
  frameHeight: number;
  scalePermille: number;
  audioActive: boolean;
  audioUnderruns: number;
  audioDrops: number;
  decodeUs: RdLatencyStats | null;
  presentUs: RdLatencyStats | null;
  totalClientUs: RdLatencyStats | null;
  udpReceiveBps: number | null;
  packetLossRatio: number | null;
  abrTargetBitrate: number | null;
  hostEncodeUs: RdLatencyStats | null;
  networkUs: RdLatencyStats | null;
  captureToPresentUs: RdLatencyStats | null;
  inputToDrawSubmitUs: RdLatencyStats | null;
  audioVideoSourceOffsetUs: number | null;
};
export type RdDisplayRefreshInfo = { refreshHz: number; millihertz: number; monitorName: string | null; source: string };
export type RdDisplayRefreshChange = { windowLabel: string; refreshHz: number; millihertz: number };
export type RdConnectOptions = {
  playoutMode?: RdPlayoutMode;
  scaleMode?: RdScaleMode;
  viewportWidth?: number;
  viewportHeight?: number;
  audioMuted?: boolean;
  audioVolume?: number;
};
export type RdStreamOptionsUpdate = {
  playoutMode?: RdPlayoutMode;
  scaleMode?: RdScaleMode;
  viewportWidth?: number;
  viewportHeight?: number;
  codec?: "auto" | "hevc" | "h264";
};
export type RdLogicalBounds = { x: number; y: number; width: number; height: number; scaleFactor: number };
export type RdSurfaceAttachResponse = { backend: "wgpu" | "canvas2d"; reason: string | null };

export type RdInputEvent =
  | { kind: "mouseMove"; x: number; y: number }
  | { kind: "relativeMove"; dx: number; dy: number }
  | { kind: "mouseButton"; button: 0 | 1 | 2; down: boolean }
  | { kind: "wheel"; dx: number; dy: number }
  | { kind: "key"; code: string; down: boolean };

export type RdHostSession = { sessionId: string; clientName: string };
export type RdHostPending = { requestId: string; clientName: string };

export type RdHostState = {
  enabled: boolean;
  running: boolean;
  allowInput: boolean;
  allowClipboard: boolean;
  sessions: RdHostSession[];
  pending: RdHostPending[];
  error: string | null;
};

export type RdHostSettings = { enabled: boolean; allowInput: boolean; allowClipboard: boolean };

export type RdHostConsentRequest = { requestId: string; clientName: string };

export const RD_HOST_CONSENT_EVENT = "rd-host-consent";

const CONNECTION_STATES: readonly RdConnectionState[] = ["connecting", "connected", "reconnecting", "ended"];

function toBytes(frame: ArrayBuffer | Uint8Array | number[]): Uint8Array {
  if (frame instanceof Uint8Array) return frame;
  if (Array.isArray(frame)) return Uint8Array.from(frame);
  return new Uint8Array(frame);
}

/** Decodes one `frames` Channel message (little endian, see the integration contract). */
export function parseRdFrameMessage(frame: ArrayBuffer | Uint8Array | number[]): RdFrameMessage {
  const bytes = toBytes(frame);
  if (bytes.byteLength < 1) throw new Error("remote desktop frame is empty");
  const kind = bytes[0];
  if (kind === RD_FRAME_KIND_VIDEO) {
    if (bytes.byteLength < RD_VIDEO_HEADER_BYTES) {
      throw new Error(`remote desktop video frame is too short: ${bytes.byteLength}`);
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return {
      kind: "video",
      width: view.getUint32(1, true),
      height: view.getUint32(5, true),
      frameId: Number(view.getBigUint64(9, true)),
      jpeg: bytes.subarray(RD_VIDEO_HEADER_BYTES),
    };
  }
  if (kind === RD_FRAME_KIND_STATUS) {
    const parsed: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(1)));
    if (!parsed || typeof parsed !== "object") throw new Error("remote desktop status frame is not an object");
    const record = parsed as { state?: unknown; reason?: unknown };
    const state = CONNECTION_STATES.find((candidate) => candidate === record.state);
    if (!state) throw new Error(`unknown remote desktop state: ${String(record.state)}`);
    return { kind: "status", state, reason: typeof record.reason === "string" ? record.reason : null };
  }
  if (kind === RD_FRAME_KIND_CURSOR) {
    const parsed: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(1)));
    if (!parsed || typeof parsed !== "object") throw new Error("remote desktop cursor frame is not an object");
    const record = parsed as Record<string, unknown>;
    const x = record.x;
    const y = record.y;
    const cursorType = record.cursorType;
    const visible = record.visible;
    if (typeof x !== "number" || !Number.isFinite(x)
      || typeof y !== "number" || !Number.isFinite(y)
      || typeof cursorType !== "number" || typeof visible !== "boolean") {
      throw new Error("remote desktop cursor frame has invalid fields");
    }
    const shapeId = record.shapeId;
    const rgbaBase64 = record.rgbaBase64;
    const width = record.width;
    const height = record.height;
    const hotspotX = record.hotspotX;
    const hotspotY = record.hotspotY;
    return {
      kind: "cursor",
      x,
      y,
      cursorType,
      visible,
      shapeId: typeof shapeId === "number" ? shapeId : null,
      rgbaBase64: typeof rgbaBase64 === "string" ? rgbaBase64 : null,
      width: typeof width === "number" ? width : null,
      height: typeof height === "number" ? height : null,
      hotspotX: typeof hotspotX === "number" ? hotspotX : null,
      hotspotY: typeof hotspotY === "number" ? hotspotY : null,
    };
  }
  if (kind === RD_FRAME_KIND_NATIVE_PRESENTED) {
    if (bytes.byteLength < 9) throw new Error(`remote desktop native-presented frame is too short: ${bytes.byteLength}`);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return { kind: "nativePresented", width: view.getUint32(1, true), height: view.getUint32(5, true) };
  }
  throw new Error(`unknown remote desktop frame kind: ${String(kind)}`);
}

export type FramePoint = { x: number; y: number };

/**
 * Maps a client point to normalized 0..1 coordinates of a frame drawn aspect-fit (centered,
 * letterboxed) inside `rect`. Points in the letterbox bars return null.
 */
export function mapClientPointToFrame(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  frameWidth: number,
  frameHeight: number,
): FramePoint | null {
  if (rect.width <= 0 || rect.height <= 0 || frameWidth <= 0 || frameHeight <= 0) return null;
  const scale = Math.min(rect.width / frameWidth, rect.height / frameHeight);
  const drawnWidth = frameWidth * scale;
  const drawnHeight = frameHeight * scale;
  const offsetX = rect.left + (rect.width - drawnWidth) / 2;
  const offsetY = rect.top + (rect.height - drawnHeight) / 2;
  const x = (clientX - offsetX) / drawnWidth;
  const y = (clientY - offsetY) / drawnHeight;
  if (x < 0 || x > 1 || y < 0 || y > 1) return null;
  return { x, y };
}

/** DOM `MouseEvent.button` (0 left, 1 middle, 2 right) to the RD button id; others are ignored. */
export function rdMouseButton(button: number): 0 | 1 | 2 | null {
  return button === 0 || button === 1 || button === 2 ? button : null;
}

const WHEEL_LINE_PX = 16;

/** Converts a WheelEvent delta to pixels regardless of its deltaMode. */
export function wheelDeltaPixels(delta: number, deltaMode: number, pagePx: number): number {
  if (deltaMode === 1) return delta * WHEEL_LINE_PX;
  if (deltaMode === 2) return delta * pagePx;
  return delta;
}

async function rdInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toIpcError(error);
  }
}

let supportedPromise: Promise<boolean> | null = null;

/** True only in a desktop build compiled with the remote-desktop feature. Never throws. */
export function rdSupported(): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  supportedPromise ??= invoke<boolean>("cmd_rd_supported").then(
    (value) => value === true,
    () => false,
  );
  return supportedPromise;
}

export function __resetRdSupportedForTests(): void {
  supportedPromise = null;
}

export function useRemoteDesktopSupported(): boolean {
  const [supported, setSupported] = useState(false);
  useEffect(() => {
    let active = true;
    void rdSupported().then((value) => {
      if (active) setSupported(value);
    });
    return () => {
      active = false;
    };
  }, []);
  return supported;
}

export type RdFrameHandler = (message: RdFrameMessage) => void;

/** Opens a viewer session; `onFrame` receives every decoded Channel message. */
export async function rdConnect(
  hostId: string,
  onFrame: RdFrameHandler,
  onFrameError?: (error: unknown) => void,
  options?: RdConnectOptions,
): Promise<{ rdSessionId: string }> {
  const frames = new Channel<ArrayBuffer | Uint8Array | number[]>((payload) => {
    let message: RdFrameMessage;
    try {
      message = parseRdFrameMessage(payload);
    } catch (error) {
      onFrameError?.(error);
      return;
    }
    onFrame(message);
  });
  return rdInvoke<{ rdSessionId: string }>("cmd_rd_connect", { hostId, frames, ...(options !== undefined ? { options } : {}) });
}

export function rdStats(rdSessionId: string): Promise<RdSessionStats> {
  return rdInvoke<RdSessionStats>("cmd_rd_stats", { rdSessionId });
}

export function rdDisplayRefresh(): Promise<RdDisplayRefreshInfo> {
  return rdInvoke<RdDisplayRefreshInfo>("cmd_rd_display_refresh");
}

export async function onRdDisplayRefreshChanged(handler: (change: RdDisplayRefreshChange) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<RdDisplayRefreshChange>("rd-display-refresh-changed", (event) => handler(event.payload));
}

export function rdSetOptions(rdSessionId: string, options: RdStreamOptionsUpdate): Promise<void> {
  return rdInvoke<void>("cmd_rd_set_options", { rdSessionId, options });
}

export function rdSetAudio(rdSessionId: string, muted: boolean, volume: number): Promise<void> {
  return rdInvoke<void>("cmd_rd_set_audio", { rdSessionId, muted, volume });
}

export function rdClipboardSend(rdSessionId: string, text: string): Promise<void> {
  return rdInvoke<void>("cmd_rd_clipboard_send", { rdSessionId, text });
}

export function rdSurfaceAttach(rdSessionId: string, bounds: RdLogicalBounds): Promise<RdSurfaceAttachResponse> {
  return rdInvoke<RdSurfaceAttachResponse>("cmd_rd_surface_attach", { rdSessionId, bounds });
}

export function rdSurfaceUpdateBounds(rdSessionId: string, bounds: RdLogicalBounds, visible: boolean): Promise<void> {
  return rdInvoke<void>("cmd_rd_surface_update_bounds", { rdSessionId, bounds, visible });
}

export function rdSurfaceDetach(rdSessionId: string): Promise<void> {
  return rdInvoke<void>("cmd_rd_surface_detach", { rdSessionId });
}

export function rdInput(rdSessionId: string, events: RdInputEvent[]): Promise<void> {
  return rdInvoke<void>("cmd_rd_input", { rdSessionId, events });
}

export function rdDisconnect(rdSessionId: string): Promise<void> {
  return rdInvoke<void>("cmd_rd_disconnect", { rdSessionId });
}

export function rdHostState(): Promise<RdHostState> {
  return rdInvoke<RdHostState>("cmd_rd_host_state");
}

export function rdHostSet(settings: RdHostSettings): Promise<RdHostState> {
  return rdInvoke<RdHostState>("cmd_rd_host_set", {
    enabled: settings.enabled,
    allowInput: settings.allowInput,
    allowClipboard: settings.allowClipboard,
  });
}

export function rdHostRespond(requestId: string, approve: boolean): Promise<void> {
  return rdInvoke<void>("cmd_rd_host_respond", { requestId, approve });
}

export async function onRdHostConsent(handler: (request: RdHostConsentRequest) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<RdHostConsentRequest>(RD_HOST_CONSENT_EVENT, (event) => {
    const payload = event.payload;
    if (payload && typeof payload.requestId === "string") {
      handler({ requestId: payload.requestId, clientName: typeof payload.clientName === "string" ? payload.clientName : "" });
    }
  });
}

const RD_ERROR_MESSAGES: Record<string, string> = {
  RD_UNSUPPORTED: "Remote desktop is not available in this build.",
  RD_HOST_OFFLINE: "The machine is offline or unreachable.",
  RD_HOST_DISABLED: "Remote desktop hosting is turned off on that machine.",
  RD_PERMISSION_REQUIRED: "The machine has not granted remote desktop access to this device.",
  RD_NOT_FOUND: "The remote desktop session no longer exists.",
  RD_CONNECT_FAILED: "Could not connect to the remote desktop.",
};

/** User-facing text for a structured RD error, matched by code only. */
export function rdErrorMessage(error: StructuredIpcError): string {
  return RD_ERROR_MESSAGES[error.code] ?? error.message;
}

/**
 * Serializes input batches: at most one `cmd_rd_input` is in flight; events queued meanwhile go
 * out together. Consecutive absolute mouse moves keep the newest position; relative moves
 * sum their deltas so coalescing preserves total camera motion.
 */
export class RdInputSender {
  private queue: RdInputEvent[] = [];
  private inFlight = false;
  private closed = false;
  private readonly rdSessionId: string;
  private readonly send: (rdSessionId: string, events: RdInputEvent[]) => Promise<void>;
  private readonly onError: (error: unknown) => void;

  constructor(
    rdSessionId: string,
    send: (rdSessionId: string, events: RdInputEvent[]) => Promise<void> = rdInput,
    onError: (error: unknown) => void = () => undefined,
  ) {
    this.rdSessionId = rdSessionId;
    this.send = send;
    this.onError = onError;
  }

  push(event: RdInputEvent): void {
    if (this.closed) return;
    const last = this.queue[this.queue.length - 1];
    if (event.kind === "mouseMove" && last?.kind === "mouseMove") {
      this.queue[this.queue.length - 1] = event;
    } else if (event.kind === "relativeMove" && last?.kind === "relativeMove") {
      this.queue[this.queue.length - 1] = { kind: "relativeMove", dx: last.dx + event.dx, dy: last.dy + event.dy };
    } else {
      this.queue.push(event);
    }
    this.pump();
  }

  close(): void {
    this.closed = true;
    this.queue = [];
  }

  private pump(): void {
    if (this.inFlight || this.closed || this.queue.length === 0) return;
    const batch = this.queue;
    this.queue = [];
    this.inFlight = true;
    void this.send(this.rdSessionId, batch)
      .catch((error: unknown) => {
        if (!this.closed) this.onError(error);
      })
      .finally(() => {
        this.inFlight = false;
        this.pump();
      });
  }
}
