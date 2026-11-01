import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserEngineGroup } from "./BrowserEngineGroup";

const engine = vi.hoisted(() => ({
  getBrowserEngineStatus: vi.fn(),
  installBrowserEngine: vi.fn(),
  removeBrowserEngine: vi.fn(),
  rollbackBrowserEngine: vi.fn(),
}));

vi.mock("../../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserTauri")>()),
  ...engine,
}));

describe("BrowserEngineGroup", () => {
  beforeEach(() => vi.resetAllMocks());
  afterEach(cleanup);

  it("renders nothing where no engine is published", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "unsupported" });
    const { container } = render(<BrowserEngineGroup />);
    await vi.waitFor(() => expect(engine.getBrowserEngineStatus).toHaveBeenCalled());
    await vi.waitFor(() => expect(container).toBeEmptyDOMElement());
  });

  it("downloads the engine and then offers removal", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine.mockResolvedValue({ state: "installed", version: "156.2.0-ferryx.1" });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toHaveTextContent("Download"));
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    await vi.waitFor(() => expect(action).toHaveTextContent("Remove"));
    expect(engine.installBrowserEngine).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/Installed \(156\.2\.0-ferryx\.1\)/)).toBeInTheDocument();
  });

  it("shows the error code when the download fails", async () => {
    engine.getBrowserEngineStatus.mockResolvedValue({ state: "notInstalled" });
    engine.installBrowserEngine.mockRejectedValue({ code: "INTERNAL_ERROR", message: "checksum" });
    render(<BrowserEngineGroup />);
    const action = await screen.findByTestId("browser-engine-action");
    await vi.waitFor(() => expect(action).toBeEnabled());
    fireEvent.click(action);
    expect(await screen.findByRole("alert")).toHaveTextContent("INTERNAL_ERROR");
    expect(action).toHaveTextContent("Download");
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
});
