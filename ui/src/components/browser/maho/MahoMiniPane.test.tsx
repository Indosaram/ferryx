// Transient Mini Pane contracts, driven through the real store.
//
// `BrowserPane` is replaced by a probe: the point of these cases is the slot's lifecycle
// (replace, promote, close), not the native page surface, which the pane's own suite covers.
// Everything else - `browserStore`, Space fallback, the key handling - is the shipped code.

import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../BrowserPane", () => ({
  BrowserPane: ({ tab, chromeMode, visible }: { tab: { browserId: string }; chromeMode?: string; visible?: boolean }) => (
    <div
      data-testid="browser-pane-probe"
      data-browser-id={tab.browserId}
      data-chrome={chromeMode}
      data-visible={String(visible)}
    />
  ),
}));

vi.mock("../../../lib/browserTauri", () => ({
  navigateBrowser: vi.fn(() => Promise.resolve()),
  reloadBrowser: vi.fn(() => Promise.resolve()),
  setBrowserVisible: vi.fn(() => Promise.resolve()),
  setBrowserBounds: vi.fn(() => Promise.resolve()),
}));

import {
  DEFAULT_SPACE_ID,
  createBrowserStore,
  selectTodayTabs,
  type BrowserStore,
} from "../../../state/browserStore";
import { browserViewportCoordinator } from "../../../lib/browserViewportCoordinator";
import { setBrowserVisible } from "../../../lib/browserTauri";
import { MahoMiniPane } from "./MahoMiniPane";

function makeStore(): BrowserStore {
  return createBrowserStore({ storage: null, now: () => 1_700_000_000_000 });
}

function seed(store: BrowserStore, browserId: string, url = `https://${browserId}.test/`): void {
  expect(store.upsertTab({ browserId, url, title: browserId }).ok).toBe(true);
}

function paneProps(store: BrowserStore) {
  return {
    store,
    onDestroyMiniSession: vi.fn(),
    onPromoted: vi.fn(),
  };
}

