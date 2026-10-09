import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ManagedChat, type ChatService } from "./ManagedChat";
import type { DeliveryReceipt } from "../../../lib/scopedContracts";
import { draftKey } from "./drafts";
const target={hostId:"qa",ownerId:"daemon",epoch:"1",backendSessionId:"chat"};
describe("managed chat surface",()=>{
 it("preserves drafts on failed send, respects IME, revokes and renders terminal fallback",async()=>{
  localStorage.clear(); let sends=0;
  let reject!: (e:Error)=>void;
  const pending=new Promise<never>((_,r)=>{reject=r;});
  const service:ChatService={send:()=>{sends++;return pending;},stage:async()=>{throw Error("upload");},reply:async()=>{},stop:async()=>{}};
  const props={target,kind:"managed" as const,service,storage:localStorage,items:[],callbacks:[],terminal:<div data-testid="terminal-fallback"/>};
  const view=render(<ManagedChat {...props}/>);
  const composer=screen.getByTestId("chat-composer");
  fireEvent.change(composer,{target:{value:"retained"}});
  fireEvent.compositionStart(composer); fireEvent.keyDown(composer,{key:"Enter"}); expect(sends).toBe(0);
  fireEvent.compositionEnd(composer); fireEvent.click(screen.getByTestId("chat-send")); expect(sends).toBe(1);
  await act(async()=>{reject(Error("provider unavailable"));await pending.catch(()=>{});});
  expect((composer as HTMLTextAreaElement).value).toBe("retained");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("retained");
  view.rerender(<ManagedChat {...props} revoked/>);
  expect(localStorage.getItem(draftKey(target))).toBeNull();
  expect((screen.getByTestId("chat-composer") as HTMLTextAreaElement).value).toBe("");
  view.rerender(<ManagedChat {...props} kind="terminal"/>); expect(screen.getByTestId("terminal-fallback")).toBeTruthy();
 });
});

type SendCall = { requestId: string; draft: { text: string; attachments: unknown[] } };
type Deferred = { promise: Promise<DeliveryReceipt>; settled: boolean; resolve: (r: DeliveryReceipt) => void; reject: (e: Error) => void };
const openQueues: Deferred[][] = [];
function makeService() {
 const calls: SendCall[] = []; const queue: Deferred[] = [];
 openQueues.push(queue);
 const service: ChatService = {
  send: (_target, draft, requestId) => {
   calls.push({ requestId, draft: { text: draft.text, attachments: [...draft.attachments] } });
   let rawResolve!: (r: DeliveryReceipt) => void; let rawReject!: (e: Error) => void;
   const promise = new Promise<DeliveryReceipt>((res, rej) => { rawResolve = res; rawReject = rej; });
   const entry: Deferred = {
    promise,
    settled: false,
    resolve: (r) => { entry.settled = true; rawResolve(r); },
    reject: (e) => { entry.settled = true; rawReject(e); },
   };
   queue.push(entry);
   return promise;
  },
  stage: async () => { throw Error("upload"); },
  reply: async () => {},
  stop: async () => {},
 };
 return { service, calls, queue };
}
const propsFor = (service: ChatService) => ({ target, kind: "managed" as const, service, storage: localStorage, items: [], callbacks: [], terminal: <div data-testid="terminal-fallback" /> });
async function settle(d: Deferred, outcome: () => void) { await act(async () => { outcome(); await d.promise.catch(() => {}); }); }

afterEach(async () => {
 const unsettled = openQueues.splice(0).flat().filter((d) => !d.settled);
 await act(async () => {
  for (const d of unsettled) d.reject(Error("test teardown: unsettled request"));
  await Promise.all(unsettled.map((d) => d.promise.catch(() => {})));
 });
 cleanup();
 localStorage.clear();
});

