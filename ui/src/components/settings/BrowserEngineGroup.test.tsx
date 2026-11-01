import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserEngineGroup } from "./BrowserEngineGroup";

const engine = vi.hoisted(() => ({
  getBrowserEngineStatus: vi.fn(),
  installBrowserEngine: vi.fn(),
  removeBrowserEngine: vi.fn(),
  rollbackBrowserEngine: vi.fn(),
}));
const paths = vi.hoisted(() => ({ getBrowserEnginePaths: vi.fn() }));
const events = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("../../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserTauri")>()),
  ...engine,
}));
vi.mock("../../lib/browserExtensions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserExtensions")>()),
  ...paths,
}));
vi.mock("@tauri-apps/api/event", () => events);

function state(): string | null {
  return screen.getByTestId("browser-engine-state").getAttribute("data-state");
}

describe("BrowserEngineGroup", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    paths.getBrowserEnginePaths.mockResolvedValue({ installDir: "/data/engine", profilesDir: "/data/profiles" });
    events.listen.mockResolvedValue(() => undefined);
  });
  afterEach(cleanup);

  it("renders nothing where no engine is published", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "unsupported" });
    const { container } = render(<BrowserEngineGroup />);
    await vi.waitFor(() => expect(engine.getBrowserEngineStatus).toHaveBeenCalled());
    await vi.waitFor(() => expect(container).toBeEmptyDOMElement());
  });

  it("exposes the deep-link root and downloads the engine, then offers removal", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine.mockResolvedValue({ state: "installed", version: "156.2.0-ferryx.1" });
    const { container } = render(<BrowserEngineGroup />);
    expect(container.querySelector("#browser-engine")).not.toBeNull();
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    expect(state()).toBe("notInstalled");
    fireEvent.click(action);
    await vi.waitFor(() => expect(state()).toBe("installed"));
    expect(engine.installBrowserEngine).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("browser-engine-state")).toHaveTextContent("156.2.0-ferryx.1");
    expect(await screen.findByTestId("browser-engine-location")).toHaveTextContent("/data/engine");
  });

  it("hides the install location when the paths command fails", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "installed", version: "1.0.0" });
    paths.getBrowserEnginePaths.mockRejectedValue({ code: "IO_ERROR", message: "nope" });
    render(<BrowserEngineGroup />);
    await vi.waitFor(() => expect(state()).toBe("installed"));
    await vi.waitFor(() => expect(paths.getBrowserEnginePaths).toHaveBeenCalled());
    expect(screen.queryByTestId("browser-engine-location")).toBeNull();
  });

  it("shows an indeterminate bar while downloading and the percentage once progress arrives", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine.mockReturnValue(new Promise(() => undefined));
    let emit: (payload: { receivedBytes: number; totalBytes: number | null }) => void = () => undefined;
    events.listen.mockImplementation(async (_name: string, handler: (event: { payload: unknown }) => void) => {
      emit = (payload) => handler({ payload });
      return () => undefined;
    });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    const bar = await screen.findByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
    expect(state()).toBe("downloading");
    await vi.waitFor(() => expect(events.listen).toHaveBeenCalledWith("browser_engine_install_progress", expect.any(Function)));
    act(() => emit({ receivedBytes: 50, totalBytes: 200 }));
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "25");
    expect(screen.getByTestId("browser-engine-percent")).toHaveTextContent("25");
  });

  it("shows the structured message and code on failure and retry re-runs the install", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine
      .mockRejectedValueOnce({ code: "IO_ERROR", message: "disk full", details: {} })
      .mockResolvedValueOnce({ state: "installed", version: "1.0.0" });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByTestId("browser-engine-error-message")).toHaveTextContent("disk full");
    expect(screen.getByTestId("browser-engine-error-code")).toHaveTextContent("IO_ERROR");
    expect(state()).toBe("notInstalled");
    fireEvent.click(screen.getByTestId("browser-engine-retry"));
    await vi.waitFor(() => expect(state()).toBe("installed"));
    expect(engine.installBrowserEngine).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("falls back to a mapped sentence when the structured message is empty", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine.mockRejectedValue({ code: "UNSUPPORTED_CAPABILITY", message: "" });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    expect(await screen.findByTestId("browser-engine-error-message")).not.toHaveTextContent(/^\s*$/);
    expect(screen.getByTestId("browser-engine-error-code")).toHaveTextContent("UNSUPPORTED_CAPABILITY");
  });

  it("asks for confirmation before removing, and cancel keeps the engine", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "installed", version: "1.0.0" });
    engine.removeBrowserEngine.mockResolvedValue({ state: "notInstalled" });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    expect(engine.removeBrowserEngine).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("browser-engine-remove-confirm-cancel"));
    expect(screen.queryByTestId("browser-engine-remove-confirm")).toBeNull();
    fireEvent.click(action);
    fireEvent.click(screen.getByTestId("browser-engine-remove-confirm-yes"));
    await vi.waitFor(() => expect(state()).toBe("notInstalled"));
    expect(engine.removeBrowserEngine).toHaveBeenCalledTimes(1);
  });

  it("names a typed install failure and retries it", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine
      .mockRejectedValueOnce({
        code: "BROWSER_ENGINE_INSTALL_FAILED",
        message: "the engine manifest signature is not valid for this build",
        details: { reason: "badSignature" },
      })
      .mockResolvedValueOnce({ state: "installed", version: "156.2.0-ferryx.1", previous: null });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    const failure = await screen.findByTestId("browser-engine-failure");
    expect(failure).toHaveAttribute("data-reason", "badSignature");
    fireEvent.click(screen.getByTestId("browser-engine-retry"));
    await vi.waitFor(() => expect(action).toHaveTextContent("Remove"));
    expect(engine.installBrowserEngine).toHaveBeenCalledTimes(2);
    expect(screen.queryByTestId("browser-engine-failure")).toBeNull();
  });

  it("rolls back to the kept engine", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "installed", version: "156.2.0-ferryx.2", previous: "156.2.0-ferryx.1" });
    engine.rollbackBrowserEngine.mockResolvedValue({ state: "installed", version: "156.2.0-ferryx.1", previous: "156.2.0-ferryx.2" });
    render(<BrowserEngineGroup />);
    const rollback = await screen.findByTestId("browser-engine-rollback");
    expect(rollback).toHaveTextContent("156.2.0-ferryx.1");
    fireEvent.click(rollback);
    await vi.waitFor(() => expect(screen.getByTestId("browser-engine-rollback")).toHaveTextContent("156.2.0-ferryx.2"));
    expect(engine.rollbackBrowserEngine).toHaveBeenCalledTimes(1);
  });

  it("offers no rollback without a kept engine", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "installed", version: "156.2.0-ferryx.1", previous: null });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toHaveTextContent("Remove"));
    expect(screen.queryByTestId("browser-engine-rollback")).toBeNull();
  });

  it("keeps offering removal when the installed engine cannot run in this session", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({
      state: "installed",
      version: "156.2.0-ferryx.1",
      inactive: "wayland-session",
    });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toHaveTextContent("Remove"));
    expect(screen.getByText(/GDK_BACKEND=x11/)).toBeInTheDocument();
  });
});