describe("MahoMiniPane", () => {
  beforeEach(() => {
    window.localStorage.clear();
    browserViewportCoordinator.reset();
  });

  it("renders the live mini browser through BrowserPane with minimal chrome", () => {
    const store = makeStore();
    seed(store, "mini-1", "https://example.com/docs");
    expect(store.setMini("mini-1").ok).toBe(true);

    render(<MahoMiniPane store={store} onDestroyMiniSession={vi.fn()} />);

    const probe = screen.getByTestId("browser-pane-probe");
    expect(probe).toHaveAttribute("data-browser-id", "mini-1");
    expect(probe).toHaveAttribute("data-chrome", "maho-minimal");
    expect(probe).toHaveAttribute("data-visible", "true");
    // The address field is centered on the bare host, Maho-style.
    expect(screen.getByRole("textbox", { name: "Address" })).toHaveValue("example.com");
    expect(screen.getByTestId("maho-mini-space-chip")).toHaveTextContent("Personal");
  });

  it("replaces a single slot and destroys the previous transient session", () => {
    const store = makeStore();
    seed(store, "mini-1");
    seed(store, "mini-2");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} />);
    expect(screen.getByTestId("browser-pane-probe")).toHaveAttribute("data-browser-id", "mini-1");

    act(() => {
      expect(store.setMini("mini-2").ok).toBe(true);
    });

    expect(screen.getByTestId("browser-pane-probe")).toHaveAttribute("data-browser-id", "mini-2");
    expect(props.onDestroyMiniSession).toHaveBeenCalledTimes(1);
    expect(props.onDestroyMiniSession).toHaveBeenCalledWith("mini-1");
  });

  it("promotes the same browserId into the target Space on Cmd/Ctrl+O without destroying it", () => {
    const store = makeStore();
    const work = store.createSpace({ name: "Work" });
    expect(work.ok).toBe(true);
    if (!work.ok) return;
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} targetSpaceId={work.value.id} />);
    fireEvent.keyDown(window, { key: "o", metaKey: true });

    expect(props.onPromoted).toHaveBeenCalledTimes(1);
    expect(props.onPromoted).toHaveBeenCalledWith("mini-1", work.value.id);
    expect(store.getState().miniBrowserId).toBeNull();
    // Same browserId, now a real tab in the target Space's Today - no reload, no re-create.
    expect(selectTodayTabs(store.getState(), work.value.id).map((tab) => tab.browserId)).toEqual(["mini-1"]);
    expect(props.onDestroyMiniSession).not.toHaveBeenCalled();
  });

  it("promotes from the Open in <Space> button too", () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Open in Personal" }));

    expect(props.onPromoted).toHaveBeenCalledWith("mini-1", DEFAULT_SPACE_ID);
    expect(props.onDestroyMiniSession).not.toHaveBeenCalled();
  });

  it("falls back to the active Space when the target Space was deleted", () => {
    const store = makeStore();
    const work = store.createSpace({ name: "Work" });
    expect(work.ok).toBe(true);
    if (!work.ok) return;
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} targetSpaceId={work.value.id} />);
    act(() => {
      expect(store.deleteSpace(work.value.id).ok).toBe(true);
    });

    const promoteButton = screen.getByRole("button", { name: "Open in Personal" });
    fireEvent.click(promoteButton);

    expect(props.onPromoted).toHaveBeenCalledWith("mini-1", DEFAULT_SPACE_ID);
    expect(selectTodayTabs(store.getState(), DEFAULT_SPACE_ID).map((tab) => tab.browserId)).toEqual(["mini-1"]);
    expect(store.getState().miniBrowserId).toBeNull();
  });

  it("closes on Esc, clearing the slot and terminating the transient session", () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} />);
    fireEvent.keyDown(window, { key: "Escape" });

    expect(store.getState().miniBrowserId).toBeNull();
    expect(props.onDestroyMiniSession).toHaveBeenCalledTimes(1);
    expect(props.onDestroyMiniSession).toHaveBeenCalledWith("mini-1");
    expect(props.onPromoted).not.toHaveBeenCalled();
    expect(screen.queryByTestId("browser-pane-probe")).toBeNull();
  });

  it("closes on the × button exactly once", () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);
    const onClose = vi.fn();

    render(<MahoMiniPane {...props} onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: "Close mini" }));

    expect(store.getState().miniBrowserId).toBeNull();
    expect(props.onDestroyMiniSession).toHaveBeenCalledTimes(1);
    expect(props.onDestroyMiniSession).toHaveBeenCalledWith("mini-1");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("ignores the promote shortcut while the host hides it", () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);
    const props = paneProps(store);

    render(<MahoMiniPane {...props} visible={false} />);
    fireEvent.keyDown(window, { key: "o", metaKey: true });

    expect(props.onPromoted).not.toHaveBeenCalled();
    expect(store.getState().miniBrowserId).toBe("mini-1");
  });

  it("never claims or hides the native surface of a live pane while the host hides it", async () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);

    // A live worktree split (or Browser-mode pane) already owns the one native surface.
    const liveOwner = browserViewportCoordinator.claim("mini-1", "worktree-split-surface", 0);
    expect(browserViewportCoordinator.getOwner("mini-1")).toEqual({
      surfaceId: "worktree-split-surface",
      priority: 0,
    });

    render(<MahoMiniPane store={store} visible={false} onDestroyMiniSession={vi.fn()} />);
    await act(async () => {});

    // No BrowserPane mounts while hidden, so nothing claims the surface and nothing hides it.
    expect(screen.queryByTestId("browser-pane-probe")).toBeNull();
    expect(screen.getByTestId("maho-mini-pane")).toHaveAttribute("data-visible", "false");
    expect(browserViewportCoordinator.getOwner("mini-1")).toEqual({
      surfaceId: "worktree-split-surface",
      priority: 0,
    });
    expect(liveOwner.isOwner()).toBe(true);
    expect(vi.mocked(setBrowserVisible)).not.toHaveBeenCalledWith("mini-1", false);
    expect(vi.mocked(setBrowserVisible)).not.toHaveBeenCalledWith("mini-1", true);
    // The transient session itself survives: only the surface is untouched.
    expect(store.getState().miniBrowserId).toBe("mini-1");

    liveOwner.release();
  });

  it("claims the surface once the host shows it again", async () => {
    const store = makeStore();
    seed(store, "mini-1");
    expect(store.setMini("mini-1").ok).toBe(true);

    const { rerender } = render(<MahoMiniPane store={store} visible={false} onDestroyMiniSession={vi.fn()} />);
    expect(screen.queryByTestId("browser-pane-probe")).toBeNull();

    rerender(<MahoMiniPane store={store} visible onDestroyMiniSession={vi.fn()} />);
    expect(screen.getByTestId("browser-pane-probe")).toHaveAttribute("data-browser-id", "mini-1");
  });

  it("compacts without destroying the session and expands the same browser", () => {
    const store = makeStore();
    seed(store, "mini-1");
    store.setMini("mini-1");
    const props = paneProps(store);
    render(<MahoMiniPane {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Compact Mini window" }));
    expect(screen.getByTestId("maho-mini-pane")).toHaveAttribute("data-compact", "true");
    expect(screen.queryByTestId("browser-pane-probe")).toBeNull();
    expect(props.onDestroyMiniSession).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Expand Mini window" }));
    expect(screen.getByTestId("browser-pane-probe")).toHaveAttribute("data-browser-id", "mini-1");
  });

  it("copies the full URL rather than the displayed host", () => {
    const store = makeStore();
    seed(store, "mini-1", "https://example.com/docs");
    store.setMini("mini-1");
    const onCopyUrl = vi.fn();
    render(<MahoMiniPane {...paneProps(store)} onCopyUrl={onCopyUrl} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    expect(onCopyUrl).toHaveBeenCalledWith("https://example.com/docs");
  });

  it("renders nothing without a mini slot", () => {
    const store = makeStore();
    render(<MahoMiniPane store={store} onDestroyMiniSession={vi.fn()} />);
    expect(screen.queryByTestId("maho-mini-pane")).toBeNull();
  });
});
