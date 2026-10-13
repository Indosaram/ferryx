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
 it("falls back to an empty draft for corrupt or wrongly shaped entries", () => {
  for (const raw of ["{not json", "null", "42", '{"text":1,"attachments":[]}', '{"text":"x"}']) {
   localStorage.setItem(draftKey(a), raw);
   expect(loadDraft(localStorage,a)).toEqual({text:"",attachments:[]});
  }
 });
});
