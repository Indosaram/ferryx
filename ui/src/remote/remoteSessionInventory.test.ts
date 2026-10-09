import { describe, expect, it } from "vitest";
import {
  applySessionActivity,
  applyWorktreeLabels,
  emptyInventory,
  groupInventoryByWorktree,
  inventoryActivityLabel,
  inventorySessionsFor,
  mergeSessionInventory,
  observationsFromContextTabs,
  parseInventoryEvent,
  parseMachineAgentStateFrame,
  parseSessionsPayload,
  preferredSessionForWorktree,
  selectInventorySession,
  sessionKey,
  workspaceLabelsFromCatalog,
} from "./remoteSessionInventory";

const MACHINE = "mach-macos-primary-01";

function boundaryPayload(overrides: Record<string, unknown> = {}) {
  return {
    revision: "41",
    completeness: "complete",
    sessions: [
      {
        workspaceId: "ws-ferryx-core",
        worktree: { wsId: "ws-ferryx-core", slug: "main" },
        target: { machineId: MACHINE, sessionId: "s-1a-working", daemonEpoch: 101 },
        sessionId: "s-1a-working",
        daemonEpoch: 101,
        running: true,
        title: "ferryx / main / bash (1a)",
        agentType: "claude",
        providerSession: { key: "claude", id: "prov-1a" },
      },
      {
        workspaceId: "ws-ferryx-core",
        worktree: { wsId: "ws-ferryx-core", slug: "main" },
        target: { machineId: MACHINE, sessionId: "s-1b-waiting", daemonEpoch: 101 },
        sessionId: "s-1b-waiting",
        daemonEpoch: 101,
        running: true,
        title: "ferryx / main / agent (1b)",
      },
      {
        workspaceId: "ws-ferryx-core",
        worktree: null,
        target: { machineId: MACHINE, sessionId: "s-root-done", daemonEpoch: 101 },
        sessionId: "s-root-done",
        daemonEpoch: 101,
        running: false,
        title: "ferryx / root",
      },
      {
        workspaceId: "ws-docs",
        worktree: { wsId: "ws-docs", slug: "docs" },
        target: { machineId: MACHINE, sessionId: "s-docs-working", daemonEpoch: 102 },
        sessionId: "s-docs-working",
        daemonEpoch: 102,
        running: true,
      },
    ],
    unavailableWorkspaceIds: [],
    ...overrides,
  };
}

describe("parseSessionsPayload", () => {
  it("keeps every session on the machine and never fabricates activity", () => {
    const result = parseSessionsPayload(MACHINE, boundaryPayload(), 1_000);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.inventory.entries.map((entry) => entry.sessionId).sort()).toEqual([
      "s-1a-working",
      "s-1b-waiting",
      "s-docs-working",
      "s-root-done",
    ]);
    for (const entry of result.inventory.entries) {
      expect(entry.activityState).toBeNull();
      expect(entry.activitySource).toBeUndefined();
    }
  });

  it("keeps the root worktree (null slug) separate from named worktrees", () => {
    const result = parseSessionsPayload(MACHINE, boundaryPayload());
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const root = result.inventory.entries.find((entry) => entry.sessionId === "s-root-done");
    expect(root?.worktreeSlug).toBeNull();
    expect(inventorySessionsFor(result.inventory, "ws-ferryx-core", null).map((e) => e.sessionId)).toEqual([
      "s-root-done",
    ]);
    expect(inventorySessionsFor(result.inventory, "ws-ferryx-core", "main").map((e) => e.sessionId)).toEqual([
      "s-1a-working",
      "s-1b-waiting",
    ]);
  });

  it("normalises numeric epochs to strings and carries provider identity", () => {
    const result = parseSessionsPayload(MACHINE, boundaryPayload());
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const entry = result.inventory.entries.find((row) => row.sessionId === "s-1a-working");
    expect(entry?.daemonEpoch).toBe("101");
    expect(entry?.providerSessionId).toBe("prov-1a");
    expect(entry?.agentType).toBe("claude");
    expect(entry?.running).toBe(true);
  });

  it("reports an invalid payload instead of treating it as an empty machine", () => {
    expect(parseSessionsPayload(MACHINE, { sessions: "nope" })).toEqual({
      ok: false,
      error: "invalid sessions payload",
    });
    expect(parseSessionsPayload(MACHINE, null).ok).toBe(false);
  });

  it("skips rows without a session id or workspace and de-duplicates repeated ids", () => {
    const result = parseSessionsPayload(MACHINE, {
      completeness: "complete",
      sessions: [
        { sessionId: "no-workspace", target: { sessionId: "no-workspace" } },
        { workspaceId: "ws-a", target: {}, running: true },
        { workspaceId: "ws-a", sessionId: "dup", target: { sessionId: "dup" }, running: true },
        { workspaceId: "ws-a", sessionId: "dup", target: { sessionId: "dup" }, running: false },
      ],
      unavailableWorkspaceIds: ["ws-offline", null, "  "],
    });
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.inventory.entries.map((e) => e.sessionId)).toEqual(["dup"]);
    expect(result.inventory.entries[0]?.running).toBe(true);
    expect(result.inventory.unavailableWorkspaceIds).toEqual(["ws-offline"]);
  });
});

