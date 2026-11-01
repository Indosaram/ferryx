import { act, fireEvent, render, screen, within, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { HubStartResult, LanePauseResult } from "../remoteSupervisor";
import { fixtureAction, fixtureLane, fixtureMachine, fixtureSnapshot } from "./phoneHomeFixtures";
import { SupervisorHome, type SupervisorHomeProps } from "./SupervisorHome";
import { unifiedHosts } from "./usePhoneSupervisor";
import { normalizeUnified } from "../../lib/unifiedSupervisor";

const NOW = 5_000_000;

function renderHome(overrides: Partial<SupervisorHomeProps> = {}) {
  const props: SupervisorHomeProps = {
    machines: [],
    now: NOW,
    needsYou: new Set(),
    onSetPaused: vi.fn(async () => ({ kind: "ok", laneKey: "", paused: true, changed: true }) as LanePauseResult),
    onOpenLane: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<SupervisorHome {...props} />) };
}

afterEach(cleanup);

describe("SupervisorHome", () => {
  it("derives the headline from the lanes and actions of every machine", () => {
    const lane = fixtureLane({ key: "a" });
    renderHome({
      machines: [
        fixtureMachine({
          machineId: "m1",
          snapshot: fixtureSnapshot({ lanes: [lane], actions: [fixtureAction({ atMs: NOW - 500, sessionId: lane.sessionId, text: "yes" })] }),
        }),
      ],
    });
    expect(screen.getByTestId("supervisor-home")).toHaveAttribute("data-headline", "typing");
    const card = screen.getByTestId("supervisor-lane-card");
    expect(within(card).getByTestId("supervisor-lane-status")).toHaveTextContent("Typing");
    expect(screen.getByTestId("supervisor-recent-action")).toHaveTextContent("“yes”");
  });

  it("names the lane by its title in both the open and the pause control", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "a", title: "Docs", cwd: "/x/site" })] }) })] });
    expect(screen.getByTestId("supervisor-lane-open")).toHaveAccessibleName(/^Open Docs,/);
    expect(screen.getByTestId("supervisor-lane-pause")).toHaveAccessibleName("Pause Docs");
    expect(screen.getByTestId("supervisor-lane-pause")).toHaveTextContent("Pause");
  });

  it("says it is still looking while discovery runs, and that nothing is linked once it settled", () => {
    const { rerender, props } = renderHome({ discovering: true });
    expect(screen.getByTestId("supervisor-home-discovering")).toBeInTheDocument();
    expect(screen.queryByTestId("supervisor-home-empty")).toBeNull();
    rerender(<SupervisorHome {...props} discovering={false} />);
    expect(screen.getByTestId("supervisor-home-empty")).toBeInTheDocument();
  });

  it("does not claim the supervisor is off when no computer can be read, and offers a retry", () => {
    const onRetry = vi.fn();
    renderHome({ onRetry, machines: [fixtureMachine({ machineId: "m1", label: "Studio", state: "offline", snapshot: null })] });
    expect(screen.getByTestId("supervisor-home")).toHaveAttribute("data-headline", "unknown");
    expect(screen.getByTestId("supervisor-home-headline")).not.toHaveTextContent(/isn't running/);
    fireEvent.click(screen.getByTestId("supervisor-machine-retry"));
    expect(onRetry).toHaveBeenCalledWith("m1");
  });

  it("states the stopped supervisor once, without a command line", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "m1", label: "Studio", snapshot: fixtureSnapshot({ hub: { available: false, running: false } }) })] });
    expect(screen.getByTestId("supervisor-hub-off")).toHaveTextContent("Start it from Ferryx on Studio.");
    expect(screen.getByTestId("supervisor-home")).not.toHaveTextContent("ferryx hub start");
  });

  it("shows plain error copy instead of a raw code", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "m1", state: "error", snapshot: null, error: { code: "UNAVAILABLE", message: "x" } })] });
    expect(screen.getByTestId("supervisor-machine-state")).not.toHaveTextContent("UNAVAILABLE");
  });

  it("shows the last three actions and expands to the rest", () => {
    const actions = Array.from({ length: 5 }, (_, i) => fixtureAction({ atMs: NOW - (i + 1) * 60_000, text: `step ${i}` }));
    renderHome({ machines: [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ actions }) })] });
    expect(screen.getAllByTestId("supervisor-recent-action")).toHaveLength(3);
    fireEvent.click(screen.getByTestId("supervisor-recent-toggle"));
    expect(screen.getAllByTestId("supervisor-recent-action")).toHaveLength(5);
  });

  it("pauses a lane with its key and disables the button until the machine answers", async () => {
    let answer!: (result: LanePauseResult) => void;
    const onSetPaused = vi.fn(() => new Promise<LanePauseResult>((resolve) => { answer = resolve; }));
    renderHome({
      onSetPaused,
      machines: [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "lane-7", title: "Docs" })] }) })],
    });
    const button = screen.getByRole("button", { name: "Pause Docs" });
    fireEvent.click(button);
    expect(onSetPaused).toHaveBeenCalledWith("m1", "lane-7", true);
    expect(button).toBeDisabled();
    expect(screen.getByRole("button", { name: "Resume Docs" })).toBe(button);
    fireEvent.click(button);
    expect(onSetPaused).toHaveBeenCalledTimes(1);
    await act(async () => answer({ kind: "ok", laneKey: "lane-7", paused: true, changed: true }));
    expect(button).toBeEnabled();
  });

  it("rolls the optimistic state back when the pause fails", async () => {
    const onSetPaused = vi.fn(async (): Promise<LanePauseResult> => ({ kind: "error", code: "OFFLINE", message: "gone", status: null }));
    renderHome({
      onSetPaused,
      machines: [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "k", title: "Docs" })] }) })],
    });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Pause Docs" })));
    expect(screen.getByRole("button", { name: "Pause Docs" })).toBeEnabled();
  });

  it("shows a later change from another surface once the snapshot confirmed the pause", async () => {
    const lanes = (paused: boolean) => [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "k", title: "Docs", paused })] }) })];
    const { rerender, props } = renderHome({ machines: lanes(false) });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Pause Docs" })));
    // The machine confirms the pause, then the desktop resumes the lane.
    rerender(<SupervisorHome {...props} machines={lanes(true)} />);
    expect(screen.getByRole("button", { name: "Resume Docs" })).toBeEnabled();
    rerender(<SupervisorHome {...props} machines={lanes(false)} />);
    expect(screen.getByRole("button", { name: "Pause Docs" })).toBeEnabled();
  });

  it("tells the person to update Ferryx on an unsupported machine", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "old-mac", label: "Old Mac", state: "unsupported", snapshot: null })] });
    const section = screen.getByTestId("supervisor-machine-old-mac");
    expect(within(section).getByTestId("supervisor-machine-state")).toHaveTextContent(
      "Update Ferryx on this computer to see its supervisor.",
    );
  });

  it("explains where to start a stopped hub", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "m1", label: "Studio", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })] });
    expect(screen.getByTestId("supervisor-hub-off")).toHaveTextContent("Start it from Ferryx on Studio.");
    expect(screen.getByTestId("supervisor-home")).toHaveAttribute("data-headline", "off");
  });

  it("starts a stopped hub from the phone and disables the button until the machine answers", async () => {
    let answer!: (result: HubStartResult) => void;
    const onStartHub = vi.fn(() => new Promise<HubStartResult>((resolve) => { answer = resolve; }));
    renderHome({ onStartHub, machines: [fixtureMachine({ machineId: "m1", label: "Studio", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })] });
    const button = screen.getByTestId("supervisor-hub-start");
    fireEvent.click(button);
    expect(onStartHub).toHaveBeenCalledWith("m1");
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent("Starting…");
    fireEvent.click(button);
    expect(onStartHub).toHaveBeenCalledTimes(1);
    await act(async () => answer({ kind: "ok", started: true }));
    expect(button).toBeEnabled();
  });

  it("offers no start button where this machine has no hub at all", () => {
    renderHome({ onStartHub: vi.fn(), machines: [fixtureMachine({ machineId: "m1", label: "Studio", snapshot: fixtureSnapshot({ hub: { available: false, running: false } }) })] });
    expect(screen.queryByTestId("supervisor-hub-start")).toBeNull();
    expect(screen.getByTestId("supervisor-hub-off")).toHaveTextContent("Start it from Ferryx on Studio.");
  });

  it("opens the lane's session on that machine when its card is tapped", () => {
    const { props } = renderHome({
      machines: [fixtureMachine({ machineId: "m2", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "k", sessionId: "sess-9" })] }) })],
    });
    fireEvent.click(screen.getByTestId("supervisor-lane-open"));
    expect(props.onOpenLane).toHaveBeenCalledWith("m2", "sess-9");
  });

  it("shows the lane's last stop reason", () => {
    renderHome({
      machines: [fixtureMachine({ machineId: "m1", snapshot: fixtureSnapshot({ lanes: [fixtureLane({ key: "k", lastReason: "tests failed twice" })] }) })],
    });
    expect(screen.getByTestId("supervisor-lane-reason")).toHaveTextContent("Stopped: tests failed twice");
  });
});

