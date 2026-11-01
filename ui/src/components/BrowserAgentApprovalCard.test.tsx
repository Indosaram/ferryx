import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserAgentApprovalCard } from "./BrowserAgentApprovalCard";
import type { BrowserAgentApproval, BrowserAgentOverview } from "../lib/browserAgent";

const agent = vi.hoisted(() => ({
  getBrowserAgentOverview: vi.fn(),
  resolveBrowserAgentApproval: vi.fn(),
  onBrowserAgentApprovals: vi.fn(),
  listener: null as null | ((pending: unknown) => void),
}));

vi.mock("../lib/browserAgent", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/browserAgent")>()),
  getBrowserAgentOverview: agent.getBrowserAgentOverview,
  resolveBrowserAgentApproval: agent.resolveBrowserAgentApproval,
  onBrowserAgentApprovals: agent.onBrowserAgentApprovals,
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
  agent.getBrowserAgentOverview.mockImplementation(async () => overview(pending));
  agent.resolveBrowserAgentApproval.mockImplementation(async () => undefined);
  agent.onBrowserAgentApprovals.mockImplementation(async (listener: (pending: unknown) => void) => {
    agent.listener = listener;
    return () => {
      agent.listener = null;
    };
  });
}

describe("BrowserAgentApprovalCard", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    agent.listener = null;
  });
  afterEach(cleanup);

  it("renders nothing while no request is pending", async () => {
    backend([]);
    const { container } = render(<BrowserAgentApprovalCard />);
    await vi.waitFor(() => expect(agent.getBrowserAgentOverview).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("allows a pairing once", async () => {
    backend([pairing]);
    render(<BrowserAgentApprovalCard />);
    expect(await screen.findByText(/Allow "qa-a" to control the browser\?/)).toBeInTheDocument();
    expect(screen.queryByTestId("browser-agent-approval-remember")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    await vi.waitFor(() => expect(agent.resolveBrowserAgentApproval).toHaveBeenCalledWith("a1", "approveOnce"));
    await vi.waitFor(() => expect(screen.queryByTestId("browser-agent-approval")).toBeNull());
  });

  it("keeps a site grant only when the persistence box is ticked", async () => {
    backend([grant]);
    render(<BrowserAgentApprovalCard />);
    expect(await screen.findByText("Let qa-a use http://127.0.0.1:18761?")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("browser-agent-approval-remember"));
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    await vi.waitFor(() => expect(agent.resolveBrowserAgentApproval).toHaveBeenCalledWith("a2", "approvePersistent"));
  });

  it("denies the request", async () => {
    backend([grant]);
    render(<BrowserAgentApprovalCard />);
    fireEvent.click(await screen.findByRole("button", { name: "Deny" }));
    await vi.waitFor(() => expect(agent.resolveBrowserAgentApproval).toHaveBeenCalledWith("a2", "deny"));
  });

  it("follows the backend's change event and shows the next request", async () => {
    backend([]);
    render(<BrowserAgentApprovalCard />);
    await vi.waitFor(() => expect(agent.listener).not.toBeNull());
    act(() => agent.listener?.([pairing, grant]));
    expect(await screen.findByText(/Allow "qa-a"/)).toBeInTheDocument();
    expect(screen.getByText("1 more waiting")).toBeInTheDocument();
    act(() => agent.listener?.([]));
    await vi.waitFor(() => expect(screen.queryByTestId("browser-agent-approval")).toBeNull());
  });

  it("keeps the card and shows the code when resolving fails", async () => {
    backend([grant]);
    agent.resolveBrowserAgentApproval.mockRejectedValue({ code: "BROWSER_APPROVAL_NOT_FOUND", message: "expired" });
    render(<BrowserAgentApprovalCard />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("BROWSER_APPROVAL_NOT_FOUND");
    expect(screen.getByTestId("browser-agent-approval")).toBeInTheDocument();
  });
});
