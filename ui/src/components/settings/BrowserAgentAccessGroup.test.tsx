import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserAgentAccessGroup } from "./BrowserAgentAccessGroup";
import type { BrowserAgentOverview } from "../../lib/browserAgent";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

const id = "0f8fad5b-d9cb-469f-a165-70867728950e";

function backend(state: BrowserAgentOverview) {
  tauri.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case "cmd_browser_agent_overview":
        return structuredClone(state);
      case "cmd_browser_agent_revoke":
        state.controllers = state.controllers.flatMap((controller) => {
          if (controller.id !== args?.controllerId) return [controller];
          if (args?.origin == null) return [];
          return [{ ...controller, grants: controller.grants.filter((grant) => grant.origin !== args.origin) }];
        });
        return true;
      case "cmd_browser_agent_set_settings":
        state.settings = args?.settings as BrowserAgentOverview["settings"];
        return undefined;
      default:
        throw new Error(`unexpected ${command}`);
    }
  });
}

describe("BrowserAgentAccessGroup", () => {
  beforeEach(() => tauri.invoke.mockReset());
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
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_revoke", { controllerId: id, origin: "http://127.0.0.1:18761" }),
    );
    expect(await screen.findByText("No sites allowed.")).toBeInTheDocument();
  });

  it("removes a controller", async () => {
    backend({ settings: { requirePairing: true }, pending: [], controllers: [{ id, label: "qa-a", revoked: false, grants: [] }] });
    render(<BrowserAgentAccessGroup />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove qa-a" }));
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_revoke", { controllerId: id, origin: null }));
    expect(await screen.findByTestId("browser-agent-none")).toBeInTheDocument();
  });

  it("switches the pairing requirement", async () => {
    backend({ settings: { requirePairing: true }, pending: [], controllers: [] });
    render(<BrowserAgentAccessGroup />);
    const toggle = await screen.findByRole("switch", { name: "Require paired agents" });
    await vi.waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toHaveAttribute("aria-checked", "true");
    fireEvent.click(toggle);
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_set_settings", { settings: { requirePairing: false } }),
    );
    await vi.waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });
});
