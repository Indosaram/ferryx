import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserAgentAccessGroup } from "./BrowserAgentAccessGroup";
import type { BrowserAgentOverview } from "../../lib/browserAgent";

const agent = vi.hoisted(() => ({
  getBrowserAgentOverview: vi.fn(),
  revokeBrowserAgent: vi.fn(),
  setBrowserAgentSettings: vi.fn(),
  onBrowserAgentApprovals: vi.fn(),
}));

vi.mock("../../lib/browserAgent", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserAgent")>()),
  ...agent,
}));

const id = "0f8fad5b-d9cb-469f-a165-70867728950e";

function backend(state: BrowserAgentOverview) {
  agent.getBrowserAgentOverview.mockImplementation(async () => structuredClone(state));
  agent.revokeBrowserAgent.mockImplementation(async (controllerId: string, origin?: string) => {
    state.controllers = state.controllers.flatMap((controller) => {
      if (controller.id !== controllerId) return [controller];
      if (origin === undefined) return [];
      return [{ ...controller, grants: controller.grants.filter((grant) => grant.origin !== origin) }];
    });
    return true;
  });
  agent.setBrowserAgentSettings.mockImplementation(async (settings: BrowserAgentOverview["settings"]) => {
    state.settings = settings;
  });
  agent.onBrowserAgentApprovals.mockImplementation(async () => () => undefined);
}

describe("BrowserAgentAccessGroup", () => {
  beforeEach(() => vi.resetAllMocks());
  afterEach(cleanup);

  it("lists paired controllers with their sites and revokes one site", async () => {
    backend({
      settings: { requirePairing: true },
      pending: [],
      controllers: [
        {
          id,
          label: "qa-a",
          revoked: false,
          grants: [{ origin: "http://127.0.0.1:18761", scopes: ["read", "interact"], persistent: true }],
        },
        { id: "gone", label: "old", revoked: true, grants: [] },
      ],
    });
    render(<BrowserAgentAccessGroup />);
    expect(await screen.findByText("qa-a")).toBeInTheDocument();
    expect(screen.queryByText("old")).toBeNull();
    expect(screen.getByText(/read pages, click and type, kept/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Revoke http://127.0.0.1:18761 for qa-a" }));
    await vi.waitFor(() => expect(agent.revokeBrowserAgent).toHaveBeenCalledWith(id, "http://127.0.0.1:18761"));
    expect(await screen.findByText("No sites allowed.")).toBeInTheDocument();
  });

  it("removes a controller", async () => {
    backend({ settings: { requirePairing: true }, pending: [], controllers: [{ id, label: "qa-a", revoked: false, grants: [] }] });
    render(<BrowserAgentAccessGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove qa-a" }));
    await vi.waitFor(() => expect(agent.revokeBrowserAgent).toHaveBeenCalledWith(id));
    expect(await screen.findByTestId("browser-agent-none")).toBeInTheDocument();
  });

  it("switches the pairing requirement", async () => {
    backend({ settings: { requirePairing: true }, pending: [], controllers: [] });
    render(<BrowserAgentAccessGroup />);
    const toggle = await screen.findByRole("switch", { name: "Require paired agents" });
    await vi.waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toHaveAttribute("aria-checked", "true");
    fireEvent.click(toggle);
    await vi.waitFor(() => expect(agent.setBrowserAgentSettings).toHaveBeenCalledWith({ requirePairing: false }));
    await vi.waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });
});
