import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { PanelSnapshot } from "../../lib/supervisorPanel/contract";
import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import { PhoneHomeView, type PhoneHomeViewProps } from "../home/PhoneHomeView";
import { PhoneSupervisorHome, type PhoneSupervisorHomeProps } from "./PhoneSupervisorHome";

const NOW = Date.UTC(2026, 10, 2, 14, 17, 0);

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

function props(overrides: Partial<PhoneSupervisorHomeProps> = {}): PhoneSupervisorHomeProps {
  return {
    state: { kind: "ready", snapshot: panelSnapshotFixture },
    now: NOW,
    actions: actionsMock(),
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
  it("renders both decisions from the shared card, recommended option first", () => {
    renderHome();
    const cards = screen.getAllByTestId("decision-card");
    expect(cards).toHaveLength(2);
    for (const card of cards) {
      const options = within(card).getAllByRole("button").filter((b) => b.hasAttribute("data-recommended"));
      expect(options[0]).toHaveAttribute("data-recommended", "true");
      expect(options.slice(1).every((option) => option.getAttribute("data-recommended") === "false")).toBe(true);
    }
    expect(screen.getByTestId("supervisor-status-line")).toHaveTextContent("2 decisions for you.");
  });

  it("answers with the prompt's id and screen revision, then re-enables the buttons", async () => {
    const actions = actionsMock();
    renderHome({ actions });
    const card = screen.getByRole("article", { name: "Decision for Fix relay reconnect" });
    await act(async () => fireEvent.click(within(card).getByRole("button", { name: "Allow" })));
    expect(actions.answer).toHaveBeenCalledWith(
      { sessionRef: "mac:relay", promptId: "prompt-relay-1", screenRevision: "rev-relay-41" },
      0,
    );
    expect(within(card).getByRole("button", { name: "Allow" })).toBeEnabled();
  });

  it("answers with the option's own index even when the recommendation is not first", async () => {
    const actions = actionsMock();
    const snapshot: PanelSnapshot = {
      ...panelSnapshotFixture,
      decisions: [{ ...panelSnapshotFixture.decisions[1], recommendedIndex: 2 }],
    };
    renderHome({ actions, state: { kind: "ready", snapshot } });
    const card = screen.getByTestId("decision-card");
    const buttons = within(card).getAllByRole("button").filter((b) => b.hasAttribute("data-recommended"));
    expect(buttons[0]).toHaveTextContent("Stop");
    await act(async () => fireEvent.click(buttons[0]));
    expect(actions.answer).toHaveBeenCalledWith(expect.objectContaining({ promptId: "prompt-roblox-1" }), 2);
  });

  it("says a stale answer changed instead of showing a raw error", async () => {
    const actions = actionsMock({ answer: vi.fn(async () => ({ ok: false, stale: true, message: "x" }) as const) });
    renderHome({ actions });
    const card = screen.getAllByTestId("decision-card")[0];
    await act(async () => fireEvent.click(within(card).getAllByRole("button").find((b) => b.hasAttribute("data-recommended"))!));
    expect(within(card).getByTestId("decision-card-error")).toHaveTextContent("This question changed. Look again.");
  });

  it("shows a refused answer's message on the card", async () => {
    const actions = actionsMock({ answer: vi.fn(async () => ({ ok: false, stale: false, message: "This phone can only view the panel." }) as const) });
    renderHome({ actions });
    const card = screen.getAllByTestId("decision-card")[0];
    await act(async () => fireEvent.click(within(card).getAllByRole("button").find((b) => b.hasAttribute("data-recommended"))!));
    expect(within(card).getByRole("alert")).toHaveTextContent("only view");
  });

  it("opens the session when a decision header is tapped", () => {
    const { props: p } = renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Open Fix relay reconnect terminal" }));
    expect(p.actions.openSession).toHaveBeenCalledWith("mac:relay");
  });

  it("navigates to that session when a session row is tapped", () => {
    const { props: p } = renderHome();
    const row = screen.getAllByTestId("session-row").find((item) => item.textContent?.includes("Release notes for 11.2"))!;
    fireEvent.click(row);
    expect(p.actions.openSession).toHaveBeenCalledWith("omarchy:notes");
  });

  it("lists each session once: decided and handled sessions are not repeated under Sessions", () => {
    renderHome();
    const sessions = screen.getByRole("region", { name: "Sessions" });
    const titles = within(sessions).getAllByTestId("session-row").map((row) => row.textContent ?? "");
    expect(titles.some((t) => t.includes("Fix relay reconnect"))).toBe(false);
    expect(titles.some((t) => t.includes("Roblox build"))).toBe(false);
    expect(titles.some((t) => t.includes("Windows installer tests"))).toBe(false);
    expect(titles).toHaveLength(2);
  });

  it("renders an unknown state as Idle with 'status unknown', never Working", () => {
    const snapshot: PanelSnapshot = {
      ...panelSnapshotFixture,
      decisions: [],
      handling: [],
      sessions: [{ ...panelSnapshotFixture.sessions[4], sessionRef: "mac:odd", state: "unknown" }],
    };
    renderHome({ state: { kind: "ready", snapshot } });
    const row = screen.getByTestId("session-row");
    expect(row).toHaveAttribute("data-state", "idle");
    expect(row).toHaveTextContent("status unknown");
  });

  it("stops a handled session through the Stop button", async () => {
    const { props: p } = renderHome();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop Windows installer tests" })));
    expect(p.actions.stop).toHaveBeenCalledWith("win:wintests");
  });

  it("shows the refusal when a stop is not accepted", async () => {
    const actions = actionsMock({ stop: vi.fn(async () => ({ ok: false, stale: false, message: "This computer doesn't support that yet. Update Ferryx on it." }) as const) });
    renderHome({ actions });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Stop Windows installer tests" })));
    expect(screen.getByTestId("supervisor-panel-error")).toHaveTextContent("doesn't support that yet");
  });

  it("shows Unverified, Why and Undo on the log, and undoes by action id", async () => {
    const { props: p } = renderHome();
    fireEvent.click(screen.getByRole("button", { name: /^All \d+$/ }));
    expect(screen.queryAllByText("Unverified")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Why: Answered Release notes: v2026.1102.1" }));
    expect(screen.getByText("You picked the stable tag the last 4 times")).toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Undo: Answered Release notes: v2026.1102.1" })));
    expect(p.actions.undo).toHaveBeenCalledWith(4);
  });

  it("marks a claim without evidence as Unverified", () => {
    const entry = { ...panelSnapshotFixture.log[0], evidence: [] };
    renderHome({ state: { kind: "ready", snapshot: { ...panelSnapshotFixture, log: [entry] } } });
    expect(screen.getByText("Unverified")).toBeInTheDocument();
  });

  it("keeps every control at least 44px tall", () => {
    renderHome();
    for (const button of screen.getAllByRole("button")) {
      expect(button.className).toMatch(/min-h-11|h-11|size-11/);
    }
  });

  it("shows a setup card instead of the panel when setup is required, and turns it on", async () => {
    const { props: p } = renderHome({ state: { kind: "ready", snapshot: { ...panelSnapshotFixture, setupRequired: true } }, computerLabel: "maho-win" });
    expect(screen.getByTestId("setup-card")).toHaveTextContent("maho-win");
    expect(screen.queryByTestId("decision-card")).toBeNull();
    expect(screen.queryByTestId("phone-panel-composer")).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Turn on" })));
    expect(p.actions.turnOn).toHaveBeenCalled();
  });

  it("toggles pause through the status line", async () => {
    const { props: p } = renderHome();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Pause" })));
    expect(p.actions.setPaused).toHaveBeenCalledWith(true);
  });

  it("sends a composed message with its typed mentions and clears it", async () => {
    const actions = actionsMock();
    renderHome({ actions });
    const input = screen.getByRole("combobox", { name: "Tell the supervisor anything" });
    fireEvent.change(input, { target: { value: "@roblox revert it" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    expect(actions.converse).toHaveBeenCalledWith("@roblox revert it", ["win:roblox"]);
    expect(input).toHaveValue("");
  });

  it("keeps the message and shows the reason when the send is refused", async () => {
    const actions = actionsMock({ converse: vi.fn(async () => ({ ok: false, stale: false, message: "This computer is not reachable right now." }) as const) });
    renderHome({ actions });
    const input = screen.getByRole("combobox", { name: "Tell the supervisor anything" });
    fireEvent.change(input, { target: { value: "retry every 5 min" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    expect(input).toHaveValue("retry every 5 min");
    expect(screen.getByTestId("supervisor-composer-error")).toHaveTextContent("not reachable");
  });

  it("pins the composer to the safe area and keeps it out of the scroller", () => {
    renderHome();
    const pinned = screen.getByTestId("phone-panel-composer");
    expect(pinned.className).toContain("env(safe-area-inset-bottom)");
    expect(pinned.closest("main")).toBeNull();
  });

  it("offers + to start a session only when the host gives a path for it", () => {
    const onStartSession = vi.fn();
    const { unmount } = renderHome();
    expect(screen.queryByTestId("phone-panel-start-session")).toBeNull();
    unmount();
    renderHome({ onStartSession });
    fireEvent.click(screen.getByRole("button", { name: "Start a session" }));
    expect(onStartSession).toHaveBeenCalledTimes(1);
  });

  it("rules 'Change' puts starter text in the pinned composer", () => {
    renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    expect(screen.getByRole("combobox", { name: "Tell the supervisor anything" })).toHaveValue("Change the rules: ");
  });

  it("shows the loading and error states with the pull hint", () => {
    const { unmount } = renderHome({ state: { kind: "loading" } });
    expect(screen.getByTestId("phone-panel-loading")).toBeInTheDocument();
    unmount();
    renderHome({ state: { kind: "error", message: "Couldn't read the supervisor panel." } });
    expect(screen.getByTestId("phone-panel-error")).toHaveTextContent("Pull down to try again.");
  });

  it("renders no tab bar on the panel home", () => {
    const view: PhoneHomeViewProps = {
      panel: props(),
      destination: "supervisor",
      onSelectTab: vi.fn(),
      worktrees: null,
    };
    render(<PhoneHomeView {...view} />);
    expect(screen.getByTestId("phone-supervisor-home")).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByTestId("mobile-home-tabs")).toBeNull();
    expect(screen.queryByTestId("mobile-home-tab-inbox")).toBeNull();
  });

  it("shows the setup card, never a fallback home, when the panel reports no hub", () => {
    const view: PhoneHomeViewProps = {
      panel: props({ state: { kind: "ready", snapshot: { ...panelSnapshotFixture, setupRequired: true } }, computerLabel: "Studio" }),
      destination: "supervisor",
      onSelectTab: vi.fn(),
      worktrees: null,
    };
    render(<PhoneHomeView {...view} />);
    expect(screen.getByTestId("setup-card")).toHaveTextContent("Turn on the supervisor on Studio");
    expect(screen.queryByTestId("decision-card")).toBeNull();
    expect(screen.queryByTestId("session-row")).toBeNull();
  });

  it("'+' shows the Worktrees list under the panel, with a way back and still no tab bar", () => {
    const onSelectTab = vi.fn();
    const view: PhoneHomeViewProps = {
      panel: props({ onStartSession: vi.fn() }),
      destination: "worktrees",
      onSelectTab,
      worktrees: <p>Worktrees list</p>,
    };
    render(<PhoneHomeView {...view} />);
    expect(screen.getByText("Worktrees list")).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).toBeNull();
    fireEvent.click(screen.getByTestId("phone-panel-start-back"));
    expect(onSelectTab).toHaveBeenCalledWith("supervisor");
  });
});
