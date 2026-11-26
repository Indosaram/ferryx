import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, listen, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  channels: [] as Array<{ onmessage: (payload: unknown) => void }>,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  isTauri: () => true,
  Channel: class {
    onmessage: (payload: unknown) => void;
    constructor(handler: (payload: unknown) => void) {
      this.onmessage = handler;
      channels.push(this);
    }
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import {
  __resetRdSupportedForTests,
  mapClientPointToFrame,
  parseRdFrameMessage,
  onRdDisplayRefreshChanged,
  rdClipboardSend,
  rdConnect,
  rdDisplayRefresh,
  rdErrorMessage,
  rdHostSet,
  RdInputSender,
  rdMouseButton,
  rdSetAudio,
  rdSetOptions,
  rdStats,
  rdSupported,
  rdSurfaceAttach,
  rdSurfaceDetach,
  rdSurfaceUpdateBounds,
  wheelDeltaPixels,
  type RdFrameMessage,
  type RdInputEvent,
  type RdSessionStats,
} from "./remoteDesktop";

function videoFrame(width: number, height: number, frameId: bigint, jpeg: number[]): Uint8Array {
  const bytes = new Uint8Array(17 + jpeg.length);
  const view = new DataView(bytes.buffer);
  bytes[0] = 0x01;
  view.setUint32(1, width, true);
  view.setUint32(5, height, true);
  view.setBigUint64(9, frameId, true);
  bytes.set(jpeg, 17);
  return bytes;
}

function statusFrame(json: string): Uint8Array {
  const body = new TextEncoder().encode(json);
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = 0x02;
  bytes.set(body, 1);
  return bytes;
}

function cursorFrame(json: string): Uint8Array {
  const bytes = statusFrame(json);
  bytes[0] = 0x03;
  return bytes;
}

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  channels.length = 0;
  __resetRdSupportedForTests();
});

describe("parseRdFrameMessage", () => {
  it("decodes a little-endian video frame header and keeps the JPEG body", () => {
    const parsed = parseRdFrameMessage(videoFrame(1920, 1080, 4294967301n, [0xff, 0xd8, 0xff]));
    expect(parsed).toEqual({ kind: "video", width: 1920, height: 1080, frameId: 4294967301, jpeg: new Uint8Array([0xff, 0xd8, 0xff]) });
  });

  it("accepts an ArrayBuffer and a number array", () => {
    const frame = videoFrame(2, 3, 7n, [1]);
    expect(parseRdFrameMessage(frame.slice().buffer)).toMatchObject({ width: 2, height: 3, frameId: 7 });
    expect(parseRdFrameMessage(Array.from(frame))).toMatchObject({ width: 2, height: 3, frameId: 7 });
  });

  it("decodes a frame view that does not start at buffer offset 0", () => {
    const frame = videoFrame(640, 480, 9n, [5, 6]);
    const padded = new Uint8Array(frame.length + 3);
    padded.set(frame, 3);
    expect(parseRdFrameMessage(padded.subarray(3))).toMatchObject({ width: 640, height: 480, frameId: 9, jpeg: new Uint8Array([5, 6]) });
  });

  it("decodes status frames", () => {
    expect(parseRdFrameMessage(statusFrame('{"state":"reconnecting","reason":"network"}'))).toEqual({
      kind: "status", state: "reconnecting", reason: "network",
    });
    expect(parseRdFrameMessage(statusFrame('{"state":"connected","reason":null}'))).toEqual({
      kind: "status", state: "connected", reason: null,
    });
  });

  it("decodes cursor frames and defaults absent shape fields to null", () => {
    expect(parseRdFrameMessage(cursorFrame('{"x":0.25,"y":0.75,"cursorType":2,"visible":true}'))).toEqual({
      kind: "cursor", x: 0.25, y: 0.75, cursorType: 2, visible: true,
      shapeId: null, rgbaBase64: null, width: null, height: null, hotspotX: null, hotspotY: null,
    });
    const cursor = { x: 10, y: 20, cursorType: 3, visible: false, shapeId: 7, rgbaBase64: "AQIDBA==", width: 1, height: 1, hotspotX: 0, hotspotY: 0 };
    expect(parseRdFrameMessage(cursorFrame(JSON.stringify(cursor)))).toEqual({ kind: "cursor", ...cursor });
  });

  it.each([
    "{", "null", "[]", "{}",
    '{"x":"0","y":0,"cursorType":0,"visible":true}',
    '{"x":0,"y":null,"cursorType":0,"visible":true}',
    '{"x":1e400,"y":0,"cursorType":0,"visible":true}',
    '{"x":0,"y":-1e400,"cursorType":0,"visible":true}',
    '{"x":0,"y":0,"cursorType":"0","visible":true}',
    '{"x":0,"y":0,"cursorType":0,"visible":1}',
  ])("rejects malformed cursor payload %s", (json) => {
    expect(() => parseRdFrameMessage(cursorFrame(json))).toThrow();
  });

  it("decodes the 9-byte native-presented header in little endian, including offset views", () => {
    const bytes = new Uint8Array([0x04, 0x80, 0x07, 0, 0, 0x38, 0x04, 0, 0]);
    expect(parseRdFrameMessage(bytes)).toEqual({ kind: "nativePresented", width: 1920, height: 1080 });
    const padded = new Uint8Array(12);
    padded.set(bytes, 3);
    expect(parseRdFrameMessage(padded.subarray(3))).toEqual({ kind: "nativePresented", width: 1920, height: 1080 });
  });

  it.each([1, 2, 3, 4, 5, 6, 7, 8])("rejects a %i-byte native-presented frame", (length) => {
    const bytes = new Uint8Array(length);
    bytes[0] = 0x04;
    expect(() => parseRdFrameMessage(bytes)).toThrow(/too short/);
  });

  it("rejects malformed frames", () => {
    expect(() => parseRdFrameMessage(new Uint8Array())).toThrow();
    expect(() => parseRdFrameMessage(new Uint8Array([0x01, 1, 2]))).toThrow(/too short/);
    expect(() => parseRdFrameMessage(statusFrame('{"state":"bogus"}'))).toThrow(/unknown remote desktop state/);
    expect(() => parseRdFrameMessage(new Uint8Array([0x09]))).toThrow(/unknown remote desktop frame kind/);
  });
});

