import { describe, expect, it } from "vitest";
import { referenceHistoryQuery } from "../agentConversation";
import {
  applyReferenceHistoryDelta,
  createReferenceHistorySync,
  referenceHistoryAnchorFromPage,
  referenceHistoryTurnFingerprints,
  referenceTurnIsOpen,
  referenceTurnWireSlices,
  type HistoryReadTicket,
} from "./referenceHistorySync";
import type { ReferenceHistoryPage, ReferenceTargetRef, ReferenceTurn } from "./referenceTypes";

const target: ReferenceTargetRef = {
  target: { hostId: "local", ownerId: "owner-pub-1", epoch: "7", backendSessionId: "sess-main" },
};

function turn(id: string, role: "user" | "assistant", text: string, extra: Partial<ReferenceTurn> = {}): ReferenceTurn {
  return { id, role, parts: [{ kind: "text", text }], ...extra };
}

function page(turns: readonly ReferenceTurn[], generation = "stream-1:inc-2"): ReferenceHistoryPage {
  return {
    source: "claude-transcript",
    availability: "native",
    turns,
    hasMore: false,
    generation,
    cursor: null,
  };
}

function frame(sessionId = "sess-main", changeSeq = "1") {
  return {
    type: "referenceHistoryChanged",
    target: { hostId: "local", ownerId: "owner-pub-1", epoch: "7", backendSessionId: sessionId },
    changeSeq,
  };
}

function fingerprints(body: ReferenceHistoryPage) {
  return referenceHistoryTurnFingerprints(JSON.stringify(body), body.turns);
}