describe("mergeSessionInventory", () => {
  const complete = parseSessionsPayload(MACHINE, boundaryPayload(), 1_000);
  const completeInventory = complete.ok ? complete.inventory : emptyInventory(MACHINE);

  it("retains earlier rows when the next answer is partial", () => {
    const partial = parseSessionsPayload(
      MACHINE,
      boundaryPayload({
        completeness: "partial",
        sessions: [
          {
            workspaceId: "ws-ferryx-core",
            worktree: { wsId: "ws-ferryx-core", slug: "main" },
            target: { machineId: MACHINE, sessionId: "s-1a-working", daemonEpoch: 101 },
            sessionId: "s-1a-working",
            daemonEpoch: 101,
            running: true,
          },
        ],
      }),
      2_000,
    );
    expect(partial.ok).toBe(true);
    if (!partial.ok) return;
    const merged = mergeSessionInventory(completeInventory, partial.inventory);
    expect(merged.completeness).toBe("partial");
    expect(merged.entries.map((entry) => entry.sessionId).sort()).toEqual([
      "s-1a-working",
      "s-1b-waiting",
      "s-docs-working",
      "s-root-done",
    ]);
  });

  it("prunes rows a complete answer no longer reports", () => {
    const narrower = parseSessionsPayload(
      MACHINE,
      boundaryPayload({
        sessions: [
          {
            workspaceId: "ws-docs",
            worktree: { wsId: "ws-docs", slug: "docs" },
            target: { machineId: MACHINE, sessionId: "s-docs-working", daemonEpoch: 102 },
            sessionId: "s-docs-working",
            daemonEpoch: 102,
            running: true,
          },
        ],
      }),
      3_000,
    );
    expect(narrower.ok).toBe(true);
    if (!narrower.ok) return;
    const merged = mergeSessionInventory(completeInventory, narrower.inventory);
    expect(merged.entries.map((entry) => entry.sessionId)).toEqual(["s-docs-working"]);
  });

  it("keeps rows of workspaces a complete answer declared unavailable", () => {
    const unavailable = parseSessionsPayload(
      MACHINE,
      boundaryPayload({ sessions: [], unavailableWorkspaceIds: ["ws-ferryx-core"] }),
      4_000,
    );
    expect(unavailable.ok).toBe(true);
    if (!unavailable.ok) return;
    const merged = mergeSessionInventory(completeInventory, unavailable.inventory);
    expect(merged.entries.map((entry) => entry.sessionId).sort()).toEqual([
      "s-1a-working",
      "s-1b-waiting",
      "s-root-done",
    ]);
  });

  it("carries previously observed activity across a refresh", () => {
    const observed =
      applySessionActivity(completeInventory, [
        { sessionId: "s-1a-working", state: "working", source: "agent_state", observedAt: 5_000 },
      ]) ?? completeInventory;
    const refreshed = parseSessionsPayload(MACHINE, boundaryPayload(), 6_000);
    expect(refreshed.ok).toBe(true);
    if (!refreshed.ok) return;
    const merged = mergeSessionInventory(observed, refreshed.inventory);
    const entry = merged.entries.find((row) => row.sessionId === "s-1a-working");
    expect(entry?.activityState).toBe("working");
    expect(entry?.activitySource).toBe("agent_state");
    expect(entry?.activityObservedAt).toBe(5_000);
  });
});

