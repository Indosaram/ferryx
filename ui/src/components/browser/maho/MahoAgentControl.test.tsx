// Maho agent control contracts, driven through the real browser store.
//
// The agent IPC module is mocked at its boundary (overview read, approval and help
// events, reclaim/revoke/resolve commands); everything below - the hook, the chip,
// the Agent Tasks section, the tab borrow chip and the controlled banner - runs as
// shipped. Each case asserts the state transitions a user sees, and that a
// structured `{ code }` failure surfaces by code rather than by message text.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  MahoAgentControlledBanner,
  MahoAgentTasksSection,
  MahoControlActivityChip,
  MahoTabBorrowChip,
  useBrowserAgentActivity,
  type BrowserAgentActivity,
} from "./MahoAgentControl";
import {
  createBrowserStore,
  type BrowserStore,
  type BrowserStoreState,
  type UpsertBrowserTabInput,
} from "../../../state/browserStore";
import type { BrowserAgentApproval, BrowserAgentOverview } from "../../../lib/browserAgent";

type Handler<T> = (payload: T) => void;

const host = vi.hoisted(() => ({
  overview: null as BrowserAgentOverview | null,
  approvalsHandler: null as null | Handler<BrowserAgentApproval[]>,
  helpHandler: null as null | Handler<{ browserId: string; reason?: string | null }>,
  getOverview: vi.fn(),
  reclaim: vi.fn(),
  revoke: vi.fn(),
  resolve: vi.fn(),
}));

vi.mock("../../../lib/browserAgent", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../lib/browserAgent")>()),
  getBrowserAgentOverview: host.getOverview,
  reclaimBrowserAgent: host.reclaim,
  revokeBrowserAgent: host.revoke,
  resolveBrowserAgentApproval: host.resolve,
  onBrowserAgentApprovals: vi.fn(async (handler: Handler<BrowserAgentApproval[]>) => {
    host.approvalsHandler = handler;
    return () => {
      if (host.approvalsHandler === handler) host.approvalsHandler = null;
    };
  }),
  onBrowserAgentHelpRequest: vi.fn(async (handler: Handler<{ browserId: string; reason?: string | null }>) => {
    host.helpHandler = handler;
    return () => {
      if (host.helpHandler === handler) host.helpHandler = null;
    };
  }),
}));

const controllerId = "0f8fad5b-d9cb-469f-a165-70867728950e";

function overview(pending: BrowserAgentApproval[] = []): BrowserAgentOverview {
  return {
    settings: { requirePairing: true },
    pending,
    controllers: [{ id: controllerId, label: "qa-agent", revoked: false, grants: [] }],
  };
}

function makeStore(): BrowserStore {
  const store = createBrowserStore({ storage: null, now: () => 1_700_000_000_000 });
  store.setState((state: BrowserStoreState) => state);
  return store;
}

function addTab(store: BrowserStore, browserId: string, overrides: Partial<UpsertBrowserTabInput> = {}): void {
  const result = store.upsertTab({ browserId, url: `https://${browserId}.example.com/`, title: browserId, ...overrides });
  expect(result.ok, `seeded ${browserId}`).toBe(true);
}

/** Renders every surface over one hook instance, the way the shell composes them. */
function Harness({
  store,
  onSelectTab,
  bannerBrowserId,
}: {
  store: BrowserStore;
  onSelectTab?: (browserId: string) => void;
  bannerBrowserId?: string;
}) {
  const activity = useBrowserAgentActivity({ store });
  return (
    <div>
      <MahoControlActivityChip activity={activity} />
      <MahoAgentTasksSection activity={activity} onSelectTab={onSelectTab} />
      {activity.controlledBrowserIds.map((browserId) => (
        <MahoTabBorrowChip
          key={browserId}
          browserId={browserId}
          controllerLabel={activity.controllerLabelFor(browserId)}
        />
      ))}
      {bannerBrowserId ? <MahoAgentControlledBanner activity={activity} browserId={bannerBrowserId} /> : null}
      <span data-testid="badge-count">{activity.badgeCount}</span>
    </div>
  );
}

async function mountSurfaces(store: BrowserStore, bannerBrowserId?: string, onSelectTab?: (id: string) => void) {
  render(<Harness store={store} bannerBrowserId={bannerBrowserId} onSelectTab={onSelectTab} />);
  await vi.waitFor(() => expect(host.getOverview).toHaveBeenCalled());
  await vi.waitFor(() => expect(host.helpHandler).not.toBeNull());
  return screen;
}

