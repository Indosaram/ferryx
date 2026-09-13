import { createElement } from "react";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorktreeRow } from "../components/WorktreeList";
import { openNativePopupMenu, type NativeMenuEntry } from "./nativeMenu";

const bridge = vi.hoisted(() => ({ listen: vi.fn(), invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke, isTauri: bridge.isTauri }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
type Handler = (event: { payload: { id: string } }) => void;
function popup() {
  const registered = deferred<void>();
  const registration = deferred<() => void>();
  const invoked = deferred<{ items: NativeMenuEntry[]; position: { x: number; y: number } }>();
  const completion = deferred<void>();
  const removed = deferred<void>();
  const unlisten = vi.fn(() => { listeners.delete(handler); removed.resolve(); });
  let handler: Handler;
  const plan = { registered, registration, invoked, completion, removed, unlisten,
    select: (id: string) => handler({ payload: { id } }) };
  plans.push(plan);
  bridge.listen.mockImplementationOnce((name: string, callback: Handler) => {
    expect(name).toBe("ferryx://menu-action");
    handler = callback;
    listeners.add(handler);
    registered.resolve();
    return registration.promise;
  });
  bridge.invoke.mockImplementationOnce((_command: string, args: { items: NativeMenuEntry[]; position: { x: number; y: number } }) => {
    invoked.resolve(args);
    return completion.promise;
  });
  return plan;
}
type Plan = ReturnType<typeof popup>;
let plans: Array<{
  registration: ReturnType<typeof deferred<() => void>>;
  completion: ReturnType<typeof deferred<void>>;
  unlisten: ReturnType<typeof vi.fn<() => void>>;
}>;
let listeners: Set<Handler>;
const entries: NativeMenuEntry[] = [{ kind: "item", id: "copy-path", label: "Copy" }];
const position = { x: 12, y: 34 };
function start(onAction = vi.fn(), signal?: AbortSignal, items = entries) {
  // The optional cancellation argument deliberately also exercises the original implementation.
  return openNativePopupMenu("cmd_native_sidebar_context_menu", items, position, onAction, signal);
}
function itemId(items: NativeMenuEntry[], original = "Copy") : string {
  for (const item of items) {
    if (item.kind === "item" && item.label === original) return item.id;
    if (item.kind === "submenu") return itemId(item.items, original);
  }
  throw new Error("Requested item absent");
}
function emit(id: string) {
  for (const listener of [...listeners]) listener({ payload: { id } });
}
async function ready(plan: Plan) {
  await plan.registered.promise;
  plan.registration.resolve(plan.unlisten);
  return plan.invoked.promise;
}
function row(path: string) {
  return render(createElement(WorktreeRow, {
    worktree: { path, head: "abc", branch: "refs/heads/orca/ws/feature", bare: false,
      detached: false, locked: null, prunable: null },
    active: false, agent: undefined, status: undefined, unread: false,
    activitySummary: undefined, onSelect: vi.fn(), onDelete: vi.fn(),
  }));
}
function context(view: ReturnType<typeof row>) {
  fireEvent.contextMenu(view.container.querySelector("button")!, { clientX: 12, clientY: 34 });
}

beforeEach(() => {
  vi.useFakeTimers();
  bridge.listen.mockReset(); bridge.invoke.mockReset(); bridge.isTauri.mockReturnValue(true);
  plans = []; listeners = new Set();
});
afterEach(async () => {
  cleanup();
  await act(async () => {
    for (const plan of plans) {
      plan.registration.resolve(plan.unlisten);
      plan.completion.resolve();
    }
  });
  vi.advanceTimersByTime(200);
  try {
    expect(listeners.size).toBe(0);
    for (const plan of plans) expect(plan.unlisten).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  } finally {
    vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals();
  }
});

describe("native popup ownership", () => {
  it("dismisses A with cleanup pending, then copies exactly B once through two actual rows", async () => {
    const copied = deferred<void>();
    const writeText = vi.fn(async (_text: string) => { copied.resolve(); });
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows", clipboard: { writeText } });
    try {
      const a = row("C:\\owned\\A");
      const b = row("C:\\owned\\B");
      const pa = popup(); context(a); await ready(pa);
      await act(async () => { pa.completion.resolve(); await pa.completion.promise; });
      expect(pa.unlisten).not.toHaveBeenCalled();
      const pb = popup(); context(b); const request = await ready(pb);
      expect(request.position).toEqual(position);
      await act(async () => { emit(itemId(request.items, "Copy Worktree Path")); await copied.promise; });
      expect(writeText.mock.calls).toEqual([["C:\\owned\\B"]]);
      emit(itemId(request.items, "Copy Worktree Path"));
      expect(writeText.mock.calls).toEqual([["C:\\owned\\B"]]);
    } finally { vi.unstubAllGlobals(); }
  });

  it.each(["registration", "invoke"] as const)("unmount cancels a real row during pending %s", async (phase) => {
    const view = row("C:\\owned\\A");
    const p = popup(); context(view); await p.registered.promise;
    const request = phase === "invoke" ? await ready(p) : null;
    view.unmount();
    await act(async () => { p.registration.resolve(p.unlisten); await p.registration.promise; });
    if (request) {
      expect(p.unlisten).toHaveBeenCalledTimes(1);
      const clipboard = vi.fn();
      vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows", clipboard: { writeText: clipboard } });
      try { p.select(itemId(request.items, "Copy Worktree Path")); expect(clipboard).not.toHaveBeenCalled(); }
      finally { vi.unstubAllGlobals(); }
    } else {
      expect(bridge.invoke).not.toHaveBeenCalled();
      expect(p.unlisten).toHaveBeenCalledTimes(1);
    }
  });

  it.each(["registration", "invoke"] as const)("reopen cancels the obsolete row popup during pending %s", async (phase) => {
    const view = row("C:\\owned\\A");
    const a = popup(); context(view); await a.registered.promise;
    if (phase === "invoke") await ready(a);
    // For canceled registration A must not invoke; reserve the next invoke gate for B.
    if (phase === "registration") bridge.invoke.mockReset();
    const b = popup(); context(view); const request = await ready(b);
    await act(async () => { a.registration.resolve(a.unlisten); a.completion.resolve(); });
    expect(a.unlisten).toHaveBeenCalledTimes(1);
    expect(b.unlisten).not.toHaveBeenCalled();
    const copied = deferred<void>();
    const writeText = vi.fn(async () => { copied.resolve(); });
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows", clipboard: { writeText } });
    try {
      await act(async () => { emit(itemId(request.items, "Copy Worktree Path")); await copied.promise; });
      expect(writeText).toHaveBeenCalledTimes(1);
    } finally { vi.unstubAllGlobals(); }
  });

  it("ignores unrelated actions and maps a submenu ID without mutating entries", async () => {
    const nested: NativeMenuEntry[] = [{ kind: "submenu", label: "Profiles", items: entries }];
    const p = popup(); const action = vi.fn(); const done = start(action, undefined, nested);
    const request = await ready(p);
    emit("unrelated");
    expect(action).not.toHaveBeenCalled(); expect(p.unlisten).not.toHaveBeenCalled();
    emit(itemId(request.items));
    expect(action.mock.calls).toEqual([["copy-path"]]);
    expect(nested).toEqual([{ kind: "submenu", label: "Profiles", items: entries }]);
    p.completion.resolve(); (await done)();
    expect(p.unlisten).toHaveBeenCalledTimes(1);
  });

  it("keeps a long-open popup selectable until native tracking completes", async () => {
    const p = popup(); const action = vi.fn(); const done = start(action);
    const request = await ready(p);
    vi.advanceTimersByTime(60_000);
    expect(p.unlisten).not.toHaveBeenCalled();
    emit(itemId(request.items)); expect(action.mock.calls).toEqual([["copy-path"]]);
    p.completion.resolve(); (await done)();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("dismissal removes the listener once at the unchanged grace deadline", async () => {
    const p = popup(); const action = vi.fn(); const done = start(action);
    await ready(p); p.completion.resolve(); const cancel = await done;
    vi.advanceTimersByTime(199); expect(p.unlisten).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1); await p.removed.promise;
    cancel(); cancel(); expect(action).not.toHaveBeenCalled(); expect(p.unlisten).toHaveBeenCalledTimes(1);
  });

  it("cleans up before propagating invoke rejection", async () => {
    const p = popup(); const done = start(); const rejected = expect(done).rejects.toThrow("invoke failed");
    await ready(p); p.completion.reject(new Error("invoke failed")); await rejected;
    expect(p.unlisten).toHaveBeenCalledTimes(1);
  });

  it("propagates registration rejection without invoking", async () => {
    const p = popup(); const done = start(); const rejected = expect(done).rejects.toThrow("listen failed");
    await p.registered.promise;
    // A rejected bridge registration owns no native subscription.
    listeners.clear(); plans = [];
    p.registration.reject(new Error("listen failed")); await rejected;
    expect(bridge.invoke).not.toHaveBeenCalled(); expect(p.unlisten).not.toHaveBeenCalled();
  });

  it("callback throw and queued duplicate still release exactly once", async () => {
    const p = popup(); const action = vi.fn(() => { throw new Error("action failed"); });
    const done = start(action); const request = await ready(p);
    expect(() => p.select(itemId(request.items))).toThrow("action failed");
    expect(() => p.select(itemId(request.items))).not.toThrow();
    expect(action).toHaveBeenCalledTimes(1); expect(p.unlisten).toHaveBeenCalledTimes(1);
    p.completion.resolve(); (await done)(); expect(vi.getTimerCount()).toBe(0);
  });

  it("supports cancellation before registration and a non-native no-op", async () => {
    const controller = new AbortController(); controller.abort();
    (await start(vi.fn(), controller.signal))(); expect(bridge.listen).not.toHaveBeenCalled();
    bridge.isTauri.mockReturnValue(false);
    (await start())(); expect(bridge.listen).not.toHaveBeenCalled(); expect(bridge.invoke).not.toHaveBeenCalled();
  });
});