describe("parseInventoryEvent", () => {
  it("decodes a full inventoryInvalidated boundary and labels worktrees from projects", () => {
    const event = parseInventoryEvent(
      MACHINE,
      JSON.stringify({
        sequence: "17",
        revision: "17",
        type: "inventoryInvalidated",
        reason: "subscribe",
        payload: {
          projects: {
            revision: "3",
            completeness: "complete",
            projects: [
              {
                workspaceId: "ws-ferryx-core",
                worktrees: [
                  { identity: { wsId: "ws-ferryx-core", slug: "main" }, branch: "refs/heads/feature/parity" },
                ],
              },
            ],
          },
          sessions: boundaryPayload(),
          completeness: "complete",
        },
      }),
      7_000,
    );
    expect(event.kind).toBe("inventory");
    if (event.kind !== "inventory") return;
    expect(event.sequence).toBe("17");
    expect(event.reason).toBe("subscribe");
    expect(event.inventory.entries).toHaveLength(4);
    const main = event.inventory.entries.find((entry) => entry.sessionId === "s-1b-waiting");
    expect(main?.worktreeLabel).toBe("feature/parity");
  });

  it("treats a degraded boundary as partial instead of an empty machine", () => {
    const event = parseInventoryEvent(
      MACHINE,
      JSON.stringify({ type: "inventoryInvalidated", payload: { completeness: "partial" } }),
    );
    expect(event.kind).toBe("partial");
    if (event.kind !== "partial") return;
    expect(event.error).toBeNull();
  });

  it("downgrades a boundary whose payload declares partial completeness", () => {
    const event = parseInventoryEvent(
      MACHINE,
      JSON.stringify({
        type: "inventoryInvalidated",
        reason: "change",
        payload: { sessions: boundaryPayload({ completeness: "complete" }), completeness: "partial" },
      }),
    );
    expect(event.kind).toBe("inventory");
    if (event.kind !== "inventory") return;
    expect(event.inventory.completeness).toBe("partial");
  });

  it("ignores frames that are not inventory boundaries and unparsable text", () => {
    expect(parseInventoryEvent(MACHINE, JSON.stringify({ type: "agent_state" })).kind).toBe("ignored");
    expect(parseInventoryEvent(MACHINE, "not json").kind).toBe("ignored");
  });
});

