import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ManagedChat, type ChatService } from "./ManagedChat";
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
