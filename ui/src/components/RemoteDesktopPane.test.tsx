import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as ShortcutsModule from "../lib/shortcuts";

const { invoke, channels, platform } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as Array<{ onmessage: (payload: unknown) => void }>,
  platform: { mac: false },
}));
vi.mock("../lib/shortcuts", async (importOriginal) => {
  const actual = (await importOriginal()) as typeof ShortcutsModule;
  return { ...actual, isMacShortcutPlatform: () => platform.mac };
});
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

import { RemoteDesktopPane } from "./RemoteDesktopPane";
import { remoteDesktopStore } from "../state/remoteDesktopStore";
import {
  forwardCapturedRemoteDesktopShortcut,
  RD_FRAME_KIND_CLIPBOARD,
  RD_FRAME_KIND_CURSOR,
  RD_FRAME_KIND_META,
  RD_FRAME_KIND_NATIVE_PRESENTED,
  RD_FRAME_KIND_STATUS,
  RD_FRAME_KIND_VIDEO,
  type RdInputEvent,
  type RdSessionMeta,
} from "../lib/remoteDesktop";
import {
  RD_AUDIO_MUTED_STORAGE_KEY,
  RD_PLAYOUT_MODE_STORAGE_KEY,
  RD_STATS_OVERLAY_STORAGE_KEY,
} from "../lib/storageKeys";

function statusFrame(state: string, reason: string | null = null): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ state, reason }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = RD_FRAME_KIND_STATUS;
  bytes.set(body, 1);
  return bytes;
}

function metaFrame(overrides: Partial<RdSessionMeta> = {}): Uint8Array {
  const meta: RdSessionMeta = {
    route: "direct",
    hostName: "DESKTOP-1LAPJMP",
    displays: [
      { index: 0, name: "Primary", width: 2560, height: 1440, primary: true },
      { index: 1, name: "Side", width: 1920, height: 1080, primary: false },
    ],
    activeDisplay: 0,
    supportsStreamOptions: true,
    width: 2560,
    height: 1440,
    codec: "h264",
    targetFps: 60,
    scalePermille: 1000,
    jpegQuality: 72,
    maxPresentFps: 30,
    fps: 30,
    rttMs: 12.4,
    packetLossRatio: 0,
    clipboardAllowed: true,
    audioAvailable: true,
    audioMuted: false,
    ...overrides,
  };
  const body = new TextEncoder().encode(JSON.stringify(meta));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = RD_FRAME_KIND_META;
  bytes.set(body, 1);
  return bytes;
}

function clipboardFrame(text: string): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ text }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = RD_FRAME_KIND_CLIPBOARD;
  bytes.set(body, 1);
  return bytes;
}

function videoFrame(width: number, height: number, frameId = 1, marker = 0xff): Uint8Array {
  const bytes = new Uint8Array(17 + 3);
  const view = new DataView(bytes.buffer);
  bytes[0] = RD_FRAME_KIND_VIDEO;
  view.setUint32(1, width, true);
  view.setUint32(5, height, true);
  view.setBigUint64(9, BigInt(frameId), true);
  // 0xFFD8 + a per-frame marker byte, so a decode can be attributed to the exact frame payload.
  bytes.set([0xff, 0xd8, marker], 17);
  return bytes;
}

/** The 9-byte native-presented notification: kind(1) + u32 width + u32 height, little endian. */
function nativePresentedFrame(width: number, height: number): Uint8Array {
  const bytes = new Uint8Array(9);
  const view = new DataView(bytes.buffer);
  bytes[0] = RD_FRAME_KIND_NATIVE_PRESENTED;
  view.setUint32(1, width, true);
  view.setUint32(5, height, true);
  return bytes;
}

/** Cursor frame; `rgba` is omitted when the host already sent that shape. */
function cursorFrame(fields: {
  shapeId: number;
  visible?: boolean;
  rgba?: Uint8Array;
  width?: number;
  height?: number;
}): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({
    x: 0.5,
    y: 0.5,
    cursorType: 1,
    visible: fields.visible ?? true,
    shapeId: fields.shapeId,
    ...(fields.rgba
      ? {
        rgbaBase64: btoa(String.fromCharCode(...fields.rgba)),
        width: fields.width,
        height: fields.height,
        hotspotX: 1,
        hotspotY: 0,
      }
      : {}),
  }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = RD_FRAME_KIND_CURSOR;
  bytes.set(body, 1);
  return bytes;
}

/** Every input event the pane pushed through `cmd_rd_input`, in order. */
function sentInputEvents(): RdInputEvent[] {
  return invoke.mock.calls
    .filter(([command]) => command === "cmd_rd_input")
    .flatMap(([, args]) => (args as { events: RdInputEvent[] }).events);
}

function connectCalls(): unknown[][] {
  return invoke.mock.calls.filter(([command]) => command === "cmd_rd_connect");
}

