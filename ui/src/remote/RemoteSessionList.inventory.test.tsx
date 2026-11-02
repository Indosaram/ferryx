import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  paneSelectionOption,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
import { REMOTE_COLLAPSED_KEY_PREFIX } from "./RemoteSidebar";
import { remoteSidebarSignals } from "./remoteSidebarLive";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";
import type { RemoteSidebarSection } from "./useRemoteSidebarModel";

const MACHINE_A = "mach-macos-primary-01";
const MACHINE_B = "mach-linux-headless-02";

function mirrorOptions(): RemoteContextOption[] {
  return [
    { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_A },
    { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_B },
    { workspaceId: "ws-docs", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_A },
    {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_A,
      sessionId: "s-1a-working",
      sessionLabel: "agent (1a)",
      attention: "working",
      daemonEpoch: 101,
    },
    {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_A,
      sessionId: "s-1b-waiting",
      sessionLabel: "agent (1b)",
      attention: "waiting",
      daemonEpoch: 101,
    },
    {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_B,
      sessionId: "s-1a-working",
      sessionLabel: "headless (1a)",
      running: true,
      daemonEpoch: 201,
    },
    {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: null,
      worktreeLabel: "ferryx-core (root)",
      machineId: MACHINE_A,
      sessionId: "s-root-done",
      sessionLabel: "root (done)",
      attention: "done",
      daemonEpoch: 101,
    },
  ];
}

function mirrorModel(): RemoteWorkspaceModel {
  return {
    context: {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      activeTerminal: { sessionId: "s-1a-working", running: true, title: "agent (1a)" },
      activeTabId: null,
      terminalTabs: [],
    },
    options: mirrorOptions(),
  };
}

const ROWS_A = [
  { workspaceId: "ws-ferryx-core", slug: null, label: "ferryx-core", projectLabel: "ferryx-core" },
  { workspaceId: "ws-ferryx-core", slug: "main", label: "main", projectLabel: "ferryx-core" },
  { workspaceId: "ws-docs", slug: "main", label: "main", projectLabel: "docs" },
];
const ROWS_B = [{ workspaceId: "ws-ferryx-core", slug: "main", label: "main", projectLabel: "ferryx-core" }];

function section(machineId: string, label: string, rows: typeof ROWS_A, source: "desktop" | "inventory" = "desktop"): RemoteSidebarSection {
  const model = sidebarModelFixture(rows, { source });
  const selection = machineId === MACHINE_A ? { workspaceId: "ws-ferryx-core", worktreeSlug: "main" } : null;
  return { machineId, label, state: "ok", model, signals: remoteSidebarSignals(model, null, selection), error: null };
}

function sections(): RemoteSidebarSection[] {
  return [section(MACHINE_A, "Studio", ROWS_A), section(MACHINE_B, "Headless", ROWS_B)];
}

function desktopViewport() {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(min-width: 768px)",
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  }));
}

/** The phone contract as an explicit stub: every media query misses (matches:false),
 * including the desktop-sidebar probe. */
function phoneViewport() {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  }));
}

type RenderInput = { model?: RemoteWorkspaceModel; sections?: RemoteSidebarSection[]; pending?: RemoteContextOption | null };

function renderMirrorWith({ model = mirrorModel(), sections: list = sections(), pending = null }: RenderInput = {}) {
  const onSelect = vi.fn();
  const onCreateTerminal = vi.fn();
  const onSelectorOpenChange = vi.fn();
  const view = render(
    <RemoteWorkspaceMirror
      model={model}
      pending={pending}
      selectorOpen
      onSelectorOpenChange={onSelectorOpenChange}
      onSelect={onSelect}
      onCreateTerminal={onCreateTerminal}
      activeMachineId={MACHINE_A}
      sidebarSections={list}
    />,
  );
  return { onSelect, onCreateTerminal, onSelectorOpenChange, ...view };
}

function sidebarRoot(): HTMLElement {
  return screen.queryByTestId("remote-desktop-sidebar") ?? screen.getByTestId("remote-sidebar-drawer");
}

