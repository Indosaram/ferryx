import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => {
  const captured: {
    event: string | null;
    handler: ((event: { payload: unknown }) => void) | null;
  } = { event: null, handler: null };
  return {
    captured,
    invoke: vi.fn(),
    listen: vi.fn(async (_event: string, handler: (payload: { payload: unknown }) => void): Promise<() => void> => {
      captured.event = _event;
      captured.handler = handler;
      return () => undefined;
    }),
  };
});

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

import {
  BROWSER_MEDIA_STATE_EVENT,
  browserMediaStore,
  controlBrowserMedia,
  createBrowserMediaStore,
  onBrowserMediaState,
  useActiveBrowserMedia,
  useBrowserMediaStates,
  type BrowserMediaState,
} from "./browserMedia";

function mediaState(overrides: Partial<BrowserMediaState> = {}): BrowserMediaState {
  return {
    browserId: "b1",
    title: "Song",
    artist: "Artist",
    album: "Album",
    artworkUrl: "https://cdn.example.com/a.jpg",
    playing: true,
    muted: false,
    hasMedia: true,
    ...overrides,
  };
}

const emit = (payload: BrowserMediaState) => {
  act(() => {
    tauri.captured.handler?.({ payload });
  });
};

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.listen.mockClear();
  tauri.captured.event = null;
  tauri.captured.handler = null;
  browserMediaStore.reset();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("onBrowserMediaState", () => {
  it("subscribes to browser_media_state and forwards the payload", async () => {
    const received: BrowserMediaState[] = [];
    const unlisten = await onBrowserMediaState((payload) => received.push(payload));
    expect(tauri.captured.event).toBe(BROWSER_MEDIA_STATE_EVENT);
    const payload = mediaState();
    tauri.captured.handler?.({ payload });
    expect(received).toEqual([payload]);
    expect(typeof unlisten).toBe("function");
  });
});

describe("controlBrowserMedia", () => {
  it("invokes the host command with camelCase arguments", async () => {
    tauri.invoke.mockResolvedValue({ action: "toggle", applied: true, playing: false });
    const result = await controlBrowserMedia("b1", "toggle");
    expect(tauri.invoke).toHaveBeenCalledWith("cmd_browser_media_control", {
      browserId: "b1",
      action: "toggle",
    });
    expect(result.applied).toBe(true);
  });

  it("surfaces a host error without swallowing it", async () => {
    tauri.invoke.mockRejectedValue({ code: "INVALID_REQUEST", message: "unknown media action" });
    await expect(controlBrowserMedia("b1", "toggle")).rejects.toMatchObject({
      code: "INVALID_REQUEST",
    });
  });
});

describe("browser media store", () => {
  it("keeps one entry per tab in report order and drops a tab without media", () => {
    const store = createBrowserMediaStore();
    store.apply(mediaState({ browserId: "a", title: "A" }));
    store.apply(mediaState({ browserId: "b", title: "B" }));
    expect([...store.getState().keys()]).toEqual(["a", "b"]);
    // A repeat report moves the tab to the end instead of duplicating it.
    store.apply(mediaState({ browserId: "a", title: "A" }));
    expect([...store.getState().keys()]).toEqual(["b", "a"]);
    store.apply(mediaState({ browserId: "a", hasMedia: false, playing: false, title: "" }));
    expect([...store.getState().keys()]).toEqual(["b"]);
  });

  it("notifies subscribers only for an actual change", () => {
    const store = createBrowserMediaStore();
    const listener = vi.fn();
    store.subscribe(listener);
    store.apply(mediaState({ browserId: "a" }));
    expect(listener).toHaveBeenCalledTimes(1);
    store.apply(mediaState({ browserId: "a", hasMedia: false }));
    expect(listener).toHaveBeenCalledTimes(2);
    // Already absent: nothing to tell.
    store.apply(mediaState({ browserId: "a", hasMedia: false }));
    expect(listener).toHaveBeenCalledTimes(2);
    store.forget("missing");
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

describe("useBrowserMediaStates", () => {
  it("returns the reports the host pushes", async () => {
    const { result } = renderHook(() => useBrowserMediaStates());
    await act(async () => {});
    expect(result.current.size).toBe(0);
    emit(mediaState({ browserId: "b1" }));
    expect(result.current.get("b1")?.title).toBe("Song");
    emit(mediaState({ browserId: "b2", title: "Other" }));
    expect([...result.current.keys()]).toEqual(["b1", "b2"]);
  });

  it("removes a tab once the host reports it has no media", async () => {
    const { result } = renderHook(() => useBrowserMediaStates());
    await act(async () => {});
    emit(mediaState({ browserId: "b1" }));
    expect(result.current.has("b1")).toBe(true);
    emit(mediaState({ browserId: "b1", hasMedia: false, playing: false }));
    expect(result.current.has("b1")).toBe(false);
  });

  it("unsubscribes on unmount", async () => {
    const stop = vi.fn();
    tauri.listen.mockImplementationOnce(async () => stop);
    const { unmount } = renderHook(() => useBrowserMediaStates());
    await act(async () => {});
    unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("stops a listener that resolves after unmount instead of leaving it registered", async () => {
    let settle: ((stop: () => void) => void) | undefined;
    tauri.listen.mockImplementationOnce(
      () => new Promise<() => void>((resolve) => { settle = resolve; }),
    );
    const { unmount } = renderHook(() => useBrowserMediaStates());
    unmount();
    const stop = vi.fn();
    settle?.(stop);
    await act(async () => {});
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

describe("useActiveBrowserMedia", () => {
  it("returns the most recently reported tab with media", async () => {
    const { result } = renderHook(() => useActiveBrowserMedia());
    await act(async () => {});
    expect(result.current).toBeNull();
    emit(mediaState({ browserId: "b1", title: "First" }));
    emit(mediaState({ browserId: "b2", title: "Second" }));
    expect(result.current?.browserId).toBe("b2");
    emit(mediaState({ browserId: "b2", hasMedia: false, playing: false }));
    expect(result.current?.browserId).toBe("b1");
  });
});
