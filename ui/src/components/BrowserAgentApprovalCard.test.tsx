import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserAgentApprovalCard } from "./BrowserAgentApprovalCard";
import type { BrowserAgentApproval, BrowserAgentOverview } from "../lib/browserAgent";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listener: null as null | ((event: { payload: unknown }) => void),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    if (event === "browser_agent_approvals_changed") tauri.listener = handler;
    return () => {
      tauri.listener = null;
    };
  }),
}));

const controllerId = "0f8fad5b-d9cb-469f-a165-70867728950e";
const pairing: BrowserAgentApproval = { id: "a1", kind: "pairController", controller: controllerId, label: "qa-a", expiresInMs: 60_000 };
const grant: BrowserAgentApproval = {
  id: "a2",
  kind: "originGrant",
  controller: controllerId,
  origin: "http://127.0.0.1:18761",
  scopes: ["read", "interact"],
  expiresInMs: 60_000,
};

function overview(pending: BrowserAgentApproval[]): BrowserAgentOverview {
  return {
    settings: { requirePairing: true },
    pending,
    controllers: [{ id: controllerId, label: "qa-a", revoked: false, grants: [] }],
  };
}

function backend(pending: BrowserAgentApproval[]) {
  tauri.invoke.mockImplementation(async (command: string) => {
    if (command === "cmd_browser_agent_overview") return overview(pending);
    if (command === "cmd_browser_agent_resolve") return undefined;
    throw new Error(`unexpected ${command}`);
  });
}

describe("BrowserAgentApprovalCard", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.listener = null;
  });
  afterEach(cleanup);

  it("renders nothing while no request is pending", async () => {
    backend([]);
    const { container } = render(<BrowserAgentApprovalCard />);
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_overview"));
    expect(container).toBeEmptyDOMElement();
  });

  it("allows a pairing once", async () => {
    backend([pairing]);
    render(<BrowserAgentApprovalCard />);
    expect(await screen.findByText(/Allow "qa-a" to control the browser\?/)).toBeInTheDocument();
    expect(screen.queryByTestId("browser-agent-approval-remember")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_resolve", { approvalId: "a1", resolution: "approveOnce" }),
    );
    await vi.waitFor(() => expect(screen.queryByTestId("browser-agent-approval")).toBeNull());
  });

  it("keeps a site grant only when the persistence box is ticked", async () => {
    backend([grant]);
    render(<BrowserAgentApprovalCard />);
    expect(await screen.findByText("Let qa-a use http://127.0.0.1:18761?")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("browser-agent-approval-remember"));
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_resolve", { approvalId: "a2", resolution: "approvePersistent" }),
    );
  });

  it("denies the request", async () => {
    backend([grant]);
    render(<BrowserAgentApprovalCard />);
    fireEvent.click(await screen.findByRole("button", { name: "Deny" }));
    await vi.waitFor(() =>
      expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_agent_resolve", { approvalId: "a2", resolution: "deny" }),
    );
  });

  it("follows the backend's change event and shows the next request", async () => {
    backend([]);
    render(<BrowserAgentApprovalCard />);
    await vi.waitFor(() => expect(tauri.listener).not.toBeNull());
    act(() => tauri.listener?.({ payload: [pairing, grant] }));
    expect(await screen.findByText(/Allow "qa-a"/)).toBeInTheDocument();
    expect(screen.getByText("1 more waiting")).toBeInTheDocument();
    act(() => tauri.listener?.({ payload: [] }));
    await vi.waitFor(() => expect(screen.queryByTestId("browser-agent-approval")).toBeNull());
  });

  it("keeps the card and shows the code when resolving fails", async () => {
    backend([grant]);
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_agent_overview") return overview([grant]);
      throw { code: "BROWSER_APPROVAL_NOT_FOUND", message: "expired" };
    });
    render(<BrowserAgentApprovalCard />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("BROWSER_APPROVAL_NOT_FOUND");
    expect(screen.getByTestId("browser-agent-approval")).toBeInTheDocument();
  });
});