function machineSection(machineId: string): HTMLElement {
  const found = sidebarRoot().querySelector<HTMLElement>(`[data-testid="remote-sidebar-section"][data-machine-id="${machineId}"]`);
  if (!found) throw new Error(`no sidebar section for ${machineId}`);
  return found;
}

function rowButton(scope: HTMLElement, label: string, index = 0): HTMLElement {
  const rows = within(scope).getAllByTestId("remote-sidebar-row").filter((row) => row.textContent?.startsWith(label));
  const row = rows[index];
  if (!row) throw new Error(`no sidebar row ${label}`);
  return within(row).getByRole("button");
}

function strip(): HTMLElement {
  return screen.getByRole("tablist", { name: "Terminal tabs" });
}

beforeEach(() => sessionStorage.clear());

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("remote sidebar drawn by the desktop sidebar view", () => {
  it("renders one section per machine with the desktop's project order and one row per worktree", () => {
    renderMirrorWith();
    const a = machineSection(MACHINE_A);
    const b = machineSection(MACHINE_B);

    expect(within(a).getByRole("heading", { name: "Studio" })).toBeInTheDocument();
    expect(within(b).getByRole("heading", { name: "Headless" })).toBeInTheDocument();
    expect(within(a).getAllByTestId("remote-sidebar-row").map((row) => row.textContent)).toEqual(["ferryx-core", "main", "main"]);
    expect(within(b).getAllByTestId("remote-sidebar-row").map((row) => row.textContent)).toEqual(["main"]);
    // Sessions live in the terminal strip, not as nested sidebar rows; rows carry no git data.
    expect(within(sidebarRoot()).queryByRole("tab", { name: /agent/ })).toBeNull();
    expect(within(sidebarRoot()).queryAllByTestId("worktree-primary-badge")).toHaveLength(0);
    expect(within(sidebarRoot()).queryByRole("button", { name: "Machines" })).toBeNull();
  });

  it("hides the desktop-only edit controls: no add project, settings, resize or row menus", () => {
    desktopViewport();
    renderMirrorWith();
    const sidebar = sidebarRoot();
    expect(within(sidebar).queryByRole("button", { name: "Add project" })).toBeNull();
    expect(within(sidebar).queryByRole("button", { name: "Settings" })).toBeNull();
    expect(within(sidebar).queryByRole("button", { name: "Hide sidebar" })).toBeNull();
    expect(within(sidebar).queryByRole("button", { name: /More actions for/ })).toBeNull();
    expect(within(sidebar).queryByRole("button", { name: /Add worktree to/ })).toBeNull();
  });

  it("marks only the attached worktree active, on its own machine, with the guide-rail marker", () => {
    desktopViewport();
    renderMirrorWith();
    const a = machineSection(MACHINE_A);
    expect(rowButton(a, "main", 0)).toHaveAttribute("aria-current", "true");
    expect(rowButton(a, "main", 1)).not.toHaveAttribute("aria-current");
    expect(rowButton(machineSection(MACHINE_B), "main")).not.toHaveAttribute("aria-current");
    expect(within(sidebarRoot()).getAllByTestId("worktree-rail-active")).toHaveLength(1);
  });

  it("marks the attached worktree's project header current, like the desktop, and no other", () => {
    desktopViewport();
    renderMirrorWith();
    // Project headers are the named buttons outside the worktree rows (a root row shares the name).
    const header = (scope: HTMLElement, name: string) => {
      const found = within(scope).getAllByRole("button", { name }).filter((b) => !b.closest('[data-testid="remote-sidebar-row"]'));
      expect(found).toHaveLength(1);
      return found[0];
    };
    const a = machineSection(MACHINE_A);
    expect(header(a, "ferryx-core")).toHaveAttribute("aria-current", "true");
    expect(header(a, "docs")).not.toHaveAttribute("aria-current");
    expect(header(machineSection(MACHINE_B), "ferryx-core")).not.toHaveAttribute("aria-current");
  });

  it("shows worktree activity from the machine's session inventory through the shared live state", () => {
    const model = sidebarModelFixture(ROWS_A);
    const inventory = {
      machineId: MACHINE_A, revision: "1", completeness: "complete" as const, unavailableWorkspaceIds: [], fetchedAt: 1,
      entries: [{
        machineId: MACHINE_A, workspaceId: "ws-docs", worktreeSlug: "main", worktreeLabel: "main", sessionId: "s-docs",
        daemonEpoch: "1", title: "docs agent", running: true, agentType: "claude", providerSessionId: null, activityState: "waiting" as const,
      }],
    };
    const signals = remoteSidebarSignals(model, inventory, null);
    renderMirrorWith({ sections: [{ machineId: MACHINE_A, label: "Studio", state: "ok", model, signals, error: null }] });

    const docsRow = rowButton(machineSection(MACHINE_A), "main", 1);
    expect(docsRow.querySelector('[data-status-state="waiting"]')).not.toBeNull();
    expect(rowButton(machineSection(MACHINE_A), "main", 0).querySelector("[data-status-state]")).toBeNull();
    expect(screen.getByTestId("sidebar-status-footer")).toHaveTextContent("1 needs input");
  });

  it("selects a worktree with its machine identity and closes the drawer", () => {
    const { onSelect, onSelectorOpenChange, onCreateTerminal } = renderMirrorWith();
    fireEvent.click(rowButton(machineSection(MACHINE_B), "main"));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith({ workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_B });
    expect(JSON.stringify(onSelect.mock.calls[0]?.[0])).not.toMatch(/bearer|authorization|token/i);
    expect(onSelectorOpenChange).toHaveBeenCalledWith(false);
    expect(onCreateTerminal).not.toHaveBeenCalled();
  });

  it("selects the root worktree with a null slug, never the context worktree", () => {
    const { onSelect } = renderMirrorWith();
    fireEvent.click(rowButton(machineSection(MACHINE_A), "ferryx-core"));
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: "ws-ferryx-core", worktreeSlug: null, machineId: MACHINE_A }));
  });

  it("disables every row while a selection is pending and marks the pending row busy", () => {
    const pending: RemoteContextOption = { workspaceId: "ws-docs", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_A };
    const { onSelect } = renderMirrorWith({ pending });
    const a = machineSection(MACHINE_A);
    const docs = rowButton(a, "main", 1);

    expect(docs.closest('[data-testid="remote-sidebar-row"]')).toHaveAttribute("aria-busy", "true");
    expect(rowButton(a, "main", 0).closest('[data-testid="remote-sidebar-row"]')).not.toHaveAttribute("aria-busy");
    expect(rowButton(machineSection(MACHINE_B), "main").closest('[data-testid="remote-sidebar-row"]')).not.toHaveAttribute("aria-busy");
    for (const row of within(sidebarRoot()).getAllByTestId("remote-sidebar-row")) {
      expect(within(row).getByRole("button")).toHaveAttribute("aria-disabled", "true");
    }
    fireEvent.click(docs);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("seeds folds from the desktop and keeps this browser's folds in session storage only", () => {
    const model = sidebarModelFixture(ROWS_A, { collapsedGroupIds: ["ws-docs"] });
    const list = [{ machineId: MACHINE_A, label: "Studio", state: "ok" as const, model, signals: {}, error: null }];
    renderMirrorWith({ sections: list });
    const a = machineSection(MACHINE_A);
    expect(within(a).getByRole("button", { name: "Expand docs" })).toHaveAttribute("aria-expanded", "false");
    expect(within(a).getAllByTestId("remote-sidebar-row")).toHaveLength(2);

    fireEvent.click(within(a).getByRole("button", { name: "Collapse ferryx-core" }));
    expect(within(a).queryAllByTestId("remote-sidebar-row")).toHaveLength(0);
    expect(JSON.parse(sessionStorage.getItem(`${REMOTE_COLLAPSED_KEY_PREFIX}${MACHINE_A}`) ?? "null")).toEqual(["ws-docs", "ws-ferryx-core"]);
    // The published model itself is never mutated: folds are local to this browser.
    expect(model.collapsedGroupIds).toEqual(["ws-docs"]);

    cleanup();
    renderMirrorWith({ sections: list });
    expect(within(machineSection(MACHINE_A)).queryAllByTestId("remote-sidebar-row")).toHaveLength(0);
  });

  it("marks a section rebuilt from the inventory as not synced with the desktop", () => {
    renderMirrorWith({ sections: [section(MACHINE_A, "Studio", ROWS_A, "inventory"), section(MACHINE_B, "Headless", ROWS_B)] });
    expect(within(machineSection(MACHINE_A)).getByTestId("remote-sidebar-not-synced")).toBeInTheDocument();
    expect(within(machineSection(MACHINE_B)).queryByTestId("remote-sidebar-not-synced")).toBeNull();
  });

  it("keeps the last list while a machine is offline and says so", () => {
    const stale = section(MACHINE_A, "Studio", ROWS_A);
    renderMirrorWith({ sections: [{ ...stale, state: "offline", error: { code: "OFFLINE", message: "gone" } }] });
    expect(within(machineSection(MACHINE_A)).getByRole("status")).toHaveTextContent("Can't reach this computer right now. Showing the last list.");
    expect(within(machineSection(MACHINE_A)).getAllByTestId("remote-sidebar-row")).toHaveLength(3);
  });

  it("renders the same rows in the persistent desktop sidebar and the mobile drawer, at their own density", () => {
    phoneViewport();
    renderMirrorWith();
    const drawerRows = within(sidebarRoot()).getAllByTestId("remote-sidebar-row").map((row) => row.textContent);
    const touchRow = rowButton(machineSection(MACHINE_A), "main");
    expect(touchRow).toHaveClass("min-h-11");
    expect(within(touchRow).getByText("main")).toHaveClass("text-sm");
    cleanup();

    desktopViewport();
    renderMirrorWith();
    expect(screen.queryByRole("dialog", { name: "Workspace context" })).toBeNull();
    expect(within(sidebarRoot()).getAllByTestId("remote-sidebar-row").map((row) => row.textContent)).toEqual(drawerRows);
    const compactRow = rowButton(machineSection(MACHINE_A), "main");
    expect(compactRow).toHaveClass("h-7");
    expect(compactRow).not.toHaveClass("min-h-11");
  });

  it("keeps the drawer closed until it is opened on a phone viewport", () => {
    render(
      <RemoteWorkspaceMirror model={mirrorModel()} pending={null} selectorOpen={false} onSelectorOpenChange={vi.fn()} onSelect={vi.fn()} sidebarSections={sections()} />,
    );
    expect(screen.queryByRole("dialog", { name: "Workspace context" })).toBeNull();
    expect(screen.queryByTestId("remote-desktop-sidebar")).toBeNull();
    expect(screen.queryByTestId("remote-sidebar-drawer")).toBeNull();
  });

  it("opens the phone drawer as a full-height side panel, not a floating sheet", () => {
    renderMirrorWith();
    const drawer = screen.getByTestId("remote-sidebar-drawer");
    expect(drawer.className).toContain("inset-y-0");
    expect(drawer.className).toContain("left-0");
    expect(drawer.className).toContain("w-[88%]");
    expect(drawer.className).toContain("max-w-[340px]");
    expect(drawer.className).toContain("animate-drawer-in");
    expect(drawer.className).not.toContain("rounded-lg");
    expect(within(drawer).getAllByTestId("remote-sidebar-row").length).toBeGreaterThan(1);
  });

  describe("swipe left to dismiss the phone drawer", () => {
    function swipe(drawer: HTMLElement, distance: number, stepMs: number) {
      let now = 1_000;
      vi.spyOn(performance, "now").mockImplementation(() => now);
      vi.spyOn(drawer, "getBoundingClientRect").mockReturnValue({ width: 340, height: 800 } as DOMRect);
      const row = within(drawer).getAllByTestId("remote-sidebar-row")[0];
      fireEvent.pointerDown(row, { clientX: 300, clientY: 300, button: 0 });
      for (let i = 1; i <= 4; i += 1) {
        now += stepMs;
        fireEvent.pointerMove(row, { clientX: 300 - (distance * i) / 4, clientY: 302 });
      }
      now += stepMs;
      fireEvent.pointerUp(row, { clientX: 300 - distance, clientY: 302 });
    }

    it("slides the drawer out from the finger and then closes it", async () => {
      const { onSelectorOpenChange, onSelect } = renderMirrorWith();
      const drawer = screen.getByTestId("remote-sidebar-drawer");

      swipe(drawer, 140, 120);

      expect(drawer.style.transform).toBe("translate3d(-100%, 0, 0)");
      await waitFor(() => expect(onSelectorOpenChange).toHaveBeenCalledWith(false));
      // the drag over a row did not select it
      expect(onSelect).not.toHaveBeenCalled();
    });

    it("springs back on a short slow swipe", () => {
      const { onSelectorOpenChange } = renderMirrorWith();
      const drawer = screen.getByTestId("remote-sidebar-drawer");

      swipe(drawer, 40, 120);

      expect(onSelectorOpenChange).not.toHaveBeenCalled();
      expect(drawer.style.transform).toBe("translate3d(0, 0, 0)");
    });
  });
});

