import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_PROBE_TIMEOUT_MS,
  probeCandidate,
  normalizeDirectCandidateOrigin,
  selectBestDirectCandidate,
  type CandidateEndpoint,
} from "./directPathUpgrade";

const LAN: CandidateEndpoint = {
  type: "lan",
  url: "http://192.168.1.20:8787",
  priority: 30,
};
const TAILSCALE: CandidateEndpoint = {
  type: "tailscale",
  url: "http://100.64.0.7:8787",
  priority: 20,
};
const RELAY: CandidateEndpoint = {
  type: "relay",
  url: "https://relay.example.com",
  priority: 10,
};

function okResponse(): Response {
  return { ok: true, status: 200 } as Response;
}

function httpErrorResponse(status: number): Response {
  return { ok: false, status } as Response;
}

/** Mirrors how the browser reports a Private Network Access preflight denial. */
function pnaError(): TypeError {
  return new TypeError(
    "Failed to fetch: blocked by Private Network Access preflight",
  );
}

function mixedContentError(): TypeError {
  return new TypeError("Mixed Content: request to http:// endpoint was blocked");
}

function corsError(): TypeError {
  return new TypeError("Access-Control-Allow-Origin header is missing (CORS)");
}

/** A request that only settles when the caller's AbortController fires. */
function hangingFetch(signal: AbortSignal | undefined): Promise<Response> {
  return new Promise<Response>((_resolve, reject) => {
    signal?.addEventListener("abort", () => {
      const error = new Error("The operation was aborted.");
      error.name = "AbortError";
      reject(error);
    });
  });
}

/**
 * Manual clock replacing `setTimeout`, so timeout behaviour is driven by an explicit
 * `advance(ms)` rather than by waiting on wall-clock time. Works identically under
 * `bun test` and `vitest`, neither of whose fake-timer APIs fully overlap.
 */
interface ManualClock {
  advance: (ms: number) => void;
  pendingCount: () => number;
  restore: () => void;
}

function installManualClock(): ManualClock {
  const realSetTimeout = globalThis.setTimeout;
  const realClearTimeout = globalThis.clearTimeout;
  let currentTime = 0;
  let nextId = 1;
  const scheduled = new Map<number, { at: number; run: () => void }>();

  globalThis.setTimeout = ((handler: () => void, ms?: number) => {
    const id = nextId++;
    scheduled.set(id, { at: currentTime + (ms ?? 0), run: handler });
    return id as unknown as ReturnType<typeof setTimeout>;
  }) as typeof globalThis.setTimeout;

  globalThis.clearTimeout = ((id?: unknown) => {
    if (typeof id === "number") scheduled.delete(id);
  }) as typeof globalThis.clearTimeout;

  return {
    advance(ms: number) {
      currentTime += ms;
      for (const [id, entry] of [...scheduled].sort(
        (a, b) => a[1].at - b[1].at,
      )) {
        if (entry.at <= currentTime) {
          scheduled.delete(id);
          entry.run();
        }
      }
    },
    pendingCount: () => scheduled.size,
    restore() {
      scheduled.clear();
      globalThis.setTimeout = realSetTimeout;
      globalThis.clearTimeout = realClearTimeout;
    },
  };
}

let fetchMock: ReturnType<typeof vi.fn>;
let originalFetch: typeof globalThis.fetch;
let clock: ManualClock | null;
let unhandledRejections: unknown[];
let onUnhandledRejection: (reason: unknown) => void;

beforeEach(() => {
  fetchMock = vi.fn();
  originalFetch = globalThis.fetch;
  globalThis.fetch = fetchMock as unknown as typeof globalThis.fetch;
  clock = null;
  unhandledRejections = [];
  onUnhandledRejection = (reason: unknown) => {
    unhandledRejections.push(reason);
  };
  process.on("unhandledRejection", onUnhandledRejection);
});

afterEach(() => {
  process.off("unhandledRejection", onUnhandledRejection);
  clock?.restore();
  globalThis.fetch = originalFetch;
});

