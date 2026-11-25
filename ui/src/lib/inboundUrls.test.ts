import { describe, expect, it, vi } from "vitest";

import { startInboundUrlDelivery, webUrls } from "./inboundUrls";

const INBOUND = "http://127.0.0.1:60321/qa/g10.html?inbound=running";

/** A queue, a listener and a tab opener the test drives by hand. */
function harness(initial: string[] = []) {
  const queue = [...initial];
  const opened: string[] = [];
  const state = { listener: null as (() => void) | null };
  const unlisten = vi.fn(() => {
    state.listener = null;
  });
  const take = vi.fn(async () => queue.splice(0, queue.length));
  const on = vi.fn(async (registered: () => void) => {
    state.listener = registered;
    return unlisten;
  });
  const deliver = (url: string) => {
    queue.push(url);
    state.listener?.();
  };
  return {
    queue,
    opened,
    take,
    on,
    unlisten,
    state,
    deliver,
    start: (canDrain?: () => boolean) => startInboundUrlDelivery({ take, on, canDrain, open: (url) => opened.push(url) }),
  };
}

describe("webUrls", () => {
  it("keeps http and https links and drops every other scheme", () => {
    expect(webUrls(["http://a.test/x", "https://b.test/y?z", "file:///etc/passwd", "javascript:1", "ferryx://open"])).toEqual([
      "http://a.test/x",
      "https://b.test/y?z",
    ]);
  });

  it("requires a host", () => {
    expect(webUrls(["http://", "https://?x=1", "HTTPS://CASE.test/z"])).toEqual(["HTTPS://CASE.test/z"]);
  });

  it("opens a padded link without its surrounding whitespace", () => {
    expect(webUrls([`  ${INBOUND}\n`])).toEqual([INBOUND]);
  });
});

