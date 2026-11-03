import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PanelEvent, PanelSnapshot } from "../../lib/supervisorPanel/contract";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import { applyPanelEvent } from "./applyPanelEvent";
import { SupervisorPanelHost } from "./SupervisorPanelHost";

afterEach(cleanup);

function renderHost(snapshot: PanelSnapshot, onFocusSession = vi.fn()) {
  const handlers: Array<(event: PanelEvent) => void> = [];
  const load = vi.fn(() => Promise.resolve(snapshot));
  const subscribe = vi.fn((handler: (event: PanelEvent) => void) => {
    handlers.push(handler);
    return Promise.resolve(() => undefined);
  });
  render(<SupervisorPanelHost onFocusSession={onFocusSession} load={load} subscribe={subscribe} />);
  return { onFocusSession, handlers };
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

  it("focuses the session when its row is clicked", async () => {
    const { onFocusSession } = renderHost(panelSnapshotFixture);
    const sessions = await screen.findByRole("region", { name: "Sessions" });
    fireEvent.click(within(sessions).getByRole("button", { name: "Release notes for 11.2" }));
    expect(onFocusSession).toHaveBeenCalledWith("omarchy:notes");
  });

  it("focuses the session of a decision card", async () => {
    const { onFocusSession } = renderHost(panelSnapshotFixture);
    const needsYou = await screen.findByRole("region", { name: "Needs you" });
    fireEvent.click(within(needsYou).getByRole("button", { name: "Fix relay reconnect" }));
    expect(onFocusSession).toHaveBeenCalledWith("mac:relay");
  });

  it("applies panel events after the first load", async () => {
    const { handlers } = renderHost(panelSnapshotFixture);
    await screen.findByRole("region", { name: "Needs you" });
    await waitFor(() => expect(handlers).toHaveLength(1));
    act(() => handlers[0]({ type: "panel.decision", sessionRef: "mac:relay", decision: null }));
    const needsYou = screen.getByRole("region", { name: "Needs you" });
    expect(within(needsYou).queryByRole("button", { name: "Fix relay reconnect" })).toBeNull();
  });
});

describe("applyPanelEvent", () => {
  it("moves a finished action from handling to the log in place", () => {
    const live = panelSnapshotFixture.handling[0];
    const next = applyPanelEvent(panelSnapshotFixture, {
      type: "panel.action",
      action: { ...live, live: false, summary: "done" },
    });
    expect(next.handling.some((entry) => entry.id === live.id)).toBe(false);
    expect(next.log.find((entry) => entry.id === live.id)?.summary).toBe("done");
  });
});
