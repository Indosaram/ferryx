import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserLibraryPanel } from "./BrowserLibraryPanel";

type Entry = { url: string; title: string; visitCount: number; lastVisitedAt: number; frecency: number };

const host = vi.hoisted(() => ({
  pending: [] as Array<(rows: unknown[]) => void>,
  changed: null as null | ((event: { payload: { kind: string } }) => void),
  subscribe: null as null | (() => void),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) => {
    if (command === "cmd_browser_history_list") {
      return new Promise((resolve) => host.pending.push(resolve));
    }
    return Promise.resolve(undefined);
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: (event: { payload: { kind: string } }) => void) => {
    host.changed = handler;
    return new Promise<() => void>((resolve) => {
      host.subscribe = () => resolve(() => undefined);
    });
  }),
}));

const entry: Entry = { url: "https://example.com/", title: "Example Domain", visitCount: 1, lastVisitedAt: 5_000, frecency: 100 };
const props = { view: "history" as const, onViewChange: vi.fn(), onOpen: vi.fn(), onClose: vi.fn() };

beforeEach(() => {
  host.pending = [];
  host.changed = null;
  host.subscribe = null;
});
afterEach(cleanup);

async function flush() {
  await act(async () => undefined);
}

describe("BrowserLibraryPanel history first open", () => {
  it("never lets an older empty answer overwrite a newer list", async () => {
    render(<BrowserLibraryPanel {...props} />);
    await flush();
    // Unknown is not empty: nothing is claimed before the store answered.
    expect(screen.queryByText("No browsing history.")).toBeNull();

    await act(async () => host.subscribe?.());
    act(() => host.changed?.({ payload: { kind: "history" } }));
    await flush();
    expect(host.pending.length).toBeGreaterThanOrEqual(2);

    // The newest query answers first with the visit; the first-open query answers last, empty.
    const newest = host.pending[host.pending.length - 1];
    const older = host.pending.slice(0, -1);
    await act(async () => newest([entry]));
    for (const resolve of older.reverse()) {
      await act(async () => resolve([]));
    }

    expect(screen.getByText("Example Domain")).toBeInTheDocument();
    expect(screen.queryByText("No browsing history.")).toBeNull();
  });

  it("queries again once subscribed, so a visit recorded before the subscription shows", async () => {
    render(<BrowserLibraryPanel {...props} />);
    await flush();
    expect(host.pending).toHaveLength(1);
    // The first query answers before the visit lands; its change event reaches no listener.
    await act(async () => host.pending[0]([]));

    await act(async () => host.subscribe?.());
    await flush();
    expect(host.pending).toHaveLength(2);
    await act(async () => host.pending[1]([entry]));

    expect(screen.getByText("Example Domain")).toBeInTheDocument();
    expect(screen.queryByText("No browsing history.")).toBeNull();
  });

  it("says the history is empty once the store answered empty", async () => {
    render(<BrowserLibraryPanel {...props} />);
    await act(async () => host.subscribe?.());
    await flush();
    for (const resolve of host.pending) {
      await act(async () => resolve([]));
    }
    expect(screen.getByText("No browsing history.")).toBeInTheDocument();
  });
});
