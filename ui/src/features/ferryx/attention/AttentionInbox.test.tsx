import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AttentionInbox } from "./AttentionInbox";
import type { AttentionRow } from "./attentionModel";

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

beforeEach(() => {
  // jsdom has no intersection engine; keep the real mascot mounted but offscreen.
  vi.stubGlobal("IntersectionObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

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

  it("says nobody is waiting when empty, leaving the session count to the sidebar footer", () => {
    render(<AttentionInbox rows={[]} onOpen={vi.fn()} now={NOW} className="inbox-empty-fixture" />);
    const empty = screen.getByTestId("attention-inbox-empty");
    expect(empty).toHaveTextContent("Nobody is waiting on you.");
    expect(empty).not.toHaveTextContent(/\d+ (open )?sessions?/);
    expect(within(empty).queryByText("✧")).not.toBeInTheDocument();
    expect(within(empty).getByTestId("attention-mascot-stage")).toBeInTheDocument();
    expect(within(empty).getByRole("button", { name: "Pause mascot animation" })).toBeInTheDocument();
    expect(empty).toHaveClass("inbox-empty-fixture");
  });

  it("replaces the mascot immediately on incoming rows and restores it without hook errors under StrictMode", () => {
    const consoleError = vi.spyOn(console, "error");
    const onOpen = vi.fn();
    const { rerender } = render(
      <StrictMode><AttentionInbox rows={[]} onOpen={onOpen} now={NOW} /></StrictMode>,
    );
    const initialMascot = screen.getByTestId("attention-mascot-stage");
    expect(screen.getByTestId("attention-inbox-empty")).toBeInTheDocument();

    expect(() => rerender(
      <StrictMode><AttentionInbox rows={[waiting]} onOpen={onOpen} now={NOW} /></StrictMode>,
    )).not.toThrow();
    expect(initialMascot).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-mascot-stage")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-inbox-empty")).not.toBeInTheDocument();
    expect(screen.getByTestId("attention-inbox")).toBeInTheDocument();
    expect(screen.getByTestId("attention-row")).toBeInTheDocument();

    expect(() => rerender(
      <StrictMode><AttentionInbox rows={[]} onOpen={onOpen} now={NOW} /></StrictMode>,
    )).not.toThrow();
    expect(screen.queryByTestId("attention-row")).not.toBeInTheDocument();
    expect(screen.getByTestId("attention-inbox-empty")).toBeInTheDocument();
    expect(screen.getByTestId("attention-mascot-stage")).not.toBe(initialMascot);
    expect(screen.getByRole("button", { name: "Pause mascot animation" })).toBeInTheDocument();
    expect(consoleError).not.toHaveBeenCalled();
  });
});
