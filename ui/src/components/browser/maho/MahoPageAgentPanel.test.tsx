import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { UnifiedSupervisorClient } from "../../../lib/unifiedSupervisor";
import type { BrowserStoreTab } from "../../../state/browserStore";
import type { BrowserAgentActivity } from "./MahoAgentControl";
import { buildBrowserPageAgentInstruction, MahoPageAgentPanel } from "./MahoPageAgentPanel";

afterEach(() => {
  cleanup();
});

const sampleTab: BrowserStoreTab = {
  browserId: "tab-pr-1",
  spaceId: "space-1",
  url: "https://github.com/Indosaram/ferryx/pull/51",
  title: "Fix claim lease release",
  section: "today",
  order: 0,
  lastActiveAt: 1000,
};

function makeActivity(overrides: Partial<BrowserAgentActivity> = {}): BrowserAgentActivity {
  return {
    activeTasks: [],
    pendingApprovals: [],
    controlledBrowserIds: [],
    handoffRequests: [],
    badgeCount: 0,
    errorCode: null,
    busy: false,
    controllers: [],
    controllerLabelFor: () => null,
    controllerIdFor: () => null,
    taskStatusFor: () => "running",
    planeFor: () => "cli",
    receiptFor: () => null,
    stateFor: () => "disconnected",
    takeOver: vi.fn(async () => {}),
    doneHandoff: vi.fn(async () => {}),
    respondToHelp: vi.fn(async () => {}),
    revokeController: vi.fn(async () => {}),
    resolveApproval: vi.fn(async () => {}),
    ...overrides,
  };
}

function makeClient(response: Awaited<ReturnType<UnifiedSupervisorClient["request"]>>): UnifiedSupervisorClient {
  return {
    request: vi.fn(async () => response),
    conversation: vi.fn(async () => ({ kind: "ok" as const, value: { messages: [], next: 0 } })),
    memory: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
    remember: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
    refresh: vi.fn(),
  };
}

describe("MahoPageAgentPanel", () => {
  it("embeds the active tab browserId, url, and title into the agent instruction", () => {
    const instruction = buildBrowserPageAgentInstruction(sampleTab, "Click Merge");
    expect(instruction).toContain("browserId=tab-pr-1");
    expect(instruction).toContain("url=https://github.com/Indosaram/ferryx/pull/51");
    expect(instruction).toContain("title=Fix claim lease release");
    expect(instruction).toContain("Click Merge");
  });

  it("submits a prompt to the unified supervisor and renders the admitted receipt", async () => {
    const client = makeClient({
      kind: "ok",
      value: {
        outcome: "admitted",
        revision: 1,
        runState: "running",
        acknowledgment: "Working on PR #51.",
        duplicate: false,
        receipt: { goal_id: "goal-42" },
      },
    });
    render(
      <MahoPageAgentPanel
        tab={sampleTab}
        activity={makeActivity()}
        onClose={vi.fn()}
        client={client}
      />,
    );

    const textarea = screen.getByLabelText("Ask agent on this page");
    fireEvent.change(textarea, { target: { value: "Summarize the failing checks" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to page agent" }));

    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(1));
    const envelope = (client.request as ReturnType<typeof vi.fn>).mock.calls[0][0] as {
      action: string;
      payload: { request: string };
    };
    expect(envelope.action).toBe("submit");
    expect(envelope.payload.request).toContain("browserId=tab-pr-1");
    expect(envelope.payload.request).toContain("Summarize the failing checks");
    expect(await screen.findByText("Working on PR #51.")).toBeInTheDocument();
    expect(client.refresh).toHaveBeenCalledTimes(1);
  });

  it("auto-submits a pendingPrompt from Command Bar Ask mode and surfaces structured errors", async () => {
    const client = makeClient({
      kind: "error",
      code: "SUPERVISOR_OFFLINE",
      message: "Daemon hub is stopped",
      status: 503,
    });
    const onHandled = vi.fn();
    render(
      <MahoPageAgentPanel
        tab={sampleTab}
        activity={makeActivity()}
        pendingPrompt={{ id: 7, text: "Fill the checkout form" }}
        onPendingPromptHandled={onHandled}
        onClose={vi.fn()}
        client={client}
      />,
    );

    await waitFor(() => expect(client.request).toHaveBeenCalledTimes(1));
    expect(onHandled).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("alert")).toHaveTextContent("SUPERVISOR_OFFLINE: Daemon hub is stopped");
  });

  it("shows a Stop button calling takeOver while an agent controls the page", () => {
    const takeOver = vi.fn(async () => {});
    render(
      <MahoPageAgentPanel
        tab={sampleTab}
        activity={makeActivity({
          controlledBrowserIds: ["tab-pr-1"],
          controllerLabelFor: () => "Claude",
          takeOver,
        })}
        onClose={vi.fn()}
        client={makeClient({ kind: "ok", value: {} })}
      />,
    );

    expect(screen.getByText("Claude is operating on this page")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(takeOver).toHaveBeenCalledWith("tab-pr-1");
  });
});
