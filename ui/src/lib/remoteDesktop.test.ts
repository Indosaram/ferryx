import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
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
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

import {
  __resetRdSupportedForTests,
  mapClientPointToFrame,
  parseRdFrameMessage,
  rdConnect,
  rdErrorMessage,
  rdHostSet,
  RdInputSender,
  rdMouseButton,
  rdSupported,
  wheelDeltaPixels,
  type RdFrameMessage,
  type RdInputEvent,
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

afterEach(() => {
  invoke.mockReset();
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

  it("surfaces structured errors and maps them by code", async () => {
    invoke.mockRejectedValueOnce({ code: "RD_HOST_DISABLED", message: "disabled", details: {} });
    const error = await rdHostSet({ enabled: true, allowInput: false, allowClipboard: false }).catch((err: unknown) => err);
    expect(error).toMatchObject({ code: "RD_HOST_DISABLED" });
    expect(invoke).toHaveBeenCalledWith("cmd_rd_host_set", { enabled: true, allowInput: false, allowClipboard: false });
    expect(rdErrorMessage({ code: "RD_HOST_DISABLED", message: "x" })).toMatch(/turned off/);
    expect(rdErrorMessage({ code: "OTHER", message: "raw message" })).toBe("raw message");
  });
});
