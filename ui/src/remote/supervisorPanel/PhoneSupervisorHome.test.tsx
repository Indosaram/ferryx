import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import type { PanelSnapshot } from "../../lib/supervisorPanel/contract";
import { PhoneHomeView, type PhoneHomeViewProps } from "../home/PhoneHomeView";
import { PhoneSupervisorHome, type PhoneSupervisorHomeProps } from "./PhoneSupervisorHome";

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);

function props(overrides: Partial<PhoneSupervisorHomeProps> = {}): PhoneSupervisorHomeProps {
  return {
    state: { kind: "ready", snapshot: panelSnapshotFixture },
    now: NOW,
    onOpenSession: vi.fn(),
    onAnswer: vi.fn(async () => ({ ok: true }) as const),
    onSetPaused: vi.fn(async () => ({ ok: true }) as const),
    onConverse: vi.fn(async () => ({ ok: true }) as const),
    onRefresh: vi.fn(),
    ...overrides,
  };
}

function renderHome(overrides: Partial<PhoneSupervisorHomeProps> = {}) {
  const p = props(overrides);
  return { props: p, ...render(<PhoneSupervisorHome {...p} />) };
}

afterEach(cleanup);

describe("PhoneSupervisorHome", () => {
  it("renders both decisions with the recommended option first", () => {
    renderHome();
    const cards = screen.getAllByTestId("phone-panel-decision");
    expect(cards).toHaveLength(2);
    for (const card of cards) {
      const options = within(card).getAllByTestId("phone-panel-option");
      expect(options[0]).toHaveAttribute("data-recommended", "true");
      expect(options.slice(1).every((option) => option.getAttribute("data-recommended") === "false")).toBe(true);
      expect(within(card).getByTestId("phone-panel-decision-reason")).not.toBeEmptyDOMElement();
    }
    expect(screen.getByTestId("phone-panel-status")).toHaveTextContent("2 decisions need you");
  });

  it("answers with the prompt's id and screen revision, then re-enables the buttons", async () => {
    const onAnswer = vi.fn(async () => ({ ok: true }) as const);
    renderHome({ onAnswer });
    const card = screen.getAllByTestId("phone-panel-decision").find((item) => item.getAttribute("data-session-ref") === "mac:relay")!;
    await act(async () => fireEvent.click(within(card).getAllByTestId("phone-panel-option")[0]));
    expect(onAnswer).toHaveBeenCalledWith("mac:relay", "prompt-relay-1", "rev-relay-41", 0);
  });

  it("says a stale answer changed instead of showing a raw error", async () => {
    const onAnswer = vi.fn(async () => ({ ok: false, stale: true, message: "x" }) as const);
    renderHome({ onAnswer });
    const card = screen.getAllByTestId("phone-panel-decision")[0];
    await act(async () => fireEvent.click(within(card).getAllByTestId("phone-panel-option")[0]));
    expect(within(card).getByTestId("phone-panel-decision-error")).toHaveTextContent("changed");
  });

  it("opens the session when a decision header is tapped", () => {
    const { props: p } = renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Open Fix relay reconnect" }));
    expect(p.onOpenSession).toHaveBeenCalledWith("mac:relay");
  });

  it("navigates to that session when a session row is tapped", () => {
    const { props: p } = renderHome();
    const row = screen.getAllByTestId("phone-panel-session-row").find((item) => item.getAttribute("data-session-ref") === "omarchy:notes")!;
    fireEvent.click(within(row).getByRole("button"));
    expect(p.onOpenSession).toHaveBeenCalledWith("omarchy:notes");
  });

  it("lists each session once: decided and handled sessions are not repeated under Sessions", () => {
    renderHome();
    const refs = screen.getAllByTestId("phone-panel-session-row").map((row) => row.getAttribute("data-session-ref"));
    expect(refs).not.toContain("mac:relay");
    expect(refs).not.toContain("win:roblox");
    expect(refs).not.toContain("win:wintests");
    expect(new Set(refs).size).toBe(refs.length);
  });

  it("renders an unknown state as Idle with 'status unknown', never Working", () => {
    const snapshot: PanelSnapshot = {
      ...panelSnapshotFixture,
      decisions: [],
      handling: [],
      sessions: [{ ...panelSnapshotFixture.sessions[4], sessionRef: "mac:odd", state: "unknown" }],
    };
    renderHome({ state: { kind: "ready", snapshot } });
    const row = screen.getByTestId("phone-panel-session-row");
    expect(row).toHaveAttribute("data-status", "idle");
    expect(row).toHaveTextContent("status unknown");
  });

  it("keeps every control at least 44px tall", () => {
    renderHome();
    for (const button of screen.getAllByRole("button")) {
      expect(button.className).toMatch(/min-h-11|h-11/);
    }
  });

  it("shows a setup card instead of the panel when setup is required", () => {
    renderHome({ state: { kind: "ready", snapshot: { ...panelSnapshotFixture, setupRequired: true } } });
    expect(screen.getByTestId("phone-panel-setup")).toBeInTheDocument();
    expect(screen.queryByTestId("phone-panel-decision")).toBeNull();
  });

  it("sends a composed message and clears it", async () => {
    const onConverse = vi.fn(async () => ({ ok: true }) as const);
    renderHome({ onConverse });
    const input = screen.getByLabelText("Message the supervisor");
    fireEvent.change(input, { target: { value: "retry every 5 min" } });
    await act(async () => fireEvent.submit(screen.getByTestId("phone-panel-composer")));
    expect(onConverse).toHaveBeenCalledWith("retry every 5 min");
    expect(input).toHaveValue("");
  });

  it("renders no tab bar on the panel home", () => {
    const home = props();
    // The panel replaces the three-destination home; the other props are only for the old home.
    const old = {
      panel: home,
      destination: "supervisor",
      onSelectTab: vi.fn(),
      supervisor: { machines: [], now: NOW, needsYou: new Set(), onSetPaused: vi.fn(), onOpenLane: vi.fn() },
      inbox: { rows: [], loading: false, onOpen: vi.fn() },
      worktrees: null,
      resume: null,
    } as unknown as PhoneHomeViewProps;
    render(<PhoneHomeView {...old} />);
    expect(screen.getByTestId("phone-supervisor-home")).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByTestId("mobile-home-tabs")).toBeNull();
    expect(screen.queryByTestId("mobile-home-tab-inbox")).toBeNull();
  });
});
