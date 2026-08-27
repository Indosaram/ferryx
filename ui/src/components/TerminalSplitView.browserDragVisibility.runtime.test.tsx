import type { ReactNode } from "react";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, LayoutState, TerminalSession } from "../lib/types";

const browserMocks = vi.hoisted(() => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

type DndHarnessProps = {
  onDragStart?: (event: { active: { data: { current: unknown } } }) => void;
  onDragEnd?: (event: { over: unknown | null }) => void;
  onDragCancel?: (event: { active: unknown }) => void;
};

const dndHarness = vi.hoisted(() => ({
  props: null as null | DndHarnessProps,
}));

vi.mock("@dnd-kit/core", () => ({
  DndContext: (props: {
    children: ReactNode;
    onDragStart?: (event: { active: { data: { current: unknown } } }) => void;
    onDragEnd?: (event: { over: unknown | null }) => void;
    onDragCancel?: (event: { active: unknown }) => void;
  }) => {
    dndHarness.props = props;
    return props.children;
  },
  DragOverlay: ({ children }: { children: ReactNode }) => children,
  KeyboardSensor: function KeyboardSensor() {},
  PointerSensor: function PointerSensor() {},
  pointerWithin: vi.fn(() => []),
  useDraggable: () => ({ setNodeRef: vi.fn(), attributes: {}, listeners: {}, isDragging: false }),
  useDroppable: () => ({ setNodeRef: vi.fn(), isOver: false }),
  useSensor: vi.fn(() => ({})),
  useSensors: vi.fn(() => []),
}));

vi.mock("@dnd-kit/sortable", () => ({
  SortableContext: ({ children }: { children: ReactNode }) => children,
  horizontalListSortingStrategy: {},
  sortableKeyboardCoordinates: vi.fn(),
  useSortable: () => ({
    setNodeRef: vi.fn(),
    attributes: {},
    listeners: {},
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(() => {
  cleanup();
  dndHarness.props = null;
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
});

beforeEach(() => {
  browserMocks.setBrowserBounds.mockClear();
  browserMocks.setBrowserVisible.mockClear();
});

function createBrowserLayout(browserId: string): LayoutState {
  const browserTab: BrowserTab = {
    kind: "browser",
    id: "tab-browser-active",
    label: "Active Browser Tab",
    browserId,
    url: "https://example.com",
    loading: false,
    canGoBack: false,
    canGoForward: false,
  };

  return {
    tabs: [browserTab],
    activeTabId: browserTab.id,
    tabGroups: {
      "group-default": {
        id: "group-default",
        tabIds: [browserTab.id],
        activeTabId: browserTab.id,
      },
    },
    tabGroupLayout: { type: "group", groupId: "group-default" },
    focusedGroupId: "group-default",
    layoutsByTabId: {
      [browserTab.id]: {
        root: { type: "leaf", leafId: "leaf-browser" },
        activeLeafId: "leaf-browser",
        expandedLeafId: null,
        sessionIdsByLeafId: {},
      },
    },
  };
}

describe("TerminalSplitView native browser webview drag visibility", () => {
  it("hides active native browser webview on workspace drag start and restores visibility on drag end", async () => {
    const browserId = "browser-instance-1";
    const layout = createBrowserLayout(browserId);

    render(
      <TerminalSplitView
        layout={layout}
        sessions={{}}
      />,
    );

    // Initial mount makes active browser visible
    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, true);
    });

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragEnd).toBeTypeOf("function");

    browserMocks.setBrowserVisible.mockClear();

    // When workspace drag starts, active native webview must be hidden to prevent swallowing pointer events
    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "tab",
            tabId: "tab-browser-active",
            groupId: "group-default",
            index: 0,
          },
        },
      },
    });

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, false);
    });

    browserMocks.setBrowserVisible.mockClear();

    // When workspace drag ends, active native webview visibility must be restored
    dndHarness.props!.onDragEnd!({
      over: null,
    });

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, true);
    });
  });

  it("hides active native browser webview on workspace drag start and restores visibility on drag cancel", async () => {
    const browserId = "browser-instance-2";
    const layout = createBrowserLayout(browserId);

    render(
      <TerminalSplitView
        layout={layout}
        sessions={{}}
      />,
    );

    // Initial mount makes active browser visible
    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, true);
    });

    expect(dndHarness.props?.onDragStart).toBeTypeOf("function");
    expect(dndHarness.props?.onDragCancel).toBeTypeOf("function");

    browserMocks.setBrowserVisible.mockClear();

    // When workspace drag starts, active native webview must be hidden
    dndHarness.props!.onDragStart!({
      active: {
        data: {
          current: {
            type: "tab",
            tabId: "tab-browser-active",
            groupId: "group-default",
            index: 0,
          },
        },
      },
    });

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, false);
    });

    browserMocks.setBrowserVisible.mockClear();

    // When workspace drag is cancelled, active native webview visibility must be restored
    dndHarness.props!.onDragCancel!({
      active: {
        id: "tab-browser-active",
      },
    });

    await waitFor(() => {
      expect(browserMocks.setBrowserVisible).toHaveBeenCalledWith(browserId, true);
    });
  });
});
