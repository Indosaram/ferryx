import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useMemo, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn(() => true) }));
vi.mock("@tauri-apps/api/core", () => core);

import { AttentionInbox } from "./AttentionInbox";
import type { AttentionRow } from "./attentionModel";
import { desktopAttentionPrompts } from "./attentionPrompts";

const NOW = 1_000_000_000;

const waiting: AttentionRow = {
  id: "ws-1:pane-1",
  revision: 1,
  workspaceId: "ws-1",
  sessionId: "pane-1",
  state: "needs-you",
  who: "Codex",
  location: "ferryx / main",
  at: NOW - 60_000,
};

const MENU = {
  promptId: "prompt-1",
  screenRevision: "rev-7",
  question: "Pick a fruit",
  options: [
    // The daemon keys each option by its 0-based index on the card.
    { key: "0", label: "apple" },
    { key: "1", label: "banana" },
    { key: "2", label: "cherry" },
  ],
  kind: "choice",
  needsConfirmation: false,
};

function Harness({ initial = [waiting] }: { initial?: AttentionRow[] }) {
  const [rows, setRows] = useState(initial);
  const prompts = useMemo(
    () => desktopAttentionPrompts(
      (row) => `daemon-${row.sessionId}`,
      (row) => setRows((current) => current.filter((candidate) => candidate.id !== row.id)),
    ),
    [],
  );
  return <AttentionInbox rows={rows} onOpen={vi.fn()} prompts={prompts} now={NOW} />;
}

function answerCalls() {
  return core.invoke.mock.calls.filter(([command]) => command === "cmd_agent_prompt_answer");
}

beforeEach(() => {
  core.invoke.mockReset();
  core.isTauri.mockReturnValue(true);
});
afterEach(cleanup);

describe("AttentionInbox prompt answers", () => {
  it("answers a needs-you row's menu with the picked option and drops the row", async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") return MENU;
      if (command === "cmd_agent_prompt_answer") return { requestId: "r", target: {}, stage: "accepted" };
      throw new Error(`unexpected ${command}`);
    });
    render(<Harness />);

    const options = await screen.findAllByTestId("attention-prompt-option");
    expect(options).toHaveLength(3);
    expect(core.invoke).toHaveBeenCalledWith("cmd_agent_prompt_get", { sessionId: "daemon-pane-1" });

    await act(async () => {
      fireEvent.click(options[1]);
    });

    expect(answerCalls()).toEqual([[
      "cmd_agent_prompt_answer",
      { sessionId: "daemon-pane-1", promptId: "prompt-1", screenRevision: "rev-7", answer: { key: "1" } },
    ]]);
    await waitFor(() => expect(screen.queryAllByTestId("attention-row")).toHaveLength(0));
  });

  it("keeps the row and says the question changed when the answer is refused as stale", async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") return MENU;
      throw { code: "REQUEST_CONFLICT", message: "the screen moved", details: {} };
    });
    render(<Harness />);

    const options = await screen.findAllByTestId("attention-prompt-option");
    await act(async () => {
      fireEvent.click(options[1]);
    });

    expect(answerCalls()).toHaveLength(1);
    expect(await screen.findByTestId("attention-prompt-stale")).toBeInTheDocument();
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);
    expect(screen.queryAllByTestId("attention-prompt-option")).toHaveLength(0);
  });

  it("renders no answer buttons for a row whose pane holds no readable prompt", async () => {
    core.invoke.mockResolvedValue(null);
    render(<Harness />);

    await waitFor(() => expect(core.invoke).toHaveBeenCalledWith("cmd_agent_prompt_get", { sessionId: "daemon-pane-1" }));
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);
    expect(screen.queryByTestId("attention-prompt")).not.toBeInTheDocument();
    expect(screen.queryAllByTestId("attention-prompt-option")).toHaveLength(0);
  });

  it("asks for a confirm before an approval goes out", async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") {
        return { ...MENU, kind: "approval", needsConfirmation: true, options: [{ key: "0", label: "Yes" }, { key: "1", label: "No" }] };
      }
      return { requestId: "r", target: {}, stage: "accepted" };
    });
    render(<Harness />);

    const options = await screen.findAllByTestId("attention-prompt-option");
    expect(options).toHaveLength(2);
    fireEvent.click(options[0]);
    expect(answerCalls()).toHaveLength(0);

    const panel = screen.getByTestId("attention-prompt");
    await act(async () => {
      fireEvent.click(within(panel).getByRole("button", { name: "Confirm" }));
    });
    expect(answerCalls()[0]?.[1]).toMatchObject({ answer: { key: "0" } });
  });

  it("asks for a confirm before a menu pick the daemon marks as needing one", async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") return { ...MENU, needsConfirmation: true };
      return { requestId: "r", target: {}, stage: "accepted" };
    });
    render(<Harness />);

    const options = await screen.findAllByTestId("attention-prompt-option");
    expect(options).toHaveLength(3);
    fireEvent.click(options[1]);
    expect(answerCalls()).toHaveLength(0);
    expect(screen.getAllByTestId("attention-row")).toHaveLength(1);

    await act(async () => {
      fireEvent.click(within(screen.getByTestId("attention-prompt")).getByRole("button", { name: "Confirm" }));
    });
    expect(answerCalls()[0]?.[1]).toMatchObject({ promptId: "prompt-1", screenRevision: "rev-7", answer: { key: "1" } });
    await waitFor(() => expect(screen.queryAllByTestId("attention-row")).toHaveLength(0));
  });

  it("asks for a confirm when an older daemon does not say whether a pick needs one", async () => {
    const legacy: Record<string, unknown> = { ...MENU };
    delete legacy.needsConfirmation;
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") return legacy;
      return { requestId: "r", target: {}, stage: "accepted" };
    });
    render(<Harness />);

    fireEvent.click((await screen.findAllByTestId("attention-prompt-option"))[0]);
    expect(answerCalls()).toHaveLength(0);
    expect(within(screen.getByTestId("attention-prompt")).getByRole("button", { name: "Confirm" })).toBeInTheDocument();
  });

  it("sends free text for a prompt without options", async () => {
    core.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_agent_prompt_get") return { ...MENU, kind: "freeText", options: [] };
      return { requestId: "r", target: {}, stage: "accepted" };
    });
    render(<Harness />);

    const input = await screen.findByRole("textbox", { name: "Answer Codex" });
    fireEvent.change(input, { target: { value: "use bun" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    expect(answerCalls()[0]?.[1]).toMatchObject({ answer: { text: "use bun" } });
  });

  it("shows a finished row whose pane holds a daemon prompt as needing you, with its answers", async () => {
    core.invoke.mockImplementation(async (command: string) => (command === "cmd_agent_prompt_get" ? MENU : null));
    render(<Harness initial={[{ ...waiting, state: "done" }]} />);

    expect(await screen.findAllByTestId("attention-prompt-option")).toHaveLength(3);
    expect(screen.getByTestId("attention-row")).toHaveAttribute("data-attention-state", "needs-you");
  });

  it("keeps a finished row done when the daemon reads no prompt", async () => {
    core.invoke.mockResolvedValue(null);
    render(<Harness initial={[{ ...waiting, state: "done" }]} />);
    await waitFor(() => expect(core.invoke).toHaveBeenCalledWith("cmd_agent_prompt_get", expect.anything()));
    await act(async () => {});
    expect(screen.getByTestId("attention-row")).toHaveAttribute("data-attention-state", "done");
    expect(screen.queryByTestId("attention-prompt")).not.toBeInTheDocument();
  });
});
