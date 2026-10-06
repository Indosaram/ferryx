import { describe, expect, it, vi } from "vitest";
import {
  REFERENCE_HISTORY_PAGE_LIMIT,
  ReferenceHistoryFetchError,
  fetchReferenceHistory,
  fenceReferenceHistory,
  isReferenceHistoryCurrent,
  parseReferenceHistoryPage,
  referenceHistoryCursorForStream,
  referenceHistoryDisposition,
  referenceHistoryErrorIsIdentity,
  referenceHistoryFence,
  referenceHistoryFenceKey,
  referenceHistoryNotice,
  referenceHistoryQuery,
} from "./agentConversation";
import {
  REFERENCE_CHAT_ROUTE_PREFIX,
  REFERENCE_SCROLLBACK_DISCLOSURE,
  type ReferenceHistoryPage,
  type ReferenceTargetRef,
} from "./chat/referenceTypes";
import type { TargetRef } from "../lib/scopedContracts";

const target: TargetRef = {
  hostId: "host-a",
  ownerId: "owner-a",
  epoch: "18446744073709551615",
  backendSessionId: "sess-1",
};

function boundTarget(providerSessionId?: string | null): ReferenceTargetRef {
  return { target, providerSessionId };
}

/** A native page body exactly as the Rust `ReferenceHistoryPage` serializes it. */
function nativeBody(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    source: "claude-transcript",
    availability: "native",
    turns: [],
    cursor: null,
    hasMore: false,
    generation: "gen-1",
    unavailableReason: null,
    ...overrides,
  };
}

function okResponse(body: unknown) {
  return {
    ok: true,
    status: 200,
    json: async () => body,
  };
}

function errorResponse(status: number, body: unknown) {
  return {
    ok: false,
    status,
    text: async () => JSON.stringify(body),
  };
}

function captureFetch(response: unknown) {
  const mockFetch = vi.fn().mockResolvedValue(response);
  vi.stubGlobal("fetch", mockFetch);
  return mockFetch;
}