describe("mapClientPointToFrame", () => {
  const rect = { left: 100, top: 50, width: 400, height: 400 };

  it("maps through the horizontal letterbox of a wide frame", () => {
    // 1600x900 drawn at 400x225, centered vertically: top bar is 87.5px.
    expect(mapClientPointToFrame(300, 250, rect, 1600, 900)).toEqual({ x: 0.5, y: 0.5 });
    expect(mapClientPointToFrame(100, 137.5, rect, 1600, 900)).toEqual({ x: 0, y: 0 });
    expect(mapClientPointToFrame(500, 362.5, rect, 1600, 900)).toEqual({ x: 1, y: 1 });
  });

  it("returns null in the letterbox bars and for degenerate sizes", () => {
    expect(mapClientPointToFrame(300, 60, rect, 1600, 900)).toBeNull();
    expect(mapClientPointToFrame(300, 250, { ...rect, width: 0 }, 1600, 900)).toBeNull();
    expect(mapClientPointToFrame(300, 250, rect, 0, 900)).toBeNull();
  });

  it("maps through the vertical pillarbox of a tall frame", () => {
    // 500x1000 drawn at 200x400, centered horizontally: left bar is 100px.
    expect(mapClientPointToFrame(250, 150, rect, 500, 1000)).toEqual({ x: 0.25, y: 0.25 });
    expect(mapClientPointToFrame(150, 150, rect, 500, 1000)).toBeNull();
  });
});