describe("reference history sync", () => {
  it("keeps a rev2 poll free of after when the capability is absent", () => {
    const sync = createReferenceHistorySync();
    sync.bind(target);
    const poll = sync.begin("poll");
    expect(poll?.after).toBeNull();
    expect(sync.onFrame(frame())).toBeNull();
    expect(sync.inFlight).toBe(true);
    sync.settle(poll as HistoryReadTicket, { kind: "page", page: page([turn("u1", "user", "hi")]) });
    expect(sync.anchor).toBeNull();
    expect(sync.begin("poll")?.after).toBeNull();
    const query = new URLSearchParams(referenceHistoryQuery({ target, cursor: { streamId: "s", offset: 1 } }));
    expect(query.has("after")).toBe(false);
    expect(query.get("cursor")).toBe("1");
  });

  it("encodes after and omits cursor when both would be sent", () => {
    const anchor = {
      streamId: "stream-1",
      generation: "stream-1:inc-2",
      anchorTurnId: "a1",
      previousTurnFingerprint: null,
    };
    const query = new URLSearchParams(referenceHistoryQuery({
      target,
      after: anchor,
      cursor: { streamId: "stream-1", offset: 4 },
    }));
    expect(query.has("cursor")).toBe(false);
    expect(JSON.parse(query.get("after") ?? "")).toEqual(anchor);
  });

  it("starts one delta fetch for a matching frame and ignores a foreign or malformed frame", () => {
    const sync = createReferenceHistorySync();
    sync.setDeltaEnabled(true);
    sync.bind(target);
    const body = page([turn("u1", "user", "hi"), turn("a1", "assistant", "old")]);
    const poll = sync.begin("poll");
    sync.settle(poll as HistoryReadTicket, { kind: "page", page: body, fingerprints: fingerprints(body) });
    expect(sync.anchor?.anchorTurnId).toBe("a1");
    expect(sync.anchor?.previousTurnFingerprint).toMatch(/^[0-9a-f]{64}$/);
    const ticket = sync.onFrame(frame());
    expect(ticket?.after?.anchorTurnId).toBe("a1");
    expect(sync.onFrame(frame("other-session"))).toBeNull();
    expect(sync.onFrame("{")).toBeNull();
    expect(sync.onFrame({ type: "referenceHistoryChanged", changeSeq: 1 })).toBeNull();
    expect(sync.inFlight).toBe(true);
  });

  it("coalesces frames during a read into one follow-up", () => {
    const sync = createReferenceHistorySync();
    sync.setDeltaEnabled(true);
    sync.bind(target);
    const body = page([turn("a1", "assistant", "old")]);
    sync.settle(sync.begin("poll") as HistoryReadTicket, { kind: "page", page: body, fingerprints: fingerprints(body) });
    const first = sync.onFrame(frame());
    expect(sync.onFrame(frame("sess-main", "2"))).toBeNull();
    expect(sync.onFrame(frame("sess-main", "3"))).toBeNull();
    expect(sync.dirty).toBe(true);
    expect(sync.inFlight).toBe(true);
    const delta = {
      mode: "delta" as const,
      source: "claude-transcript" as const,
      generation: "stream-1:inc-2",
      replaceFromTurnId: "a1",
      turns: [turn("a1", "assistant", "new"), turn("a2", "assistant", "more")],
      nextAnchor: {
        streamId: "stream-1",
        generation: "stream-1:inc-2",
        anchorTurnId: "a2",
        previousTurnFingerprint: "fp",
      },
      tailOpen: true,
    };
    const settled = sync.settle(first as HistoryReadTicket, { kind: "delta", delta });
    expect(settled.kind).toBe("page");
    if (settled.kind !== "page") return;
    expect(settled.page.turns.map((item) => item.id)).toEqual(["a1", "a2"]);
    expect(settled.followUp?.after?.anchorTurnId).toBe("a2");
    expect(sync.inFlight).toBe(true);
  });

  it("replaces the open turn and appends later turns", () => {
    const held = [turn("u1", "user", "hi"), turn("a1", "assistant", "old")];
    const applied = applyReferenceHistoryDelta(held, {
      mode: "delta",
      source: "claude-transcript",
      generation: "stream-1:inc-2",
      replaceFromTurnId: "a1",
      turns: [turn("a1", "assistant", "new"), turn("a2", "assistant", "more")],
      nextAnchor: { streamId: "stream-1", generation: "stream-1:inc-2", anchorTurnId: "a2", previousTurnFingerprint: "fp" },
      tailOpen: true,
    });
    expect(applied?.turns.map((item) => item.id)).toEqual(["u1", "a1", "a2"]);
    expect(applied?.turns[1]?.parts[0]).toMatchObject({ text: "new" });
  });

  it("chooses the oldest open turn, else the newest closed turn", () => {
    const running = {
      id: "t1",
      role: "assistant" as const,
      parts: [{ kind: "tool" as const, name: "read", summary: "", input: "", output: "", callId: "c1", status: "running" as const }],
    };
    const later = turn("a2", "assistant", "after");
    const anchor = referenceHistoryAnchorFromPage(page([turn("u1", "user", "hi"), running, later]));
    expect(anchor?.anchorTurnId).toBe("t1");
    const closed = referenceHistoryAnchorFromPage(page([
      turn("u1", "user", "hi"),
      {
        id: "a1",
        role: "assistant",
        parts: [{ kind: "tool", name: "read", summary: "", input: "", output: "ok", callId: "c1", status: "completed" }],
      },
    ]));
    expect(closed?.anchorTurnId).toBe("a1");
    expect(closed?.previousTurnFingerprint).toMatch(/^[0-9a-f]{64}$/);
  });

  it("turns a reset or a delta parse failure into one full read", () => {
    const sync = createReferenceHistorySync();
    sync.setDeltaEnabled(true);
    sync.bind(target);
    const body = page([turn("a1", "assistant", "old")]);
    sync.settle(sync.begin("poll") as HistoryReadTicket, { kind: "page", page: body, fingerprints: fingerprints(body) });
    const ticket = sync.onFrame(frame()) as HistoryReadTicket;
    const reset = sync.settle(ticket, { kind: "reset" });
    expect(reset.kind).toBe("full");
    if (reset.kind !== "full") return;
    expect(reset.ticket.after).toBeNull();
    sync.settle(reset.ticket, { kind: "page", page: body, fingerprints: fingerprints(body) });
    const again = sync.onFrame(frame()) as HistoryReadTicket;
    const malformed = sync.settle(again, { kind: "malformed" });
    expect(malformed.kind).toBe("full");
    if (malformed.kind !== "full") return;
    expect(malformed.ticket.after).toBeNull();
  });

  it("drops a stale response after the target changes or a newer read starts", () => {
    const sync = createReferenceHistorySync();
    sync.setDeltaEnabled(true);
    sync.bind(target);
    const ticket = sync.begin("poll") as HistoryReadTicket;
    sync.bind({ target: { ...target.target, backendSessionId: "other" } });
    expect(sync.settle(ticket, { kind: "page", page: page([turn("a1", "assistant", "late")]) }).kind).toBe("stale");
    expect(sync.anchor).toBeNull();
    const live = sync.begin("poll") as HistoryReadTicket;
    sync.invalidateFlight();
    expect(sync.settle(live, { kind: "page", page: page([turn("a1", "assistant", "late")]) }).kind).toBe("stale");
  });

  it("slices turn JSON without reordering keys", () => {
    const body = JSON.stringify({
      source: "claude-transcript",
      turns: [{ role: "user", id: "u1", parts: [{ kind: "text", text: "hi" }] }],
      hasMore: false,
    });
    expect(referenceTurnWireSlices(body)).toEqual([
      JSON.stringify({ role: "user", id: "u1", parts: [{ kind: "text", text: "hi" }] }),
    ]);
  });

  it("a submitted user turn is never an open tail", () => {
    const submitted = turn("u1", "user", "submitted");
    expect(referenceTurnIsOpen(submitted, true)).toBe(false);
    expect(referenceTurnIsOpen({ ...submitted, parts: [{ kind: "thinking", text: "secret" }] }, true)).toBe(false);
    expect(referenceTurnIsOpen(turn("a1", "assistant", "still growing"), true)).toBe(true);
    expect(referenceTurnIsOpen(turn("a1", "assistant", "closed"), false)).toBe(false);
  });
});

