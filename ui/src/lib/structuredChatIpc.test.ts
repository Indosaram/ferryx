import { describe, expect, it, vi } from "vitest";
import {
  STRUCTURED_CHAT_COMMAND,
  desktopStructuredChatTransport,
} from "./structuredChatIpc";
import { bindStructuredChatRemote, remoteStructuredChatTransport } from "./structuredChatRemote";
import type { StructuredChatOp } from "./structuredChatTransport";

const SIGNAL = new AbortController().signal;

describe("desktop structured chat adapter", () => {
  it("sends the op and body through one Tauri command and decodes its answer", async () => {
    const call = vi.fn(async (request: { op: StructuredChatOp; sessionId?: string; body: unknown }) => {
      expect(request).toEqual({ op: "turn", sessionId: "s1", body: { text: "hi" } });
      return { seq: 4 };
    });
    const transport = desktopStructuredChatTransport(call);
    await expect(transport({ op: "turn", sessionId: "s1", body: { text: "hi" } }, SIGNAL)).resolves.toEqual({
      ok: true,
      data: 4,
    });
    expect(STRUCTURED_CHAT_COMMAND).toBe("cmd_structured_chat");
  });

  it("keeps a provider failure as its own code rather than the IPC code", async () => {
    const transport = desktopStructuredChatTransport(async () => {
      throw { code: "InternalError", message: "PROVIDER_FAILED: child died" };
    });
    await expect(
      transport({ op: "capabilities", sessionId: "s1", body: {} }, SIGNAL),
    ).resolves.toMatchObject({ ok: false, code: "PROVIDER_FAILED" });
  });

  it("maps a missing session to NOT_FOUND so the lane can resync", async () => {
    const transport = desktopStructuredChatTransport(async () => {
      throw { code: "SessionNotFound", message: "no such session" };
    });
    await expect(
      transport({ op: "history", sessionId: "gone", body: { after: 0 } }, SIGNAL),
    ).resolves.toMatchObject({ ok: false, code: "NOT_FOUND" });
  });
});

describe("remote structured chat adapter", () => {
  it("refuses every call when the host is not connected", async () => {
    bindStructuredChatRemote(null);
    const transport = remoteStructuredChatTransport();
    await expect(
      transport({ op: "listSessions", body: {} }, SIGNAL),
    ).resolves.toMatchObject({ ok: false, code: "UNAUTHORIZED" });
  });

  it("carries the bearer token and the route the HTTP contract names", async () => {
    const seen: { path: string; auth: string | undefined; body?: unknown }[] = [];
    bindStructuredChatRemote({
      baseUrl: "https://host",
      token: "tok",
      fetchImpl: async (path, init) => {
        seen.push({
          path,
          auth: (init.headers as Record<string, string>).Authorization,
          body: init.body === undefined ? undefined : JSON.parse(String(init.body)),
        });
        return new Response(JSON.stringify({ parts: [] }), { status: 200 });
      },
    });
    const transport = remoteStructuredChatTransport();
    await expect(transport({ op: "events", sessionId: "s1", body: { after: 9 } }, SIGNAL)).resolves.toEqual({
      ok: true,
      data: { parts: [], nextSeq: 9 },
    });
    expect(seen[0]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/events?after=9",
      auth: "Bearer tok",
      body: undefined,
    });
    bindStructuredChatRemote(null);
  });
});
