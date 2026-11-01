import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserEngineGroup } from "./BrowserEngineGroup";

const engine = vi.hoisted(() => ({
  getBrowserEngineStatus: vi.fn(),
  installBrowserEngine: vi.fn(),
  removeBrowserEngine: vi.fn(),
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
