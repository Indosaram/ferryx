import { expect, it } from "vitest";
import { DesignSession, type Capture, type DesignBridge } from "./session";
import type { GuestEvent } from "./model";
const identity = { browserId: "b", webviewLabel: "v", generation: "1", operationId: "o", viewportRevision: 1 };
const target = { hostId: "remote", ownerId: "owner", epoch: "1", backendSessionId: "s" };
const capture: Capture = { selection: { identity, mode: "rectangle", rect: { x: 0, y: 0, width: 1, height: 1 }, viewport: { width: 1, height: 1, dpr: 1, zoom: 1 }, url: "https://example.invalid", title: "fixture" }, png: new Uint8Array([137,80,78,71]), sha256: "abc" };
it("freezes explicit target and exact bytes; remote failure preserves immutable draft; sends once", async () => {
  let delivered = 0; let fail = true; let staged: Uint8Array | undefined;
  const bridge: DesignBridge = { subscribe: async () => () => {}, begin: async () => {}, cancel: async () => {}, capture: async () => capture, validate: async () => {} };
  const session = new DesignSession(bridge, { stage: async (t, bytes, hash) => { staged = bytes; return { hostId: t.hostId, attachmentId: "attachment", sha256: hash, sizeBytes: bytes.length, mediaType: "image/png" }; } }, {
    validateTarget: async () => {}, deliver: async (draft, requestId) => { delivered++; if (fail) throw new Error("REMOTE_UNAVAILABLE"); return { target: draft.target, requestId, stage: "accepted" }; },
  });
  session.capture = capture;
  const chosen = { ...target }; await session.confirm(chosen, "note", "request"); chosen.hostId = "wrong";
  expect(staged).toEqual(capture.png); expect(session.draft?.target.hostId).toBe("remote");
  expect(Object.isFrozen(session.draft)).toBe(true);
  await expect(session.send()).rejects.toThrow("REMOTE_UNAVAILABLE"); expect(session.draft?.note).toBe("note");
  fail = false; const [a,b] = await Promise.all([session.send(), session.send()]); expect(a).toEqual(b); expect(delivered).toBe(2);
});
it("subscribes before arm and discards capture callback after invalidation", async () => {
  let listener: ((event: GuestEvent) => void) | undefined;
  let resolve!: (value: Capture) => void;
  const pending = new Promise<Capture>(r => { resolve = r; });
  const bridge: DesignBridge = { subscribe: async fn => { listener = fn; return () => { listener = undefined; }; }, begin: async () => { listener!({ type: "selected", selection: capture.selection }); }, cancel: async () => {}, capture: () => pending, validate: async () => {} };
  const session = new DesignSession(bridge, { stage: async () => { throw new Error("unused"); } }, { validateTarget: async () => {}, deliver: async () => { throw new Error("unused"); } });
  await session.begin(identity, "element"); expect(listener).toBeDefined();
  await session.cancel(); resolve(capture); await pending; await Promise.resolve();
  expect(session.capture).toBeUndefined();
});
it("cancel preserves a confirmed draft but prevents sending that cancelled selection", async () => {
  let writes = 0;
  const session = new DesignSession({ subscribe: async () => () => {}, begin: async () => {}, cancel: async () => {}, capture: async () => capture, validate: async () => {} }, { stage: async () => ({hostId:target.hostId,attachmentId:"a",sha256:capture.sha256,sizeBytes:capture.png.length,mediaType:"image/png"}) }, { validateTarget: async () => {}, deliver: async (draft, requestId) => { writes++; return { target:draft.target, requestId, stage:"accepted" }; } });
  session.capture = capture; await session.confirm(target,"keep","cancelled-request");
  await session.cancel(); await expect(session.send()).rejects.toThrow("TARGET_EXPIRED");
  expect(session.draft?.note).toBe("keep"); expect(writes).toBe(0);
});
