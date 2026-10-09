import { describe, expect, it } from "vitest";
import type { RemoteContextOption, RemoteTerminalTabInfo } from "./RemoteSessionList";
import {
  buildRemoteInventory,
  inventoryProjectKey,
  inventorySessionKey,
  inventoryWorktreeKey,
  isSameInventorySelection,
  resolveInventoryActivity,
  rollupActivity,
  type RemoteInventoryEntry,
} from "./remoteInventoryView";

const MACHINE_A = "mach-macos-primary-01";
const MACHINE_B = "mach-linux-headless-02";

function fixtureOptions(): RemoteContextOption[] {
  return [
    {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      machineId: MACHINE_A,
      projectLabel: "ferryx-core",
    },
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

function entryOf(
  inventory: ReturnType<typeof buildRemoteInventory>,
  machineId: string,
  workspaceId: string,
  slug: string | null,
  sessionId: string,
) {
  return inventory.projects
    .find((project) => project.machineId === machineId && project.workspaceId === workspaceId)
    ?.worktrees.find((worktree) => worktree.slug === slug)
    ?.entries.find((entry) => entry.sessionId === sessionId);
}

function paneTabIds(entries: RemoteInventoryEntry[]): string[] {
  return entries.flatMap((entry) => (entry.kind === "pane" ? [entry.tabId] : []));
}

describe("remote inventory grouping", () => {
  it("keeps identical workspace and worktree names on different machines in separate groups", () => {
    const inventory = buildRemoteInventory({ options: fixtureOptions(), activeMachineId: MACHINE_A });

    expect(inventory.projects.map((project) => project.key)).toEqual([
      inventoryProjectKey({ machineId: MACHINE_A, workspaceId: "ws-ferryx-core" }),
      inventoryProjectKey({ machineId: MACHINE_B, workspaceId: "ws-ferryx-core" }),
      inventoryProjectKey({ machineId: MACHINE_A, workspaceId: "ws-docs" }),
    ]);
    const mainWorktrees = inventory.projects
      .filter((project) => project.workspaceId === "ws-ferryx-core")
      .map((project) => project.worktrees[0]);
    expect(mainWorktrees[0]?.key).not.toEqual(mainWorktrees[1]?.key);
    expect(mainWorktrees.map((worktree) => worktree.machineId)).toEqual([MACHINE_A, MACHINE_B]);
  });

  it("names a project group from declared labels and falls back to the workspace id", () => {
    const inventory = buildRemoteInventory({ options: fixtureOptions(), activeMachineId: MACHINE_A });

    expect(inventory.projects.map((project) => project.label)).toEqual([
      "ferryx-core",
      "ws-ferryx-core",
      "ws-docs",
    ]);
  });

  it("places a null-slug session in its own root worktree row of the right project", () => {
    const inventory = buildRemoteInventory({ options: fixtureOptions(), activeMachineId: MACHINE_A });
    const project = inventory.projects.find(
      (candidate) => candidate.machineId === MACHINE_A && candidate.workspaceId === "ws-ferryx-core",
    );
    const root = project?.worktrees.find((worktree) => worktree.isRootWorktree);

    expect(root?.slug).toBeNull();
    expect(root?.label).toBe("ferryx-core (root)");
    expect(root?.entries.map((entry) => entry.sessionId)).toEqual(["s-root-done"]);
    expect(root?.selection.worktreeSlug).toBeNull();
    expect(project?.worktrees.filter((worktree) => worktree.isRootWorktree)).toHaveLength(1);
    expect(project?.worktrees.some((worktree) => worktree.slug === "main")).toBe(true);
  });

  it("never collides row keys or selection payloads for the same session id on two machines", () => {
    const options = fixtureOptions();
    const inventory = buildRemoteInventory({ options, activeMachineId: null });

    const rows = inventory.projects.flatMap((project) =>
      project.worktrees.flatMap((worktree) => worktree.entries),
    );
    const keys = rows.map((entry) => entry.key);
    expect(new Set(keys).size).toBe(keys.length);

    const sameSid = options.filter((option) => option.sessionId === "s-1a-working");
    expect(new Set(sameSid.map((option) => inventorySessionKey(option))).size).toBe(2);
    expect(new Set(sameSid.map((option) => inventoryWorktreeKey(option))).size).toBe(2);

    const machineB = entryOf(inventory, MACHINE_B, "ws-ferryx-core", "main", "s-1a-working");
    const machineA = entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working");
    expect(machineB?.selection).toMatchObject({
      machineId: MACHINE_B,
      daemonEpoch: 201,
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      sessionId: "s-1a-working",
    });
    expect(isSameInventorySelection(machineB!.selection, machineA!.selection)).toBe(false);
  });

  it("treats a differing declared epoch as a different session generation", () => {
    const base = {
      machineId: MACHINE_A,
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      sessionId: "s-1a-working",
      tabId: null,
    };
    expect(isSameInventorySelection({ ...base, daemonEpoch: 101 }, { ...base, daemonEpoch: 202 })).toBe(false);
    expect(isSameInventorySelection({ ...base, daemonEpoch: 101 }, { ...base, daemonEpoch: 101 })).toBe(true);
    expect(isSameInventorySelection({ ...base, daemonEpoch: null }, { ...base, daemonEpoch: 101 })).toBe(true);
    expect(
      inventorySessionKey({ ...base, daemonEpoch: 101 }),
    ).not.toEqual(inventorySessionKey({ ...base, daemonEpoch: 202 }));
    expect(
      isSameInventorySelection({ ...base, machineId: MACHINE_B, daemonEpoch: 101 }, { ...base, daemonEpoch: 101 }),
    ).toBe(false);
  });

  it("marks active rows only when machine, workspace, worktree and epoch all agree", () => {
    const options = fixtureOptions();
    const context = {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      activeSessionId: "s-1a-working",
      activeTabId: null,
      activeMachineId: MACHINE_A,
    };

    const matching = buildRemoteInventory({ options, context, activeMachineId: MACHINE_A });
    expect(entryOf(matching, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.active).toBe(true);
    expect(entryOf(matching, MACHINE_B, "ws-ferryx-core", "main", "s-1a-working")?.active).toBe(false);

    const matchingEpoch = buildRemoteInventory({
      options,
      context: { ...context, daemonEpoch: 101 },
      activeMachineId: MACHINE_A,
    });
    expect(entryOf(matchingEpoch, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.active).toBe(true);

    const staleEpoch = buildRemoteInventory({
      options,
      context: { ...context, daemonEpoch: 999 },
      activeMachineId: MACHINE_A,
    });
    expect(entryOf(staleEpoch, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.active).toBe(false);

    const otherWorktree = buildRemoteInventory({
      options,
      context: { ...context, worktreeSlug: "feature/api" },
      activeMachineId: MACHINE_A,
    });
    expect(entryOf(otherWorktree, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.active).toBe(false);
  });
});

describe("remote inventory activity honesty", () => {
  it("reports activity only from declared metadata and leaves a running session unknown", () => {
    expect(resolveInventoryActivity(undefined, null, "waiting")).toBe("waiting");
    expect(resolveInventoryActivity("running", true)).toBeNull();
    expect(resolveInventoryActivity(undefined)).toBeNull();

    const inventory = buildRemoteInventory({ options: fixtureOptions(), activeMachineId: null });
    const runningOnly = entryOf(inventory, MACHINE_B, "ws-ferryx-core", "main", "s-1a-working");
    expect(runningOnly?.kind).toBe("session");
    expect(runningOnly?.kind === "session" && runningOnly.running).toBe(true);
    expect(runningOnly?.activity).toBeNull();
    expect(entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.activity).toBe("working");
  });

  it("rolls up worktree and project indicators with the desktop precedence: waiting over working over done", () => {
    expect(rollupActivity([])).toBeNull();
    expect(rollupActivity([null, undefined])).toBeNull();
    expect(rollupActivity(["working", "done"])).toBe("done");
    expect(rollupActivity(["done", "working"])).toBe("done");
    expect(rollupActivity(["working", "waiting"])).toBe("waiting");
    expect(rollupActivity(["done", "waiting"])).toBe("waiting");

    const inventory = buildRemoteInventory({ options: fixtureOptions(), activeMachineId: MACHINE_A });
    const main = inventory.projects[0]?.worktrees.find((worktree) => worktree.slug === "main");
    expect(main?.activity).toBe("waiting");
    expect(inventory.projects[0]?.activity).toBe("waiting");
  });
});

describe("remote inventory pane placement", () => {
  it("folds a mirrored pane into the session row of the same worktree instead of duplicating it", () => {
    const panes: RemoteTerminalTabInfo[] = [
      {
        id: "tab-1a",
        label: "agent (1a)",
        activityState: "working",
        sessionId: "s-1a-working",
        worktreeSlug: "main",
        worktreeLabel: "main",
      },
    ];
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes,
      context: {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-1a",
        activeSessionId: "s-1a-working",
      },
      activeMachineId: MACHINE_A,
    });

    const worktree = inventory.projects[0]?.worktrees.find((candidate) => candidate.slug === "main");
    const sessionRows = worktree?.entries.filter((entry) => entry.kind === "session") ?? [];
    const folded = sessionRows.find((entry) => entry.kind === "session" && entry.sessionId === "s-1a-working");

    expect(sessionRows).toHaveLength(2);
    expect(folded?.kind === "session" && folded.pane?.id).toBe("tab-1a");
    expect(folded?.active).toBe(true);
    expect(worktree?.active).toBe(true);
    expect(inventory.orphanPanes).toHaveLength(0);
  });

  it("lets the pane's live state override a stale option state, but never the reverse", () => {
    const panes: RemoteTerminalTabInfo[] = [
      {
        id: "tab-1b",
        label: "agent (1b)",
        activityState: "done",
        sessionId: "s-1b-waiting",
        worktreeSlug: "main",
        worktreeLabel: "main",
      },
      {
        id: "tab-1a",
        label: "agent (1a)",
        sessionId: "s-1a-working",
        worktreeSlug: "main",
        worktreeLabel: "main",
      },
    ];
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes,
      context: { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main" },
      activeMachineId: MACHINE_A,
    });

    expect(entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1b-waiting")?.activity).toBe("done");
    expect(entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working")?.activity).toBe("working");
  });

  it("never lets a pane without declared activity erase the activity the option already reported", () => {
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes: [
        {
          id: "tab-1a",
          label: "agent (1a)",
          sessionId: "s-1a-working",
          worktreeSlug: "main",
          worktreeLabel: "main",
        },
      ],
      context: {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-1a",
      },
      activeMachineId: MACHINE_A,
    });

    const row = entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working");
    expect(row?.kind).toBe("session");
    expect(row?.activity).toBe("working");
    expect(row?.kind === "session" && row.pane?.id).toBe("tab-1a");
    expect(row?.active).toBe(true);
  });

  it("refuses a pane whose epoch disagrees with the session row it would fold onto", () => {
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes: [
        {
          id: "tab-1a-next",
          label: "agent (1a) restarted",
          sessionId: "s-1a-working",
          daemonEpoch: 999,
          activityState: "waiting",
          worktreeSlug: "main",
          worktreeLabel: "main",
        },
      ],
      context: { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main" },
      activeMachineId: MACHINE_A,
    });

    const row = entryOf(inventory, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working");
    expect(row?.kind === "session" && row.pane).toBeNull();
    expect(row?.activity).toBe("working");
    const main = inventory.projects[0]?.worktrees.find((worktree) => worktree.slug === "main");
    expect(paneTabIds(main?.entries ?? [])).toEqual(["tab-1a-next"]);
  });

  it("highlights a pane only when its epoch matches the current context epoch", () => {
    const panesFor = (daemonEpoch: number) => [
      {
        id: "tab-1a",
        label: "agent (1a)",
        sessionId: "s-1a-working",
        daemonEpoch,
        worktreeSlug: "main" as const,
        worktreeLabel: "main",
      },
    ];
    const context = {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      worktreeLabel: "main",
      activeTabId: "tab-1a",
      daemonEpoch: 101,
    };

    const stale = buildRemoteInventory({
      options: fixtureOptions(),
      panes: panesFor(999),
      context,
      activeMachineId: MACHINE_A,
    });
    const staleRow = entryOf(stale, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working");
    expect(staleRow?.kind === "session" && staleRow.pane).toBeNull();
    expect(staleRow?.active).toBe(false);
    const stalePane = stale.projects[0]?.worktrees
      .find((worktree) => worktree.slug === "main")
      ?.entries.find((entry) => entry.kind === "pane" && entry.tabId === "tab-1a");
    expect(stalePane?.active).toBe(false);
    expect(
      stale.projects.flatMap((project) => project.worktrees).flatMap((worktree) => worktree.entries)
        .filter((entry) => entry.active),
    ).toHaveLength(0);

    const current = buildRemoteInventory({
      options: fixtureOptions(),
      panes: panesFor(101),
      context,
      activeMachineId: MACHINE_A,
    });
    const currentRow = entryOf(current, MACHINE_A, "ws-ferryx-core", "main", "s-1a-working");
    expect(currentRow?.kind === "session" && currentRow.pane?.id).toBe("tab-1a");
    expect(currentRow?.active).toBe(true);
  });

  it("keeps a pane with an explicit null slug on the root worktree instead of the context worktree", () => {
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes: [{ id: "tab-root", label: "root shell", worktreeSlug: null, sessionId: "s-root-pane" }],
      context: {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        worktreeLabel: "main",
        activeTabId: "tab-root",
      },
      activeMachineId: MACHINE_A,
    });

    const root = inventory.projects[0]?.worktrees.find((worktree) => worktree.isRootWorktree);
    const main = inventory.projects[0]?.worktrees.find((worktree) => worktree.slug === "main");
    expect(paneTabIds(root?.entries ?? [])).toEqual(["tab-root"]);
    expect(paneTabIds(main?.entries ?? [])).toEqual([]);
    // The root worktree carries the pre-existing root session row plus this pane, so the active
    // entry is the pane by identity, never by position.
    const rootPane = (root?.entries ?? []).find(
      (entry) => entry.kind === "pane" && entry.tabId === "tab-root",
    );
    expect(rootPane?.active).toBe(true);
    expect((root?.entries ?? []).filter((entry) => entry.active)).toEqual([rootPane]);
    expect(root?.active).toBe(true);
    expect(main?.active).toBe(false);
    expect(inventory.projects[0]?.worktrees.filter((worktree) => worktree.active)).toHaveLength(1);
    expect(
      inventory.projects.flatMap((project) => project.worktrees).flatMap((worktree) => worktree.entries)
        .filter((entry) => entry.active),
    ).toEqual([rootPane]);
  });

  it("places a pane that declares its own workspace in that workspace's project", () => {
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes: [{ id: "tab-docs", label: "docs shell", workspaceId: "ws-docs", worktreeSlug: "main" }],
      context: { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main" },
      activeMachineId: MACHINE_A,
    });

    const docs = inventory.projects.find(
      (project) => project.machineId === MACHINE_A && project.workspaceId === "ws-docs",
    );
    expect(paneTabIds(docs?.worktrees[0]?.entries ?? [])).toEqual(["tab-docs"]);
    expect(inventory.orphanPanes).toHaveLength(0);
  });

  it("never attaches a pane to another machine's worktree when a machine is active", () => {
    const inventory = buildRemoteInventory({
      options: fixtureOptions(),
      panes: [{ id: "tab-foreign", label: "foreign shell", workspaceId: "ws-ferryx-core", worktreeSlug: "alien" }],
      context: { workspaceId: "ws-ferryx-core", worktreeSlug: "main", worktreeLabel: "main" },
      activeMachineId: MACHINE_A,
    });

    expect(inventory.orphanPanes.map((tab) => tab.id)).toEqual(["tab-foreign"]);
    const machineBEmp = entryOf(inventory, MACHINE_B, "ws-ferryx-core", "main", "s-1a-working");
    expect(machineBEmp?.kind === "session" && machineBEmp.pane).toBeNull();
  });

  it("attaches a pane by worktree label and reports panes with no worktree as orphans", () => {
    const options: RemoteContextOption[] = [
      {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "feature/api",
        worktreeLabel: "feature/api",
        machineId: MACHINE_A,
      },
    ];
    const inventory = buildRemoteInventory({
      options,
      panes: [
        { id: "tab-api", label: "api", worktreeLabel: "feature/api" },
        { id: "tab-outsider", label: "outsider", worktreeSlug: "not-listed" },
      ],
      context: { workspaceId: "ws-ferryx-core", worktreeSlug: "feature/api", worktreeLabel: "feature/api" },
      activeMachineId: MACHINE_A,
    });

    expect(inventory.projects[0]?.worktrees[0]?.entries.map((entry) => entry.key)).toEqual([
      `${inventoryWorktreeKey(options[0])}\u0000pane\u0000tab-api`,
    ]);
    expect(inventory.orphanPanes.map((tab) => tab.id)).toEqual(["tab-outsider"]);
  });
});