describe("selectInventorySession", () => {
  const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
  const inventory = parsed.ok ? parsed.inventory : emptyInventory(MACHINE);

  it("selects the exact requested session, never a neighbour", () => {
    const found = selectInventorySession(inventory, {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      sessionId: "s-1b-waiting",
      daemonEpoch: "101",
    });
    expect(found?.entry.sessionId).toBe("s-1b-waiting");
    expect(found?.epochVerified).toBe(true);
  });

  it("refuses to attach to a stopped session unless inspecting it", () => {
    const attach = selectInventorySession(inventory, {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: null,
      sessionId: "s-root-done",
    });
    expect(attach).toBeNull();
    const inspect = selectInventorySession(inventory, {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: null,
      sessionId: "s-root-done",
      requireRunning: false,
    });
    expect(inspect?.entry.running).toBe(false);
    expect(inspect?.epochVerified).toBe(false);
  });

  it("returns null for an unknown session id instead of falling back", () => {
    expect(
      selectInventorySession(inventory, {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        sessionId: "s-missing",
      }),
    ).toBeNull();
  });

  it("rejects a session whose daemon epoch disagrees with the caller", () => {
    expect(
      selectInventorySession(inventory, {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        sessionId: "s-1b-waiting",
        daemonEpoch: "999",
      }),
    ).toBeNull();
  });

  it("marks the epoch unverified when either side has none", () => {
    const found = selectInventorySession(inventory, {
      workspaceId: "ws-ferryx-core",
      worktreeSlug: "main",
      sessionId: "s-1b-waiting",
    });
    expect(found?.epochVerified).toBe(false);
  });

  it("separates the same workspace id across worktrees and machines", () => {
    const other = parseSessionsPayload(
      "mach-linux-headless-02",
      {
        completeness: "complete",
        sessions: [
          {
            workspaceId: "ws-ferryx-core",
            worktree: { wsId: "ws-ferryx-core", slug: "main" },
            target: { sessionId: "s-1b-waiting", daemonEpoch: 201 },
            sessionId: "s-1b-waiting",
            running: true,
          },
        ],
      },
    );
    expect(other.ok).toBe(true);
    if (!other.ok) return;
    expect(
      selectInventorySession(other.inventory, {
        workspaceId: "ws-ferryx-core",
        worktreeSlug: "main",
        sessionId: "s-1b-waiting",
        daemonEpoch: "101",
      }),
    ).toBeNull();
    expect(sessionKey("mach-a", "s-1")).not.toBe(sessionKey("mach-b", "s-1"));
  });
});

describe("parseMachineAgentStateFrame", () => {
  it("decodes an agent_state control frame from the attached terminal socket", () => {
    const frame = parseMachineAgentStateFrame(
      JSON.stringify({
        type: "agent_state",
        target: { machineId: MACHINE, sessionId: "s-1b-waiting", daemonEpoch: 101 },
        state: "waiting",
        agent: "claude",
        detail: "question pending",
      }),
    );
    expect(frame).toEqual({
      machineId: MACHINE,
      sessionId: "s-1b-waiting",
      daemonEpoch: "101",
      state: "waiting",
      agent: "claude",
    });
  });

  it("rejects unknown states and non agent_state frames instead of guessing", () => {
    expect(
      parseMachineAgentStateFrame(
        JSON.stringify({ type: "agent_state", target: { sessionId: "s-1" }, state: "thinking" }),
      ),
    ).toBeNull();
    expect(
      parseMachineAgentStateFrame(JSON.stringify({ type: "inventoryInvalidated", payload: {} })),
    ).toBeNull();
    expect(parseMachineAgentStateFrame("binary\u0001\u0002")).toBeNull();
  });
});

describe("preferredSessionForWorktree", () => {
  const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
  const inventory = parsed.ok ? parsed.inventory : emptyInventory(MACHINE);

  it("prefers the earlier candidate and otherwise resolves deterministically", () => {
    const preferred = preferredSessionForWorktree(inventory, "ws-ferryx-core", "main", [
      "s-1b-waiting",
    ]);
    expect(preferred?.sessionId).toBe("s-1b-waiting");
    const stable = preferredSessionForWorktree(inventory, "ws-ferryx-core", "main");
    expect(stable?.sessionId).toBe("s-1a-working");
    expect(preferredSessionForWorktree(inventory, "ws-ferryx-core", "main")?.sessionId).toBe(
      stable?.sessionId,
    );
  });

  it("returns null for a worktree the machine reported no sessions for", () => {
    expect(preferredSessionForWorktree(inventory, "ws-missing", "main")).toBeNull();
  });
});

