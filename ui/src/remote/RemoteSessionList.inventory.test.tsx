import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteWorkspaceMirror, type RemoteContextOption, type RemoteWorkspaceModel } from "./RemoteSessionList";

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

function renderMirror() {
  return renderMirrorWith(mirrorModel());
}

function renderMirrorWith(model: RemoteWorkspaceModel) {
  const onSelect = vi.fn();
  const onCreateTerminal = vi.fn();
  const onSelectorOpenChange = vi.fn();
  render(
    <RemoteWorkspaceMirror
      model={model}
      pending={null}
      selectorOpen
      onSelectorOpenChange={onSelectorOpenChange}
      onSelect={onSelect}
      onCreateTerminal={onCreateTerminal}
      activeMachineId={MACHINE_A}
    />,
  );
  return { onSelect, onCreateTerminal, onSelectorOpenChange };
}

function groupRows(root: HTMLElement): Array<{ projectKey: string; worktree: string; sessions: string[] }> {
  return within(root)
    .getAllByTestId("remote-worktree-group")
    .map((group) => {
      const [worktreeRow] = within(group).getAllByRole("button");
      return {
        projectKey: group.closest("[data-project-key]")?.getAttribute("data-project-key") ?? "",
        worktree: worktreeRow?.getAttribute("aria-label") ?? "",
        sessions: within(group)
          .queryAllByTestId("remote-session-row")
          .map((row) => row.textContent ?? "")
          .sort(),
      };
    });
}

function serializeGroups(root: HTMLElement): string[] {
  return groupRows(root)
    .map((group) => `${group.projectKey}|${group.worktree}|${group.sessions.join(",")}`)
    .sort();
}

