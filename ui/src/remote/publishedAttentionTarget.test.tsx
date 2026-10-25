// A desktop Inbox row names the desktop's local pane id; the remote's options name the machine's
// backend session ids. This drives a real row from the desktop's state through the notification
// store, the publisher and the remote loader, with the two ids distinct, and checks the remote opens
// that exact backend session (or its worktree), and that older snapshots without the ids still load.
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { sidebarSessionSignals, withBackendSessions, type PublishedAttentionRow } from "../components/sidebar/sidebarSessionSignals";
import { buildAttentionRows } from "../features/ferryx/attention/attentionModel";
import { wireActivityRecording } from "../lib/notificationCenter/activityRecording";
import { createNotificationCenterStore } from "../lib/notificationCenter/notificationCenterStore";
import { selectActivityNotificationTargets, type WorkspaceState } from "../state/workspaceStore";
import { RemoteSidebar } from "./RemoteSidebar";
import { publishedAttentionOption, type RemoteWorkspaceModel } from "./RemoteSessionList";
import { loadRemoteSidebar } from "./remoteSidebarSource";
import { sidebarModelFixture } from "./remoteSidebarTestUtils";

const LOCAL_ID = "session:7f3c2a10-local";
const BACKEND_ID = "pty-backend-123";
const FEATURE = { path: "/repo/feature", head: "a", branch: "refs/heads/feature", bare: false, detached: false, locked: null, prunable: null,
  identity: { wsId: "ws", slug: "feature" } };

/** The mounted desktop state: one waiting agent in `feature`, its local and backend ids distinct. */
function desktopState(withBackend = true): WorkspaceState {
  return {
    workspaceId: "ws", worktrees: [FEATURE], activeWorktreePath: FEATURE.path, unreadTabIds: {}, unreadWorktreePaths: {},
    sessions: { [LOCAL_ID]: { id: LOCAL_ID, cwd: FEATURE.path, worktreePath: FEATURE.path, workspaceId: "ws",
      worktree: { wsId: "ws", slug: "feature" }, ...(withBackend ? { backendSessionId: BACKEND_ID } : {}), lifecycle: "working" } },
    layout: { tabs: [{ id: "tab-1", label: "feature", sessionId: LOCAL_ID }], activeTabId: "tab-1", layoutsByTabId: {} },
    activityBySessionId: { [LOCAL_ID]: { state: "waiting", seen: false, title: "codex: needs input", isAgent: true } },
  } as unknown as WorkspaceState;
}

/** The rows the desktop Inbox shows for that state, recorded by the real activity recorder. */
function desktopInboxRows(state: WorkspaceState) {
  const store = createNotificationCenterStore({ storage: null });
  wireActivityRecording({
    store, isObserved: () => false,
    events: (listener) => {
      for (const t of selectActivityNotificationTargets(state)) listener({ ...t, previousState: "working" }, { accepted: true } as never);
      return () => undefined;
    },
  });
  return buildAttentionRows(store.getSnapshot().entries);
}

/** Publishes the rows over the wire and returns what the remote loader accepted. */
async function publishAndLoad(rows: readonly PublishedAttentionRow[]): Promise<readonly PublishedAttentionRow[]> {
  const built = sidebarModelFixture([{ workspaceId: "ws", slug: null }, { workspaceId: "ws", slug: "feature" }]);
  const body = new TextEncoder().encode(JSON.stringify({ model: { ...built, signals: sidebarSessionSignals(built, { attentionRows: rows }) } }));
  const res = await loadRemoteSidebar(async () => ({ status: 200, body }) as never);
  if (res.kind !== "ok") throw new Error(`snapshot rejected: ${JSON.stringify(res)}`);
  return res.model.signals?.attentionRows ?? [];
}

const ROOT = { workspaceId: "ws", worktreeSlug: null, worktreeLabel: "main", machineId: "m" };
const FEATURE_OPTION = { workspaceId: "ws", worktreeSlug: "feature", worktreeLabel: "feature", machineId: "m" };
const BACKEND_OPTION = { ...FEATURE_OPTION, sessionId: BACKEND_ID };
const remote = (options: RemoteWorkspaceModel["options"]) =>
  ({ context: { workspaceId: null, worktreeSlug: null, worktreeLabel: null }, options }) as unknown as RemoteWorkspaceModel;