describe("activity observations", () => {
  const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
  const inventory = parsed.ok ? parsed.inventory : emptyInventory(MACHINE);

  it("reads only explicit states from published context tabs", () => {
    const observations = observationsFromContextTabs(
      [
        { id: "tab-1a", sessionId: "s-1a-working", activityState: "working" },
        { id: "tab-1b", sessionId: "s-1b-waiting", state: "waiting", agentType: "codex" },
        { id: "tab-blank", sessionId: "s-1c" },
        { id: "tab-no-session", activityState: "done" },
      ],
      8_000,
    );
    expect(observations).toHaveLength(2);
    expect(
      observations.map((observation) => ({
        sessionId: observation.sessionId,
        state: observation.state,
        source: observation.source,
        agentType: observation.agentType ?? null,
      })),
    ).toEqual([
      { sessionId: "s-1a-working", state: "working", source: "context_tabs", agentType: null },
      { sessionId: "s-1b-waiting", state: "waiting", source: "context_tabs", agentType: "codex" },
    ]);
    for (const observation of observations) {
      expect(observation.observedAt).toBe(8_000);
      expect(observation.machineId ?? null).toBeNull();
      expect(observation.daemonEpoch ?? null).toBeNull();
    }
  });

  it("applies an observation to its row and drops observations for unknown sessions", () => {
    const merged = applySessionActivity(
      inventory,
      [
        { sessionId: "s-1b-waiting", state: "working", source: "agent_state", observedAt: 9_000 },
        { sessionId: "s-not-on-machine", state: "done", source: "agent_state" },
      ],
      9_000,
    );
    expect(merged?.entries).toHaveLength(inventory.entries.length);
    const entry = merged?.entries.find((row) => row.sessionId === "s-1b-waiting");
    expect(entry?.activityState).toBe("working");
    expect(entry?.activitySource).toBe("agent_state");
    expect(merged?.entries.some((row) => row.sessionId === "s-not-on-machine")).toBe(false);
  });

  it("reports running as running, never as working", () => {
    const unknown = inventory.entries.find((entry) => entry.sessionId === "s-1a-working");
    expect(unknown && inventoryActivityLabel(unknown)).toEqual({
      state: null,
      detail: "running",
      source: "machine",
    });
    const stopped = inventory.entries.find((entry) => entry.sessionId === "s-root-done");
    expect(stopped && inventoryActivityLabel(stopped)).toEqual({
      state: null,
      detail: "stopped",
      source: "machine",
    });
    const observed = applySessionActivity(inventory, [
      { sessionId: "s-1a-working", state: "done", source: "context_tabs" },
    ]);
    const labelled = observed?.entries.find((entry) => entry.sessionId === "s-1a-working");
    expect(labelled && inventoryActivityLabel(labelled)).toEqual({
      state: "done",
      detail: "done",
      source: "context_tabs",
    });
  });
});

describe("groupInventoryByWorktree", () => {
  it("groups by workspace, slug and machine without collapsing sessions", () => {
    const first = parseSessionsPayload(MACHINE, boundaryPayload());
    const second = parseSessionsPayload("mach-linux-headless-02", {
      completeness: "complete",
      sessions: [
        {
          workspaceId: "ws-ferryx-core",
          worktree: { wsId: "ws-ferryx-core", slug: "headless-dev" },
          target: { sessionId: "s-h1", daemonEpoch: 201 },
          sessionId: "s-h1",
          running: true,
        },
      ],
    });
    expect(first.ok && second.ok).toBe(true);
    if (!first.ok || !second.ok) return;
    const groups = groupInventoryByWorktree(first.inventory);
    /* Group order is the picker's concern; this asserts the group set and the rows inside
       each group, not the array order the comparator happens to produce. */
    expect(
      groups.map((group) => `${group.workspaceId}:${group.worktreeSlug ?? "root"}`).sort(),
    ).toEqual(["ws-docs:docs", "ws-ferryx-core:main", "ws-ferryx-core:root"].sort());
    const main = groups.find((group) => group.worktreeSlug === "main");
    expect(main?.sessions.map((entry) => entry.sessionId)).toEqual(["s-1a-working", "s-1b-waiting"]);
    expect(groupInventoryByWorktree(second.inventory)[0]?.sessions.map((e) => e.sessionId)).toEqual(["s-h1"]);
  });

  it("leaves rows without a matching project unchanged when labels cannot be resolved", () => {
    const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
    if (!parsed.ok) return;
    expect(applyWorktreeLabels(parsed.inventory.entries, { projects: [] })).toEqual(parsed.inventory.entries);
    expect(applyWorktreeLabels(parsed.inventory.entries, null)).toEqual(parsed.inventory.entries);
  });
});

