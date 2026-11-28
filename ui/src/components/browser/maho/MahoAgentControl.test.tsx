// Maho agent control contracts, driven through the real browser store.
//
// The agent IPC module is mocked at its boundary (overview read, approval and help
// events, reclaim/revoke/resolve commands); everything below - the hook, the chip,
// the Agent Tasks section, the tab borrow chip and the controlled banner - runs as
// shipped. Each case asserts the state transitions a user sees, and that a
// structured `{ code }` failure surfaces by code rather than by message text.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";

import {
  MahoAgentControlledBanner,
  MahoAgentTasksSection,
  MahoControlActivityChip,
  MahoTabBorrowChip,
  agentTaskStatusText,
  controlStatePhrase,
  controlStatusText,
  planeLabel,
  stopLabelForState,
  stateAllowsStop,
  useBrowserAgentActivity,
  type BrowserAgentActivity,
  type MahoAgentTaskHint,
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
  helpHandler: null as null | Handler<{ browserId: string; reason?: string | null; requestId?: string | null }>,
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
  onBrowserAgentHelpRequest: vi.fn(async (handler: Handler<{ browserId: string; reason?: string | null; requestId?: string | null }>) => {
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
  taskHints,
  visibleBrowserId,
  compact,
  chipProps,
}: {
  store: BrowserStore;
  onSelectTab?: (browserId: string) => void;
  bannerBrowserId?: string;
  taskHints?: Record<string, MahoAgentTaskHint>;
  visibleBrowserId?: string | null;
  compact?: boolean;
  chipProps?: Pick<ComponentProps<typeof MahoControlActivityChip>, "onDetails" | "showDetails">;
}) {
  const activity = useBrowserAgentActivity({ store, taskHints, visibleBrowserId });
  return (
    <div>
      <MahoControlActivityChip activity={activity} compact={compact} {...chipProps} />
      <MahoAgentTasksSection activity={activity} onSelectTab={onSelectTab} />
      <div data-testid="tab-row-chips">
        {activity.controlledBrowserIds.map((browserId) => (
          <MahoTabBorrowChip
            key={browserId}
            browserId={browserId}
            controllerLabel={activity.controllerLabelFor(browserId)}
          />
        ))}
      </div>
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

async function mountSurfacesWith(options: {
  store: BrowserStore;
  bannerBrowserId?: string;
  taskHints?: Record<string, MahoAgentTaskHint>;
  visibleBrowserId?: string | null;
  compact?: boolean;
}) {
  render(<Harness {...options} />);
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
      expect(view.getByTestId("maho-control-activity-chip")).toHaveAttribute("aria-label", "qa-agent - Acting on this tab - Docs"),
    );
    expect(within(view.getByTestId("maho-control-activity-chip")).getByRole("button", { name: "Stop" })).toBeEnabled();

    const section = view.getByTestId("maho-agent-tasks");
    expect(within(section).getByRole("button", { name: "Running: Docs" })).toBeInTheDocument();
    expect(within(section).getByTestId("maho-agent-tasks-count")).toHaveTextContent("1");

    // The tab row's own chip, outside the Agent Tasks section.
    const rowChips = within(view.getByTestId("tab-row-chips"));
    expect(rowChips.getByTestId("maho-tab-borrow-chip")).toHaveTextContent("qa-agent");
    expect(rowChips.getByTestId("maho-tab-borrow-chip")).toHaveAttribute("data-browser-id", "docs");

    const banner = await view.findByTestId("maho-agent-controlled-banner");
    expect(banner).toHaveAttribute("data-browser-id", "docs");
    expect(banner).toHaveTextContent("qa-agent");
    expect(within(banner).getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("renders nothing agent-related while no agent holds a tab", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs" });
    const view = await mountSurfaces(store, "docs");

    expect(view.getByTestId("badge-count")).toHaveTextContent("0");
    expect(view.queryByTestId("maho-control-activity-chip")).toBeNull();
    expect(within(view.getByTestId("tab-row-chips")).queryByTestId("maho-tab-borrow-chip")).toBeNull();
    expect(view.queryByTestId("maho-agent-controlled-banner")).toBeNull();
    expect(view.queryByTestId("maho-agent-tasks")).toBeNull();
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
      expect(view.getByTestId("maho-control-activity-chip")).toHaveAttribute("aria-label", "qa-agent - Acting on this tab - Docs"),
    );
    expect(view.getByTestId("maho-agent-controlled-banner")).toBeInTheDocument();
  });

  it("focuses a controlled tab from the Agent Tasks section", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const onSelectTab = vi.fn();
    const view = await mountSurfaces(store, "docs", onSelectTab);

    fireEvent.click(within(view.getByTestId("maho-agent-tasks")).getByRole("button", { name: "Running: Docs" }));
    expect(onSelectTab).toHaveBeenCalledWith("docs");
  });
});