describe("input helpers", () => {
  it("maps DOM buttons and wheel delta modes", () => {
    expect([0, 1, 2, 3].map(rdMouseButton)).toEqual([0, 1, 2, null]);
    expect(wheelDeltaPixels(3, 0, 800)).toBe(3);
    expect(wheelDeltaPixels(3, 1, 800)).toBe(48);
    expect(wheelDeltaPixels(1, 2, 800)).toBe(800);
  });

  it("keeps one batch in flight and collapses queued mouse moves", async () => {
    const batches: RdInputEvent[][] = [];
    let release: () => void = () => undefined;
    const send = vi.fn((_: string, events: RdInputEvent[]) => {
      batches.push(events);
      return batches.length === 1 ? new Promise<void>((resolve) => { release = resolve; }) : Promise.resolve();
    });
    const sender = new RdInputSender("rd-1", send);
    sender.push({ kind: "mouseButton", button: 0, down: true });
    sender.push({ kind: "mouseMove", x: 0.1, y: 0.1 });
    sender.push({ kind: "mouseMove", x: 0.2, y: 0.2 });
    sender.push({ kind: "key", code: "KeyA", down: true });
    expect(batches).toEqual([[{ kind: "mouseButton", button: 0, down: true }]]);
    const drained = new Promise<void>((resolve) => {
      send.mockImplementationOnce((_: string, events: RdInputEvent[]) => {
        batches.push(events);
        resolve();
        return Promise.resolve();
      });
    });
    release();
    await drained;
    expect(batches[1]).toEqual([{ kind: "mouseMove", x: 0.2, y: 0.2 }, { kind: "key", code: "KeyA", down: true }]);
    expect(send).toHaveBeenCalledWith("rd-1", batches[1]);
  });
});

describe("relative input coalescing", () => {
  it("sums queued deltas while preserving non-move event boundaries", async () => {
    const batches: RdInputEvent[][] = [];
    let release!: () => void;
    let nextBatch!: () => void;
    const drained = new Promise<void>((resolve) => { nextBatch = resolve; });
    const send = vi.fn((_: string, events: RdInputEvent[]) => {
      batches.push(events);
      if (batches.length === 1) return new Promise<void>((resolve) => { release = resolve; });
      nextBatch();
      return Promise.resolve();
    });
    const sender = new RdInputSender("rd-relative", send);
    sender.push({ kind: "mouseButton", button: 0, down: true });
    sender.push({ kind: "relativeMove", dx: 2, dy: -1 });
    sender.push({ kind: "relativeMove", dx: 3, dy: 4 });
    sender.push({ kind: "relativeMove", dx: -1, dy: 2 });
    sender.push({ kind: "key", code: "KeyA", down: true });
    sender.push({ kind: "relativeMove", dx: 1, dy: 1 });
    sender.push({ kind: "mouseMove", x: 0.1, y: 0.2 });
    sender.push({ kind: "mouseMove", x: 0.3, y: 0.4 });
    expect(send).toHaveBeenCalledTimes(1);
    release();
    await drained;
    expect(batches[1]).toEqual([
      { kind: "relativeMove", dx: 4, dy: 5 },
      { kind: "key", code: "KeyA", down: true },
      { kind: "relativeMove", dx: 1, dy: 1 },
      { kind: "mouseMove", x: 0.3, y: 0.4 },
    ]);
    expect(send).toHaveBeenLastCalledWith("rd-relative", batches[1]);
    sender.close();
  });
});