describe("candidate origin validation", () => {
  it.each([
    "http://10.0.0.1:8787", "https://172.16.0.1", "http://172.31.255.255",
    "http://192.168.1.1", "http://127.0.0.2", "http://localhost:8787",
    "http://[::1]:8787", "https://100.64.0.1", "https://100.127.255.255",
  ])("accepts private or loopback origin %s", (url) => {
    expect(normalizeDirectCandidateOrigin(url)).toBe(new URL(url).origin);
  });

  it.each([
    "https://evil.example", "https://8.8.8.8", "http://172.15.0.1",
    "http://172.32.0.1", "http://100.63.255.255", "http://100.128.0.1",
    "http://192.168.1.1.evil.example", "http://192.168.1.1@evil.example",
    "http://user:pass@192.168.1.1", "ftp://192.168.1.1", "/relative",
    "http://192.168.1.1/path", "http://192.168.1.1?redirect=evil",
    "http://192.168.1.1#fragment", "http://[2001:4860:4860::8888]",
  ])("rejects unsafe origin without probing %s", async (url) => {
    expect(normalizeDirectCandidateOrigin(url)).toBeNull();
    expect(await probeCandidate({ ...LAN, url })).toMatchObject({ ok: false, reason: "blocked" });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("never selects an untrusted origin even with the highest priority", async () => {
    fetchMock.mockResolvedValue(okResponse());
    expect(await selectBestDirectCandidate([
      { ...LAN, url: "https://evil.example", priority: 999 }, TAILSCALE, LAN,
    ])).toBe(LAN);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});

describe("probeCandidate", () => {
  it("reports a reachable candidate and hits the health endpoint with an abort signal", async () => {
    fetchMock.mockResolvedValue(okResponse());

    const result = await probeCandidate(LAN);

    expect(result.ok).toBe(true);
    expect(result.candidate).toBe(LAN);
    expect(result.reason).toBeUndefined();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://192.168.1.20:8787/api/v1/health");
    expect(init.redirect).toBe("error");
    expect(init.credentials).toBe("omit");
    expect(init.signal).toBeInstanceOf(AbortSignal);
  });

  it("does not double up on slashes when the candidate url has a trailing slash", async () => {
    fetchMock.mockResolvedValue(okResponse());

    await probeCandidate({ ...LAN, url: "http://192.168.1.20:8787/" });

    expect(fetchMock.mock.calls[0][0]).toBe("http://192.168.1.20:8787/api/v1/health");
  });

  it.each([
    ["PNA preflight denial", pnaError()],
    ["Mixed Content block", mixedContentError()],
    ["CORS rejection", corsError()],
  ])("classifies a %s as blocked without an uncaught rejection", async (_label, error) => {
    fetchMock.mockRejectedValue(error);

    const result = await probeCandidate(LAN);

    expect(result.ok).toBe(false);
    expect(result.reason).toBe("blocked");
    await Promise.resolve();
    expect(unhandledRejections).toEqual([]);
  });

  it("classifies a plain network failure as network", async () => {
    fetchMock.mockRejectedValue(new TypeError("Failed to fetch"));

    const result = await probeCandidate(LAN);

    expect(result).toMatchObject({ ok: false, reason: "network" });
  });

  it("treats a non-2xx health response as unreachable", async () => {
    fetchMock.mockResolvedValue(httpErrorResponse(503));

    const result = await probeCandidate(LAN);

    expect(result).toMatchObject({ ok: false, reason: "http-error" });
    expect(result.detail).toContain("503");
  });

  it("aborts and reports a timeout when the health request stalls past the deadline", async () => {
    clock = installManualClock();
    let capturedSignal: AbortSignal | undefined;
    fetchMock.mockImplementation((_url: string, init: RequestInit) => {
      capturedSignal = init.signal ?? undefined;
      return hangingFetch(init.signal ?? undefined);
    });

    const pending = probeCandidate(LAN, 1500);
    clock.advance(1499);
    expect(capturedSignal?.aborted).toBe(false);

    clock.advance(1);
    const result = await pending;

    expect(capturedSignal?.aborted).toBe(true);
    expect(result).toMatchObject({ ok: false, reason: "timeout" });
    expect(unhandledRejections).toEqual([]);
  });

  it("uses the documented default timeout when none is supplied", async () => {
    clock = installManualClock();
    fetchMock.mockImplementation((_url: string, init: RequestInit) =>
      hangingFetch(init.signal ?? undefined),
    );

    const pending = probeCandidate(LAN);
    clock.advance(DEFAULT_PROBE_TIMEOUT_MS);

    await expect(pending).resolves.toMatchObject({ reason: "timeout" });
    expect(DEFAULT_PROBE_TIMEOUT_MS).toBe(1500);
  });

  it("clears its timeout timer once the probe settles", async () => {
    clock = installManualClock();
    fetchMock.mockResolvedValue(okResponse());

    await probeCandidate(LAN, 1500);

    expect(clock.pendingCount()).toBe(0);
  });
});

describe("selectBestDirectCandidate", () => {
  it("upgrades to the fast LAN candidate and probes candidates concurrently", async () => {
    const started: string[] = [];
    let releaseAll: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      releaseAll = resolve;
    });
    fetchMock.mockImplementation((url: string) => {
      started.push(url);
      return gate.then(() => okResponse());
    });

    const pending = selectBestDirectCandidate([TAILSCALE, LAN, RELAY]);
    // Both direct probes are in flight before any of them settles.
    expect(started).toHaveLength(2);
    releaseAll?.();

    expect(await pending).toBe(LAN);
    // The relay is the fallback, never a probe target.
    expect(started.some((url) => url.includes("relay.example.com"))).toBe(false);
  });

  it("falls back to Tailscale when the LAN candidate is blocked by PNA", async () => {
    fetchMock.mockImplementation((url: string) =>
      url.includes("192.168.1.20")
        ? Promise.reject(pnaError())
        : Promise.resolve(okResponse()),
    );

    expect(await selectBestDirectCandidate([LAN, TAILSCALE, RELAY])).toBe(
      TAILSCALE,
    );
    expect(unhandledRejections).toEqual([]);
  });

  it("returns null so the caller stays on the relay when every direct candidate fails", async () => {
    fetchMock.mockRejectedValue(pnaError());

    expect(await selectBestDirectCandidate([LAN, TAILSCALE, RELAY])).toBeNull();
    expect(unhandledRejections).toEqual([]);
  });

  it("returns null when a direct candidate stalls past the timeout", async () => {
    clock = installManualClock();
    fetchMock.mockImplementation((_url: string, init: RequestInit) =>
      hangingFetch(init.signal ?? undefined),
    );

    const pending = selectBestDirectCandidate([LAN, TAILSCALE, RELAY], 800);
    clock.advance(800);

    expect(await pending).toBeNull();
    expect(unhandledRejections).toEqual([]);
  });

  it("still upgrades when the slower reachable candidate outranks a fast one", async () => {
    clock = installManualClock();
    fetchMock.mockImplementation((url: string, init: RequestInit) =>
      url.includes("100.64.0.7")
        ? hangingFetch(init.signal ?? undefined)
        : Promise.resolve(okResponse()),
    );

    const pending = selectBestDirectCandidate([TAILSCALE, LAN, RELAY], 800);
    clock.advance(800);

    expect(await pending).toBe(LAN);
  });

  it("prefers the highest priority reachable candidate regardless of input order", async () => {
    fetchMock.mockResolvedValue(okResponse());

    const highTailscale: CandidateEndpoint = { ...TAILSCALE, priority: 99 };
    expect(await selectBestDirectCandidate([LAN, highTailscale])).toBe(
      highTailscale,
    );
    expect(await selectBestDirectCandidate([highTailscale, LAN])).toBe(
      highTailscale,
    );
  });

  it("returns null for a relay-only candidate list without probing", async () => {
    expect(await selectBestDirectCandidate([RELAY])).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