describe("SupervisorHome unified supervisor", () => {
  const unifiedWire = {
    identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
    projects: [{ projectId: "p-1", name: "qa-supervisor" }],
    goals: [{ goalId: "g-1", supervisorId: "s-1", projectId: "p-1", laneKey: "sv-1", title: "Write result.txt", runState: "running",
      stateDetail: null, revision: 2, pendingWaits: [], nextTrigger: null, updatedAt: "", mode: "full", turnLimit: 20, turnsUsed: 1, grant: ["edit"] }],
    health: { state: "ready", generation: 1, pid: 3, capabilities: [], serverVersion: null, fault: null, detail: null, queuedDecisions: 0 },
  };
  const live = normalizeUnified(unifiedWire)!;

  it("renders the host's goals without a machine picker and pauses with the revision", async () => {
    const requests: unknown[] = [];
    const client = {
      request: vi.fn(async (envelope: unknown) => {
        requests.push(envelope);
        return { kind: "ok" as const, value: {} };
      }),
      conversation: vi.fn(async () => ({ kind: "ok" as const, value: { messages: [], next: 0 } })),
      memory: vi.fn(async () => ({ kind: "error" as const, code: "MEMORY_RECEIPTS_UNAVAILABLE", message: "", status: 503 })),
      remember: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
      refresh: vi.fn(),
    };
    renderHome({
      machines: [fixtureMachine({ machineId: "studio", snapshot: fixtureSnapshot({ supervisor: live }) })],
      unified: [{ machineId: "studio", label: "Studio", connection: "live", error: null, view: live, client }],
    });
    await act(async () => {
      await client.conversation.mock.results[0]?.value;
      await client.memory.mock.results[0]?.value;
    });
    expect(screen.getByTestId("supervisor-unified").dataset.surface).toBe("phone");
    expect(screen.queryByRole("combobox", { name: /machine/i })).toBeNull();
    expect(screen.getByTestId("supervisor-goal-id")).toHaveTextContent("g-1");
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-pause")));
    expect(requests[0]).toMatchObject({ goalId: "g-1", expectedRevision: 2, action: "pause" });
    expect(client.refresh).toHaveBeenCalledTimes(1);
  });

  it("keeps a disconnected host's last goals, says so, and substitutes no other supervisor", () => {
    const lastSeen = new Map();
    const clientFor = vi.fn(() => null);
    const liveMachines = [
      fixtureMachine({ machineId: "studio", label: "Studio", snapshot: fixtureSnapshot({ supervisor: live }) }),
      fixtureMachine({ machineId: "laptop", label: "Laptop", snapshot: fixtureSnapshot() }),
    ];
    expect(unifiedHosts(liveMachines, lastSeen, clientFor).map((host) => [host.machineId, host.connection])).toEqual([["studio", "live"]]);
    const gone = [
      fixtureMachine({ machineId: "studio", label: "Studio", state: "offline", snapshot: null, error: { code: "OFFLINE", message: "tunnel closed" } }),
      fixtureMachine({ machineId: "laptop", label: "Laptop", snapshot: fixtureSnapshot() }),
    ];
    const hosts = unifiedHosts(gone, lastSeen, clientFor);
    expect(hosts).toHaveLength(1);
    expect(hosts[0]).toMatchObject({ machineId: "studio", connection: "disconnected", client: null, error: { code: "OFFLINE" } });
    expect(hosts[0]!.view.goals[0]!.goalId).toBe("g-1");
    // A hub that closed on a reachable machine is unavailable, not disconnected.
    const closed = [fixtureMachine({ machineId: "studio", label: "Studio", snapshot: fixtureSnapshot({ supervisor: null }) })];
    expect(unifiedHosts(closed, lastSeen, clientFor)[0]).toMatchObject({ connection: "unavailable", client: null, error: { code: "SUPERVISOR_UNAVAILABLE" } });

    renderHome({ machines: gone, unified: hosts });
    const banner = screen.getByTestId("supervisor-connection");
    expect(banner.dataset.state).toBe("disconnected");
    expect(banner).toHaveTextContent("Studio is disconnected");
    expect(screen.getByTestId("supervisor-goal-id")).toHaveTextContent("g-1");
    expect(screen.getByTestId("supervisor-pause")).toBeDisabled();
  });
});