/** Mutable stand-in for the browser's Pointer Lock owner; jsdom implements none of it. */
const pointerLock = { element: null as Element | null };

function installPointerLock(canvas: HTMLCanvasElement) {
  const exitPointerLock = vi.fn(() => {
    pointerLock.element = null;
    document.dispatchEvent(new Event("pointerlockchange"));
  });
  const requestPointerLock = vi.fn(async () => {
    pointerLock.element = canvas;
    document.dispatchEvent(new Event("pointerlockchange"));
  });
  Object.defineProperty(document, "pointerLockElement", { configurable: true, get: () => pointerLock.element });
  Object.defineProperty(document, "exitPointerLock", { configurable: true, writable: true, value: exitPointerLock });
  Object.defineProperty(canvas, "requestPointerLock", { configurable: true, writable: true, value: requestPointerLock });
  return { exitPointerLock, requestPointerLock };
}

/** jsdom's MouseEvent drops `movementX/Y` from its init dict, so they are pinned on the instance. */
function relativePointerMove(canvas: HTMLCanvasElement, movementX: number, movementY: number) {
  const event = new PointerEvent("pointermove", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "movementX", { value: movementX });
  Object.defineProperty(event, "movementY", { value: movementY });
  fireEvent(canvas, event);
}

function paneRect(width: number, height: number) {
  return { left: 0, top: 0, width, height, right: width, bottom: height, x: 0, y: 0, toJSON: () => ({}) };
}

/** Installs a native-surface backend answer for `cmd_rd_surface_attach` on the shared mock. */
function surfaceBackend(backend: "wgpu" | "canvas2d", reason: string | null = null, sessionId = "rd-1") {
  invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_rd_connect") return { rdSessionId: sessionId };
    if (command === "cmd_rd_surface_attach") return { backend, reason };
    if (command === "cmd_rd_stats") return rdStatsFixture();
    return undefined;
  });
}

/** Minimal `cmd_rd_stats` answer: the HUD renders it whenever it is mounted. */
function rdStatsFixture() {
  const latency = { frames: 75, p50Us: 1_000, p95Us: 2_000, p99Us: 3_000, maxUs: 6_000 };
  return {
    rdSessionId: "rd-1",
    targetFps: 75,
    presentFps: 75,
    droppedFrames: 0,
    codec: "hevc",
    hwAccel: "videotoolbox",
    presentBackend: "wgpu",
    transportMode: "quic_datagram",
    playoutMode: "immediate",
    frameWidth: 1920,
    frameHeight: 1080,
    scalePermille: 1000,
    audioActive: true,
    audioUnderruns: 0,
    audioDrops: 0,
    decodeUs: latency,
    presentUs: latency,
    totalClientUs: null,
    udpReceiveBps: null,
    packetLossRatio: 0,
    abrTargetBitrate: 24_500_000,
    hostEncodeUs: null,
    networkUs: latency,
    captureToPresentUs: latency,
    inputToDrawSubmitUs: null,
    audioVideoSourceOffsetUs: null,
  };
}

beforeEach(() => {
  platform.mac = false;
  window.localStorage.clear();
  // Default: the host declines the native surface, so the Canvas 2D path (and every pre-existing
  // frame-drawing test) stays the baseline.
  invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_rd_connect") return { rdSessionId: "rd-1" };
    if (command === "cmd_rd_surface_attach") return { backend: "canvas2d", reason: "mock" };
    if (command === "cmd_rd_stats") return rdStatsFixture();
    return undefined;
  });
});

