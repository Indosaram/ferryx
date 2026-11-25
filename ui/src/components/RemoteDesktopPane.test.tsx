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
import { remoteDesktopStore } from "../state/remoteDesktopStore";
import type { RdInputEvent } from "../lib/remoteDesktop";

function statusFrame(state: string, reason: string | null = null): Uint8Array {
  const body = new TextEncoder().encode(JSON.stringify({ state, reason }));
  const bytes = new Uint8Array(1 + body.length);
  bytes[0] = 0x02;
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

/** Every input event the pane pushed through `cmd_rd_input`, in order. */
function sentInputEvents(): RdInputEvent[] {
  return invoke.mock.calls
    .filter(([command]) => command === "cmd_rd_input")
    .flatMap(([, args]) => (args as { events: RdInputEvent[] }).events);
}

function connectCalls(): unknown[][] {
  return invoke.mock.calls.filter(([command]) => command === "cmd_rd_connect");
}

beforeEach(() => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_rd_connect") return { rdSessionId: "rd-1" };
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

    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));

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

  it("ignores input before the session reports connected", async () => {
    render(<RemoteDesktopPane hostId="host-1" label="Studio" visible />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("cmd_rd_connect", expect.anything()));
    const canvas = screen.getByTestId("remote-desktop-canvas");
    expect(fireEvent.keyDown(canvas, { code: "KeyA" })).toBe(true);
    expect(invoke).not.toHaveBeenCalledWith("cmd_rd_input", expect.anything());
  });
});
