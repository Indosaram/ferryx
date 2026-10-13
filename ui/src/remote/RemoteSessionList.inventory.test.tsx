import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  paneSelectionOption,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";

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
    expect(drawer.className).toContain("w-[88%]");
    expect(drawer.className).toContain("max-w-[340px]");
    expect(drawer.className).toContain("animate-drawer-in");
    expect(drawer.className).not.toContain("max-h-full");
    expect(drawer.className).not.toContain("rounded-lg");
    expect(within(drawer).getByTestId("remote-new-terminal")).toBeInTheDocument();
    expect(within(drawer).getAllByTestId("remote-worktree-group").length).toBeGreaterThan(1);
  });

  it("sizes touch rows for thumbs under a matchMedia matches:false stub", () => {
    phoneViewport();
    renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });

    const rows = within(dialog).getAllByTestId("remote-session-row");
    const workingRow = rows.find((row) => row.textContent === "agent (1a)")!;
    expect(workingRow).toHaveClass("min-h-11");
    expect(workingRow).not.toHaveClass("min-h-[24px]");
    expect(within(workingRow).getByText("agent (1a)")).toHaveClass("text-[14px]");
    expect(within(workingRow).getByTestId("tab-terminal-icon")).toHaveClass("size-4");

    const [firstGroup] = within(dialog).getAllByTestId("remote-worktree-group");
    const worktreeRow = within(firstGroup).getAllByRole("button")[0];
    expect(worktreeRow).toHaveClass("min-h-11");
    expect(worktreeRow).not.toHaveClass("h-7");
    expect(worktreeRow.querySelector('[data-testid="worktree-label-text"]')).toHaveClass("text-[14px]");

    const section = document.querySelector('section[aria-label="ws-ferryx-core"]');
    expect(section).not.toBeNull();
    const header = section!.firstElementChild as HTMLElement;
    expect(header.className).toContain("text-[12px]");
    expect(header.className).toContain("font-medium");
    expect(header.className).toContain("text-chat-foreground-secondary");
    expect(header.className).toContain("border-b");
    expect(header.className).not.toContain("uppercase");
  });

  it("labels touch rows with the T3 status vocabulary as a caption outside the row button", () => {
    phoneViewport();
    renderMirror();
    const dialog = screen.getByRole("dialog", { name: "Workspace context" });
    const rows = within(dialog).getAllByTestId("remote-session-row");
    const captionOf = (row: HTMLElement | undefined) =>
      row?.parentElement?.querySelector('[data-testid="row-status-label"]') ?? null;

    const workingRow = rows.find((row) => row.textContent === "agent (1a)")!;
    const workingCaption = captionOf(workingRow) as HTMLElement | null;
    expect(workingCaption).not.toBeNull();
    expect(workingCaption!.textContent).toBe("Working");
    expect(workingCaption!.className).toContain("text-status-working");
    expect(workingCaption!.className).toContain("text-[12px]");
    expect(workingCaption!.className).toContain("tabular-nums");
    expect(workingCaption!.className).toContain("right-2");
    expect(workingRow.querySelector('[data-testid="row-status-label"]')).toBeNull();

    const waitingRow = rows.find((row) => row.textContent === "agent (1b)")!;
    const waitingCaption = captionOf(waitingRow) as HTMLElement | null;
    expect(waitingCaption).not.toBeNull();
    expect(waitingCaption!.textContent).toBe("Approval");
    expect(waitingCaption!.className).toContain("text-status-warning");

    expect(captionOf(rows.find((row) => row.textContent === "root (done)")!)).toBeNull();
    expect(captionOf(rows.find((row) => row.textContent === "headless (1a)")!)).toBeNull();
  });

  it("renders pane rows with touch height, labels and a status caption", () => {
    phoneViewport();
    const model = mirrorModel();
    model.context.terminalTabs = [
      {
        id: "tab-editor",
        label: "editor",
        sessionId: "s-editor",
        worktreeSlug: "main",
        activityState: "working",
      },
    ];
    renderMirrorWith(model);

    const tab = screen.getByRole("tab", { name: "editor (working)" });
    expect(tab).toHaveClass("min-h-11");
    expect(tab).not.toHaveClass("min-h-[24px]");
    expect(within(tab).getByText("editor")).toHaveClass("text-[14px]");
    const caption = tab.parentElement?.querySelector(
      '[data-testid="row-status-label"]',
    ) as HTMLElement | null;
    expect(caption).not.toBeNull();
    expect(caption!.textContent).toBe("Working");
  });

  it("keeps desktop sidebar rows at min-h-[24px] when matchMedia matches:true", () => {
    desktopViewport();
    const model = mirrorModel();
    model.context.terminalTabs = [
      { id: "tab-editor", label: "editor", sessionId: "s-editor", worktreeSlug: "main", activityState: "working" },
    ];
    renderMirrorWith(model);
    const sidebar = screen.getByTestId("remote-desktop-sidebar");

    for (const row of within(sidebar).getAllByTestId("remote-session-row")) {
      expect(row).toHaveClass("min-h-[24px]");
      expect(row).not.toHaveClass("min-h-11");
    }
    const tab = within(sidebar).getByRole("tab", { name: "editor (working)" });
    expect(tab).toHaveClass("min-h-[24px]");
    expect(tab).not.toHaveClass("min-h-11");
    expect(within(sidebar).queryAllByTestId("row-status-label")).toHaveLength(0);
    const [firstGroup] = within(sidebar).getAllByTestId("remote-worktree-group");
    const worktreeRow = within(firstGroup).getAllByRole("button")[0];
    expect(worktreeRow).toHaveClass("h-7");
    expect(worktreeRow).not.toHaveClass("min-h-11");
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
