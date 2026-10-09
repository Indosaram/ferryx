import { describe, it, expect } from "vitest";
import { draftKey, loadDraft, saveDraft, revokeDrafts } from "./drafts";
import type { ChatDraft } from "../../../lib/scopedContracts";
const a = {hostId:"one",ownerId:"daemon",epoch:"1",backendSessionId:"same"};
const b = {...a,hostId:"two"};
describe("protected chat drafts", () => {
 it("isolates the full immutable target and revokes every protected key", () => {
  localStorage.clear();
  saveDraft(localStorage,a,{text:"private A",attachments:[]});
  expect(loadDraft(localStorage,b).text).toBe("");
  expect(draftKey(a)).not.toBe(draftKey({...a,epoch:"2"}));
  saveDraft(localStorage,b,{text:"private B",attachments:[]});
  localStorage.setItem("ferryx.other","keep");
  revokeDrafts(localStorage);
  expect(loadDraft(localStorage,a).text).toBe("");
  expect(loadDraft(localStorage,b).text).toBe("");
  expect(localStorage.getItem("ferryx.other")).toBe("keep");
 });
});

type StoredHeld = { requestId: string; payload: ChatDraft };
type StoredDraft = ChatDraft & { held?: StoredHeld };
const saveWithHeld = saveDraft as unknown as (s: Storage, t: typeof a, d: ChatDraft, h?: StoredHeld) => void;
const readStored = (t: typeof a): StoredDraft => loadDraft(localStorage, t) as StoredDraft;

describe("held retry envelopes", () => {
 it("binds the draft key to every immutable target field", () => {
  expect(draftKey(a)).not.toBe(draftKey({ ...a, hostId: "host-x" }));
  expect(draftKey(a)).not.toBe(draftKey({ ...a, ownerId: "other" }));
  expect(draftKey(a)).not.toBe(draftKey({ ...a, epoch: "2" }));
  expect(draftKey(a)).not.toBe(draftKey({ ...a, backendSessionId: "session-x" }));
 });
 it("persists a held retry payload that a later edit cannot repurpose", () => {
  localStorage.clear();
  const held: StoredHeld = { requestId: "req-1", payload: { text: "original", attachments: [] } };
  saveWithHeld(localStorage, a, { text: "original", attachments: [] }, held);
  const stored = readStored(a);
  expect(stored.held?.requestId).toBe("req-1");
  expect(stored.held?.payload.text).toBe("original");
  saveWithHeld(localStorage, a, { text: "edited", attachments: [] }, stored.held);
  const edited = readStored(a);
  expect(edited.text).toBe("edited");
  expect(edited.held?.requestId).toBe("req-1");
  expect(edited.held?.payload.text).toBe("original");
 });
 it("reads a legacy bare draft as unheld and revokes held envelopes with the prefix sweep", () => {
  localStorage.clear();
  localStorage.setItem(draftKey(a), JSON.stringify({ text: "legacy", attachments: [] }));
  expect(readStored(a).text).toBe("legacy");
  expect(readStored(a).held).toBeUndefined();
  saveWithHeld(localStorage, b, { text: "held B", attachments: [] }, { requestId: "req-9", payload: { text: "held B", attachments: [] } });
  revokeDrafts(localStorage);
  expect(readStored(a).text).toBe("");
  expect(readStored(b).text).toBe("");
  expect(readStored(b).held).toBeUndefined();
 });
});