afterEach(() => {
  cleanup();
  // Each test owns the global store: dropping its sessions here is test scaffolding, not the
  // pane's behavior (the pane itself never disconnects on unmount).
  remoteDesktopStore.reset();
  invoke.mockReset();
  channels.length = 0;
  pointerLock.element = null;
  Reflect.deleteProperty(document, "pointerLockElement");
  Reflect.deleteProperty(document, "exitPointerLock");
  Reflect.deleteProperty(URL, "createObjectURL");
  Reflect.deleteProperty(URL, "revokeObjectURL");
  for (const dialog of document.querySelectorAll('[data-test-modal="true"]')) dialog.remove();
  window.localStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("RemoteDesktopPane", () => {
  it("connects with the host id, shows status and keeps the session on unmount", async () => {
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    expect(screen.getByTestId("remote-desktop-status")).toHaveTextContent("Connecting");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.objectContaining({ hostId: "host-1" })));
    act(() => channels[0].onmessage(statusFrame("reconnecting", "network lost")));
    expect(screen.getByTestId("remote-desktop-status")).toHaveTextContent("Reconnecting… network lost");

    // GAP-7: leaving a worktree detaches the canvas but must not drop the connection.
    view.unmount();
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")).toMatchObject({ rdSessionId: "rd-1" }));
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());
  });

  it("disconnects only through the explicit Disconnect button", async () => {
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));

    fireEvent.click(screen.getByTestId("rd-disconnect-btn"));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_disconnect", { rdSessionId: "rd-1" }));
    await waitFor(() => expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "idle"));
  });

  it("reuses the same rdSessionId when the pane remounts", async () => {
    const first = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));

    first.unmount();
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);

    // Same session attached again: no second dial, no teardown in between.
    await waitFor(() => expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "connected"));
    expect(connectCalls()).toHaveLength(1);
    expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1");
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());
  });

  it("releases held keys and stops drawing while hidden, without disconnecting", async () => {
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) });
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    await waitFor(() => expect(drawImage).toHaveBeenCalledTimes(1));

    fireEvent.keyDown(canvas, { code: "ShiftLeft", key: "Shift" });
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "ShiftLeft", down: true }));

    view.rerender(<RemoteDesktopPane hostId="host-1" label="Studio" visible={false} />);

    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "ShiftLeft", down: false }));
    expect(remoteDesktopStore.getSession("host-1")).toMatchObject({ status: "connected", rdSessionId: "rd-1" });
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());

    // Frames that arrive while hidden are dropped, not painted.
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    await act(async () => {
      await Promise.resolve();
    });
    expect(drawImage).toHaveBeenCalledTimes(1);
  });

  it("releases the captured pointer by id when the pane is hidden", async () => {
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) });
    const setPointerCapture = vi.spyOn(canvas, "setPointerCapture");
    const releasePointerCapture = vi.spyOn(canvas, "releasePointerCapture");
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    await waitFor(() => expect(drawImage).toHaveBeenCalledTimes(1));

    fireEvent.pointerDown(canvas, { button: 0, clientX: 50, clientY: 25, pointerId: 7 });
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "mouseButton", button: 0, down: true }));
    expect(setPointerCapture).toHaveBeenCalledTimes(1);
    // jsdom may substitute MouseEvent for PointerEvent, so the id is read from the capture itself.
    const capturedId = setPointerCapture.mock.calls[0]?.[0];

    // Hiding hands the pointer back by the id the pane captured (and never disconnects).
    view.rerender(<RemoteDesktopPane hostId="host-1" label="Studio" visible={false} />);

    await waitFor(() => expect(releasePointerCapture).toHaveBeenCalledWith(capturedId));
    expect(releasePointerCapture.mock.calls.map(([id]) => id)).toEqual([capturedId]);
    expect(remoteDesktopStore.getSession("host-1")).toMatchObject({ status: "connected", rdSessionId: "rd-1" });
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_disconnect", expect.anything());
  });

  it("shows a code-mapped error with a reconnect action", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") throw { code: "RD_HOST_OFFLINE", message: "offline", details: {} };
      return undefined;
    });
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("offline or unreachable");
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "error");
    invoke.mockImplementation(async (command: string) => (command === "cmd_rd_connect" ? { rdSessionId: "rd-2" } : undefined));
    fireEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(connectCalls()).toHaveLength(2));
    await waitFor(() => expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "connecting"));
  });

  it("surfaces the offline error by structured code even when the message text disagrees", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") {
        // A generic transport message: only the code may decide the user-facing text.
        throw { code: "RD_HOST_OFFLINE", message: "request timeout after 30000ms", details: {} };
      }
      return undefined;
    });
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);

    expect(await screen.findByRole("alert")).toHaveTextContent("The machine is offline or unreachable.");
    expect(remoteDesktopStore.getSession("host-1")?.error).toEqual({
      code: "RD_HOST_OFFLINE",
      message: "request timeout after 30000ms",
    });
  });

  it("shows the typed refusal when hosting is off on the far machine, then reconnects once it is on", async () => {
    // C3 regression: a disabled host answers RD_HOST_DISABLED instead of leaving the pane on
    // "Connecting…" forever. The refusal must be user-visible text, not a spinner.
    let calls = 0;
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") {
        calls += 1;
        if (calls === 1) throw { code: "RD_HOST_DISABLED", message: "off", details: {} };
        return { rdSessionId: "rd-after-enable" };
      }
      return undefined;
    });
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Remote desktop hosting is turned off on that machine.");
    // The refusal replaces the connecting overlay: no lingering progress state.
    expect(screen.queryByText(/Connecting/)).toBeNull();
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "error");

    fireEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.objectContaining({ hostId: "host-1" })));
    await waitFor(() => expect(calls).toBe(2));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("draws frames and sends normalized pointer and key input once connected", async () => {
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: 200, height: 100, right: 200, bottom: 100, x: 0, y: 0, toJSON: () => ({}) });
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    await waitFor(() => expect(drawImage).toHaveBeenCalled());
    expect(canvas.width).toBe(400);

    fireEvent.pointerDown(canvas, { button: 0, clientX: 50, clientY: 25, pointerId: 1 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-1",
      events: [{ kind: "mouseMove", x: 0.25, y: 0.25 }],
    }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-1",
      events: [{ kind: "mouseButton", button: 0, down: true }],
    }));
    const keyDown = fireEvent.keyDown(canvas, { code: "KeyA", key: "a" });
    expect(keyDown).toBe(false);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-1",
      events: [{ kind: "key", code: "KeyA", down: true }],
    }));
  });

  it("renders the overlay without a scrim while no frame has arrived", async () => {
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const overlay = screen.getByTestId("remote-desktop-overlay");
    expect(overlay).toHaveAttribute("data-scrim", "false");
    expect(overlay).not.toHaveClass("bg-black/75");
  });

  it("paints a high-contrast scrim with light text over a frozen frame while reconnecting", async () => {
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    // The canvas is already hit-testable while connected (`canvasVisible = hasFrame || isConnected`),
    // so visibility alone does not prove the bitmap was decoded. Wait for the Canvas 2D paint and for
    // the overlay to clear (that is what flips `hasFrame`), otherwise the scrim assertions race it.
    await waitFor(() => expect(drawImage).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("remote-desktop-overlay")).toBeNull());
    expect(screen.getByTestId("remote-desktop-canvas")).toHaveStyle({ visibility: "visible" });

    act(() => channels[0].onmessage(statusFrame("reconnecting")));
    const overlay = screen.getByTestId("remote-desktop-overlay");
    expect(overlay).toHaveAttribute("data-scrim", "true");
    expect(overlay).toHaveClass("bg-black/75");
    const status = screen.getByTestId("remote-desktop-status");
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "connecting");
    expect(status).toHaveAttribute("role", "status");
    expect(status).toHaveClass("text-white");
  });

  it("keeps the scrim and high-contrast error text when a reconnect fails over a frozen frame", async () => {
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    let connects = 0;
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") {
        connects += 1;
        if (connects > 1) throw { code: "RD_HOST_OFFLINE", message: "offline", details: {} };
        return { rdSessionId: "rd-1" };
      }
      return undefined;
    });
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    // Same race as above: wait for the painted frame (overlay cleared) before ending the session,
    // otherwise `hasFrame` is still false and no scrim/error palette is applied.
    await waitFor(() => expect(drawImage).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("remote-desktop-overlay")).toBeNull());
    expect(screen.getByTestId("remote-desktop-canvas")).toHaveStyle({ visibility: "visible" });

    act(() => channels[0].onmessage(statusFrame("ended")));
    fireEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    const alert = await screen.findByRole("alert");
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "error");
    expect(alert).toHaveClass("text-red-300");
    expect(screen.getByTestId("remote-desktop-overlay")).toHaveAttribute("data-scrim", "true");
    expect(screen.getByTestId("remote-desktop-canvas")).toHaveStyle({ visibility: "visible" });
  });

  it("ignores input before the session reports connected", async () => {
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const canvas = screen.getByTestId("remote-desktop-canvas");
    expect(fireEvent.keyDown(canvas, { code: "KeyA" })).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_input", expect.anything());
  });

  it("keeps the interactive canvas live in wgpu mode with zero JPEG frames", async () => {
    // macOS transparency + input regression: the native surface draws beneath a transparent
    // WKWebView, so the pane must not paint over it, and the canvas must stay hit-testable even
    // though no JPEG frame will ever arrive to turn `hasFrame` on.
    surfaceBackend("wgpu", null, "rd-wgpu");
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const pane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-native", "true"));
    expect(pane).toHaveAttribute("data-rd-presented", "false");
    expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_attach", expect.objectContaining({ rdSessionId: "rd-wgpu" }));

    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue(paneRect(400, 400));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(nativePresentedFrame(1600, 900)));

    expect(pane).toHaveAttribute("data-rd-presented", "true");
    expect(canvas.style.visibility).toBe("visible");
    // The native view owns the pixels now, so the opaque backing must be gone.
    expect(pane).not.toHaveClass("bg-black");
    // No placeholder may cover a live surface.
    expect(screen.queryByTestId("remote-desktop-status")).toBeNull();

    fireEvent.pointerDown(canvas, { button: 0, clientX: 200, clientY: 200, pointerId: 1 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-wgpu",
      events: [{ kind: "mouseMove", x: 0.5, y: 0.5 }],
    }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-wgpu",
      events: [{ kind: "mouseButton", button: 0, down: true }],
    }));
    const keyDown = fireEvent.keyDown(canvas, { code: "KeyA", key: "a" });
    expect(keyDown).toBe(false);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-wgpu",
      events: [{ kind: "key", code: "KeyA", down: true }],
    }));

    view.unmount();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_detach", { rdSessionId: "rd-wgpu" }));
  });

  it("remaps input after a live aspect-ratio change in wgpu mode", async () => {
    // B2: a live StreamOptionsRequest rescale presents a new frame geometry. Keeping the first
    // geometry would leave (200, 20) inside a stale top letterbox bar and swallow the click.
    surfaceBackend("wgpu", null, "rd-wgpu");
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const pane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-native", "true"));
    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue(paneRect(400, 400));

    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(nativePresentedFrame(1600, 900)));
    // 1600x900 letterboxes to 400x225 with an 87.5px top bar, so (200, 20) is not on the desktop.
    expect(fireEvent.pointerDown(canvas, { button: 0, clientX: 200, clientY: 20, pointerId: 1 })).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_input", expect.anything());

    // A square frame fills the pane edge to edge: the same point is now the desktop's top-centre.
    act(() => channels[0].onmessage(nativePresentedFrame(1000, 1000)));
    fireEvent.pointerDown(canvas, { button: 0, clientX: 200, clientY: 20, pointerId: 2 });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-wgpu",
      events: [{ kind: "mouseMove", x: 0.5, y: 0.05 }],
    }));
  });

  it("coalesces a JPEG burst into one Canvas 2D decode per animation frame", async () => {
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 0;
    const requestFrame = vi.fn((callback: FrameRequestCallback) => {
      frames.set(++nextFrame, callback);
      return nextFrame;
    });
    const cancelFrame = vi.fn((id: number) => { frames.delete(id); });
    vi.stubGlobal("requestAnimationFrame", requestFrame);
    vi.stubGlobal("cancelAnimationFrame", cancelFrame);
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    const createImageBitmap = vi.fn(async (_source: Blob) => ({ close: vi.fn() }));
    vi.stubGlobal("createImageBitmap", createImageBitmap);

    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => {
      for (let index = 1; index <= 5; index += 1) {
        channels[0].onmessage(videoFrame(100 * index, 50 * index, index, index));
      }
    });

    // Five frames arriving inside one display frame arm exactly one callback and decode nothing.
    expect(requestFrame).toHaveBeenCalledTimes(1);
    expect(createImageBitmap).not.toHaveBeenCalled();

    await act(async () => {
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(16);
    });

    // Latest-frame-wins: one decode, carrying frame #5's payload (3 header-tail bytes + marker).
    expect(createImageBitmap).toHaveBeenCalledTimes(1);
    const decoded = createImageBitmap.mock.calls[0][0];
    expect(decoded.size).toBe(3);
    expect(drawImage).toHaveBeenCalledTimes(1);
    const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
    expect(canvas.width).toBe(500);
    expect(canvas.height).toBe(250);

    // A frame arriving after the flush arms a fresh callback; unmounting must cancel it.
    act(() => channels[0].onmessage(videoFrame(600, 300, 6, 6)));
    expect(frames.size).toBe(1);
    const armed = [...frames.keys()];
    view.unmount();
    expect(cancelFrame).toHaveBeenCalledWith(armed[0]);
    expect(frames.size).toBe(0);
  });

  it("falls back to Canvas 2D silently when the host declines the native surface", async () => {
    surfaceBackend("canvas2d", "no adapter", "rd-1");
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));

    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const pane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_attach", expect.objectContaining({ rdSessionId: "rd-1" })));
    // The decline is silent: Canvas 2D stays the presenter and no banner is raised.
    expect(pane).toHaveAttribute("data-rd-native", "false");
    expect(pane).toHaveAttribute("data-rd-presented", "false");
    expect(pane).toHaveClass("bg-black");

    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(400, 200)));
    await waitFor(() => expect(drawImage).toHaveBeenCalled());
    expect((screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement).width).toBe(400);
    expect(pane).toHaveAttribute("data-rd-native", "false");
    // A Canvas 2D presenter never owns a native surface, so bounds are never pushed.
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_surface_update_bounds", expect.anything());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("falls back to Canvas 2D silently when the surface attach itself fails", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") return { rdSessionId: "rd-1" };
      if (command === "cmd_rd_surface_attach") throw { code: "RD_CONNECT_FAILED", message: "attach", details: {} };
      return undefined;
    });
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({ drawImage } as unknown as CanvasRenderingContext2D);
    vi.stubGlobal("createImageBitmap", vi.fn(async () => ({ close: vi.fn() })));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const pane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_attach", expect.anything()));
    await waitFor(() => expect(warn).toHaveBeenCalled());
    expect(pane).toHaveAttribute("data-rd-native", "false");
    expect(screen.queryByRole("alert")).toBeNull();

    act(() => channels[0].onmessage(statusFrame("connected")));
    act(() => channels[0].onmessage(videoFrame(320, 180)));
    await waitFor(() => expect(drawImage).toHaveBeenCalled());
    expect((screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement).width).toBe(320);
  });

  it("renders route badge, telemetry and drives display, special keys, quality, clipboard, audio, fullscreen and disconnect", async () => {
    const updatedMeta = {
      route: "direct",
      hostName: "DESKTOP-1LAPJMP",
      displays: [
        { index: 0, name: "Primary", width: 2560, height: 1440, primary: true },
        { index: 1, name: "Side", width: 1920, height: 1080, primary: false },
      ],
      activeDisplay: 1,
      supportsStreamOptions: true,
      width: 1920,
      height: 1080,
      codec: "h264",
      targetFps: 60,
      scalePermille: 1000,
      jpegQuality: 72,
      maxPresentFps: 30,
      fps: 30,
      rttMs: 12,
      packetLossRatio: 0,
      clipboardAllowed: true,
      audioAvailable: true,
      audioMuted: false,
    };
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") return { rdSessionId: "rd-1" };
      if (command === "cmd_rd_set_stream_options") return updatedMeta;
      if (command === "cmd_rd_set_audio_muted") return true;
      return undefined;
    });

    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(remoteDesktopStore.getSession("host-1")?.rdSessionId).toBe("rd-1"));
    act(() => {
      channels[0].onmessage(statusFrame("connected"));
      channels[0].onmessage(metaFrame());
    });

    expect(screen.getByTestId("rd-route-badge")).toHaveTextContent("Direct");
    expect(screen.getByTestId("rd-telemetry")).toHaveTextContent("2560×1440 · 30 fps · 12 ms");

    // Multi-monitor switch
    fireEvent.click(screen.getByTestId("rd-display-trigger"));
    fireEvent.click(screen.getByTestId("rd-display-option-1"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_set_stream_options", {
        rdSessionId: "rd-1",
        options: { displayIndex: 1 },
      }),
    );

    // Special key (Ctrl+Alt+Del SAS) & shortcut capture toggle + native menu forwarding
    const canvas = screen.getByTestId("remote-desktop-canvas");
    expect(canvas).toHaveAttribute("data-rd-capture-keys", "true");
    canvas.focus();
    expect(forwardCapturedRemoteDesktopShortcut("cmdW")).toBe(true);
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_special_key", {
        rdSessionId: "rd-1",
        action: "cmdW",
      }),
    );

    fireEvent.click(screen.getByTestId("rd-keys-trigger"));
    fireEvent.click(screen.getByTestId("rd-key-ctrlAltDel"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_special_key", {
        rdSessionId: "rd-1",
        action: "ctrlAltDel",
      }),
    );

    fireEvent.click(screen.getByTestId("rd-keys-trigger"));
    fireEvent.click(screen.getByTestId("rd-capture-shortcuts-toggle"));
    expect(canvas).toHaveAttribute("data-rd-capture-keys", "false");

    // Scale 1:1 & quality preset & keyframe refresh
    fireEvent.click(screen.getByTestId("rd-quality-trigger"));
    fireEvent.click(screen.getByTestId("rd-scale-actual"));
    expect(canvas).toHaveAttribute("data-rd-fit-mode", "actual");

    fireEvent.click(screen.getByTestId("rd-quality-trigger"));
    fireEvent.click(screen.getByTestId("rd-quality-fast"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_set_stream_options", {
        rdSessionId: "rd-1",
        options: expect.objectContaining({ scalePermille: 750, jpegQuality: 55 }),
      }),
    );

    fireEvent.click(screen.getByTestId("rd-quality-trigger"));
    fireEvent.click(screen.getByTestId("rd-request-keyframe"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_request_keyframe", { rdSessionId: "rd-1" }),
    );

    // Remote clipboard arrival + send clipboard text
    act(() => channels[0].onmessage(clipboardFrame("from-host-123")));
    expect(screen.getByTestId("rd-clipboard-unread")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("rd-clipboard-trigger"));
    expect(screen.getByTestId("rd-clipboard-remote-preview")).toHaveTextContent("from-host-123");
    fireEvent.change(screen.getByTestId("rd-clipboard-input"), { target: { value: "to-host-456" } });
    fireEvent.click(screen.getByTestId("rd-clipboard-send-text"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_send_clipboard", {
        rdSessionId: "rd-1",
        text: "to-host-456",
      }),
    );

    // Audio mute toggle
    fireEvent.click(screen.getByTestId("rd-audio-toggle"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_set_audio_muted", {
        rdSessionId: "rd-1",
        muted: true,
      }),
    );

    // Fullscreen fallback toggle (in jsdom where requestFullscreen is absent)
    fireEvent.click(screen.getByTestId("rd-fullscreen-toggle"));
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-fullscreen", "true");
    fireEvent.click(screen.getByTestId("rd-fullscreen-toggle"));
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-fullscreen", "false");

    // Manual disconnect transitions to idle and invokes cmd_rd_disconnect
    fireEvent.click(screen.getByTestId("rd-disconnect-btn"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_disconnect", { rdSessionId: "rd-1" }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "idle"),
    );
  });
});