beforeEach(() => {
  vi.clearAllMocks();
  host.approvalsHandler = null;
  host.helpHandler = null;
  host.overview = overview();
  host.getOverview.mockImplementation(async () => host.overview);
  host.reclaim.mockImplementation(async () => undefined);
  host.revoke.mockImplementation(async () => true);
  host.resolve.mockImplementation(async () => undefined);
});

afterEach(cleanup);

describe("useBrowserAgentActivity surfaces", () => {
  it("counts an agent-opened controlled tab across chip, section, borrow chip and banner", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfaces(store, "docs");

    expect(view.getByTestId("badge-count")).toHaveTextContent("1");
    await vi.waitFor(() =>
      expect(view.getByTestId("maho-control-activity-chip")).toHaveTextContent("qa-agent"),
    );
    expect(view.getByTestId("maho-control-activity-chip")).toHaveTextContent("1");

    const section = view.getByTestId("maho-agent-tasks");
    expect(within(section).getByText("Docs")).toBeInTheDocument();
    expect(within(section).getByTestId("maho-agent-tasks-count")).toHaveTextContent("1");

    expect(view.getByTestId("maho-tab-borrow-chip")).toHaveTextContent("qa-agent");

    const banner = await view.findByTestId("maho-agent-controlled-banner");
    expect(banner).toHaveAttribute("data-browser-id", "docs");
    expect(banner).toHaveTextContent("qa-agent");
    expect(banner).toHaveTextContent("is controlling this tab");
  });

  it("renders nothing agent-related while no agent holds a tab", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs" });
    const view = await mountSurfaces(store, "docs");

    expect(view.getByTestId("badge-count")).toHaveTextContent("0");
    expect(view.queryByTestId("maho-control-activity-chip")).toBeNull();
    expect(view.queryByTestId("maho-tab-borrow-chip")).toBeNull();
    expect(view.queryByTestId("maho-agent-controlled-banner")).toBeNull();
    expect(within(view.getByTestId("maho-agent-tasks")).getByText("No agent activity")).toBeInTheDocument();
  });

  it("increments the badge when the backend adds a controlled tab", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs" });
    const view = await mountSurfaces(store, "docs");
    expect(view.getByTestId("badge-count")).toHaveTextContent("0");

    act(() => {
      store.upsertTab({ browserId: "docs", agentControlled: true });
    });

    await vi.waitFor(() => expect(view.getByTestId("badge-count")).toHaveTextContent("1"));
    await vi.waitFor(() =>
      expect(view.getByTestId("maho-control-activity-chip")).toHaveTextContent("qa-agent"),
    );
    expect(view.getByTestId("maho-agent-controlled-banner")).toBeInTheDocument();
  });

  it("focuses a controlled tab from the Agent Tasks section", async () => {    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const onSelectTab = vi.fn();
    const view = await mountSurfaces(store, "docs", onSelectTab);

    fireEvent.click(within(view.getByTestId("maho-agent-tasks")).getByRole("button", { name: "Focus Docs" }));
    expect(onSelectTab).toHaveBeenCalledWith("docs");
  });
});

