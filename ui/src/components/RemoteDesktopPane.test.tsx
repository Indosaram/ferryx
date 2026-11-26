import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

import { RemoteDesktopPane } from "./RemoteDesktopPane";
import { forwardCapturedRemoteDesktopShortcut, type RdSessionMeta } from "../lib/remoteDesktop";

function statusFrame(state: string, reason: string | null = null): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ state, reason }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = 0x02;
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
  bytes[0] = 0x03;
  bytes.set(body, 1);
  return bytes;
}

function clipboardFrame(text: string): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ text }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = 0x05;
  bytes.set(body, 1);
  return bytes;
}

function videoFrame(width: number, height: number): Uint8Array {
  const bytes = new Uint8Array(17 + 2);
  const view = new DataView(bytes.buffer);
  bytes[0] = 0x01;
  view.setUint32(1, width, true);
  view.setUint32(5, height, true);
  view.setBigUint64(9, 1n, true);
  bytes.set([0xff, 0xd8], 17);
  return bytes;
}

beforeEach(() => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_rd_connect") return { rdSessionId: "rd-1" };
    return undefined;
  });
});

afterEach(() => {
  cleanup();
  invoke.mockReset();
  channels.length = 0;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("RemoteDesktopPane", () => {
  it("connects with the host id, shows status and disconnects on unmount", async () => {
    const view = render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    expect(screen.getByTestId("remote-desktop-status")).toHaveTextContent("Connecting");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.objectContaining({ hostId: "host-1" })));
    act(() => channels[0].onmessage(statusFrame("reconnecting", "network lost")));
    expect(screen.getByTestId("remote-desktop-status")).toHaveTextContent("Reconnecting… network lost");
    view.unmount();
    // Either the cleanup or the late connect resolution disconnects; both must name the session.
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_disconnect", { rdSessionId: "rd-1" }));
  });

  it("shows a code-mapped error with a reconnect action", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_rd_connect") throw { code: "RD_HOST_OFFLINE", message: "offline", details: {} };
      return undefined;
    });
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    expect(await screen.findByRole("alert")).toHaveTextContent("offline or unreachable");
    invoke.mockImplementation(async (command: string) => (command === "cmd_rd_connect" ? { rdSessionId: "rd-2" } : undefined));
    fireEvent.click(screen.getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
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

  it("ignores input before the session reports connected", async () => {
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const canvas = screen.getByTestId("remote-desktop-canvas");
    expect(fireEvent.keyDown(canvas, { code: "KeyA" })).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_input", expect.anything());
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
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
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

    // Manual disconnect transitions to ended and invokes cmd_rd_disconnect
    fireEvent.click(screen.getByTestId("rd-disconnect-btn"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("cmd_rd_disconnect", { rdSessionId: "rd-1" }),
    );
    expect(screen.getByTestId("remote-desktop-pane")).toHaveAttribute("data-rd-state", "ended");
  });
});