describe("remote terminal tab strip", () => {
  function tabNames() {
    return within(strip()).getAllByRole("tab").map((tab) => tab.getAttribute("aria-label"));
  }

  it("lists the attached worktree's sessions on its own machine only, with declared activity", () => {
    renderMirrorWith();
    expect(tabNames().sort()).toEqual(["agent (1a) (working)", "agent (1b) (waiting)"]);
    const tabs = within(strip()).getAllByRole("tab");
    expect(tabs.find((tab) => tab.textContent === "agent (1a)")?.querySelector('[data-testid="tab-working-indicator"]')).not.toBeNull();
    expect(tabs.find((tab) => tab.textContent === "agent (1b)")?.querySelector('[data-testid="tab-waiting-indicator"]')).not.toBeNull();
  });

  it("marks the mirrored session selected only when its epoch matches the context epoch", () => {
    const matching = mirrorModel();
    matching.context.activeTerminal = { sessionId: "s-1a-working", running: true, daemonEpoch: 101 };
    renderMirrorWith({ model: matching });
    expect(within(strip()).getByRole("tab", { name: "agent (1a) (working)" })).toHaveAttribute("aria-selected", "true");
    cleanup();

    const stale = mirrorModel();
    stale.context.activeTerminal = { sessionId: "s-1a-working", running: true, daemonEpoch: 999 };
    renderMirrorWith({ model: stale });
    expect(within(strip()).getByRole("tab", { name: "agent (1a) (working)" })).toHaveAttribute("aria-selected", "false");
  });

  it("selects an existing session with machine, epoch, workspace and session identity and never creates a terminal", () => {
    const { onSelect, onCreateTerminal } = renderMirrorWith();
    fireEvent.click(within(strip()).getByRole("tab", { name: "agent (1b) (waiting)" }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith({
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_A,
      daemonEpoch: 101,
      sessionId: "s-1b-waiting",
    });
    expect(JSON.stringify(onSelect.mock.calls[0]?.[0])).not.toMatch(/bearer|authorization|token/i);
    expect(onCreateTerminal).not.toHaveBeenCalled();
  });

  it("keeps the second machine's identical session id as a distinct target once that machine is attached", () => {
    const onSelect = vi.fn();
    render(
      <RemoteWorkspaceMirror
        model={mirrorModel()}
        pending={null}
        selectorOpen
        onSelectorOpenChange={vi.fn()}
        onSelect={onSelect}
        activeMachineId={MACHINE_B}
        sidebarSections={sections()}
      />,
    );
    expect(within(strip()).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["headless (1a)"]);
    fireEvent.click(within(strip()).getByRole("tab", { name: "headless (1a)" }));
    expect(onSelect).toHaveBeenCalledWith({
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_B,
      daemonEpoch: 201,
      sessionId: "s-1a-working",
    });
  });

  it("creates a terminal only from the explicit New terminal control", () => {
    const { onSelect, onCreateTerminal } = renderMirrorWith();
    fireEvent.click(screen.getByTestId("remote-new-terminal"));
    expect(onCreateTerminal).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("selects a root-worktree pane with a null slug instead of the context worktree", () => {
    const model = mirrorModel();
    model.context.activeTabId = "tab-root";
    model.context.terminalTabs = [{ id: "tab-root", label: "root shell", worktreeSlug: null, sessionId: "s-root", daemonEpoch: 303 }];
    const { onSelect } = renderMirrorWith({ model });

    fireEvent.click(within(strip()).getByRole("tab", { name: "root shell" }));

    expect(onSelect).toHaveBeenCalledWith({
      workspaceId: "ws-ferryx-core",
      worktreeSlug: null,
      worktreeLabel: null,
      tabId: "tab-root",
      sessionId: "s-root",
      daemonEpoch: 303,
    });
  });

  it("selects a pane that declares its own workspace instead of the context workspace", () => {
    const model = mirrorModel();
    model.context.terminalTabs = [{ id: "tab-docs", label: "docs shell", workspaceId: "ws-docs", worktreeSlug: "main" }];
    const { onSelect } = renderMirrorWith({ model });

    fireEvent.click(within(strip()).getByRole("tab", { name: "docs shell" }));

    expect(onSelect).toHaveBeenCalledWith({ workspaceId: "ws-docs", worktreeSlug: "main", worktreeLabel: "main", tabId: "tab-docs" });
  });

  it("disables the tabs and marks the pending session busy while a selection is pending", () => {
    const pending: RemoteContextOption = { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main", machineId: MACHINE_A, sessionId: "s-1b-waiting" };
    renderMirrorWith({ pending });
    for (const tab of within(strip()).getAllByRole("tab")) expect(tab).toBeDisabled();
    expect(within(strip()).getByRole("tab", { name: "agent (1b) (waiting)" })).toHaveAttribute("aria-busy", "true");
    expect(screen.getByTestId("remote-new-terminal")).toBeDisabled();
  });
});

describe("paneSelectionOption", () => {
  it("uses the pane's own declared workspace and slug instead of the active context", () => {
    const option = paneSelectionOption(mirrorModel(), {
      id: "tab-docs",
      label: "docs",
      workspaceId: "ws-docs",
      worktreeSlug: "feature/x",
      worktreeLabel: "feature-x",
      sessionId: "s-docs",
    });
    expect(option).toEqual({
      workspaceId: "ws-docs",
      worktreeSlug: "feature/x",
      worktreeLabel: "feature-x",
      tabId: "tab-docs",
      sessionId: "s-docs",
    });
  });

  it("inherits workspaceId, worktreeSlug and label from the context when the pane declares neither", () => {
    const option = paneSelectionOption(mirrorModel(), {
      id: "tab-shell",
      label: "shell",
      sessionId: "s-shell",
    });
    expect(option).toEqual({
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      tabId: "tab-shell",
      sessionId: "s-shell",
    });
  });

  it("honours a declared null slug: the pane's own choice beats the context", () => {
    const option = paneSelectionOption(mirrorModel(), {
      id: "tab-root",
      label: "root",
      worktreeSlug: null,
      sessionId: "s-root",
    });
    expect(option).not.toBeNull();
    expect(option!.worktreeSlug).toBeNull();
    expect(option!.worktreeLabel).toBeNull();
    expect(option!.workspaceId).toBe("ws-ferryx-core");
  });

  it("spreads daemonEpoch only when the pane declares one", () => {
    const withEpoch = paneSelectionOption(mirrorModel(), {
      id: "tab-a",
      label: "a",
      sessionId: "s-a",
      daemonEpoch: 303,
    });
    expect(withEpoch).toHaveProperty("daemonEpoch", 303);

    const nullEpoch = paneSelectionOption(mirrorModel(), {
      id: "tab-b",
      label: "b",
      sessionId: "s-b",
      daemonEpoch: null,
    });
    expect(nullEpoch).not.toHaveProperty("daemonEpoch");

    const noEpoch = paneSelectionOption(mirrorModel(), { id: "tab-c", label: "c", sessionId: "s-c" });
    expect(noEpoch).not.toHaveProperty("daemonEpoch");
  });

  it("returns null when the pane declares its own workspaceId as null", () => {
    expect(
      paneSelectionOption(mirrorModel(), { id: "tab-n", label: "n", workspaceId: null }),
    ).toBeNull();
  });
});