describe("handoff and take over", () => {
  it("takes the tab back on Take over without closing it", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(view.getByTestId("maho-agent-controlled-banner")).getByRole("button", { name: "Stop" }));

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

    fireEvent.click(within(helpBar).getByRole("button", { name: "Complete" }));

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

  it("disconnects only the selected task from the Agent Tasks section", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfaces(store, "docs");

    fireEvent.click(within(view.getByTestId("maho-agent-tasks")).getByRole("button", { name: "Disconnect Docs" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("docs", true));
    expect(host.revoke).not.toHaveBeenCalled();
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

    fireEvent.click(within(view.getByTestId("maho-agent-controlled-banner")).getByRole("button", { name: "Stop" }));

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
    const { container } = render(<MahoAgentControlledBanner activity={idleActivity()} browserId="docs" />);
    expect(container).toBeEmptyDOMElement();
  });
});

/** An activity with no agent on any tab: the shape every surface must tolerate. */
function idleActivity(overrides: Partial<BrowserAgentActivity> = {}): BrowserAgentActivity {
  return {
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
    taskStatusFor: () => "running",
    planeFor: () => "embedded",
    receiptFor: () => null,
    stateFor: () => "idle",
    takeOver: async () => undefined,
    doneHandoff: async () => undefined,
    respondToHelp: async () => undefined,
    revokeController: async () => undefined,
    resolveApproval: async () => undefined,
    ...overrides,
  };
}

describe("task lifecycle", () => {
  it("labels each status the way the reference's task rows do", () => {
    expect(agentTaskStatusText("queued")).toBe("Queued");
    expect(agentTaskStatusText("running")).toBe("Running");
    expect(agentTaskStatusText("waiting")).toBe("Needs you");
    expect(agentTaskStatusText("done")).toBe("Done");
    expect(agentTaskStatusText("failed")).toBe("Failed");
  });

  it("shows the status prefix and the receipt summary on the row", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfacesWith({
      store,
      bannerBrowserId: "docs",
      taskHints: { docs: { status: "running", plane: "cli", receipt: "Opened 3 pages" } },
    });

    const row = await view.findByTestId("maho-agent-task-row");
    expect(row).toHaveAttribute("data-status", "running");
    expect(within(row).getByRole("button", { name: "Running: Docs" })).toBeInTheDocument();
    expect(within(row).getByTestId("maho-agent-task-receipt")).toHaveTextContent("Opened 3 pages");
    // The receipt is echoed on the chip so the collapsed drawer still shows the last update.
    expect(view.getByTestId("maho-control-activity-receipt")).toHaveTextContent("Opened 3 pages");
  });

  it("says Needs you and offers no Disconnect on a finished task", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    addTab(store, "pricing", { title: "Pricing", agentControlled: true });
    const view = await mountSurfacesWith({
      store,
      taskHints: { docs: { status: "waiting" }, pricing: { status: "done", receipt: "Compared 4 plans" } },
    });

    expect(await view.findByRole("button", { name: "Needs you: Docs" })).toBeInTheDocument();
    expect(view.getByRole("button", { name: "Done: Pricing" })).toBeInTheDocument();
    // A terminal row is history: there is nothing left to disconnect.
    expect(view.queryByRole("button", { name: "Disconnect Pricing" })).toBeNull();
    expect(view.getByRole("button", { name: "Disconnect Docs" })).toBeInTheDocument();
  });

  it("distinguishes reading from acting on the chip", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfacesWith({ store, taskHints: { docs: { status: "running", verb: "reading" } } });

    const chip = await view.findByTestId("maho-control-activity-chip");
    expect(chip).toHaveAttribute("data-state", "reading");
    expect(chip).toHaveAttribute("aria-label", "qa-agent - Reading this tab - Docs");
    expect(within(chip).getByText("AI - Reading this tab")).toBeInTheDocument();
  });

  it("badges the CLI/MCP plane the controller holds its grant through", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfacesWith({
      store,
      bannerBrowserId: "docs",
      taskHints: { docs: { status: "running", plane: "mcp" } },
    });

    const chip = await view.findByTestId("maho-control-activity-chip");
    expect(chip).toHaveAttribute("data-plane", "mcp");
    expect(within(chip).getByTestId("maho-control-activity-plane")).toHaveTextContent("MCP");
    expect(within(chip).getByText("AI - Acting on this tab")).toBeInTheDocument();
    // The hover title spells the plane out in the same line, for a reader who cannot see the tag.
    expect(chip).toHaveAttribute("title", "AI - MCP - Acting on this tab");
    expect(within(view.getByTestId("maho-agent-controlled-banner")).getByTestId("maho-agent-banner-plane")).toHaveTextContent("MCP");
  });

  it("says a background tab is being worked on elsewhere, never 'this tab'", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    addTab(store, "mail", { title: "Mail" });
    const view = await mountSurfacesWith({ store, visibleBrowserId: "mail" });

    const chip = await view.findByTestId("maho-control-activity-chip");
    expect(chip).toHaveAttribute("aria-label", "qa-agent - Working in another tab - Docs");
    expect(within(chip).getByText("AI - Working in another tab")).toBeInTheDocument();
    // Attribution only ever replaces the live verbs, never an approval wait.
    expect(controlStatePhrase("acting", { background: true })).toBe("Working in another tab");
    expect(controlStatePhrase("acting", { otherWindow: true })).toBe("Working in another Maho window");
    expect(controlStatePhrase("waitingApproval", { background: true })).toBe("Waiting for approval");
  });

  it("reads a failed task as an error on the banner and offers no Stop", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    addTab(store, "broken", { title: "Broken", agentControlled: true });
    const view = await mountSurfacesWith({
      store,
      bannerBrowserId: "broken",
      taskHints: { docs: { status: "done" }, broken: { status: "failed" } },
    });

    const banner = await view.findByTestId("maho-agent-controlled-banner");
    expect(banner).toHaveAttribute("data-state", "error");
    expect(within(banner).getByTestId("maho-agent-banner-state")).toHaveTextContent("Action failed");
    expect(within(banner).queryByRole("button", { name: /Stop|Revoke/ })).toBeNull();
    expect(view.getByRole("button", { name: "Failed: Broken" })).toBeInTheDocument();
  });

  it("says Disconnected when no live controller holds the lease", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    host.overview = { settings: { requirePairing: true }, pending: [], controllers: [] };
    const view = await mountSurfaces(store, "docs");

    const banner = await view.findByTestId("maho-agent-controlled-banner");
    await vi.waitFor(() => expect(banner).toHaveAttribute("data-state", "disconnected"));
    expect(view.getByTestId("maho-agent-banner-state")).toHaveTextContent("Disconnected");
  });

  it("turns the chip into a strip: glyph and kill switch only", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const view = await mountSurfacesWith({ store, compact: true, taskHints: { docs: { status: "running" } } });

    const chip = await view.findByTestId("maho-control-activity-chip");
    expect(chip).toHaveAttribute("data-compact", "true");
    expect(view.queryByText("AI - Acting on this tab")).toBeNull();
    expect(within(chip).queryByRole("button", { name: "Details" })).toBeNull();
    expect(within(chip).getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("expands its own task rows on Details when no drawer handler is supplied", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    render(<Harness store={store} />);
    await vi.waitFor(() => expect(host.helpHandler).not.toBeNull());

    const chip = await screen.findByTestId("maho-control-activity-chip");
    // The section is mounted once by the harness, so the chip's own copy is the second one.
    expect(screen.getAllByTestId("maho-agent-tasks")).toHaveLength(1);
    fireEvent.click(within(chip).getByRole("button", { name: "Details" }));
    await vi.waitFor(() => expect(screen.getAllByTestId("maho-agent-tasks")).toHaveLength(2));
  });

  it("hides Details when the surrounding surface already shows the rows", async () => {
    const store = makeStore();
    addTab(store, "docs", { title: "Docs", agentControlled: true });
    const onDetails = vi.fn();
    render(
      <Harness
        store={store}
        chipProps={{ onDetails, showDetails: false }}
      />,
    );
    await vi.waitFor(() => expect(host.helpHandler).not.toBeNull());

    const chip = await screen.findByTestId("maho-control-activity-chip");
    expect(within(chip).queryByRole("button", { name: "Details" })).toBeNull();
    expect(onDetails).not.toHaveBeenCalled();
  });
});