describe("commands", () => {
  it("reports unsupported when cmd_rd_supported is missing", async () => {
    invoke.mockRejectedValueOnce({ code: "UNKNOWN", message: "command cmd_rd_supported not found" });
    await expect(rdSupported()).resolves.toBe(false);
  });

  it("passes the frames channel to cmd_rd_connect and forwards decoded messages", async () => {
    invoke.mockResolvedValueOnce({ rdSessionId: "rd-9" });
    const seen: RdFrameMessage[] = [];
    const rejected: unknown[] = [];
    await expect(rdConnect("host-a", (message) => seen.push(message), (error) => rejected.push(error))).resolves.toEqual({ rdSessionId: "rd-9" });
    expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", { hostId: "host-a", frames: channels[0] });
    channels[0].onmessage(statusFrame('{"state":"connected","reason":null}'));
    channels[0].onmessage(new Uint8Array([0x07]));
    expect(seen).toEqual([{ kind: "status", state: "connected", reason: null }]);
    expect(rejected).toHaveLength(1);
  });

  it("passes defined connection options without changing the channel contract", async () => {
    invoke.mockResolvedValueOnce({ rdSessionId: "rd-options" });
    const options = { playoutMode: "latest_wins", scaleMode: "fit", viewportWidth: 1920, viewportHeight: 1080, audioMuted: true, audioVolume: 0.5 } as const;
    await rdConnect("host-b", () => undefined, undefined, options);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", { hostId: "host-b", frames: channels[0], options });
  });

  it("returns camelCase session statistics", async () => {
    const stats: RdSessionStats = {
      rdSessionId: "rd-9", targetFps: 75, presentFps: 74.9, droppedFrames: 1,
      codec: "hevc", hwAccel: "videotoolbox", presentBackend: "wgpu", transportMode: "quic_datagram", playoutMode: "adaptive",
      frameWidth: 1920, frameHeight: 1080, scalePermille: 1000, audioActive: true, audioUnderruns: 0, audioDrops: 0,
      decodeUs: { frames: 75, p50Us: 10, p95Us: 20, p99Us: 30, maxUs: 40 }, presentUs: null, totalClientUs: null,
      udpReceiveBps: 5000000, packetLossRatio: 0, abrTargetBitrate: 4000000, hostEncodeUs: null, networkUs: null,
      captureToPresentUs: null, inputToDrawSubmitUs: null, audioVideoSourceOffsetUs: -1000,
    };
    invoke.mockResolvedValueOnce(stats);
    await expect(rdStats("rd-9")).resolves.toEqual(stats);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_stats", { rdSessionId: "rd-9" });
  });

  it("updates stream options and audio using camelCase IPC arguments", async () => {
    invoke.mockResolvedValue(undefined);
    const options = { playoutMode: "adaptive", scaleMode: "scale75", viewportWidth: 1280, viewportHeight: 720, codec: "h264" } as const;
    await rdSetOptions("rd-9", options);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_set_options", { rdSessionId: "rd-9", options });
    await rdSetAudio("rd-9", true, 0.25);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_set_audio", { rdSessionId: "rd-9", muted: true, volume: 0.25 });
  });

  it("attaches, updates and detaches the native surface", async () => {
    const bounds = { x: 10, y: 20, width: 800, height: 600, scaleFactor: 2 };
    const response = { backend: "wgpu", reason: null };
    invoke.mockResolvedValueOnce(response);
    await expect(rdSurfaceAttach("rd-9", bounds)).resolves.toEqual(response);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_attach", { rdSessionId: "rd-9", bounds });
    invoke.mockResolvedValue(undefined);
    await rdSurfaceUpdateBounds("rd-9", bounds, false);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", { rdSessionId: "rd-9", bounds, visible: false });
    await rdSurfaceDetach("rd-9");
    expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_detach", { rdSessionId: "rd-9" });
  });

  it("reads display refresh and forwards changes with an unlisten callback", async () => {
    const refresh = { refreshHz: 75, millihertz: 75000, monitorName: null, source: "monitor" };
    invoke.mockResolvedValueOnce(refresh);
    await expect(rdDisplayRefresh()).resolves.toEqual(refresh);
    expect(invoke).toHaveBeenCalledWith("cmd_rd_display_refresh", undefined);
    const unlisten = vi.fn();
    listen.mockResolvedValueOnce(unlisten);
    const handler = vi.fn();
    await expect(onRdDisplayRefreshChanged(handler)).resolves.toBe(unlisten);
    expect(listen).toHaveBeenCalledWith("rd-display-refresh-changed", expect.any(Function));
    const change = { windowLabel: "main", refreshHz: 120, millihertz: 120000 };
    listen.mock.calls[0][1]({ payload: change });
    expect(handler).toHaveBeenCalledWith(change);
  });

  it("sends clipboard text", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await rdClipboardSend("rd-9", "clipboard text");
    expect(invoke).toHaveBeenCalledWith("cmd_rd_clipboard_send", { rdSessionId: "rd-9", text: "clipboard text" });
  });

  it("surfaces structured errors and maps them by code", async () => {
    invoke.mockRejectedValueOnce({ code: "RD_HOST_DISABLED", message: "disabled", details: {} });
    const error = await rdHostSet({ enabled: true, allowInput: false, allowClipboard: false }).catch((err: unknown) => err);
    expect(error).toMatchObject({ code: "RD_HOST_DISABLED" });
    expect(invoke).toHaveBeenCalledWith("cmd_rd_host_set", { enabled: true, allowInput: false, allowClipboard: false });
    expect(rdErrorMessage({ code: "RD_HOST_DISABLED", message: "x" })).toMatch(/turned off/);
    expect(rdErrorMessage({ code: "OTHER", message: "raw message" })).toBe("raw message");
  });
});
