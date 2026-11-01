import { trace } from "./autopilot-redraft-seams/trace.ts";
// Every fetch is recorded; only the old card lookup gets an empty, harmless Slack reply.
globalThis.fetch = (async (url: string | URL | Request) => {
  const method = String(url).split("/").pop()!;
  trace("network", method);
  if (method === "conversations.history") return Response.json({ ok: true, messages:
    process.env.REDRAFT_CARD ? [{ ts: "100.1", text: "card", blocks: [{ type: "actions", elements: [] }] }] : [] });
  if (method === "chat.update" && process.env.REDRAFT_CARD) return Response.json({ ok: true });
  throw new Error(`Unexpected network request: ${method}`);
}) as typeof fetch;
