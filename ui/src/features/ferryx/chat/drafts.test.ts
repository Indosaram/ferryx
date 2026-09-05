import { describe, it, expect } from "vitest";
import { draftKey, loadDraft, saveDraft, revokeDrafts } from "./drafts";
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