describe("managed chat delivery receipts", () => {
 it("keeps an edit made during a pending request when the matching receipt settles", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "first" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  expect(calls).toHaveLength(1);
  expect(composer.disabled).toBe(false);
  fireEvent.change(composer, { target: { value: "first plus edit" } });
  await settle(queue[0], () => queue[0].resolve({ requestId: calls[0].requestId, target, stage: "accepted" }));
  expect(composer.value).toBe("first plus edit");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("first plus edit");
  expect(calls).toHaveLength(1);
 });
 it("does not clear on a receipt whose request id does not match and holds it", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "keep me" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].resolve({ requestId: "stale-req-000", target, stage: "accepted" }));
  expect(composer.value).toBe("keep me");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("keep me");
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  expect(calls).toHaveLength(1);
 });
 it("dispatches the held payload only on explicit retry, never on reconnect or readiness", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  const view = render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "queued" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].reject(Error("ambiguous network")));
  const heldId = calls[0].requestId;
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  expect(composer.value).toBe("queued");
  window.dispatchEvent(new Event("online"));
  view.rerender(<ManagedChat {...propsFor(service)} />);
  expect(calls).toHaveLength(1);
  fireEvent.click(screen.getByTestId("chat-retry"));
  expect(calls).toHaveLength(2);
  expect(calls[1].requestId).toBe(heldId);
  expect(calls[1].draft.text).toBe("queued");
  await settle(queue[1], () => queue[1].resolve({ requestId: heldId, target, stage: "accepted" }));
  expect(composer.value).toBe("");
  expect(screen.queryByTestId("chat-held")).toBeNull();
  expect(calls).toHaveLength(2);
 });
 it("does not dispatch a restored held request on a fresh mount, only on explicit retry", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  const first = render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "held across remount" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].reject(Error("ambiguous network")));
  const heldId = calls[0].requestId;
  expect(calls).toHaveLength(1);
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  first.unmount();
  render(<ManagedChat {...propsFor(service)} />);
  await act(async () => { await Promise.resolve(); });
  expect(calls).toHaveLength(1);
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  expect((screen.getByTestId("chat-composer") as HTMLTextAreaElement).value).toBe("held across remount");
  fireEvent.click(screen.getByTestId("chat-retry"));
  expect(calls).toHaveLength(2);
  expect(calls[1].requestId).toBe(heldId);
  expect(calls[1].draft.text).toBe("held across remount");
  await settle(queue[1], () => queue[1].resolve({ requestId: heldId, target, stage: "accepted" }));
  expect(calls).toHaveLength(2);
  expect(screen.queryByTestId("chat-held")).toBeNull();
 });
 it("keeps a newer edit when a held retry settles and never repurposes the held request id", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "original" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].reject(Error("timeout")));
  const heldId = calls[0].requestId;
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  fireEvent.change(composer, { target: { value: "original v2" } });
  const stored = JSON.parse(localStorage.getItem(draftKey(target))!);
  expect(stored.held?.requestId).toBe(heldId);
  expect(stored.held?.payload.text).toBe("original");
  fireEvent.click(screen.getByTestId("chat-retry"));
  expect(calls[1].requestId).toBe(heldId);
  expect(calls[1].draft.text).toBe("original");
  await settle(queue[1], () => queue[1].resolve({ requestId: heldId, target, stage: "accepted" }));
  expect(composer.value).toBe("original v2");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("original v2");
  expect(screen.queryByTestId("chat-held")).toBeNull();
  fireEvent.click(screen.getByTestId("chat-send"));
  expect(calls).toHaveLength(3);
  expect(calls[2].requestId).not.toBe(heldId);
  expect(calls[2].draft.text).toBe("original v2");
  await settle(queue[2], () => queue[2].resolve({ requestId: calls[2].requestId, target, stage: "accepted" }));
  expect(composer.value).toBe("");
 });
 it("shows the staged receipt distinctly and holds the draft instead of clearing", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "staged text" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].resolve({ requestId: calls[0].requestId, target, stage: "staged" }));
  expect(composer.value).toBe("staged text");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("staged text");
  expect(screen.getByTestId("chat-delivery-stage").textContent ?? "").toContain("Staged");
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  expect(calls).toHaveLength(1);
 });
 it("does not clear on a receipt bound to a replaced target epoch", async () => {
  localStorage.clear();
  const { service, calls, queue } = makeService();
  render(<ManagedChat {...propsFor(service)} />);
  const composer = screen.getByTestId("chat-composer") as HTMLTextAreaElement;
  fireEvent.change(composer, { target: { value: "epoch text" } });
  fireEvent.click(screen.getByTestId("chat-send"));
  await settle(queue[0], () => queue[0].resolve({ requestId: calls[0].requestId, target: { ...target, epoch: "999" }, stage: "accepted" }));
  expect(composer.value).toBe("epoch text");
  expect(JSON.parse(localStorage.getItem(draftKey(target))!).text).toBe("epoch text");
  expect(screen.getByTestId("chat-held")).toBeTruthy();
  expect(calls).toHaveLength(1);
 });
 it("exposes explicit user Start action invoking service.start with zero automatic launch", async () => {
  localStorage.clear();
  let startCalled = 0;
  const service: ChatService = {
   send: async () => ({ requestId: "r", target, stage: "accepted" }),
   stage: async () => { throw Error("upload"); },
   reply: async () => {},
   stop: async () => {},
   start: async (t, p) => {
    startCalled++;
    return { target: t, provider: p ?? "codex", threadId: "th-100" };
   },
  };
  render(<ManagedChat {...propsFor(service)} />);
  expect(startCalled).toBe(0);
  const startButton = screen.getByTestId("agent-start");
  expect(startButton).toBeTruthy();
  fireEvent.click(startButton);
  await act(async () => { await Promise.resolve(); });
  expect(startCalled).toBe(1);
 });
});
