import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AttentionInbox } from "./AttentionInbox";
import { buildAttentionRows, type AttentionRow } from "./attentionModel";
import { notificationEntryId } from "../../../lib/notificationCenter/types";

const NOW = 1_000_000_000;

function row(overrides: Partial<AttentionRow> & Pick<AttentionRow, "id" | "state">): AttentionRow {
  return {
    revision: 1,
    workspaceId: "ws-1",
    sessionId: overrides.id,
    who: "Claude Code",
    location: "ferryx / main",
    at: NOW - 2 * 60_000,
    ...overrides,
  };
}

const waiting = row({ id: "w", state: "needs-you", who: "omo", text: "Auth method — Which library should we use?" });
const finished = row({ id: "d", state: "done", who: "Codex", location: "ferryx / feat-inbox", text: "Fix login bug" });

afterEach(cleanup);

describe("AttentionInbox", () => {
  it("shows the two states with who, where, and the actual question", () => {
    render(<AttentionInbox rows={[waiting, finished]} onOpen={vi.fn()} now={NOW} />);

    const needsYou = screen.getByRole("region", { name: "Needs input" });
    const done = screen.getByRole("region", { name: "Finished" });
    expect(within(needsYou).getByText("omo")).toBeInTheDocument();
    expect(within(needsYou).getByText("Waiting")).toBeInTheDocument();
    expect(within(needsYou).getByTestId("attention-row-text")).toHaveTextContent("Auth method — Which library should we use?");
    expect(within(done).getByText("Codex")).toBeInTheDocument();
    expect(within(done).getByText("ferryx / feat-inbox")).toBeInTheDocument();
    expect(within(done).getByText("Done")).toBeInTheDocument();
    expect(screen.getAllByText("2m ago")).toHaveLength(2);
  });

  it("puts requests above completions", () => {
    render(<AttentionInbox rows={[waiting, finished]} onOpen={vi.fn()} now={NOW} />);
    expect(screen.getAllByTestId("attention-row").map((el) => el.dataset.attentionState)).toEqual(["needs-you", "done"]);
  });

  it("opens the row's session when clicked", () => {
    const onOpen = vi.fn();
    render(<AttentionInbox rows={[waiting]} onOpen={onOpen} now={NOW} />);
    fireEvent.click(screen.getByTestId("attention-row"));
    expect(onOpen).toHaveBeenCalledWith(waiting);
  });

  it("labels a supervisor hand-off from the fixed map and focuses its pane on click", () => {
    const [stopped] = buildAttentionRows([{
      id: notificationEntryId("ws-1", "pane-1"), workspaceId: "ws-1", sessionId: "pane-1",
      labels: { agentLabel: "Ledger", workspaceLabel: "ferryx", worktreeLabel: "main" },
      subject: "agent", reason: "supervisor", firstOccurredAt: NOW, lastOccurredAt: NOW,
      updateOrder: 1, revision: 1, occurrenceCount: 1, read: { unread: true },
      supervisor: { laneKey: "ledger", stopReason: "delivery_uncertain", awaitingApproval: false, goalRevision: 2 },
    }]);
    const onOpen = vi.fn();
    render(<AttentionInbox rows={[stopped]} onOpen={onOpen} now={NOW} />);
    expect(screen.getByTestId("attention-row-supervisor")).toHaveTextContent("Instruction may not have arrived");
    expect(screen.queryByText("Waiting")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("attention-row"));
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: "ws-1", sessionId: "pane-1", revision: 1 }));
  });

  it("dismissing a row does not also open it", () => {
    const onOpen = vi.fn();
    const onDismiss = vi.fn();
    render(<AttentionInbox rows={[waiting]} onOpen={onOpen} onDismiss={onDismiss} now={NOW} />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss omo" }));
    expect(onDismiss).toHaveBeenCalledWith(waiting);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("removes a handled row from the list at once instead of leaving it disabled", () => {
    function Harness() {
      const [rows, setRows] = useState<AttentionRow[]>([waiting, finished]);
      const drop = (target: AttentionRow) => setRows((current) => current.filter((candidate) => candidate.id !== target.id));
      return <AttentionInbox rows={rows} onOpen={drop} onDismiss={drop} now={NOW} />;
    }
    render(<Harness />);

    fireEvent.click(screen.getAllByTestId("attention-row")[0]);
    expect(screen.queryByText("omo")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss Codex" }));
    expect(screen.queryAllByTestId("attention-row")).toHaveLength(0);
    expect(screen.getByTestId("attention-inbox-empty")).toBeInTheDocument();
    expect(document.querySelectorAll("button:disabled")).toHaveLength(0);
  });

  it("offers state filters only when both states are present", () => {
    const { rerender } = render(<AttentionInbox rows={[waiting, finished]} onOpen={vi.fn()} now={NOW} />);
    const filters = screen.getByRole("group", { name: "Status filter" });
    expect(within(filters).getAllByRole("button").map((button) => button.textContent)).toEqual(["All2", "Input1", "Done1"]);
    fireEvent.click(within(filters).getByRole("button", { name: /Input/ }));
    expect(screen.getAllByTestId("attention-row").map((el) => el.dataset.attentionState)).toEqual(["needs-you"]);
    fireEvent.click(within(filters).getByRole("button", { name: /All/ }));
    expect(screen.getAllByTestId("attention-row")).toHaveLength(2);

    rerender(<AttentionInbox rows={[finished]} onOpen={vi.fn()} now={NOW} />);
    expect(screen.queryByRole("group", { name: "Status filter" })).not.toBeInTheDocument();
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);
  });

  it("does not stay stuck on a filter whose rows are gone", () => {
    const { rerender } = render(<AttentionInbox rows={[waiting, finished]} onOpen={vi.fn()} now={NOW} />);
    fireEvent.click(within(screen.getByRole("group", { name: "Status filter" })).getByRole("button", { name: /Done/ }));
    rerender(<AttentionInbox rows={[waiting]} onOpen={vi.fn()} now={NOW} />);
    expect(screen.getAllByTestId("attention-row").map((el) => el.dataset.attentionState)).toEqual(["needs-you"]);
  });

  it("draws no coloured side stripe on a row", () => {
    render(<AttentionInbox rows={[waiting, finished]} onOpen={vi.fn()} now={NOW} />);
    for (const el of screen.getAllByTestId("attention-row")) expect(el.className).not.toMatch(/before:w-\[3px\]|before:bg-status/);
  });

  it("says nobody is waiting when empty, leaving the session count to the sidebar footer", () => {
    render(<AttentionInbox rows={[]} onOpen={vi.fn()} now={NOW} />);
    const empty = screen.getByTestId("attention-inbox-empty");
    expect(empty).toHaveTextContent("Nobody is waiting on you.");
    expect(empty).not.toHaveTextContent(/\d+ (open )?sessions?/);
  });

  it("labels a needs-you row by the program's blocked kind and keeps the plain label without one", () => {
    render(<AttentionInbox rows={[row({ id: "p", state: "needs-you", kind: "permission" }), waiting]} onOpen={vi.fn()} now={NOW} />);
    const needsYou = screen.getByRole("region", { name: "Needs input" });
    expect(within(needsYou).getByText("Permission")).toBeInTheDocument();
    expect(within(needsYou).getByText("Waiting")).toBeInTheDocument();
  });

  it("renders HTML in a program's detail as literal text", () => {
    const { container } = render(
      <AttentionInbox rows={[row({ id: "h", state: "needs-you", kind: "question", text: "<b>x</b>" })]} onOpen={vi.fn()} now={NOW} />,
    );
    expect(screen.getByTestId("attention-row-text").textContent).toBe("<b>x</b>");
    expect(container.querySelector("b")).toBeNull();
  });
});