afterEach(cleanup);

describe("a desktop Inbox row opens its pane on the remote", () => {
  it("publishes the pane's backend session and worktree and opens that exact session", async () => {
    const state = desktopState();
    const local = desktopInboxRows(state);
    expect(local.map((row) => row.sessionId)).toEqual([LOCAL_ID]);

    const [published] = await publishAndLoad(withBackendSessions(local, [[state.workspaceId, state]]));

    expect(published).toMatchObject({ sessionId: LOCAL_ID, backendSessionId: BACKEND_ID, worktreeSlug: "feature" });
    const opened = publishedAttentionOption(remote([ROOT, FEATURE_OPTION, BACKEND_OPTION]), { machineId: "m", row: published });
    expect(opened).toEqual({ ...BACKEND_OPTION, attachOnly: true });
  });

  it("opens the pane's worktree, never the root, when the machine no longer lists the session", async () => {
    const state = desktopState();
    const [published] = await publishAndLoad(withBackendSessions(desktopInboxRows(state), [[state.workspaceId, state]]));

    expect(publishedAttentionOption(remote([ROOT, FEATURE_OPTION]), { machineId: "m", row: published })).toEqual({ ...FEATURE_OPTION, attachOnly: true });
  });

  it("reads the owning workspace from the parked snapshots when another workspace is mounted", async () => {
    const parked = desktopState();
    const mounted = { ...desktopState(), workspaceId: "other", sessions: {} } as unknown as WorkspaceState;
    const [published] = await publishAndLoad(withBackendSessions(desktopInboxRows(parked), [[mounted.workspaceId, mounted], ["ws", parked]]));

    expect(published).toMatchObject({ backendSessionId: BACKEND_ID, worktreeSlug: "feature" });
  });
});

describe("older snapshots and panes without a backend session", () => {
  it("loads and lists rows published without backendSessionId or worktreeSlug, opening the root attach-only", async () => {
    const legacy = desktopInboxRows(desktopState());
    const rows = await publishAndLoad(legacy);

    expect(rows).toEqual(legacy);
    expect(publishedAttentionOption(remote([ROOT, BACKEND_OPTION]), { machineId: "m", row: rows[0] })).toEqual({ ...ROOT, attachOnly: true });

    const model = { ...sidebarModelFixture([{ workspaceId: "ws", slug: null }]), signals: { unreadWorktreePaths: {}, agentStateByPath: {}, attentionRows: rows } };
    render(<RemoteSidebar sections={[{ machineId: "m", label: "m", state: "ok", model, signals: {}, error: null }]} density="touch" pending={null}
      onSelect={() => undefined} view="inbox" onViewChange={() => undefined} inbox={{ rows: [], onOpen: () => undefined }} />);
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);
    expect(screen.getByRole("tab", { name: /^Inbox/ })).toHaveAttribute("aria-label", "Inbox (1)");
  });

  it("publishes a pane with no backend session yet without the id, keeping its worktree", async () => {
    const state = desktopState(false);
    const [published] = await publishAndLoad(withBackendSessions(desktopInboxRows(state), [[state.workspaceId, state]]));

    expect(published.backendSessionId).toBeUndefined();
    expect(published.worktreeSlug).toBe("feature");
  });

  it("does not trust a snapshot whose backendSessionId is not a string", async () => {
    const built = sidebarModelFixture([{ workspaceId: "ws", slug: null }]);
    const [row] = desktopInboxRows(desktopState());
    const wire = { ...built, signals: { unreadWorktreePaths: {}, agentStateByPath: {}, attentionRows: [{ ...row, backendSessionId: 7 }] } };
    const res = await loadRemoteSidebar(async () => ({ status: 200, body: new TextEncoder().encode(JSON.stringify({ model: wire })) }) as never);

    // A malformed published model is replaced by the machine's own inventory model.
    expect(res.kind === "ok" && res.model.source).not.toBe("desktop");
  });
});