describe("reference history page parsing", () => {
  it("parses a native page with every rich part variant intact", () => {
    const body = nativeBody({
      generation: "gen-9",
      cursor: { streamId: "dev:ino", offset: 4096 },
      hasMore: true,
      turns: [
        { role: "user", parts: [{ kind: "text", text: "hi", phase: "finalAnswer" }] },
        {
          role: "assistant",
          startedAt: "2026-10-06T00:00:00.000Z",
          endedAt: "2026-10-06T00:00:05.000Z",
          source: "runtime",
          parts: [
            { kind: "thinking", text: "hmm" },
            {
              kind: "skill",
              skill: {
                name: "debugging",
                evidence: "instructions",
                status: "loaded",
                path: "/s/SKILL.md",
              },
            },
            {
              kind: "tool",
              name: "read",
              summary: "s",
              input: "i",
              output: "cut",
              error: false,
              outputRef: "r1",
              outputSize: 900,
              skill: { name: "debugging", evidence: "invocation", status: "loaded", path: "/s/SKILL.md" },
              images: [{ mediaType: "image/png", ref: "img-1" }],
            },
            { kind: "image", mediaType: "image/png", ref: "img-2" },
            { kind: "compact", text: "summary" },
            { kind: "notice", text: "job done", source: "async-result" },
            {
              kind: "taskResult",
              tasks: [
                {
                  id: "t1",
                  title: "task one",
                  agent: "explore",
                  model: "smol",
                  status: "completed",
                  durationMs: 1200,
                  turns: 3,
                  toolCalls: 4,
                  tokens: 500,
                  result: "ok",
                  resultCut: false,
                },
              ],
            },
          ],
          abandoned: { count: 2, branches: 1, summary: "walked away" },
        },
      ],
    });

    const page = parseReferenceHistoryPage(body);

    expect(page.source).toBe("claude-transcript");
    expect(page.availability).toBe("native");
    expect(page.generation).toBe("gen-9");
    expect(page.hasMore).toBe(true);
    expect(page.cursor).toEqual({ streamId: "dev:ino", offset: 4096 });
    expect(page.turns).toHaveLength(2);

    const [user, assistant] = page.turns;
    expect(user.role).toBe("user");
    expect(user.parts[0]).toEqual({ kind: "text", text: "hi", phase: "finalAnswer" });

    expect(assistant.role).toBe("assistant");
    expect(assistant.startedAt).toBe("2026-10-06T00:00:00.000Z");
    expect(assistant.endedAt).toBe("2026-10-06T00:00:05.000Z");
    expect(assistant.source).toBe("runtime");
    expect(assistant.abandoned).toEqual({ count: 2, branches: 1, summary: "walked away" });
    expect(assistant.parts.map((part) => part.kind)).toEqual([
      "thinking",
      "skill",
      "tool",
      "image",
      "compact",
      "notice",
      "taskResult",
    ]);

    expect(assistant.parts[1]).toEqual({
      kind: "skill",
      skill: { name: "debugging", evidence: "instructions", status: "loaded", path: "/s/SKILL.md" },
    });

    const tool = assistant.parts[2];
    expect(tool).toMatchObject({
      kind: "tool",
      name: "read",
      outputRef: "r1",
      outputSize: 900,
      skill: { name: "debugging", evidence: "invocation", status: "loaded", path: "/s/SKILL.md" },
      images: [{ mediaType: "image/png", ref: "img-1" }],
    });

    const taskResult = assistant.parts[6];
    expect(taskResult).toMatchObject({
      kind: "taskResult",
      tasks: [
        {
          id: "t1",
          status: "completed",
          durationMs: 1200,
          turns: 3,
          toolCalls: 4,
          tokens: 500,
          resultCut: false,
        },
      ],
    });
  });

  it("round-trips the parsed page through JSON without losing source or generation", () => {
    const body = nativeBody({ generation: "gen-9", turns: [{ role: "user", parts: [{ kind: "text", text: "hi" }] }] });
    const page = parseReferenceHistoryPage(body);
    expect(JSON.parse(JSON.stringify(page))).toEqual(page);
  });

  it("parses an explicit scrollback page as a page, not an error", () => {
    const page = parseReferenceHistoryPage(
      nativeBody({
        source: "scrollback",
        availability: "scrollback",
        generation: "gen-2",
        unavailableReason: "no native reader for this pane",
      }),
    );

    expect(page.availability).toBe("scrollback");
    expect(page.turns).toHaveLength(0);
    expect(referenceHistoryDisposition(page)).toBe("disclose");
    expect(referenceHistoryNotice(page)).toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
  });

  it("parses a not-started session as an empty conversation, disclosed differently from scrollback", () => {
    const page = parseReferenceHistoryPage(
      nativeBody({
        source: "omo-transcript",
        availability: "notStarted",
        unavailableReason: "session_not_written",
      }),
    );

    expect(page.availability).toBe("notStarted");
    expect(page.turns).toHaveLength(0);
    const notice = referenceHistoryNotice(page);
    expect(notice).not.toBeNull();
    expect(notice).not.toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
  });

  it("keeps a native page as render with no disclosure", () => {
    const page = parseReferenceHistoryPage(nativeBody());
    expect(referenceHistoryDisposition(page)).toBe("render");
    expect(referenceHistoryNotice(page)).toBeNull();
  });

  it("requires unavailableReason whenever the page is not native", () => {
    const body = nativeBody({ source: "scrollback", availability: "scrollback" });
    delete (body as Record<string, unknown>).unavailableReason;
    expect(() => parseReferenceHistoryPage(body)).toThrowError(ReferenceHistoryFetchError);
    expect(() => parseReferenceHistoryPage(body)).toThrowError(/unavailableReason/);
  });

  it("rejects a malformed page instead of defaulting any field", () => {
    const cases: Record<string, unknown>[] = [
      { ...nativeBody(), source: "claude-transcript " },
      { ...nativeBody(), source: "hermes-transcript" },
      { ...nativeBody(), availability: "native-ish" },
      { ...nativeBody(), generation: undefined },
      { ...nativeBody(), generation: 7 },
      { ...nativeBody(), hasMore: "yes" },
      { ...nativeBody(), turns: "none" },
      { ...nativeBody(), cursor: { streamId: "dev:ino", offset: "4096" } },
      { ...nativeBody(), turns: [{ role: "system", parts: [] }] },
      { ...nativeBody(), turns: [{ role: "user", parts: [{ kind: "audio", text: "x" }] }] },
      { ...nativeBody(), turns: [{ role: "user" }] },
      { ...nativeBody(), turns: [{ role: "assistant", parts: [{ kind: "taskResult", tasks: "one" }] }] },
      { ...nativeBody(), turns: [{ role: "assistant", parts: [{ kind: "tool", name: "read" }] }] },
      { ...nativeBody(), turns: [{ role: "assistant", parts: [{ kind: "skill" }] }] },
      {
        ...nativeBody(),
        turns: [
          {
            role: "assistant",
            parts: [{ kind: "skill", skill: { name: "x", evidence: "instructions" } }],
          },
        ],
      },
      { ...nativeBody(), turns: [{ role: "assistant", parts: [{ kind: "skill", skill: "debugging" }] }] },
    ];

    for (const body of cases) {
      expect(() => parseReferenceHistoryPage(body)).toThrowError(ReferenceHistoryFetchError);
    }

    expect(() => parseReferenceHistoryPage(null)).toThrowError(/is not an object/);
    expect(() => parseReferenceHistoryPage("page")).toThrowError(/is not an object/);
  });

  it("accepts a page whose cursor is omitted entirely", () => {
    const body = nativeBody();
    delete (body as Record<string, unknown>).cursor;
    const page = parseReferenceHistoryPage(body);
    expect(page.cursor).toBeUndefined();
  });
});

