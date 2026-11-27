/**
 * The remote adapter (lane H5): every op the structured surface can send, over the real HTTP
 * wire shape the gateway serves.
 *
 * The point of these cases is the contract, not the adapter's own plumbing: an op that reaches
 * the wrong method, the wrong path, or the wrong body is a request the gateway will refuse, and
 * the failure shows up as an empty panel rather than an error. So each case asserts the exact
 * request the route table promises, and the decoded value the caller gets back.
 */
import { afterEach, describe, expect, it } from "vitest";
import { bindStructuredChatRemote, remoteStructuredChatTransport } from "./structuredChatRemote";
import { desktopStructuredChatTransport } from "./structuredChatIpc";
import type { StructuredChatOp } from "./structuredChatTransport";

const SIGNAL = new AbortController().signal;

interface Seen {
  readonly path: string;
  readonly method: string;
  readonly body: unknown;
}

/** Bind the adapter to a fake gateway and record what it was asked. */
function gateway(reply: unknown = {}) {
  const seen: Seen[] = [];
  bindStructuredChatRemote({
    baseUrl: "https://host",
    token: "tok",
    fetchImpl: async (path, init) => {
      seen.push({
        path,
        method: init.method ?? "GET",
        body: init.body === undefined ? undefined : JSON.parse(String(init.body)),
      });
      return new Response(JSON.stringify(reply), { status: 200 });
    },
  });
  return { seen, transport: remoteStructuredChatTransport() };
}

afterEach(() => {
  bindStructuredChatRemote(null);
});

describe("remote structured chat: extension UI", () => {
  it("answers a request by id, with the response body the card filled", async () => {
    const { seen, transport } = gateway(undefined);
    await expect(
      transport(
        { op: "answerExtensionUi", sessionId: "s1", body: { requestId: "req-1", value: "beta" } },
        SIGNAL,
      ),
    ).resolves.toEqual({ ok: true, data: undefined });
    expect(seen[0]).toEqual({
      // The request id is a path segment, so the answer names exactly one request even when the
      // body is read by a proxy that logs it.
      path: "/api/v1/structured-chat/sessions/s1/extension-ui/req-1",
      method: "POST",
      body: { requestId: "req-1", value: "beta" },
    });
  });

  it("carries a multi-question answer as the provider's own keyed shape", async () => {
    const { seen, transport } = gateway(undefined);
    const answers = { q1: { selected: ["bun"] }, q2: { selected: ["linux", "macos"] } };
    await transport(
      { op: "answerExtensionUi", sessionId: "s1", body: { requestId: "req-2", answers } },
      SIGNAL,
    );
    expect(seen[0].body).toEqual({ requestId: "req-2", answers });
  });

  it("sends a cancellation as `cancelled`, never as an empty value", async () => {
    const { seen, transport } = gateway(undefined);
    await transport(
      { op: "answerExtensionUi", sessionId: "s1", body: { requestId: "req-3", cancelled: true } },
      SIGNAL,
    );
    expect(seen[0].body).toEqual({ requestId: "req-3", cancelled: true });
  });
});

describe("remote structured chat: commands and branches", () => {
  it("reads the agent's slash commands and drops rows the decoder cannot name", async () => {
    const { transport } = gateway({
      commands: [
        { name: "/compact", description: "Summarize", source: "builtin", syntax: "/compact [instructions]" },
        { name: "", description: "no name" },
      ],
    });
    const result = await transport({ op: "commands", sessionId: "s1", body: {} }, SIGNAL);
    expect(result).toMatchObject({ ok: true });
    if (!result.ok) return;
    expect(result.data).toEqual([
      { name: "/compact", description: "Summarize", source: "builtin", syntax: "/compact [instructions]" },
    ]);
  });

  it("reads the tree as nodes the drawer can indent, keeping the current leaf", async () => {
    const { seen, transport } = gateway({
      currentLeafId: "n2",
      nodes: [
        { id: "n1", role: "user", preview: "first", active: false },
        { id: "n2", parentId: "n1", role: "assistant", preview: "reply", active: true },
      ],
    });
    const result = await transport({ op: "tree", sessionId: "s1", body: {} }, SIGNAL);
    expect(seen[0]).toEqual({ path: "/api/v1/structured-chat/sessions/s1/tree", method: "GET", body: undefined });
    expect(result).toMatchObject({ ok: true });
    if (!result.ok) return;
    expect(result.data.currentLeafId).toBe("n2");
    expect(result.data.nodes.map((node) => node.id)).toEqual(["n1", "n2"]);
  });

  it("navigates by entry id and sends no summarize bit unless asked", async () => {
    const { seen, transport } = gateway(undefined);
    await transport({ op: "navigateTree", sessionId: "s1", body: { targetId: "n3" } }, SIGNAL);
    expect(seen[0]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/navigate-tree",
      method: "POST",
      body: { targetId: "n3" },
    });
  });

  it("forks at an entry and reads back the session it produced", async () => {
    const { seen, transport } = gateway({ sessionId: "forked-1", threadId: "th-1" });
    const result = await transport({ op: "fork", sessionId: "s1", body: { entryId: "n1" } }, SIGNAL);
    expect(seen[0]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/fork",
      method: "POST",
      body: { entryId: "n1" },
    });
    expect(result).toEqual({ ok: true, data: { sessionId: "forked-1", threadId: "th-1" } });
  });

  it("refuses a fork reply that names no session, because a fork with no id cannot be opened", async () => {
    const { transport } = gateway({ threadId: "th-1" });
    await expect(transport({ op: "fork", sessionId: "s1", body: { entryId: "n1" } }, SIGNAL)).resolves.toMatchObject({
      ok: false,
    });
  });

  it("clones the session and edits one message by entry id", async () => {
    const { seen, transport } = gateway({ sessionId: "cloned-1" });
    await expect(transport({ op: "clone", sessionId: "s1", body: {} }, SIGNAL)).resolves.toEqual({
      ok: true,
      data: { sessionId: "cloned-1" },
    });
    expect(seen[0]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/clone",
      method: "POST",
      body: {},
    });

    await transport(
      { op: "editMessage", sessionId: "s1", body: { entryId: "n1", role: "user", text: "rewritten" } },
      SIGNAL,
    );
    expect(seen[1]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/edit-message",
      method: "POST",
      body: { entryId: "n1", role: "user", text: "rewritten" },
    });
  });
});

