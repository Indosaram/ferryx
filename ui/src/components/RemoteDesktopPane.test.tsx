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
});
