import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { PanelEvent, PanelSnapshot, PrLane } from "../../lib/supervisorPanel/contract";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import * as tauri from "../../lib/tauri";
import { SupervisorPanelHost, type SupervisorPanelHostProps } from "./SupervisorPanelHost";

afterEach(cleanup);

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);
const PR_LANE: PrLane = { prs: [], outage: null, refreshedAtMs: NOW };

function actionsMock(overrides: Partial<PanelActions> = {}): PanelActions {
  return {
    answer: vi.fn(async () => ({ ok: true }) as const),
    setPaused: vi.fn(async () => ({ ok: true }) as const),
    setLanePaused: vi.fn(async () => ({ ok: true }) as const),
    converse: vi.fn(async () => ({ ok: true }) as const),
    stop: vi.fn(async () => ({ ok: true }) as const),
    undo: vi.fn(async () => ({ ok: true }) as const),
    turnOn: vi.fn(async () => ({ ok: true }) as const),
    openSession: vi.fn(),
    ...overrides,
  };
}

function renderHost(snapshot: PanelSnapshot, overrides: Partial<PanelActions> = {}, extra: Partial<SupervisorPanelHostProps> = {}) {
  const handlers: Array<(event: PanelEvent) => void> = [];
  const load = vi.fn(() => Promise.resolve(snapshot));
  const subscribe = vi.fn((handler: (event: PanelEvent) => void) => {
    handlers.push(handler);
    return Promise.resolve(() => undefined);
  });
  const actions = actionsMock(overrides);
  const onFocusSession = vi.fn();
  const actionsFor = vi.fn((input: { openSession: (ref: string) => void; onChanged: () => void }) => ({
    ...actions,
    openSession: (ref: string) => {
      input.openSession(ref);
      actions.openSession(ref);
    },
  }));
  render(
    <SupervisorPanelHost
      onFocusSession={onFocusSession}
      load={load}
      subscribe={subscribe}
      actionsFor={actionsFor}
      now={() => NOW}
      {...extra}
    />,
  );
  return { onFocusSession, handlers, actions, load, actionsFor };
}

