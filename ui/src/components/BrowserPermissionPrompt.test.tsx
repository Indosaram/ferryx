import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserMediaCaptureIndicator, BrowserPermissionPrompts } from "./BrowserPermissionPrompt";
import type { BrowserPermissionPrompt } from "../lib/browserSitePermissions";

type Handler = (event: { payload: unknown }) => void;
const events = vi.hoisted(() => ({ handlers: new Map<string, Handler[]>() }));
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    events.handlers.set(name, [...(events.handlers.get(name) ?? []), handler]);
    return () => events.handlers.set(name, (events.handlers.get(name) ?? []).filter((h) => h !== handler));
  }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));

function fire(name: string, payload: unknown) {
  act(() => {
    for (const handler of events.handlers.get(name) ?? []) handler({ payload });
  });
}

const cameraPrompt: BrowserPermissionPrompt = {
  requestId: 7,
  browserId: "b1",
  profile: "default",
  origin: "https://meet.test",
  kinds: ["camera", "microphone"],
  canRemember: true,
};

async function subscribed(name: string) {
  await vi.waitFor(() => expect(events.handlers.get(name)?.length ?? 0).toBeGreaterThan(0));
}

describe("BrowserPermissionPrompts", () => {
  beforeEach(() => {
    events.handlers.clear();
    ipc.invoke.mockReset();
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_permission_pending") return [];
      if (command === "cmd_browser_permission_decide") return true;
      throw new Error(`unexpected ${command}`);
    });
  });
  afterEach(cleanup);

  it("shows the site and what it wants, and Block sends a remembered deny", async () => {
    render(<BrowserPermissionPrompts browserId="b1" />);
    await subscribed("browser_permission_prompt");
    fire("browser_permission_prompt", cameraPrompt);
    const prompt = await screen.findByTestId("browser-permission-prompt");
    expect(prompt).toHaveTextContent("https://meet.test");
    expect(prompt).toHaveTextContent("camera and microphone");
    fireEvent.click(screen.getByRole("button", { name: "Block" }));
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_browser_permission_decide", { requestId: 7, allow: false, remember: true });
    expect(screen.queryByTestId("browser-permission-prompt")).toBeNull();
  });

  it("Allow without Remember grants only this request", async () => {
    render(<BrowserPermissionPrompts browserId="b1" />);
    await subscribed("browser_permission_prompt");
    fire("browser_permission_prompt", cameraPrompt);
    fireEvent.click(await screen.findByRole("checkbox", { name: "Remember this decision" }));
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_browser_permission_decide", { requestId: 7, allow: true, remember: false });
  });

  it("ignores other tabs' prompts and offers no Remember for a private tab", async () => {
    render(<BrowserPermissionPrompts browserId="b1" />);
    await subscribed("browser_permission_prompt");
    fire("browser_permission_prompt", { ...cameraPrompt, browserId: "b2" });
    expect(screen.queryByTestId("browser-permission-prompt")).toBeNull();
    fire("browser_permission_prompt", { ...cameraPrompt, requestId: 8, profile: "private", canRemember: false });
    await screen.findByTestId("browser-permission-prompt");
    expect(screen.queryByRole("checkbox", { name: "Remember this decision" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_browser_permission_decide", { requestId: 8, allow: true, remember: false });
  });

  it("drops a prompt the host closed and shows prompts pending before mount", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_permission_pending") return [cameraPrompt];
      throw new Error(`unexpected ${command}`);
    });
    render(<BrowserPermissionPrompts browserId="b1" />);
    await screen.findByTestId("browser-permission-prompt");
    await subscribed("browser_permission_prompt_closed");
    fire("browser_permission_prompt_closed", { browserId: "b1", requestId: 7 });
    expect(screen.queryByTestId("browser-permission-prompt")).toBeNull();
  });

  it("shows the structured error code when the decision fails", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_permission_pending") return [];
      throw { code: "INTERNAL_ERROR", message: "gone" };
    });
    render(<BrowserPermissionPrompts browserId="b1" />);
    await subscribed("browser_permission_prompt");
    fire("browser_permission_prompt", cameraPrompt);
    fireEvent.click(await screen.findByRole("button", { name: "Allow" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("INTERNAL_ERROR");
  });
});

describe("BrowserMediaCaptureIndicator", () => {
  beforeEach(() => {
    events.handlers.clear();
    ipc.invoke.mockReset();
  });
  afterEach(cleanup);

  it("shows camera and microphone while the tab captures them", async () => {
    ipc.invoke.mockResolvedValue({ browserId: "b1", origin: null, camera: false, microphone: false, screen: false });
    render(<BrowserMediaCaptureIndicator browserId="b1" />);
    await subscribed("browser_media_capture_changed");
    expect(screen.queryByTestId("browser-media-capture-indicator")).toBeNull();
    fire("browser_media_capture_changed", { browserId: "b1", origin: "https://meet.test", camera: true, microphone: true, screen: false });
    expect(await screen.findByLabelText("Camera in use by https://meet.test")).toBeInTheDocument();
    expect(screen.getByLabelText("Microphone in use by https://meet.test")).toBeInTheDocument();
    fire("browser_media_capture_changed", { browserId: "b1", origin: null, camera: false, microphone: false, screen: false });
    expect(screen.queryByTestId("browser-media-capture-indicator")).toBeNull();
  });

  it("says the state is unknown when it cannot be read", async () => {
    ipc.invoke.mockRejectedValue({ code: "INTERNAL_ERROR", message: "x" });
    render(<BrowserMediaCaptureIndicator browserId="b1" />);
    expect(await screen.findByTestId("browser-media-capture-unknown")).toBeInTheDocument();
  });
});