describe("help bar by request id", () => {
  it("completes exactly the request it was asked about", async () => {
    const store = makeStore();
    addTab(store, "signin", { title: "Sign in", agentControlled: true });
    addTab(store, "pay", { title: "Pay", agentControlled: true });
    const view = await mountSurfaces(store, "signin");

    act(() => host.helpHandler?.({ browserId: "signin", requestId: "req-signin", reason: "sign in" }));
    act(() => host.helpHandler?.({ browserId: "pay", requestId: "req-pay", reason: "confirm card" }));

    const helpBar = await view.findByTestId("maho-agent-help-bar");
    expect(helpBar).toHaveAttribute("data-request-id", "req-signin");
    fireEvent.click(within(helpBar).getByRole("button", { name: "Complete" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("signin", false));
    // The other tab's request is untouched: it is addressed by its own id.
    expect(host.reclaim).not.toHaveBeenCalledWith("pay", false);
    expect(host.reclaim).not.toHaveBeenCalledWith("pay", true);
  });

  it("cancels by request id, reclaiming the tab for the human", async () => {
    const store = makeStore();
    addTab(store, "signin", { title: "Sign in", agentControlled: true });
    const view = await mountSurfaces(store, "signin");

    act(() => host.helpHandler?.({ browserId: "signin", requestId: "req-1", reason: "sign in" }));
    const helpBar = await view.findByTestId("maho-agent-help-bar");
    fireEvent.click(within(helpBar).getByRole("button", { name: "Cancel" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("signin", true));
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-help-bar")).toBeNull());
    // Cancel keeps the human in control: the lease is gone, not handed back.
    await vi.waitFor(() => expect(store.getState().tabs.signin.agentControlled).toBe(false));
  });

  it("falls back to the tab id for a payload that carries no request id", async () => {
    const store = makeStore();
    addTab(store, "signin", { title: "Sign in", agentControlled: true });
    const view = await mountSurfaces(store, "signin");

    act(() => host.helpHandler?.({ browserId: "signin" }));
    const helpBar = await view.findByTestId("maho-agent-help-bar");
    expect(helpBar).toHaveAttribute("data-request-id", "signin");
    fireEvent.click(within(helpBar).getByRole("button", { name: "Complete" }));

    await vi.waitFor(() => expect(host.reclaim).toHaveBeenCalledWith("signin", false));
    await vi.waitFor(() => expect(view.queryByTestId("maho-agent-help-bar")).toBeNull());
  });
});

describe("label helpers", () => {
  it("composes the status line with the plane omitted for the embedded agent", () => {
    expect(planeLabel("embedded")).toBe("");
    expect(planeLabel("cli")).toBe("CLI");
    expect(planeLabel("mcp")).toBe("MCP");
    expect(controlStatusText("acting", "embedded")).toBe("AI - Acting on this tab");
    expect(controlStatusText("acting", "cli")).toBe("AI - CLI - Acting on this tab");
    expect(controlStatusText("idle", "cli")).toBe("");
  });

  it("withdraws authority on a blocked controller and interrupts a live one", () => {
    expect(stopLabelForState("paused")).toBe("Revoke");
    expect(stopLabelForState("waitingApproval")).toBe("Revoke");
    expect(stopLabelForState("acting")).toBe("Stop");
    expect(stateAllowsStop("acting")).toBe(true);
    expect(stateAllowsStop("disconnected")).toBe(false);
    expect(stateAllowsStop("error")).toBe(false);
    expect(stateAllowsStop("idle")).toBe(false);
  });
});