describe("handoff and take over", () => {
  it("takes the tab back on Take over without closing it", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(view.getByTestId("maho-agent-controlled-banner")).getByRole("button", { name: "Take over" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("docs", true));
    // The tab survives: reclaiming a lease never closes or reloads the page.
    expect(store.getState().tabs.docs).toBeDefined();
    await vi.waitFor(() => expect(store.getState().tabs.docs.agentControlled).toBe(false));
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-controlled-banner")).toBeNull());
  });

  it("shows the help bar with the reason and completes the handoff on Done", async () => {
    const store = makeStore();
    addTab(store, "signin", { title: "Sign in", agentControlled: true });
    const view = await mountSurfaces(store, "signin");

    act(() => host.helpHandler?.({ browserId: "signin", reason: "human sign-in required" }));

    const helpBar = await view.findByTestId("maho-agent-help-bar");
    expect(helpBar).toHaveTextContent("human sign-in required");
    expect(view.getByTestId("badge-count")).toHaveTextContent("2");

    fireEvent.click(within(helpBar).getByRole("button", { name: "Done" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("signin", false));
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-help-bar")).toBeNull());
    // The lease was handed back, so the tab is still the agent's to drive.
    expect(store.getState().tabs.signin.agentControlled).toBe(true);
  });

  it("takes over a pending handoff from the sidebar section", async () => {
    const store = makeStore();
    addTab(store, "signin", { title: "Sign in", agentControlled: true });
    const view = await mountSurfaces(store, "signin");

    act(() => host.helpHandler?.({ browserId: "signin" }));

    const handoffRow = await view.findByTestId("maho-agent-task-handoff");
    fireEvent.click(within(handoffRow).getByRole("button", { name: "Take over" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("signin", true));
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-task-handoff")).toBeNull());
    await vi.waitFor(() => expect(store.getState().tabs.signin.agentControlled).toBe(false));
  });

  it("revokes a controller from the Agent Tasks section", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(view.getByTestId("maho-agent-tasks")).getByRole("button", { name: "Revoke qa-agent on Docs" }));

    await vi.waitFor(() => expect(host.revoke).toHaveBeenCalledWith(controllerId));
  });
});

describe("pending approvals in the section", () => {
  it("approves and denies pending requests by id", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    host.overview = overview([
      { id: "a1", kind: "originGrant", controller: controllerId, origin: "https://docs.example.com", scopes: ["read"], expiresInMs: 60_000 },
    ]);
    const view = await mountSurfaces(store, "docs");

    const first = await view.findByTestId("maho-agent-task-approval");
    expect(first).toHaveTextContent("https://docs.example.com");
    fireEvent.click(within(first).getByRole("button", { name: "Approve" }));
    await vi.waitFor(() => expect(host.resolve).toHaveBeenCalledWith("a1", "approveOnce"));
    // The resolved request leaves the list before the next event arrives.
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-task-approval")).toBeNull());

    act(() =>
      host.approvalsHandler?.([
        { id: "a2", kind: "pairController", controller: controllerId, label: "qa-agent", expiresInMs: 60_000 },
      ]),
    );
    const second = await view.findByText(/Allow "qa-agent" to control the browser/);
    const row = second.closest("[data-testid='maho-agent-task-approval']");
    expect(row).not.toBeNull();
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Deny" }));
    await vi.waitFor(() => expect(host.resolve).toHaveBeenCalledWith("a2", "deny"));
  });
});

describe("structured IPC failures", () => {
  it("surfaces { code } from a failed take over without reading the message", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    host.reclaim.mockRejectedValue({ code: "AGENT_SESSION_GONE", message: "session 12345 vanished" });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(view.getByTestId("maho-agent-controlled-banner")).getByRole("button", { name: "Take over" }));

    const alert = await view.findByTestId("maho-agent-banner-error");
    expect(alert).toHaveTextContent("AGENT_SESSION_GONE");
    expect(alert).not.toHaveTextContent("session 12345 vanished");
    // A refused reclaim leaves the agent in control; the surface stays up.
    expect(store.getState().tabs.docs.agentControlled).toBe(true);
    expect(view.getByTestId("maho-agent-controlled-banner")).toBeInTheDocument();
  });

  it("surfaces { code } from a failed approval resolution", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    host.overview = overview([
      { id: "a1", kind: "nativeCapability", controller: controllerId, tool: "browser.screenshot", expiresInMs: 60_000 },
    ]);
    host.resolve.mockRejectedValue({ code: "BROWSER_APPROVAL_NOT_FOUND", message: "expired" });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(await view.findByTestId("maho-agent-task-approval")).getByRole("button", { name: "Approve" }));

    expect(await view.findByTestId("maho-agent-tasks-error")).toHaveTextContent("BROWSER_APPROVAL_NOT_FOUND");
  });

  it("surfaces { code } from a failed overview read", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    host.getOverview.mockRejectedValue({ code: "BROWSER_AGENT_UNAVAILABLE", message: "host offline" });
    const view = await mountSurfaces(store, "docs");

    expect(await view.findByTestId("maho-agent-tasks-error")).toHaveTextContent("BROWSER_AGENT_UNAVAILABLE");
  });
});

describe("MahoAgentControlledBanner isolation", () => {
  it("renders nothing for a tab no agent controls", () => {
    const activity: BrowserAgentActivity = {
      activeTasks: [],
      pendingApprovals: [],
      controlledBrowserIds: [],
      handoffRequests: [],
      badgeCount: 0,
      errorCode: null,
      busy: false,
      controllers: [],
      controllerLabelFor: () => null,
      controllerIdFor: () => null,
      takeOver: async () => undefined,
      doneHandoff: async () => undefined,
      revokeController: async () => undefined,
      resolveApproval: async () => undefined,
    };
    const { container } = render(<MahoAgentControlledBanner activity={activity} browserId="docs" />);
    expect(container).toBeEmptyDOMElement();
  });
});
