import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { panelSnapshotFixture } from "../../lib/supervisorPanel/fixtures";
import type { PanelEvent, PanelSnapshot } from "../../lib/supervisorPanel/contract";
import type { PanelActions } from "../../lib/supervisorPanel/actions";
import { emptyPanelSnapshot } from "../../lib/supervisorPanel/emptySnapshot";
import { InboxPopover } from "./InboxPopover";

afterEach(cleanup);

function actions(overrides: Partial<PanelActions> = {}): PanelActions {
  return {
    answer: vi.fn(async () => ({ ok: true as const })),
    setPaused: vi.fn(async () => ({ ok: true as const })),
    setLanePaused: vi.fn(async () => ({ ok: true as const })),
    converse: vi.fn(async () => ({ ok: true as const })),
    stop: vi.fn(async () => ({ ok: true as const })),
    undo: vi.fn(async () => ({ ok: true as const })),
    turnOn: vi.fn(async () => ({ ok: true as const })),
    openSession: vi.fn(),
    ...overrides,
  };
}

/** A feed that hands back its handlers, so a test can push one `panel.*` frame. */
function feed() {
  const handlers: Array<(event: PanelEvent) => void> = [];
  return {
    handlers,
    subscribe: async (handler: (event: PanelEvent) => void) => {
      handlers.push(handler);
      return () => undefined;
    },
  };
}

function renderInbox(overrides: {
  snapshot?: PanelSnapshot;
  load?: () => Promise<PanelSnapshot>;
  actions?: PanelActions;
  onClose?: () => void;
  onFocusSession?: (sessionRef: string) => void;
} = {}) {
  const onClose = overrides.onClose ?? vi.fn();
  const onFocusSession = overrides.onFocusSession ?? vi.fn();
  const channel = feed();
  // Mirrors desktopPanelActions: the popover hands its session focus in as `openSession`.
  const actionsFor = vi.fn((input: { openSession: (sessionRef: string) => void }) =>
    overrides.actions ?? actions({ openSession: input.openSession }),
  );
  const result = render(
    <InboxPopover
      onClose={onClose}
      onFocusSession={onFocusSession}
      load={overrides.load ?? (async () => overrides.snapshot ?? panelSnapshotFixture)}
      subscribe={channel.subscribe}
      actionsFor={actionsFor}
      now={() => Date.UTC(2026, 10, 2, 14, 17, 0)}
    />,
  );
  return { ...result, onClose, onFocusSession, channel, actionsFor };
}

describe("InboxPopover", () => {
  it("renders one answerable card per decision the panel holds", async () => {
    renderInbox();

    const popup = await screen.findByTestId("inbox-popover");
    expect(screen.getByRole("dialog", { name: "Inbox" })).toBe(popup);
    await waitFor(() =>
      expect(within(popup).getAllByTestId("decision-card")).toHaveLength(panelSnapshotFixture.decisions.length),
    );
    expect(within(popup).getByText("Run the relay database migration?")).toBeInTheDocument();
    expect(within(popup).getByText("2 waiting")).toBeInTheDocument();
  });

  it("answers through the panel's guarded route with the option index the owner clicked", async () => {
    const panelActions = actions();
    renderInbox({ actions: panelActions });

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() => expect(within(popup).getAllByTestId("decision-card")).toHaveLength(2));

    const relay = within(popup).getByRole("article", { name: "Decision for Fix relay reconnect" });
    fireEvent.click(within(relay).getByRole("button", { name: "Allow" }));

    await waitFor(() =>
      expect(panelActions.answer).toHaveBeenCalledWith(
        { sessionRef: "mac:relay", promptId: "prompt-relay-1", screenRevision: "rev-relay-41" },
        0,
      ),
    );
  });

  it("opens the session's terminal from a card's header", async () => {
    const onFocusSession = vi.fn();
    const onClose = vi.fn();
    renderInbox({ onFocusSession, onClose });

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() => expect(within(popup).getAllByTestId("decision-card")).toHaveLength(2));
    fireEvent.click(within(popup).getByRole("button", { name: "Open Fix relay reconnect terminal" }));

    expect(onFocusSession).toHaveBeenCalledWith("mac:relay");
  });

  it("says the inbox is clear when the panel holds no decision", async () => {
    renderInbox({ snapshot: { ...panelSnapshotFixture, decisions: [] } });

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() => expect(within(popup).getByTestId("inbox-empty")).toBeInTheDocument());
    expect(within(popup).getByTestId("inbox-empty")).toHaveTextContent("Nothing is waiting on you.");
    expect(within(popup).queryAllByTestId("decision-card")).toHaveLength(0);
    expect(within(popup).getByText("Nothing waiting")).toBeInTheDocument();
  });

  it("asks for the supervisor instead of claiming an empty inbox while setup is required", async () => {
    renderInbox({ load: async () => emptyPanelSnapshot() });

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() =>
      expect(within(popup).getByText("Turn the supervisor on to collect decisions here.")).toBeInTheDocument(),
    );
    expect(within(popup).queryByTestId("inbox-empty")).toBeNull();
  });

  it("follows a panel decision frame without reading the hub again", async () => {
    const channel = feed();
    const load = vi.fn(async () => ({ ...panelSnapshotFixture, decisions: [] }));
    render(
      <InboxPopover
        onClose={vi.fn()}
        onFocusSession={vi.fn()}
        load={load}
        subscribe={channel.subscribe}
        actionsFor={() => actions()}
        now={() => Date.UTC(2026, 10, 2, 14, 17, 0)}
      />,
    );

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() => expect(within(popup).getByTestId("inbox-empty")).toBeInTheDocument());

    const decision = panelSnapshotFixture.decisions[0]!;
    channel.handlers.forEach((handler) => handler({ type: "panel.decision", sessionRef: decision.sessionRef, decision }));

    await waitFor(() => expect(within(popup).getAllByTestId("decision-card")).toHaveLength(1));
    expect(within(popup).getByText("Run the relay database migration?")).toBeInTheDocument();
    // The frame was folded in, so the popup did not re-read the snapshot for it.
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and on its close button", async () => {
    const onClose = vi.fn();
    renderInbox({ onClose });
    await screen.findByTestId("inbox-popover");

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Close inbox" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("keeps a stale refusal in the card instead of closing it", async () => {
    const panelActions = actions({
      answer: vi.fn(async () => ({ ok: false as const, stale: true, message: "ignored" })),
    });
    renderInbox({ actions: panelActions });

    const popup = await screen.findByTestId("inbox-popover");
    await waitFor(() => expect(within(popup).getAllByTestId("decision-card")).toHaveLength(2));
    const relay = within(popup).getByRole("article", { name: "Decision for Fix relay reconnect" });
    fireEvent.click(within(relay).getByRole("button", { name: "Allow" }));

    expect(await within(relay).findByTestId("decision-card-error")).toHaveTextContent("This question changed. Look again.");
  });

  it("is portalled to <body>, outside the app root, so no pane paints over it", async () => {
    renderInbox();
    const popup = await screen.findByTestId("inbox-popover");
    expect(popup).toHaveAttribute("aria-modal", "true");
    expect(popup.parentElement?.parentElement).toBe(document.body);
  });
});