/** Renders a connected pane and returns its canvas, ready for input. */
async function connectPane() {
  const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
  const canvas = screen.getByTestId("remote-desktop-canvas") as HTMLCanvasElement;
  vi.spyOn(canvas, "getBoundingClientRect").mockReturnValue(paneRect(400, 400));
  act(() => channels[0].onmessage(statusFrame("connected")));
  await waitFor(() => expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-interactive", "true"));
  await waitFor(() => expect(screen.getByRole("button", { name: "Toggle pointer lock" })).toBeEnabled());
  return { view, canvas };
}

describe("RemoteDesktopPane Pointer Lock", () => {
  it("forwards relative motion while locked and drops the lock plus held keys on blur", async () => {
    const { canvas } = await connectPane();
    const { exitPointerLock, requestPointerLock } = installPointerLock(canvas);
    const pane = screen.getByTestId("remote-desktop-pane");

    fireEvent.click(screen.getByRole("button", { name: "Toggle pointer lock" }));
    await waitFor(() => expect(requestPointerLock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-pointer-locked", "true"));

    // Relative motion is the whole point of the lock: no absolute position is claimed.
    relativePointerMove(canvas, 12, -7);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_input", {
      rdSessionId: "rd-1",
      events: [{ kind: "relativeMove", dx: 12, dy: -7 }],
    }));

    // A zero-delta move carries no information and must not reach the host.
    const sentBefore = sentInputEvents().length;
    relativePointerMove(canvas, 0, 0);
    expect(sentInputEvents()).toHaveLength(sentBefore);

    // A held key must be lifted when the window loses focus, not left down on the host.
    fireEvent.keyDown(canvas, { code: "KeyW", key: "w" });
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "KeyW", down: true }));

    fireEvent(window, new Event("blur"));
    await waitFor(() => expect(exitPointerLock).toHaveBeenCalled());
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-pointer-locked", "false"));
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "KeyW", down: false }));
  });

  it("drops the lock and releases held keys when the pane becomes occluded", async () => {
    const { view, canvas } = await connectPane();
    const { exitPointerLock, requestPointerLock } = installPointerLock(canvas);
    const pane = screen.getByTestId("remote-desktop-pane");

    fireEvent.click(screen.getByRole("button", { name: "Toggle pointer lock" }));
    await waitFor(() => expect(requestPointerLock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-pointer-locked", "true"));
    fireEvent.keyDown(canvas, { code: "KeyD", key: "d" });
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "KeyD", down: true }));

    // A drop covering the pane owns the pointer now: the stream must not keep the lock.
    view.rerender(<RemoteDesktopPane hostId="host-1" label="Studio" visible occluded />);
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-interactive", "false"));
    await waitFor(() => expect(exitPointerLock).toHaveBeenCalled());
    await waitFor(() => expect(sentInputEvents()).toContainEqual({ kind: "key", code: "KeyD", down: false }));
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-pointer-locked", "false"));
  });
});

