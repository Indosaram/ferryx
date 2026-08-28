import { cleanup, render, screen } from "@testing-library/react";
import { DndContext } from "@dnd-kit/core";
import { SortableContext, horizontalListSortingStrategy } from "@dnd-kit/sortable";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../../lib/activity";
import type { WorkspaceTab } from "../../lib/types";
import { SortableTab } from "./SortableTab";

afterEach(() => {
  cleanup();
});

function makeActivity(overrides: Partial<ActivitySummary> = {}): ActivitySummary {
  return {
    workingCount: 0,
    waitingCount: 0,
    doneCount: 0,
    runningCount: 0,
    hasWorking: false,
    hasWaiting: false,
    hasDone: false,
    hasUnread: false,
    ...overrides,
  };
}

function renderSortableTab({
  tab = { id: "tab-1", label: "main", sessionId: "session-1" } as WorkspaceTab,
  groupId = "group-1",
  index = 0,
  active = true,
  unread = false,
  activity,
}: {
  tab?: WorkspaceTab;
  groupId?: string;
  index?: number;
  active?: boolean;
  unread?: boolean;
  activity?: ActivitySummary;
} = {}) {
  return render(
    <DndContext>
      <SortableContext items={[`tab:${tab.id}`]} strategy={horizontalListSortingStrategy}>
        <SortableTab
          tab={tab}
          groupId={groupId}
          index={index}
          active={active}
          unread={unread}
          activity={activity}
          isRenaming={false}
          renameValue=""
          onRenameValueChange={vi.fn()}
          onCommitRename={vi.fn()}
          onCancelRename={vi.fn()}
          onActivate={vi.fn()}
          onClose={vi.fn()}
          onContextMenu={vi.fn()}
        />
      </SortableContext>
    </DndContext>,
  );
}

describe("SortableTab agent icon rendering", () => {
  it("renders [data-testid='tab-agent-icon'][data-agent-type='codex'] with size-4 and the working status dot when agentType is codex and working", () => {
    const act = makeActivity({
      agentType: "codex",
      workingCount: 1,
      runningCount: 1,
      hasWorking: true,
    });

    renderSortableTab({ activity: act });

    const agentIcon = screen.getByTestId("tab-agent-icon");
    expect(agentIcon).toBeInTheDocument();
    expect(agentIcon).toHaveAttribute("data-agent-type", "codex");
    expect(agentIcon.className).toMatch(/\bsize-4\b/);
    // Codex ships a full-color brand mark, so it must not be inverted.
    expect(agentIcon.className).not.toMatch(/agent-tab-logo--monochrome/);

    const workingIndicator = screen.getByTestId("tab-working-indicator");
    expect(workingIndicator).toBeInTheDocument();
    expect(workingIndicator.querySelector('[data-status-state="working"]')).toBeInTheDocument();
  });

  it("renders [data-testid='tab-agent-icon'][data-agent-type='omo'] without invert filter for OMO light badge", () => {
    const act = makeActivity({
      agentType: "omo",
      workingCount: 1,
      runningCount: 1,
      hasWorking: true,
    });

    renderSortableTab({ activity: act });

    const agentIcon = screen.getByTestId("tab-agent-icon");
    expect(agentIcon).toBeInTheDocument();
    expect(agentIcon).toHaveAttribute("data-agent-type", "omo");
    expect(agentIcon.className).not.toMatch(/\binvert\b/);
  });

  it("renders a terminal fallback icon for an unsupported agentType with size-4", () => {
    const act = makeActivity({
      agentType: "not-a-known-agent",
    });

    renderSortableTab({ activity: act });

    expect(screen.queryByTestId("tab-agent-icon")).not.toBeInTheDocument();
    const fallbackIcon = screen.getByTestId("tab-terminal-icon");
    expect(fallbackIcon).toBeInTheDocument();
    expect(fallbackIcon.getAttribute("class")).toMatch(/\bsize-4\b/);
  });

  it("never renders the Codex logo for non-Codex or unknown agent identities", () => {
    const nonCodexTypes = ["generic", "terminal", "devin", "not-codex", "unknown"];
    for (const nonCodexType of nonCodexTypes) {
      const { unmount } = renderSortableTab({
        activity: makeActivity({
          agentType: nonCodexType,
          workingCount: 1,
          runningCount: 1,
          hasWorking: true,
        }),
      });

      expect(screen.queryByTestId("tab-agent-icon")).not.toBeInTheDocument();
      expect(document.querySelector('[data-agent-type="codex"]')).toBeNull();
      expect(screen.getByTestId("tab-terminal-icon")).toBeInTheDocument();
      unmount();
    }
  });

  it("renders a terminal icon when activity summary has no agentType", () => {
    const act = makeActivity({
      workingCount: 1,
      runningCount: 1,
      hasWorking: true,
    });

    renderSortableTab({ activity: act });

    expect(screen.queryByTestId("tab-agent-icon")).not.toBeInTheDocument();
    expect(screen.getByTestId("tab-terminal-icon")).toBeInTheDocument();
    expect(screen.getByTestId("tab-working-indicator")).toBeInTheDocument();
  });
});

describe("working indicator placement alongside an agent icon", () => {
  it("renders the spinner as a sibling of the agent icon, not nested inside it", () => {
    renderSortableTab({
      activity: makeActivity({ workingCount: 1, runningCount: 1, hasWorking: true, agentType: "codex" }),
    });

    const icon = screen.getByTestId("tab-agent-icon");
    const indicator = screen.getByTestId("tab-working-indicator");

    expect(icon.contains(indicator)).toBe(false);
    expect(indicator.parentElement).toBe(icon.parentElement);
    expect(indicator.className).not.toMatch(/absolute/);
    expect(screen.getByTestId("tab-working-indicator").querySelector('[data-status-state="working"]')).not.toBeNull();
  });
});
