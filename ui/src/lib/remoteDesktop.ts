import { useEffect, useState } from "react";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toIpcError } from "./tauri";
import type { StructuredIpcError } from "./types";

/** Remote desktop (ferryx-rd) bridge. Every command exists only in builds with the
 * `remote-desktop` feature except `cmd_rd_supported`, which callers check first. */

export const RD_FRAME_KIND_VIDEO = 0x01;
export const RD_FRAME_KIND_STATUS = 0x02;
export const RD_FRAME_KIND_META = 0x03;
export const RD_FRAME_KIND_AUDIO = 0x04;
export const RD_FRAME_KIND_CLIPBOARD = 0x05;
/** kind(1) + width(4) + height(4) + frameId(8). */
export const RD_VIDEO_HEADER_BYTES = 17;
/** kind(1) + sampleRate(4) + channels(2). */
export const RD_AUDIO_HEADER_BYTES = 7;

export type RdConnectionState = "connecting" | "connected" | "reconnecting" | "ended";

export type RdRouteKind = "direct" | "relay";

export type RdDisplayInfo = {
  index: number;
  name: string;
  width: number;
  height: number;
  primary: boolean;
};

export type RdSessionMeta = {
  route: string;
  hostName: string;
  displays: RdDisplayInfo[];
  activeDisplay: number;
  supportsStreamOptions: boolean;
  width: number;
  height: number;
  codec: string | null;
  targetFps: number;
  scalePermille: number;
  jpegQuality: number;
  maxPresentFps: number;
  fps: number;
  rttMs: number | null;
  packetLossRatio: number | null;
  clipboardAllowed: boolean;
  audioAvailable: boolean;
  audioMuted: boolean;
};

export type RdStreamOptions = {
  displayIndex?: number;
  scalePermille?: number;
  fps?: number;
  bitrate?: number;
  jpegQuality?: number;
  maxPresentFps?: number;
};

export type RdSpecialKeyAction =
  | "ctrlAltDel"
  | "altTab"
  | "cmdTab"
  | "meta"
  | "cmdW"
  | "ctrlW"
  | "altF4"
  | "escape"
  | "printScreen";

export type RdQualityPresetId = "sharp" | "balanced" | "fast";

export type RdQualityPreset = {
  id: RdQualityPresetId;
  label: string;
  description: string;
  options: RdStreamOptions;
};

export const RD_QUALITY_PRESETS: readonly RdQualityPreset[] = [
  {
    id: "sharp",
    label: "Sharp",
    description: "Native scale · up to 60 fps",
    options: { scalePermille: 1000, fps: 60, bitrate: 0, jpegQuality: 85, maxPresentFps: 60 },
  },
  {
    id: "balanced",
    label: "Balanced",
    description: "Native scale · 30 fps",
    options: { scalePermille: 1000, fps: 60, bitrate: 0, jpegQuality: 72, maxPresentFps: 30 },
  },
  {
    id: "fast",
    label: "Fast",
    description: "75% scale · lower bandwidth",
    options: { scalePermille: 750, fps: 30, bitrate: 4_000_000, jpegQuality: 55, maxPresentFps: 30 },
  },
];

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

export type RdMetaFrame = {
  kind: "meta";
  meta: RdSessionMeta;
};

export type RdAudioFrame = {
  kind: "audio";
  sampleRate: number;
  channels: number;
  pcm: Uint8Array;
};

export type RdClipboardFrame = {
  kind: "clipboard";
  text: string;
};

export type RdFrameMessage =
  | RdVideoFrame
  | RdStatusFrame
  | RdMetaFrame
  | RdAudioFrame
  | RdClipboardFrame;