describe("remote structured chat: session actions", () => {
  it("renames with a PUT, because a name is a field of the session rather than an event", async () => {
    const { seen, transport } = gateway(undefined);
    await transport({ op: "rename", sessionId: "s1", body: { name: "new name" } }, SIGNAL);
    expect(seen[0]).toEqual({
      path: "/api/v1/structured-chat/sessions/s1/name",
      method: "PUT",
      body: { name: "new name" },
    });
  });

  it("reads the session's totals", async () => {
    const { seen, transport } = gateway({
      turns: 4,
      totalTokens: 2000,
      inputTokens: 1200,
      outputTokens: 800,
      contextWindow: 128000,
      costUsd: 0.42,
    });
    const result = await transport({ op: "stats", sessionId: "s1", body: {} }, SIGNAL);
    expect(seen[0].method).toBe("GET");
    expect(result).toMatchObject({
      ok: true,
      data: { turns: 4, totalTokens: 2000, inputTokens: 1200, outputTokens: 800, contextWindow: 128000, costUsd: 0.42 },
    });
  });

  it("reloads, clears the queue and saves model preferences on their own routes", async () => {
    const { seen, transport } = gateway(undefined);
    await transport({ op: "reload", sessionId: "s1", body: {} }, SIGNAL);
    await transport({ op: "clearQueue", sessionId: "s1", body: {} }, SIGNAL);
    await transport(
      { op: "modelPreferences", sessionId: "s1", body: { favoriteModels: ["gpt-5"], scopedModels: ["gpt-5-mini"] } },
      SIGNAL,
    );
    expect(seen.map((row) => [row.method, row.path])).toEqual([
      ["POST", "/api/v1/structured-chat/sessions/s1/reload"],
      ["POST", "/api/v1/structured-chat/sessions/s1/queue/clear"],
      ["PUT", "/api/v1/structured-chat/sessions/s1/models/preferences"],
    ]);
    expect(seen[2].body).toEqual({ favoriteModels: ["gpt-5"], scopedModels: ["gpt-5-mini"] });
  });
});

describe("remote structured chat: media", () => {
  it("reads one content item by tool call and index, and decodes its bytes", async () => {
    const { seen, transport } = gateway({ mimeType: "image/png", dataBase64: "iVBORw0KGgo=" });
    const result = await transport(
      { op: "media", sessionId: "s1", body: { toolCallId: "call-1", contentIndex: 2 } },
      SIGNAL,
    );
    expect(seen[0]).toEqual({
      // One content item of one tool call: both ids are path segments, so a media read is a
      // cacheable GET rather than a body-carrying query.
      path: "/api/v1/structured-chat/sessions/s1/media/call-1/2",
      method: "GET",
      body: undefined,
    });
    expect(result).toEqual({ ok: true, data: { mimeType: "image/png", dataBase64: "iVBORw0KGgo=" } });
  });

  it("refuses a media reply with no bytes rather than drawing an empty picture", async () => {
    const { transport } = gateway({ mimeType: "image/png" });
    await expect(
      transport({ op: "media", sessionId: "s1", body: { toolCallId: "c", contentIndex: 0 } }, SIGNAL),
    ).resolves.toMatchObject({ ok: false });
  });
});

describe("structured chat over IPC carries the same new ops", () => {
  it("sends each new op through the one Tauri command with its own body", async () => {
    const seen: { op: StructuredChatOp; body: unknown }[] = [];
    const transport = desktopStructuredChatTransport(async (request) => {
      seen.push({ op: request.op, body: request.body });
      return undefined;
    });
    await transport(
      { op: "answerExtensionUi", sessionId: "s1", body: { requestId: "r1", confirmed: true } },
      SIGNAL,
    );
    await transport({ op: "navigateTree", sessionId: "s1", body: { targetId: "n1" } }, SIGNAL);
    await transport(
      { op: "editMessage", sessionId: "s1", body: { entryId: "n1", role: "user", text: "t" } },
      SIGNAL,
    );
    await transport({ op: "rename", sessionId: "s1", body: { name: "n" } }, SIGNAL);
    await transport({ op: "clearQueue", sessionId: "s1", body: {} }, SIGNAL);
    await transport(
      { op: "modelPreferences", sessionId: "s1", body: { favoriteModels: ["m"] } },
      SIGNAL,
    );
    expect(seen).toEqual([
      { op: "answerExtensionUi", body: { requestId: "r1", confirmed: true } },
      { op: "navigateTree", body: { targetId: "n1" } },
      { op: "editMessage", body: { entryId: "n1", role: "user", text: "t" } },
      { op: "rename", body: { name: "n" } },
      { op: "clearQueue", body: {} },
      { op: "modelPreferences", body: { favoriteModels: ["m"] } },
    ]);
  });
});
