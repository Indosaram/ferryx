import { act, fireEvent, render, screen, within, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LanePauseResult } from "../remoteSupervisor";
import { fixtureAction, fixtureLane, fixtureMachine, fixtureSnapshot } from "./phoneHomeFixtures";
import { SupervisorHome, type SupervisorHomeProps } from "./SupervisorHome";

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
    expect(screen.getByTestId("supervisor-home-headline")).toHaveTextContent("typing");
    const card = screen.getByTestId("supervisor-lane-card");
    expect(within(card).getByTestId("supervisor-lane-status")).toHaveTextContent("Typing");
    expect(screen.getByTestId("supervisor-recent-action")).toHaveTextContent("“yes”");
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

  it("explains how to start a stopped hub", () => {
    renderHome({ machines: [fixtureMachine({ machineId: "m1", label: "Studio", snapshot: fixtureSnapshot({ hub: { available: true, running: false } }) })] });
    expect(screen.getByTestId("supervisor-hub-off")).toHaveTextContent("The supervisor is not running on Studio. Start it with ferryx hub start.");
    expect(screen.getByTestId("supervisor-home-headline")).toHaveTextContent("off");
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
