import { act, fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LanePauseResult } from "../remoteSupervisor";
import { fixtureLane } from "./phoneHomeFixtures";
import { SupervisorSessionStrip } from "./SupervisorSessionStrip";

const ok: LanePauseResult = { kind: "ok", laneKey: "k", paused: true, changed: true };

afterEach(cleanup);

describe("SupervisorSessionStrip", () => {
  it("shows what the supervisor typed and takes over by pausing first, then focusing the input", async () => {
    const order: string[] = [];
    let answer!: (result: LanePauseResult) => void;
    const onSetPaused = vi.fn((laneKey: string, paused: boolean) => {
      order.push(`pause:${laneKey}:${paused}`);
      return new Promise<LanePauseResult>((resolve) => { answer = resolve; });
    });
    const onFocusInput = vi.fn(() => order.push("focus"));
    render(
      <SupervisorSessionStrip
        state={{ kind: "acting", text: "run the tests", atMs: 1000, lane: fixtureLane({ key: "k" }) }}
        now={2000}
        onSetPaused={onSetPaused}
        onFocusInput={onFocusInput}
      />,
    );
    expect(screen.getByTestId("supervisor-session-strip")).toHaveAttribute("data-state", "acting");
    // H-56: the typed text is shown as-is, with no decorative curly quotes around it.
    expect(screen.getByText("run the tests")).toBeInTheDocument();
    expect(screen.getByTestId("supervisor-session-strip").textContent).not.toMatch(/[“”]/);
    fireEvent.click(screen.getByTestId("supervisor-take-over"));
    expect(onFocusInput).not.toHaveBeenCalled();
    await act(async () => answer(ok));
    expect(order).toEqual(["pause:k:true", "focus"]);
  });

  it("keeps the keyboard where it is when the pause is refused, and offers a retry", async () => {
    const onFocusInput = vi.fn();
    render(
      <SupervisorSessionStrip
        state={{ kind: "acting", text: null, atMs: 0, lane: fixtureLane({ key: "k" }) }}
        now={0}
        onSetPaused={async () => ({ kind: "error", code: "OFFLINE", message: "x", status: null })}
        onFocusInput={onFocusInput}
      />,
    );
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-take-over")));
    expect(onFocusInput).not.toHaveBeenCalled();
    expect(screen.getByTestId("supervisor-take-over")).toHaveTextContent("Try again");
  });

  it("announces only a short state sentence, not the buttons or timestamps", () => {
    render(
      <SupervisorSessionStrip state={{ kind: "acting", text: "x", atMs: 0, lane: fixtureLane({ key: "k" }) }} now={120_000} onSetPaused={vi.fn()} onFocusInput={vi.fn()} />,
    );
    const live = screen.getByRole("status");
    expect(live).toHaveTextContent("Supervisor is typing in this session");
    expect(live.querySelector("button")).toBeNull();
    expect(screen.getByTestId("supervisor-session-strip")).not.toHaveAttribute("role");
  });

  it("hands the keyboard over from the left-to-you state", () => {
    const onFocusInput = vi.fn();
    render(<SupervisorSessionStrip state={{ kind: "left-to-you" }} now={0} onSetPaused={vi.fn()} onFocusInput={onFocusInput} />);
    fireEvent.click(screen.getByTestId("supervisor-answer"));
    expect(onFocusInput).toHaveBeenCalledTimes(1);
  });

  it("offers Resume on a paused lane", async () => {
    const onSetPaused = vi.fn(async () => ok);
    render(
      <SupervisorSessionStrip state={{ kind: "paused", lane: fixtureLane({ key: "k", paused: true }) }} now={0} onSetPaused={onSetPaused} onFocusInput={vi.fn()} />,
    );
    expect(screen.getByTestId("supervisor-session-strip")).toHaveAttribute("data-state", "paused");
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-resume")));
    expect(onSetPaused).toHaveBeenCalledWith("k", false);
  });
});