describe("SupervisorPanelHost", () => {
  it("renders the setup state on an empty snapshot", async () => {
    renderHost(emptyPanelSnapshot());
    expect(await screen.findByRole("region", { name: "Supervisor setup" })).toBeTruthy();
  });

  it("falls back to the setup state when the backend load fails", async () => {
    const load = vi.fn(() => Promise.reject(new Error("no such command")));
    const subscribe = vi.fn(() => Promise.resolve(() => undefined));
    render(<SupervisorPanelHost onFocusSession={vi.fn()} load={load} subscribe={subscribe} />);
    expect(await screen.findByRole("region", { name: "Supervisor setup" })).toBeTruthy();
  });

  it("renders the styled panel, not a read-only list", async () => {
    renderHost(panelSnapshotFixture);
    expect(await screen.findAllByTestId("decision-card")).toHaveLength(2);
    expect(screen.getByTestId("supervisor-status-line")).toBeInTheDocument();
    expect(screen.getByTestId("rules-line")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Tell the supervisor anything" })).toBeInTheDocument();
    expect(screen.getAllByTestId("handling-row")).toHaveLength(1);
  });

  it("focuses the session when its row is clicked", async () => {
    const { onFocusSession } = renderHost(panelSnapshotFixture);
    const sessions = await screen.findByRole("region", { name: "Sessions" });
    fireEvent.click(within(sessions).getByText("Release notes for 11.2"));
    expect(onFocusSession).toHaveBeenCalledWith("omarchy:notes");
  });

  it("focuses the session of a decision card", async () => {
    const { onFocusSession } = renderHost(panelSnapshotFixture);
    fireEvent.click(await screen.findByRole("button", { name: "Open Fix relay reconnect terminal" }));
    expect(onFocusSession).toHaveBeenCalledWith("mac:relay");
  });

  it("answers a decision with the prompt's id and revision", async () => {
    const { actions } = renderHost(panelSnapshotFixture);
    const card = await screen.findByRole("article", { name: "Decision for Fix relay reconnect" });
    await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Deny" })));
    expect(actions.answer).toHaveBeenCalledWith(
      { sessionRef: "mac:relay", promptId: "prompt-relay-1", screenRevision: "rev-relay-41" },
      1,
    );
  });

  it("shows a missing backend command on the card instead of success", async () => {
    renderHost(panelSnapshotFixture, {
      answer: vi.fn(async () => ({ ok: false, stale: false, message: "This build of Ferryx can't do that yet (cmd_supervisor_answer is missing)." }) as const),
    });
    const card = await screen.findByRole("article", { name: "Decision for Fix relay reconnect" });
    await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Allow" })));
    expect(within(card).getByRole("alert")).toHaveTextContent("cmd_supervisor_answer is missing");
  });

  it("pauses, stops, undoes and sends through the actions", async () => {
    const { actions } = renderHost(panelSnapshotFixture);
    await screen.findByTestId("supervisor-status-line");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Pause" })));
    expect(actions.setPaused).toHaveBeenCalledWith(true);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop Windows installer tests" })));
    expect(actions.stop).toHaveBeenCalledWith("win:wintests");
    fireEvent.click(screen.getByRole("button", { name: /^All \d+$/ }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Undo: Answered Release notes: v2026.1102.1" })));
    expect(actions.undo).toHaveBeenCalledWith(4);
    const input = screen.getByRole("combobox", { name: "Tell the supervisor anything" });
    fireEvent.change(input, { target: { value: "@roblox stop" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    expect(actions.converse).toHaveBeenCalledWith("@roblox stop", ["win:roblox"]);
  });

  it("turns the supervisor on from the setup card and shows a refusal", async () => {
    const { actions } = renderHost(emptyPanelSnapshot(), {
      turnOn: vi.fn(async () => ({ ok: false, stale: false, message: "no desktop command" }) as const),
    });
    // Wait for the loaded setup card outside act: inside it, React holds the load's state update
    // until act exits, so the lookup could never see it.
    const turnOn = await screen.findByRole("button", { name: "Turn on" });
    await act(async () => fireEvent.click(turnOn));
    expect(actions.turnOn).toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("no desktop command");
  });

  it("applies panel events after the first load", async () => {
    const { handlers } = renderHost(panelSnapshotFixture);
    await screen.findAllByTestId("decision-card");
    await waitFor(() => expect(handlers).toHaveLength(1));
    act(() => handlers[0]({ type: "panel.decision", sessionRef: "mac:relay", decision: null }));
    expect(screen.queryByRole("article", { name: "Decision for Fix relay reconnect" })).toBeNull();
    expect(screen.getByRole("article", { name: "Decision for Roblox build" })).toBeInTheDocument();
  });

  it("replays an event that raced the first load, with the shared reducer", async () => {
    let resolve!: (snapshot: PanelSnapshot) => void;
    const load = vi.fn(() => new Promise<PanelSnapshot>((r) => { resolve = r; }));
    const handlers: Array<(event: PanelEvent) => void> = [];
    const subscribe = vi.fn((handler: (event: PanelEvent) => void) => {
      handlers.push(handler);
      return Promise.resolve(() => undefined);
    });
    render(<SupervisorPanelHost onFocusSession={vi.fn()} load={load} subscribe={subscribe} actionsFor={() => actionsMock()} now={() => NOW} />);
    await waitFor(() => expect(handlers).toHaveLength(1));
    act(() => handlers[0]({ type: "panel.decision", sessionRef: "mac:relay", decision: null }));
    await act(async () => resolve(panelSnapshotFixture));
    expect(await screen.findByRole("article", { name: "Decision for Roblox build" })).toBeInTheDocument();
    expect(screen.queryByRole("article", { name: "Decision for Fix relay reconnect" })).toBeNull();
  });

  it("does not bring a finished action back when a late live frame arrives", async () => {
    const { handlers } = renderHost(panelSnapshotFixture);
    await screen.findAllByTestId("handling-row");
    await waitFor(() => expect(handlers).toHaveLength(1));
    const live = panelSnapshotFixture.handling[0];
    act(() => handlers[0]({ type: "panel.action", action: { ...live, live: false, updatedAtMs: live.updatedAtMs + 1 } }));
    expect(screen.queryAllByTestId("handling-row")).toHaveLength(0);
    act(() => handlers[0]({ type: "panel.action", action: live }));
    expect(screen.queryAllByTestId("handling-row")).toHaveLength(0);
  });

  it("shows the stored prompt and Reply rows from the transcript read, beside the cards and composer", async () => {
    const pages = [
      {
        messages: [
          { id: 1, role: "owner" as const, text: "Retry every 5 min", evidence: [], requestId: "q1", createdAtMs: 1 },
          { id: 2, role: "supervisor" as const, text: "Done: retrying every 5 min.", evidence: [], requestId: "reply:q1", createdAtMs: 2 },
        ],
        nextCursor: 2,
        hasMore: false,
      },
    ];
    const loadTranscript = vi.fn(async (cursor: number) => (cursor === 0 ? pages[0] : { messages: [], nextCursor: cursor, hasMore: false }));
    renderHost(panelSnapshotFixture, {}, { loadTranscript });
    expect(await screen.findByText("Retry every 5 min")).toBeInTheDocument();
    expect(screen.getByText("Done: retrying every 5 min.")).toBeInTheDocument();
    expect(loadTranscript).toHaveBeenCalledWith(0);
    expect(screen.getAllByTestId("decision-card")).toHaveLength(2);
    expect(screen.getByRole("combobox", { name: "Tell the supervisor anything" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export transcript" })).toBeInTheDocument();
  });

  it("turns a session row Failed from a telemetry panel.session frame alone", async () => {
    const base = panelSnapshotFixture.sessions[0];
    const session = { ...base, sessionRef: "mac:killed", title: "Killed agent", state: "idle" as const, stale: false, updatedAtMs: NOW - 5000 };
    const { handlers } = renderHost({ ...emptyPanelSnapshot(), setupRequired: false, sessions: [session] });
    const row = await screen.findByRole("button", { name: /Killed agent/ });
    expect(row.getAttribute("data-state")).toBe("idle");
    await waitFor(() => expect(handlers).toHaveLength(1));
    act(() =>
      handlers[0]({
        type: "panel.session",
        session: { ...session, updatedAtMs: NOW, health: { type: "failed", reason: "the agent exited in the middle of a turn" } },
      }),
    );
    expect(screen.getByRole("button", { name: /Killed agent/ }).getAttribute("data-state")).toBe("failed");
  });

  // The panel lists the active project's worktrees for a PR hand-off. That read must wait for
  // the backend registry: a rejected SSH host is never registered, so asking anyway fails with
  // WORKSPACE_NOT_FOUND and reads as a local listing rather than "unavailable".
  it("holds the worktree read until the backend accepts the workspace", async () => {
    const listWorktrees = vi.spyOn(tauri, "listWorktrees").mockResolvedValue([]);
    const loadPrs = vi.fn(async () => PR_LANE);
    const load = vi.fn(() => Promise.resolve(panelSnapshotFixture));
    const subscribe = vi.fn(() => Promise.resolve(() => undefined));
    const panel = (registeredWorkspaceId: string | null) => (
      <SupervisorPanelHost
        onFocusSession={vi.fn()}
        load={load}
        subscribe={subscribe}
        actionsFor={vi.fn(() => actionsMock())}
        now={() => NOW}
        workspaceId="ssh:remote"
        registeredWorkspaceId={registeredWorkspaceId}
        loadPrs={loadPrs}
      />
    );
    const view = render(panel(null));
    await waitFor(() => expect(loadPrs).toHaveBeenCalled());
    expect(listWorktrees).not.toHaveBeenCalled();

    view.rerender(panel("ssh:remote"));
    await waitFor(() => expect(listWorktrees).toHaveBeenCalledWith("ssh:remote"));
    listWorktrees.mockRestore();
  });
});