function worktreeRowSummaries(root: HTMLElement): string[] {
  return within(root)
    .getAllByTestId("remote-worktree-group")
    .map((group) => {
      const [row] = within(group).getAllByRole("button");
      const aria = row?.getAttribute("aria-label") ?? "";
      const visible = row
        ? within(row).getByTestId("worktree-label-text").textContent ?? ""
        : "";
      const primary = row?.querySelector('[data-testid="worktree-primary-badge"]') ? "primary" : "-";
      return `${aria}|${visible}|${primary}`;
    })
    .sort();
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("remote session inventory rows", () => {
  it("renders every session under its own worktree, root worktree included, with no machine chooser", () => {
    renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    expect(within(dialog).getAllByRole("button", { name: "ws-ferryx-core / main" })).toHaveLength(2);
    expect(within(dialog).getByRole("button", { name: "ws-docs / main" })).toBeInTheDocument();

    // Placement, not row order: every session row sits inside the worktree that owns it, and each
    // worktree group is qualified by its machine.
    expect(serializeGroups(dialog)).toEqual(
      [
        `mach-macos-primary-01\u0000ws-ferryx-core|ws-ferryx-core / main|agent (1a),agent (1b)`,
        `mach-macos-primary-01\u0000ws-ferryx-core|ws-ferryx-core / ferryx-core (root)|root (done)`,
        `mach-linux-headless-02\u0000ws-ferryx-core|ws-ferryx-core / main|headless (1a)`,
        `mach-macos-primary-01\u0000ws-docs|ws-docs / main|`,
      ].sort(),
    );
    expect(within(dialog).getAllByTestId("remote-session-row")).toHaveLength(4);
    expect(within(dialog).getAllByTestId("worktree-primary-badge")).toHaveLength(1);

    expect(dialog.textContent).not.toMatch(/mach-macos-primary-01|mach-linux-headless-02/);
    expect(within(dialog).queryByRole("button", { name: "Machines" })).toBeNull();
  });

  it("labels worktree rows with the worktree name and marks only the root row as primary", () => {
    renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    // Accessible names keep the full project/worktree identity; the visible text carries the
    // worktree name alone, and the root worktree is never named after the active context.
    expect(worktreeRowSummaries(dialog)).toEqual(
      [
        "ws-docs / main|main|-",
        "ws-ferryx-core / ferryx-core (root)|root|primary",
        "ws-ferryx-core / main|main|-",
        "ws-ferryx-core / main|main|-",
      ].sort(),
    );
  });

  it("selects an existing session with machine, epoch, workspace and session identity and never creates a terminal", () => {
    const { onSelect, onCreateTerminal, onSelectorOpenChange } = renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    const waitingRow = within(dialog)
      .getAllByTestId("remote-session-row")
      .find((row) => row.textContent === "agent (1b)");
    expect(waitingRow).toBeDefined();
    fireEvent.click(waitingRow!);

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
    expect(onSelectorOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the second machine's identical session id as a distinct row and target", () => {
    const { onSelect } = renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    const headlessRow = within(dialog)
      .getAllByTestId("remote-session-row")
      .find((row) => row.textContent === "headless (1a)");
    fireEvent.click(headlessRow!);

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
    const { onSelect, onCreateTerminal } = renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    fireEvent.click(within(dialog).getByTestId("remote-new-terminal"));

    expect(onCreateTerminal).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("shows a status glyph only for sessions that declare activity metadata", () => {
    renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });
    const rows = within(dialog).getAllByTestId("remote-session-row");

    const working = rows.find((row) => row.textContent === "agent (1a)")!;
    const waiting = rows.find((row) => row.textContent === "agent (1b)")!;
    const done = rows.find((row) => row.textContent === "root (done)")!;
    const runningOnly = rows.find((row) => row.textContent === "headless (1a)")!;

    expect(working.querySelector('[data-status-state="working"]')).not.toBeNull();
    expect(waiting.querySelector('[data-status-state="waiting"]')).not.toBeNull();
    expect(done.querySelector('[data-status-state="done"]')).not.toBeNull();
    expect(runningOnly.querySelector("[data-status-state]")).toBeNull();
    expect(runningOnly.querySelector('[data-testid$="-indicator"]')).toBeNull();
  });

  it("marks the mirrored session active only when its epoch matches the context epoch", () => {
    const matching = mirrorModel();
    matching.context.activeTerminal = { sessionId: "s-1a-working", running: true, daemonEpoch: 101 };
    renderMirrorWith(matching);
    const activeRow = within(screen.getByTestId("remote-sidebar-drawer"))
      .getAllByTestId("remote-session-row")
      .find((row) => row.textContent === "agent (1a)");
    expect(activeRow).toHaveAttribute("aria-current", "true");
    cleanup();

    const stale = mirrorModel();
    stale.context.activeTerminal = { sessionId: "s-1a-working", running: true, daemonEpoch: 999 };
    renderMirrorWith(stale);
    const staleRow = within(screen.getByTestId("remote-sidebar-drawer"))
      .getAllByTestId("remote-session-row")
      .find((row) => row.textContent === "agent (1a)");
    expect(staleRow).not.toHaveAttribute("aria-current");
  });

  it("selects a root-worktree pane with a null slug instead of the context worktree", () => {
    const model = mirrorModel();
    model.context.activeTabId = "tab-root";
    model.context.terminalTabs = [
      { id: "tab-root", label: "root shell", worktreeSlug: null, sessionId: "s-root", daemonEpoch: 303 },
    ];
    const { onSelect } = renderMirrorWith(model);

    fireEvent.click(within(screen.getByTestId("remote-sidebar-drawer")).getByRole("tab", { name: "root shell" }));

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
    model.context.terminalTabs = [
      { id: "tab-docs", label: "docs shell", workspaceId: "ws-docs", worktreeSlug: "main" },
    ];
    const { onSelect } = renderMirrorWith(model);

    fireEvent.click(within(screen.getByTestId("remote-sidebar-drawer")).getByRole("tab", { name: "docs shell" }));

    expect(onSelect).toHaveBeenCalledWith({
      workspaceId: "ws-docs",
      worktreeSlug: "main",
      worktreeLabel: "main",
      tabId: "tab-docs",
    });
  });

  it("renders the same inventory rows in the persistent desktop sidebar and the mobile drawer", () => {
    renderMirror();
    const drawerGroups = serializeGroups(screen.getByTestId("remote-sidebar-drawer"));
    cleanup();

    desktopViewport();
    renderMirror();
    const sidebar = screen.getByTestId("remote-desktop-sidebar");

    expect(screen.queryByRole("dialog", { name: "Workspace context" })).toBeNull();
    expect(serializeGroups(sidebar)).toEqual(drawerGroups);
    expect(drawerGroups).toHaveLength(4);
    expect(within(sidebar).getAllByTestId("remote-session-row")).toHaveLength(4);
  });

  it("keeps the drawer closed until it is opened on a phone viewport", () => {
    const onSelect = vi.fn();
    render(
      <RemoteWorkspaceMirror
        model={mirrorModel()}
        pending={null}
        selectorOpen={false}
        onSelectorOpenChange={vi.fn()}
        onSelect={onSelect}
      />,
    );

    expect(screen.queryByRole("dialog", { name: "Workspace context" })).toBeNull();
    expect(screen.queryByTestId("remote-desktop-sidebar")).toBeNull();
    expect(screen.queryByTestId("remote-sidebar-drawer")).toBeNull();
  });

  it("opens the phone drawer as a full-height side panel, not a floating sheet", () => {
    renderMirror();
    const drawer = screen.getByTestId("remote-sidebar-drawer");

    expect(drawer.className).toContain("inset-y-0");
    expect(drawer.className).toContain("left-0");
    expect(drawer.className).toContain("w-[85%]");
    expect(drawer.className).not.toContain("max-h-full");
    expect(drawer.className).not.toContain("rounded-lg");
    expect(within(drawer).getByTestId("remote-new-terminal")).toBeInTheDocument();
    expect(within(drawer).getAllByTestId("remote-worktree-group").length).toBeGreaterThan(1);
  });
});