describe("reference history generation fencing", () => {
  it("accepts the response that answers the request and rejects a late foreign generation", () => {
    const expected = referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-1");

    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-1"))).toBe(true);

    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-2"))).toBe(false);
  });

  it("rejects a response from another session or another pane incarnation", () => {
    const expected = referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-1");

    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-2", boundTarget("provider-7"), "gen-1"))).toBe(false);

    const otherEpoch: ReferenceTargetRef = { target: { ...target, epoch: "7" }, providerSessionId: "provider-7" };
    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", otherEpoch, "gen-1"))).toBe(false);

    const otherOwner: ReferenceTargetRef = { target: { ...target, ownerId: "owner-b" }, providerSessionId: "provider-7" };
    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", otherOwner, "gen-1"))).toBe(false);

    const otherBackend: ReferenceTargetRef = { target: { ...target, backendSessionId: "sess-9" }, providerSessionId: "provider-7" };
    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", otherBackend, "gen-1"))).toBe(false);
  });

  it("treats a provider-session refinement as the same pane, not a stale response", () => {
    const expected = referenceHistoryFence("sess-1", boundTarget(null), "gen-1");
    expect(isReferenceHistoryCurrent(expected, referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-1"))).toBe(true);
  });

  it("fences a completed read into a current page or a drop", () => {
    const page: ReferenceHistoryPage = parseReferenceHistoryPage(nativeBody({ generation: "gen-3" }));
    const expected = referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-3");

    expect(
      fenceReferenceHistory(expected, {
        page,
        fence: referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-3"),
      }),
    ).toEqual({ kind: "current", page });

    expect(
      fenceReferenceHistory(expected, {
        page,
        fence: referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-4"),
      }),
    ).toEqual({ kind: "stale" });
  });

  it("keys a fence on session, target identity and generation", () => {
    const fence = referenceHistoryFence("sess-1", boundTarget("provider-7"), "gen-1");
    expect(referenceHistoryFenceKey(fence)).toBe(
      `sess-1#host-a|owner-a|18446744073709551615|sess-1#gen-1`,
    );
  });
});

describe("reference history cursor stream binding", () => {
  it("keeps a cursor for its own stream and drops one minted against another", () => {
    const cursor = { streamId: "dev:ino", offset: 4096 };
    expect(referenceHistoryCursorForStream(cursor, "dev:ino")).toEqual(cursor);
    expect(referenceHistoryCursorForStream(cursor, "dev:other")).toBeNull();
  });
});

describe("reference history query binding", () => {
  it("binds host, owner, epoch and backend session, and omits an unknown provider session", () => {
    const query = new URLSearchParams(referenceHistoryQuery({ target: boundTarget(null) }));

    expect(query.get("hostId")).toBe("host-a");
    expect(query.get("ownerId")).toBe("owner-a");
    expect(query.get("epoch")).toBe("18446744073709551615");
    expect(query.get("backendSessionId")).toBe("sess-1");
    expect(query.get("limit")).toBe(String(REFERENCE_HISTORY_PAGE_LIMIT));
    expect(query.has("providerSessionId")).toBe(false);
    expect(query.has("cursor")).toBe(false);
  });

  it("sends the provider session only where a reader identified one", () => {
    const bound = new URLSearchParams(referenceHistoryQuery({ target: boundTarget("provider-7") }));
    expect(bound.get("providerSessionId")).toBe("provider-7");

    const blank = new URLSearchParams(referenceHistoryQuery({ target: boundTarget("   ") }));
    expect(blank.has("providerSessionId")).toBe(false);
  });

  it("carries the cursor stream and offset when paging", () => {
    const query = new URLSearchParams(
      referenceHistoryQuery({
        target: boundTarget("provider-7"),
        limit: 25,
        cursor: { streamId: "dev:ino", offset: 4096 },
      }),
    );

    expect(query.get("limit")).toBe("25");
    expect(query.get("cursor")).toBe("4096");
    expect(query.get("cursorStream")).toBe("dev:ino");
  });
});

describe("reference history fetch", () => {
  it("reads the frozen route bound to the target and returns the page with its fence", async () => {
    const body = nativeBody({ generation: "gen-5", turns: [{ role: "user", parts: [{ kind: "text", text: "hi" }] }] });
    const mockFetch = captureFetch(okResponse(body));

    try {
      const read = await fetchReferenceHistory({
        baseUrl: "http://127.0.0.1:8899",
        sessionId: "session-1",
        token: "test-token",
        target: boundTarget("provider-7"),
      });

      const url = String(mockFetch.mock.calls[0][0]);
      expect(url).toContain(`${REFERENCE_CHAT_ROUTE_PREFIX}/session-1/history?`);
      const query = new URLSearchParams(url.slice(url.indexOf("?") + 1));
      expect(query.get("epoch")).toBe("18446744073709551615");
      expect(query.get("providerSessionId")).toBe("provider-7");

      expect(read.page.generation).toBe("gen-5");
      expect(read.page.turns).toHaveLength(1);
      expect(read.fence).toEqual({
        sessionId: "session-1",
        target: boundTarget("provider-7"),
        generation: "gen-5",
      });
      expect(isReferenceHistoryCurrent(read.fence, read.fence)).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("returns an unavailable page as a successful read rather than an error", async () => {
    captureFetch(
      okResponse(
        nativeBody({
          source: "scrollback",
          availability: "scrollback",
          generation: "gen-2",
          unavailableReason: "no native reader for this pane",
        }),
      ),
    );

    try {
      const read = await fetchReferenceHistory({
        baseUrl: "http://127.0.0.1:8899",
        sessionId: "session-1",
        token: "test-token",
        target: boundTarget(null),
      });

      expect(read.page.availability).toBe("scrollback");
      expect(referenceHistoryNotice(read.page)).toBe(REFERENCE_SCROLLBACK_DISCLOSURE);
      expect(referenceHistoryDisposition(read.page)).toBe("disclose");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("throws a typed identity error instead of substituting another transcript", async () => {
    const cases: [number, string][] = [
      [401, "UNAUTHORIZED"],
      [403, "FORBIDDEN"],
      [404, "NOT_FOUND"],
      [409, "REQUEST_CONFLICT"],
      [410, "TARGET_EXPIRED"],
    ];

    for (const [status, code] of cases) {
      captureFetch(errorResponse(status, { error: { code, message: "nope", retryable: false, details: {} } }));
      try {
        await expect(
          fetchReferenceHistory({
            baseUrl: "http://127.0.0.1:8899",
            sessionId: "session-1",
            token: "test-token",
            target: boundTarget("provider-7"),
          }),
        ).rejects.toMatchObject({ code });
      } finally {
        vi.unstubAllGlobals();
      }
    }
  });

  it("reads a structured error code from the scoped envelope and maps a bare status otherwise", async () => {
    captureFetch(errorResponse(418, { error: { code: "UNSUPPORTED", message: "no", retryable: false, details: {} } }));
    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
        }),
      ).rejects.toMatchObject({ code: "UNSUPPORTED" });
    } finally {
      vi.unstubAllGlobals();
    }

    captureFetch(errorResponse(500, { message: "boom" }));
    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
        }),
      ).rejects.toMatchObject({ code: "REQUEST_FAILED" });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reports a malformed body as MALFORMED_RESPONSE rather than a page", async () => {
    captureFetch({
      ok: true,
      status: 200,
      json: async () => {
        throw new Error("not json");
      },
    });

    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
        }),
      ).rejects.toMatchObject({ code: "MALFORMED_RESPONSE" });
    } finally {
      vi.unstubAllGlobals();
    }

    captureFetch(okResponse({ sessionId: "session-1" }));
    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
        }),
      ).rejects.toMatchObject({ code: "MALFORMED_RESPONSE" });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reports a transport failure as NETWORK_ERROR and rethrows an abort untouched", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("socket closed")));
    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
        }),
      ).rejects.toMatchObject({ code: "NETWORK_ERROR" });
    } finally {
      vi.unstubAllGlobals();
    }

    const controller = new AbortController();
    controller.abort();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("aborted")));
    try {
      await expect(
        fetchReferenceHistory({
          baseUrl: "http://127.0.0.1:8899",
          sessionId: "session-1",
          token: "test-token",
          target: boundTarget(null),
          signal: controller.signal,
        }),
      ).rejects.toThrowError("aborted");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("classifies identity refusals separately from transport and parse failures", () => {
    for (const code of ["UNAUTHORIZED", "FORBIDDEN", "NOT_FOUND", "TARGET_EXPIRED", "REQUEST_CONFLICT", "INVENTORY_INCOMPLETE"]) {
      expect(referenceHistoryErrorIsIdentity(code)).toBe(true);
    }
    for (const code of ["MALFORMED_RESPONSE", "NETWORK_ERROR", "REQUEST_FAILED", "UNSUPPORTED"]) {
      expect(referenceHistoryErrorIsIdentity(code)).toBe(false);
    }
  });
});