describe("RemoteDesktopPane remote cursor", () => {
  const rgba = new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);

  function installObjectUrls() {
    let minted = 0;
    const createObjectURL = vi.fn(() => {
      minted += 1;
      return `blob:cursor-${minted}`;
    });
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, writable: true, value: createObjectURL });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, writable: true, value: revokeObjectURL });
    return { createObjectURL, revokeObjectURL };
  }

  it("applies a shape, reuses it for later moves, revokes it on swap and on unmount", async () => {
    const { createObjectURL, revokeObjectURL } = installObjectUrls();
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const pane = screen.getByTestId("remote-desktop-pane");
    const applied = () => pane.getAttribute("data-rd-cursor") ?? "";

    act(() => channels[0].onmessage(cursorFrame({ shapeId: 7, rgba, width: 2, height: 2 })));
    await waitFor(() => expect(applied()).toContain("blob:cursor-1"));
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    // Hotspot 1,0 rides along with the image so the remote pointer lands where the host says.
    expect(applied()).toContain("1 0");

    // The same shape arriving again on a later move reuses the cached URL: no new object URL.
    act(() => channels[0].onmessage(cursorFrame({ shapeId: 7 })));
    expect(createObjectURL).toHaveBeenCalledTimes(1);
    expect(revokeObjectURL).not.toHaveBeenCalled();

    act(() => channels[0].onmessage(cursorFrame({ shapeId: 8, rgba, width: 2, height: 2 })));
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:cursor-1"));
    await waitFor(() => expect(applied()).toContain("blob:cursor-2"));

    // A hidden cursor is `none`, never a stale image.
    act(() => channels[0].onmessage(cursorFrame({ shapeId: 8, visible: false })));
    await waitFor(() => expect(applied()).toBe("none"));

    view.unmount();
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:cursor-2"));
  });
});

