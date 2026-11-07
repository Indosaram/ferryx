import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

/** jsdom replaces the global URL class, so fs reads need a plain string path. */
const SW_SOURCE = readFileSync(
  resolve(dirname(fileURLToPath(import.meta.url)), "../../public/sw.js"),
  "utf8",
);

type Listener = (event: Record<string, unknown>) => void;

/** Runs the shipped sw.js in a bare worker-like global and captures its event listeners. */
function loadServiceWorker(windows: Array<{ url: string; focus: () => Promise<unknown>; postMessage: (m: unknown) => void }> = []) {
  const listeners = new Map<string, Listener>();
  const showNotification = vi.fn(async () => undefined);
  const openWindow = vi.fn(async () => null);
  const self = {
    addEventListener: (type: string, listener: Listener) => listeners.set(type, listener),
    registration: { showNotification },
    location: { origin: "https://phone.example.test" },
    clients: { matchAll: vi.fn(async () => windows), openWindow, claim: vi.fn() },
    skipWaiting: vi.fn(),
  };
  runInNewContext(SW_SOURCE, { self, URL, caches: {}, fetch: vi.fn(), Response, Promise, console });
  const dispatch = async (type: string, event: Record<string, unknown>) => {
    let pending: Promise<unknown> = Promise.resolve();
    listeners.get(type)?.({ ...event, waitUntil: (p: Promise<unknown>) => { pending = p; } });
    await pending;
  };
  return { listeners, showNotification, openWindow, dispatch };
}

function pushData(payload: unknown) {
  return { json: () => payload, text: () => JSON.stringify(payload) };
}