describe("workspace labels", () => {
  const catalog = {
    projects: [
      { workspaceId: "ws-ferryx-core", repoRoot: "/Users/dev/ferryx" },
      { workspaceId: "ws-docs", gitRemote: "git@github.com:Indosaram/ferryx-docs.git" },
      { workspaceId: "ws-opaque" },
    ],
  };

  it("derives desktop-style names from the repository basename, never a full path", () => {
    const labels = workspaceLabelsFromCatalog(catalog);
    expect(labels.get("ws-ferryx-core")).toBe("ferryx");
    expect(labels.get("ws-docs")).toBe("ferryx-docs");
    expect(labels.has("ws-opaque")).toBe(false);
    for (const label of labels.values()) {
      expect(label).not.toContain("/");
      expect(label).not.toContain("\\");
    }
  });

  it("carries the workspace label onto inventory groups", () => {
    const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
    if (!parsed.ok) return;
    const groups = groupInventoryByWorktree(parsed.inventory, catalog);
    const core = groups.find((group) => group.workspaceId === "ws-ferryx-core" && group.worktreeSlug === "main");
    expect(core?.workspaceLabel).toBe("ferryx");
    const docs = groups.find((group) => group.workspaceId === "ws-docs");
    expect(docs?.workspaceLabel).toBe("ferryx-docs");
  });
});