describe("RemoteDesktopPane modal yielding", () => {
  function openModal() {
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    dialog.setAttribute("aria-label", "Confirm");
    dialog.setAttribute("data-test-modal", "true");
    document.body.appendChild(dialog);
    return dialog;
  }

  async function connectedNativePane() {
    surfaceBackend("wgpu", null, "rd-wgpu");
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    const pane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-native", "true"));
    act(() => channels[0].onmessage(statusFrame("connected")));
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-interactive", "true"));
    return { view, pane };
  }

  it("suspends input and hides the native surface beneath a modal off macOS, restoring on close", async () => {
    const { pane } = await connectedNativePane();
    const canvas = screen.getByTestId("remote-desktop-canvas");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", expect.objectContaining({
      rdSessionId: "rd-wgpu",
      visible: true,
    })));

    const dialog = await act(async () => openModal());
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-interactive", "false"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", {
      rdSessionId: "rd-wgpu",
      bounds: null,
      visible: false,
    }));
    // A child surface above the WebView would paint over the dialog.
    expect(pane).toHaveAttribute("data-rd-surface-visible", "false");
    expect(fireEvent.keyDown(canvas, { code: "KeyA" })).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_input", expect.anything());

    await act(async () => {
      dialog.remove();
    });
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-interactive", "true"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", {
      rdSessionId: "rd-wgpu",
      bounds: expect.objectContaining({ width: expect.any(Number) }),
      visible: true,
    }));
    expect(pane).toHaveAttribute("data-rd-surface-visible", "true");
  });

  it("keeps the macOS surface beneath a modal while still suspending input", async () => {
    platform.mac = true;
    const { pane } = await connectedNativePane();

    await act(async () => {
      openModal();
    });
    await waitFor(() => expect(pane).toHaveAttribute("data-rd-interactive", "false"));
    // WKWebView paints the dialog over the native view, so the surface may stay up.
    expect(pane).toHaveAttribute("data-rd-surface-visible", "true");
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_surface_update_bounds", expect.objectContaining({ bounds: null }));
  });

  it("hides the surface on every platform when the tab is hidden or the pane is occluded", async () => {
    platform.mac = true;
    surfaceBackend("wgpu", null, "rd-1");
    const hidden = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible={false} />);
    const hiddenPane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(hiddenPane).toHaveAttribute("data-rd-native", "true"));
    // `visible=false` is an owner hide, so it overrides the macOS keep-visible rule.
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", {
      rdSessionId: "rd-1",
      bounds: null,
      visible: false,
    }));
    expect(hiddenPane).toHaveAttribute("data-rd-surface-visible", "false");
    hidden.unmount();
    // The session outlives the pane (GAP-7), so the second phase needs a fresh session id to
    // prove the covered pane hides its own surface rather than the one that just unmounted.
    remoteDesktopStore.reset();

    invoke.mockClear();
    surfaceBackend("wgpu", null, "rd-2");
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible occluded />);
    const coveredPane = screen.getByTestId("remote-desktop-pane");
    await waitFor(() => expect(coveredPane).toHaveAttribute("data-rd-native", "true"));
    // A drop covering the pane hides it everywhere, macOS included.
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_surface_update_bounds", {
      rdSessionId: "rd-2",
      bounds: null,
      visible: false,
    }));
    expect(coveredPane).toHaveAttribute("data-rd-surface-visible", "false");
  });
});