export type RdInputEvent =
  | { kind: "mouseMove"; x: number; y: number }
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
  if (kind === RD_FRAME_KIND_META) {
    const parsed: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(1)));
    if (!parsed || typeof parsed !== "object") throw new Error("remote desktop meta frame is not an object");
    const r = parsed as Record<string, unknown>;
    const rawDisplays = Array.isArray(r.displays) ? r.displays : [];
    const displays: RdDisplayInfo[] = rawDisplays
      .filter((d): d is Record<string, unknown> => Boolean(d && typeof d === "object"))
      .map((d) => ({
        index: typeof d.index === "number" ? d.index : 0,
        name: typeof d.name === "string" ? d.name : "",
        width: typeof d.width === "number" ? d.width : 0,
        height: typeof d.height === "number" ? d.height : 0,
        primary: d.primary === true,
      }));
    return {
      kind: "meta",
      meta: {
        route: typeof r.route === "string" ? r.route : "relay",
        hostName: typeof r.hostName === "string" ? r.hostName : "",
        displays,
        activeDisplay: typeof r.activeDisplay === "number" ? r.activeDisplay : 0,
        supportsStreamOptions: r.supportsStreamOptions === true,
        width: typeof r.width === "number" ? r.width : 0,
        height: typeof r.height === "number" ? r.height : 0,
        codec: typeof r.codec === "string" ? r.codec : null,
        targetFps: typeof r.targetFps === "number" ? r.targetFps : 60,
        scalePermille: typeof r.scalePermille === "number" ? r.scalePermille : 1000,
        jpegQuality: typeof r.jpegQuality === "number" ? r.jpegQuality : 72,
        maxPresentFps: typeof r.maxPresentFps === "number" ? r.maxPresentFps : 30,
        fps: typeof r.fps === "number" ? r.fps : 0,
        rttMs: typeof r.rttMs === "number" && Number.isFinite(r.rttMs) ? r.rttMs : null,
        packetLossRatio:
          typeof r.packetLossRatio === "number" && Number.isFinite(r.packetLossRatio)
            ? r.packetLossRatio
            : null,
        clipboardAllowed: r.clipboardAllowed === true,
        audioAvailable: r.audioAvailable === true,
        audioMuted: r.audioMuted === true,
      },
    };
  }
  if (kind === RD_FRAME_KIND_AUDIO) {
    if (bytes.byteLength < RD_AUDIO_HEADER_BYTES) {
      throw new Error(`remote desktop audio frame is too short: ${bytes.byteLength}`);
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return {
      kind: "audio",
      sampleRate: view.getUint32(1, true),
      channels: view.getUint16(5, true),
      pcm: bytes.subarray(RD_AUDIO_HEADER_BYTES),
    };
  }
  if (kind === RD_FRAME_KIND_CLIPBOARD) {
    const parsed: unknown = JSON.parse(new TextDecoder().decode(bytes.subarray(1)));
    if (!parsed || typeof parsed !== "object") throw new Error("remote desktop clipboard frame is not an object");
    const record = parsed as { text?: unknown };
    if (typeof record.text !== "string") throw new Error("remote desktop clipboard frame missing text");
    return { kind: "clipboard", text: record.text };
  }
  throw new Error(`unknown remote desktop frame kind: ${String(kind)}`);
}

export type FramePoint = { x: number; y: number };
export type RdFitMode = "fit" | "actual";

/**
 * Maps a client point to normalized 0..1 coordinates of a frame drawn either aspect-fit
 * (centered, letterboxed) or 1:1 actual size inside `rect`. Points outside return null.
 */
export function mapClientPointToFrame(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  frameWidth: number,
  frameHeight: number,
  fitMode: RdFitMode = "fit",
): FramePoint | null {
  if (rect.width <= 0 || rect.height <= 0 || frameWidth <= 0 || frameHeight <= 0) return null;
  if (fitMode === "actual") {
    const x = (clientX - rect.left) / rect.width;
    const y = (clientY - rect.top) / rect.height;
    if (x < 0 || x > 1 || y < 0 || y > 1) return null;
    return { x, y };
  }
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
  return rdInvoke<{ rdSessionId: string }>("cmd_rd_connect", { hostId, frames });
}

export function rdInput(rdSessionId: string, events: RdInputEvent[]): Promise<void> {
  return rdInvoke<void>("cmd_rd_input", { rdSessionId, events });
}

export function rdSpecialKey(rdSessionId: string, action: RdSpecialKeyAction): Promise<void> {
  return rdInvoke<void>("cmd_rd_special_key", { rdSessionId, action });
}

export function rdSetStreamOptions(rdSessionId: string, options: RdStreamOptions): Promise<RdSessionMeta> {
  return rdInvoke<RdSessionMeta>("cmd_rd_set_stream_options", { rdSessionId, options });
}