describe("identity invariants", () => {
  it("rejects rows addressed to another machine and downgrades the answer", () => {
    const parsed = parseSessionsPayload(MACHINE, {
      completeness: "complete",
      sessions: [
        {
          workspaceId: "ws-ferryx-core",
          worktree: { wsId: "ws-ferryx-core", slug: "main" },
          target: { machineId: "mach-someone-else", sessionId: "s-foreign", daemonEpoch: 7 },
          sessionId: "s-foreign",
          running: true,
        },
        {
          workspaceId: "ws-ferryx-core",
          worktree: { wsId: "ws-ferryx-core", slug: "main" },
          target: { machineId: MACHINE, sessionId: "s-ours", daemonEpoch: 11 },
          sessionId: "s-ours",
          running: true,
        },
      ],
    });
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.inventory.entries.map((entry) => entry.sessionId)).toEqual(["s-ours"]);
    expect(parsed.inventory.completeness).toBe("partial");
    expect(parsed.inventory.rejectedRowCount).toBe(1);
  });

  it("rejects a row whose running flag is not a boolean and never assumes true", () => {
    const parsed = parseSessionsPayload(MACHINE, {
      completeness: "complete",
      sessions: [
        {
          workspaceId: "ws-ferryx-core",
          worktree: null,
          target: { machineId: MACHINE, sessionId: "s-unknown-running", daemonEpoch: 3 },
          sessionId: "s-unknown-running",
        },
      ],
    });
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.inventory.entries).toEqual([]);
    expect(parsed.inventory.completeness).toBe("partial");
  });

  it("keeps previous rows when a downgraded answer dropped them", () => {
    const full = parseSessionsPayload(MACHINE, boundaryPayload(), 1_000);
    const dropped = parseSessionsPayload(
      MACHINE,
      {
        completeness: "complete",
        sessions: [
          {
            workspaceId: "ws-ferryx-core",
            worktree: { wsId: "ws-ferryx-core", slug: "main" },
            target: { machineId: MACHINE, sessionId: "s-1a-working", daemonEpoch: 101 },
            sessionId: "s-1a-working",
            running: true,
          },
          { workspaceId: "ws-ferryx-core", target: { machineId: MACHINE, sessionId: "broken" } },
        ],
      },
      2_000,
    );
    expect(full.ok && dropped.ok).toBe(true);
    if (!full.ok || !dropped.ok) return;
    expect(dropped.inventory.completeness).toBe("partial");
    const merged = mergeSessionInventory(full.inventory, dropped.inventory);
    expect(merged.entries.map((entry) => entry.sessionId).sort()).toEqual([
      "s-1a-working",
      "s-1b-waiting",
      "s-docs-working",
      "s-root-done",
    ]);
  });

  it("does not carry activity across a reused session id on a new epoch", () => {
    const before = parseSessionsPayload(MACHINE, boundaryPayload(), 1_000);
    expect(before.ok).toBe(true);
    if (!before.ok) return;
    const observed = applySessionActivity(before.inventory, [
      {
        sessionId: "s-1a-working",
        state: "working",
        source: "agent_state",
        machineId: MACHINE,
        daemonEpoch: "101",
        observedAt: 1_500,
      },
    ]);
    expect(observed?.entries.find((entry) => entry.sessionId === "s-1a-working")?.activityState).toBe(
      "working",
    );
    const handover = parseSessionsPayload(
      MACHINE,
      {
        completeness: "complete",
        sessions: [
          {
            workspaceId: "ws-ferryx-core",
            worktree: { wsId: "ws-ferryx-core", slug: "main" },
            target: { machineId: MACHINE, sessionId: "s-1a-working", daemonEpoch: 202 },
            sessionId: "s-1a-working",
            running: true,
          },
        ],
      },
      2_000,
    );
    expect(handover.ok).toBe(true);
    if (!handover.ok || !observed) return;
    const merged = mergeSessionInventory(observed, handover.inventory);
    const entry = merged.entries.find((row) => row.sessionId === "s-1a-working");
    expect(entry?.daemonEpoch).toBe("202");
    expect(entry?.activityState).toBeNull();
  });

  it("drops an observation whose machine or epoch disagrees with the row", () => {
    const parsed = parseSessionsPayload(MACHINE, boundaryPayload());
    if (!parsed.ok) return;
    const applied = applySessionActivity(parsed.inventory, [
      { sessionId: "s-1b-waiting", state: "done", source: "agent_state", machineId: "mach-other" },
      { sessionId: "s-1b-waiting", state: "done", source: "agent_state", daemonEpoch: "999" },
    ]);
    expect(applied).toBe(parsed.inventory);
    const matching = applySessionActivity(parsed.inventory, [
      {
        sessionId: "s-1b-waiting",
        state: "waiting",
        source: "agent_state",
        machineId: MACHINE,
        daemonEpoch: "101",
      },
    ]);
    expect(matching?.entries.find((entry) => entry.sessionId === "s-1b-waiting")?.activityState).toBe(
      "waiting",
    );
  });

  it("never resolves a worktree pick to a stopped session", () => {
    const stoppedOnly = parseSessionsPayload(MACHINE, {
      completeness: "complete",
      sessions: [
        {
          workspaceId: "ws-ferryx-core",
          worktree: { wsId: "ws-ferryx-core", slug: "main" },
          target: { machineId: MACHINE, sessionId: "s-stopped", daemonEpoch: 101 },
          sessionId: "s-stopped",
          running: false,
        },
      ],
    });
    expect(stoppedOnly.ok).toBe(true);
    if (!stoppedOnly.ok) return;
    expect(preferredSessionForWorktree(stoppedOnly.inventory, "ws-ferryx-core", "main", ["s-stopped"])).toBeNull();
  });
});