describe("RemoteDesktopPane toolbar and stats HUD wiring", () => {
  it("toggles the stats HUD from Ctrl+Shift+Alt+S without forwarding the chord", async () => {
    const { canvas } = await connectPane();

    fireEvent.keyDown(canvas, { code: "KeyS", key: "s", ctrlKey: true, shiftKey: true, altKey: true });
    const overlay = await screen.findByTestId("remote-desktop-stats-overlay");
    expect(overlay).toBeInTheDocument();
    // The chord belongs to the viewer: the host must never see a KeyS it did not receive.
    expect(sentInputEvents()).not.toContainEqual({ kind: "key", code: "KeyS", down: true });
    expect(window.localStorage.getItem(RD_STATS_OVERLAY_STORAGE_KEY)).toBe("true");

    fireEvent.keyDown(canvas, { code: "KeyS", key: "s", ctrlKey: true, shiftKey: true, altKey: true });
    await waitFor(() => expect(screen.queryByTestId("remote-desktop-stats-overlay")).toBeNull());
    expect(window.localStorage.getItem(RD_STATS_OVERLAY_STORAGE_KEY)).toBe("false");
  });

  it("mounts the HUD from the toolbar button and re-issues the playout preset and mute", async () => {
    await connectPane();

    fireEvent.click(screen.getByRole("button", { name: "Toggle stats overlay" }));
    expect(await screen.findByTestId("remote-desktop-stats-overlay")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Competitive" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_set_options", {
      rdSessionId: "rd-1",
      options: { playoutMode: "latest_wins" },
    }));
    // The preference is written only after the host accepts the change.
    await waitFor(() => expect(window.localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBe("latest_wins"));

    fireEvent.click(screen.getByRole("button", { name: "Mute audio" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_set_audio", {
      rdSessionId: "rd-1",
      muted: true,
      volume: 1,
    }));
    await waitFor(() => expect(window.localStorage.getItem(RD_AUDIO_MUTED_STORAGE_KEY)).toBe("true"));
  });
});