describe("startInboundUrlDelivery", () => {
  it("drains the links queued before it started and opens each in a tab", async () => {
    const test = harness([INBOUND, "https://example.test/two"]);
    const delivery = test.start();
    await test.take.mock.results[0].value;

    expect(test.take).toHaveBeenCalledTimes(1);
    expect(test.opened).toEqual([INBOUND, "https://example.test/two"]);
    delivery.dispose();
  });

  it("opens the links a later event announces", async () => {
    const test = harness();
    const delivery = test.start();
    await test.take.mock.results[0].value;
    expect(test.opened).toEqual([]);

    test.deliver("https://example.test/later");
    await test.take.mock.results[1].value;

    expect(test.take).toHaveBeenCalledTimes(2);
    expect(test.opened).toEqual(["https://example.test/later"]);
    delivery.dispose();
  });

  it("drains only http(s) links, so a stray scheme never becomes a tab", async () => {
    const test = harness(["file:///etc/passwd", INBOUND]);
    const delivery = test.start();
    await test.take.mock.results[0].value;

    expect(test.opened).toEqual([INBOUND]);
    delivery.dispose();
  });

  it("does not take queued URLs while draining is disabled", () => {
    const test = harness([INBOUND]);
    const delivery = test.start(() => false);
    test.deliver("https://example.test/remote");

    expect(test.take).not.toHaveBeenCalled();
    expect(test.queue).toEqual([INBOUND, "https://example.test/remote"]);
    expect(test.opened).toEqual([]);
    delivery.dispose();
  });

  it("drains the retained URLs when enabled and triggered without another event", async () => {
    const test = harness([INBOUND]);
    let canDrain = false;
    const delivery = test.start(() => canDrain);
    test.deliver("https://example.test/remote");
    expect(test.take).not.toHaveBeenCalled();

    canDrain = true;
    delivery.trigger();
    await test.take.mock.results[0].value;

    expect(test.take).toHaveBeenCalledTimes(1);
    expect(test.queue).toEqual([]);
    expect(test.opened).toEqual([INBOUND, "https://example.test/remote"]);
    delivery.dispose();
  });

  it("uses the latest opener for an in-flight take and ignores triggers after dispose", async () => {
    let release!: (urls: string[]) => void;
    const pending = new Promise<string[]>((done) => {
      release = done;
    });
    const firstOpen = vi.fn();
    const latestOpen = vi.fn();
    const openRef = { current: firstOpen };
    const take = vi.fn(() => pending);
    const unlisten = vi.fn();
    const on = vi.fn(async () => unlisten);
    const delivery = startInboundUrlDelivery({ take, on, open: (url) => openRef.current(url) });

    openRef.current = latestOpen;
    release([INBOUND]);
    await pending;

    expect(take).toHaveBeenCalledTimes(1);
    expect(on).toHaveBeenCalledTimes(1);
    expect(unlisten).not.toHaveBeenCalled();
    expect(firstOpen).not.toHaveBeenCalled();
    expect(latestOpen).toHaveBeenCalledExactlyOnceWith(INBOUND);

    delivery.dispose();
    delivery.trigger();
    expect(take).toHaveBeenCalledTimes(1);
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(latestOpen).toHaveBeenCalledTimes(1);
  });

  it("coalesces an event that arrives while a drain is in flight instead of dropping it", async () => {
    const queue = ["https://example.test/first"];
    const opened: string[] = [];
    let listener: (() => void) | null = null;
    let release!: () => void;
    const gate = new Promise<void>((done) => {
      release = done;
    });
    let drained!: () => void;
    const coalesced = new Promise<void>((done) => {
      drained = done;
    });
    let gated = true;
    const take = vi.fn(async () => {
      if (gated) {
        gated = false;
        await gate;
      } else {
        drained();
      }
      return queue.splice(0, queue.length);
    });
    const delivery = startInboundUrlDelivery({
      take,
      on: async (registered) => {
        listener = registered;
        return () => {
          listener = null;
        };
      },
      open: (url) => opened.push(url),
    });

    // The event lands while the first take is still waiting on the gate.
    queue.push("https://example.test/second");
    (listener as (() => void) | null)?.();
    release();
    await coalesced;

    expect(take).toHaveBeenCalledTimes(2);
    expect(opened).toEqual(["https://example.test/first", "https://example.test/second"]);
    delivery.dispose();
  });

  it("releases the listener on dispose and ignores a take that resolves afterwards", async () => {
    let release!: (urls: string[]) => void;
    const late = new Promise<string[]>((done) => {
      release = done;
    });
    const opened: string[] = [];
    const unlisten = vi.fn();
    const on = vi.fn(async () => unlisten);
    const { dispose } = startInboundUrlDelivery({ take: () => late, on, open: (url) => opened.push(url) });
    await on.mock.results[0].value;

    dispose();
    release(["https://example.test/late"]);
    await late;

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(opened).toEqual([]);
  });

  it("releases a listener whose subscription resolves only after dispose", async () => {
    let unlistened!: () => void;
    const released = new Promise<void>((done) => {
      unlistened = done;
    });
    const unlisten = vi.fn(() => unlistened());
    let release!: (dispose: () => void) => void;
    const subscribing = new Promise<() => void>((done) => {
      release = done;
    });
    const { dispose } = startInboundUrlDelivery({ take: async () => [], on: () => subscribing, open: () => {} });

    dispose();
    // The Tauri listen call was still in flight when the window unmounted.
    release(unlisten);
    await released;

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("keeps the queue intact when a take fails, and drains it on the next event", async () => {
    const opened: string[] = [];
    let fail = true;
    const queue = ["https://example.test/retry"];
    let listener: (() => void) | null = null;
    const take = vi.fn(async () => {
      if (fail) throw new Error("ipc not ready");
      return queue.splice(0, queue.length);
    });
    const delivery = startInboundUrlDelivery({
      take,
      on: async (registered) => {
        listener = registered;
        return () => {
          listener = null;
        };
      },
      open: (url) => opened.push(url),
    });
    await expect(take.mock.results[0].value).rejects.toThrow("ipc not ready");
    expect(opened).toEqual([]);
    expect(queue).toEqual(["https://example.test/retry"]);

    fail = false;
    (listener as (() => void) | null)?.();
    await take.mock.results[1].value;

    expect(opened).toEqual(["https://example.test/retry"]);
    delivery.dispose();
  });
});
