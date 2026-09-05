import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab } from "../lib/types";
import { BROWSER_SHORTCUT_EVENT, type BrowserFindResult } from "../lib/browserTauri";
import { BrowserPane } from "./BrowserPane";

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

const findState = vi.hoisted(() => ({
  deferreds: [] as Array<{ promise: Promise<BrowserFindResult>; resolve: (value: BrowserFindResult) => void }>,
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(async () => () => undefined),
}));

const browserMocks = vi.hoisted(() => ({
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  clearBrowserFind: vi.fn(async () => undefined),
  downloadBrowserUrl: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: eventMocks.listen }));
vi.mock("./BrowserToolbar", () => ({ BrowserToolbar: () => <div data-testid="browser-toolbar" /> }));
vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return {
    ...actual,
    ...browserMocks,
    findBrowser: vi.fn(() => {
      const d = deferred<BrowserFindResult>();
      findState.deferreds.push(d);
      return d.promise;
    }),
  };
});

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "http://localhost:3000",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

afterEach(cleanup);

beforeEach(() => {
  findState.deferreds = [];
  window.localStorage.clear();
});

describe("BrowserPane find response ordering", () => {
  it("ignores a stale find response that resolves after a newer query", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    act(() => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { action: "find" } }));
    });
    const input = screen.getByLabelText("Find in page");

    await act(async () => {
      fireEvent.change(input, { target: { value: "a" } });
    });
    await act(async () => {
      fireEvent.change(input, { target: { value: "ab" } });
    });

    expect(findState.deferreds).toHaveLength(2);

    // Newest query resolves first with the correct count for "ab".
    await act(async () => {
      findState.deferreds[1].resolve({ matchCount: 2, found: true });
    });
    expect(screen.getByText("2 matches")).toBeInTheDocument();

    // Stale response for "a" resolves last and must NOT overwrite the current result.
    await act(async () => {
      findState.deferreds[0].resolve({ matchCount: 9, found: true });
    });
    expect(screen.getByText("2 matches")).toBeInTheDocument();
  });

  it("ignores a stale find response that resolves after the query is cleared", async () => {
    render(<BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);

    act(() => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { action: "find" } }));
    });
    const input = screen.getByLabelText("Find in page");

    await act(async () => {
      fireEvent.change(input, { target: { value: "a" } });
    });
    await act(async () => {
      fireEvent.change(input, { target: { value: "" } });
    });
    // Retype so the count span is rendered from findResult again.
    await act(async () => {
      fireEvent.change(input, { target: { value: "zzz" } });
    });
    expect(findState.deferreds).toHaveLength(2); // one for "a", one for "zzz"

    await act(async () => {
      findState.deferreds[1].resolve({ matchCount: 3, found: true });
    });
    expect(screen.getByText("3 matches")).toBeInTheDocument();

    await act(async () => {
      findState.deferreds[0].resolve({ matchCount: 42, found: true });
    });
    expect(screen.getByText("3 matches")).toBeInTheDocument();
  });

  it("ignores a find response for a browser this component no longer targets", async () => {
    const { rerender } = render(
      <BrowserPane tab={tab} onNavigate={() => undefined} onReload={() => undefined} />,
    );

    act(() => {
      fireEvent(window, new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { action: "find" } }));
    });
    const input = screen.getByLabelText("Find in page");

    await act(async () => {
      fireEvent.change(input, { target: { value: "a" } });
    });
    expect(findState.deferreds).toHaveLength(1);

    // The component is reused for a different browser before the find resolves.
    await act(async () => {
      rerender(
        <BrowserPane
          tab={{ ...tab, browserId: "browser-2" }}
          onNavigate={() => undefined}
          onReload={() => undefined}
        />,
      );
    });

    await act(async () => {
      findState.deferreds[0].resolve({ matchCount: 9, found: true });
    });
    // The stale browser-1 response must not populate the browser-2 bar.
    expect(screen.getByText("0 matches")).toBeInTheDocument();
  });
});