describe("service worker push handling", () => {
  it("maps the gateway payload to showNotification with the session id as tag", async () => {
    const sw = loadServiceWorker();
    await sw.dispatch("push", {
      data: pushData({
        agent_id: "sess-42",
        agent_name: "Claude Code",
        worktree: "feature-x",
        state: "waiting_input",
        title: "Needs input",
        body: "Claude Code in feature-x",
      }),
    });

    expect(sw.showNotification).toHaveBeenCalledTimes(1);
    expect(sw.showNotification).toHaveBeenCalledWith("Needs input", expect.objectContaining({
      body: "Claude Code in feature-x",
      tag: "sess-42",
      renotify: true,
      data: { sessionId: "sess-42", state: "waiting_input", url: "/#session=sess-42" },
    }));
  });

  it("collapses the test alert under one tag and survives a non-JSON payload", async () => {
    const sw = loadServiceWorker();
    await sw.dispatch("push", {
      data: pushData({ agent_id: "", agent_name: "Ferryx", state: "test", title: "Test alert", body: "Phone alerts are working." }),
    });
    expect(sw.showNotification).toHaveBeenLastCalledWith("Test alert", expect.objectContaining({ tag: "ferryx-test" }));

    await sw.dispatch("push", { data: { json: () => { throw new Error("not json"); }, text: () => "plain" } });
    expect(sw.showNotification).toHaveBeenLastCalledWith("Ferryx", expect.objectContaining({ body: "plain", tag: "ferryx-test" }));
  });

  it("focuses an open window and hands it the session on click", async () => {
    const focus = vi.fn(async () => undefined);
    const postMessage = vi.fn();
    const sw = loadServiceWorker([{ url: "https://phone.example.test/", focus, postMessage }]);
    const close = vi.fn();
    await sw.dispatch("notificationclick", { notification: { close, data: { sessionId: "sess-42" } } });

    expect(close).toHaveBeenCalled();
    expect(postMessage).toHaveBeenCalledWith({ type: "ferryx:open-session", sessionId: "sess-42" });
    expect(focus).toHaveBeenCalled();
    expect(sw.openWindow).not.toHaveBeenCalled();
  });

  it("opens the session URL when no window is open", async () => {
    const sw = loadServiceWorker([]);
    await sw.dispatch("notificationclick", { notification: { close: vi.fn(), data: { sessionId: "a b" } } });
    expect(sw.openWindow).toHaveBeenCalledWith("https://phone.example.test/#session=a%20b");
  });

  it("opens the decision card from the push deep link", async () => {
    const sw = loadServiceWorker([]);
    const url = "/?panel=decision&session=mac%3As1";
    await sw.dispatch("push", {
      data: pushData({ agent_id: "mac:s1", state: "decision", title: "Decision needed", body: "Claude Code", url }),
    });
    expect(sw.showNotification).toHaveBeenCalledWith("Decision needed", expect.objectContaining({
      tag: "mac:s1",
      data: { sessionId: "mac:s1", state: "decision", url },
    }));

    await sw.dispatch("notificationclick", { notification: { close: vi.fn(), data: { sessionId: "mac:s1", state: "decision", url } } });
    expect(sw.openWindow).toHaveBeenCalledWith("https://phone.example.test" + url);

    const postMessage = vi.fn();
    const open = loadServiceWorker([{ url: "https://phone.example.test/", focus: vi.fn(async () => undefined), postMessage }]);
    await open.dispatch("notificationclick", { notification: { close: vi.fn(), data: { sessionId: "mac:s1", state: "decision", url } } });
    expect(postMessage).toHaveBeenCalledWith({ type: "ferryx:open-url", url });
  });

  it("never follows a deep link off this origin and never turns a decision ref into a session route", async () => {
    for (const url of ["//evil.example/x", "/\\evil.example/x", "https://evil.example/", "/ok\n//evil.example", "panel=decision"]) {
      const sw = loadServiceWorker([]);
      await sw.dispatch("push", {
        data: pushData({ agent_id: "mac:s1", state: "decision", title: "t", body: "b", url }),
      });
      // The fallback is the panel itself: `#session=mac%3As1` would be read as a backend id.
      expect(sw.showNotification, url).toHaveBeenCalledWith("t", expect.objectContaining({
        data: { sessionId: "mac:s1", state: "decision", url: "/?panel=decision" },
      }));
    }
  });

  it("routes a decision tap to the same decision link whether or not a window is open", async () => {
    const url = "/?panel=decision&session=win%3Aa%2Fb";
    const data = { sessionId: "win:a/b", state: "decision", url };
    const cold = loadServiceWorker([]);
    await cold.dispatch("notificationclick", { notification: { close: vi.fn(), data } });
    expect(cold.openWindow).toHaveBeenCalledWith("https://phone.example.test" + url);

    const focus = vi.fn(async () => undefined);
    const postMessage = vi.fn();
    const warm = loadServiceWorker([
      { url: "https://other.example/", focus: vi.fn(), postMessage: vi.fn() },
      { url: "https://phone.example.test/", focus, postMessage },
    ]);
    await warm.dispatch("notificationclick", { notification: { close: vi.fn(), data } });
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(postMessage).toHaveBeenCalledWith({ type: "ferryx:open-url", url });
    expect(focus).toHaveBeenCalledTimes(1);
    expect(warm.openWindow).not.toHaveBeenCalled();

    // A decision tap whose stored link was lost still opens the panel, not a session route.
    const bare = loadServiceWorker([{ url: "https://phone.example.test/", focus: vi.fn(async () => undefined), postMessage }]);
    postMessage.mockClear();
    await bare.dispatch("notificationclick", { notification: { close: vi.fn(), data: { sessionId: "mac:s1", state: "decision" } } });
    expect(postMessage).toHaveBeenCalledWith({ type: "ferryx:open-url", url: "/?panel=decision" });
  });

  it("keeps the session tap for non-decision alerts", async () => {
    const postMessage = vi.fn();
    const sw = loadServiceWorker([{ url: "https://phone.example.test/", focus: vi.fn(async () => undefined), postMessage }]);
    await sw.dispatch("notificationclick", { notification: { close: vi.fn(), data: { sessionId: "s1", state: "waiting_input" } } });
    expect(postMessage).toHaveBeenCalledWith({ type: "ferryx:open-session", sessionId: "s1" });
  });
});