export function rdSendClipboard(rdSessionId: string, text: string): Promise<void> {
  return rdInvoke<void>("cmd_rd_send_clipboard", { rdSessionId, text });
}

export function rdSetAudioMuted(rdSessionId: string, muted: boolean): Promise<boolean> {
  return rdInvoke<boolean>("cmd_rd_set_audio_muted", { rdSessionId, muted });
}

export function rdRequestKeyframe(rdSessionId: string): Promise<void> {
  return rdInvoke<void>("cmd_rd_request_keyframe", { rdSessionId });
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
 * out together, with consecutive mouse moves collapsed to the newest one.
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

/**
 * Lightweight cross-platform Web Audio player for decoded 48kHz stereo f32 LE PCM chunks.
 * Safe no-op in environments without AudioContext (e.g. jsdom tests).
 */
export class RdAudioPlayer {
  private ctx: AudioContext | null = null;
  private gain: GainNode | null = null;
  private nextTime = 0;
  private muted = false;
  private closed = false;

  setMuted(muted: boolean): void {
    this.muted = muted;
    if (this.gain && this.ctx) {
      this.gain.gain.setValueAtTime(muted ? 0 : 1, this.ctx.currentTime);
    }
    if (!muted) this.resume();
  }

  resume(): void {
    if (this.closed || !this.ctx || this.ctx.state !== "suspended") return;
    void this.ctx.resume().catch(() => undefined);
  }

  push(frame: RdAudioFrame): void {
    if (this.closed || this.muted || frame.channels === 0 || frame.sampleRate === 0) return;
    const bytesPerFrame = frame.channels * 4;
    const sampleFrames = Math.floor(frame.pcm.byteLength / bytesPerFrame);
    if (sampleFrames <= 0) return;
    const AudioCtx =
      typeof globalThis !== "undefined"
        ? (globalThis.AudioContext ?? (globalThis as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext)
        : undefined;
    if (!AudioCtx) return;
    if (!this.ctx) {
      try {
        this.ctx = new AudioCtx({ sampleRate: frame.sampleRate });
        this.gain = this.ctx.createGain();
        this.gain.gain.value = this.muted ? 0 : 1;
        this.gain.connect(this.ctx.destination);
      } catch {
        return;
      }
    }
    if (this.ctx.state === "suspended") {
      void this.ctx.resume().catch(() => undefined);
    }
    const buffer = this.ctx.createBuffer(frame.channels, sampleFrames, frame.sampleRate);
    const view = new DataView(frame.pcm.buffer, frame.pcm.byteOffset, sampleFrames * bytesPerFrame);
    for (let ch = 0; ch < frame.channels; ch += 1) {
      const channelData = buffer.getChannelData(ch);
      for (let i = 0; i < sampleFrames; i += 1) {
        channelData[i] = view.getFloat32((i * frame.channels + ch) * 4, true);
      }
    }
    const source = this.ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(this.gain ?? this.ctx.destination);
    const now = this.ctx.currentTime;
    if (this.nextTime < now || this.nextTime > now + 0.18) {
      this.nextTime = now + 0.02;
    }
    source.start(this.nextTime);
    this.nextTime += buffer.duration;
  }

  close(): void {
    this.closed = true;
    const ctx = this.ctx;
    this.ctx = null;
    this.gain = null;
    this.nextTime = 0;
    if (ctx) {
      void ctx.close().catch(() => undefined);
    }
  }
}

type CapturedShortcutHandler = (action: RdSpecialKeyAction) => boolean;
const shortcutCaptureHandlers = new Set<CapturedShortcutHandler>();

/** Registers an active RemoteDesktopPane handler for native menu shortcuts (e.g. macOS Cmd+W). */
export function registerRemoteDesktopShortcutCapture(handler: CapturedShortcutHandler): () => void {
  shortcutCaptureHandlers.add(handler);
  return () => {
    shortcutCaptureHandlers.delete(handler);
  };
}

/** Forwards a native menu shortcut to a focused RemoteDesktopPane with shortcut capture enabled. */
export function forwardCapturedRemoteDesktopShortcut(action: RdSpecialKeyAction): boolean {
  for (const handler of shortcutCaptureHandlers) {
    if (handler(action)) return true;
  }
  return false;
}
