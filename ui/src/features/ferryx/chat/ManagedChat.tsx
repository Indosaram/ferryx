import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AttachmentReceipt, ChatDraft, DeliveryReceipt, TargetRef } from "../../../lib/scopedContracts";
import { ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN, ATTACHMENT_MAX_TURN_BYTES } from "../../../lib/scopedContracts";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { draftKey, loadDraft, revokeDrafts, saveDraft } from "./drafts";
export interface ChatItem { id: string; kind: "message" | "tool"; text: string; }
export interface Question { id: string; question: string; isSecret?: boolean; options?: {label:string;description:string}[] | null; }
export interface Callback { id: string | number; threadId: string; turnId: string; kind: "approval" | "question"; text: string; questions?: Question[]; }
export interface ChatService {
 send(target: TargetRef, draft: ChatDraft, requestId: string): Promise<DeliveryReceipt>;
 stage(target: TargetRef, file: File, signal: AbortSignal): Promise<AttachmentReceipt>;
 reply(target: TargetRef, callback: Callback, result: unknown): Promise<void>;
 stop(target: TargetRef): Promise<void>;
}
export interface ManagedChatProps { target: TargetRef; kind: "managed" | "terminal"; service: ChatService; storage: Storage; items: ChatItem[]; callbacks: Callback[]; revoked?: boolean; terminal: ReactNode; }
export function ManagedChat(props: ManagedChatProps) {
 return <ChatSession key={draftKey(props.target)} {...props}/>;
}
function ChatSession({target,kind,service,storage,items,callbacks,revoked,terminal}:ManagedChatProps) {
 const [draft,setDraft]=useState<ChatDraft>(()=>revoked?{text:"",attachments:[]}:loadDraft(storage,target));
 const [error,setError]=useState<string>(); const [busy,setBusy]=useState(false);
 const [uploading,setUploading]=useState(false); const [previews,setPreviews]=useState<{id:string;url:string;name:string}[]>([]);
 const composing=useRef(false); const alive=useRef(true); const denied=useRef(revoked); denied.current=revoked;
 const upload=useRef<AbortController>(); const urls=useRef<string[]>([]);
 useEffect(()=>()=>{alive.current=false;upload.current?.abort();for(const url of urls.current)URL.revokeObjectURL(url);},[]);
 useEffect(()=>{if(revoked){revokeDrafts(storage);upload.current?.abort();setDraft({text:"",attachments:[]});setPreviews([]);for(const url of urls.current)URL.revokeObjectURL(url);urls.current=[];}},[revoked,storage]);
 function update(value:ChatDraft){setDraft(value);try{saveDraft(storage,target,value);}catch(e){setError(String(e));}}
 async function send(){
  if(busy||uploading||revoked||(!draft.text.trim()&&!draft.attachments.length))return;
  setBusy(true);setError(undefined);
  try{
   const receipt=await service.send(Object.freeze({...target}),structuredClone(draft),crypto.randomUUID());
   if(receipt.stage==="staged"||draftKey(receipt.target)!==draftKey(target))throw Error("Provider has not accepted this draft");
   if(alive.current&&!denied.current){update({text:"",attachments:[]});setPreviews([]);for(const url of urls.current)URL.revokeObjectURL(url);urls.current=[];}
  }catch(e){if(alive.current)setError(String(e));}finally{if(alive.current)setBusy(false);}
 }
 async function stage(file:File){
  if(uploading||busy||revoked)return;
  if(file.size>ATTACHMENT_MAX_FILE_BYTES||draft.attachments.length>=ATTACHMENT_MAX_FILES_PER_TURN||draft.attachments.reduce((n,a)=>n+a.sizeBytes,0)+file.size>ATTACHMENT_MAX_TURN_BYTES){setError("Attachment exceeds the turn limit");return;}
  const controller=new AbortController();upload.current=controller;setUploading(true);setError(undefined);
  try{
   const receipt=await service.stage(Object.freeze({...target}),file,controller.signal);
   if(controller.signal.aborted||!alive.current||denied.current)return;
   if(receipt.hostId!==target.hostId||receipt.sizeBytes!==file.size)throw Error("Attachment receipt does not match this target");
   update({...draft,attachments:[...draft.attachments,receipt]});
   const url=file.type.startsWith("image/")?URL.createObjectURL(file):"";if(url)urls.current.push(url);
   setPreviews(old=>[...old,{id:receipt.attachmentId,url,name:file.name}]);
  }catch(e){if(!controller.signal.aborted&&alive.current)setError(String(e));}finally{if(alive.current)setUploading(false);}
 }
 if(kind==="terminal")return <section data-testid="chat-mode" aria-label="Terminal">{terminal}</section>;
 return <section data-testid="chat-mode" aria-label="Managed Codex chat" className="flex min-h-0 min-w-0 flex-col gap-3 bg-background p-3 text-foreground">
  <header className="flex items-center justify-between gap-2"><h2 className="text-sm font-medium">Codex chat</h2><Button size="sm" variant="ghost" data-testid="agent-stop" disabled={revoked} onClick={()=>{void service.stop(target).catch(e=>setError(String(e)));}}>Stop</Button></header>
  <div role="log" aria-label="Conversation" className="min-h-0 flex-1 select-text overflow-auto">
   {!items.length&&<p className="text-sm text-muted-foreground">Send a message to start this conversation.</p>}
   {items.map(item=><article key={item.id} className="border-b border-border py-3"><h3 className="text-xs text-muted-foreground">{item.kind==="tool"?"Tool result":"Message"}</h3><pre className="whitespace-pre-wrap break-words font-sans text-sm">{item.text}</pre></article>)}
  </div>
  {callbacks.map(callback=><RequestCard key={JSON.stringify([callback.id,callback.threadId,callback.turnId])} callback={callback} disabled={!!revoked} respond={result=>service.reply(target,callback,result)}/>)}
  {error&&<p role="alert" className="text-sm text-destructive">{error}</p>}
  {revoked&&<p role="status" className="text-sm text-muted-foreground">Control access revoked. Protected drafts cleared.</p>}
  <form onSubmit={event=>{event.preventDefault();void send();}} className="flex flex-col gap-2">
   <label htmlFor={`composer-${encodeURIComponent(target.backendSessionId)}`} className="text-sm">Message</label>
   <textarea id={`composer-${encodeURIComponent(target.backendSessionId)}`} data-testid="chat-composer" rows={3} maxLength={65536} value={draft.text} disabled={busy||revoked||uploading} className="w-full resize-y rounded-md border border-input bg-background p-2 text-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring" onChange={e=>update({...draft,text:e.target.value})} onCompositionStart={()=>{composing.current=true;}} onCompositionEnd={()=>{composing.current=false;}} onKeyDown={e=>{if(e.key==="Enter"&&!e.shiftKey&&!composing.current&&!e.nativeEvent.isComposing){e.preventDefault();void send();}}}/>
   <ul className="flex flex-wrap gap-2">{draft.attachments.map(attachment=>{const preview=previews.find(p=>p.id===attachment.attachmentId);return <li key={attachment.attachmentId} className="flex items-center gap-2 rounded-md border border-border p-2 text-xs">{preview?.url&&<img src={preview.url} alt={preview.name} className="h-16 w-16 object-contain"/>}<span>{preview?.name??attachment.mediaType}</span><Button type="button" variant="ghost" size="sm" disabled={busy||revoked} onClick={()=>update({...draft,attachments:draft.attachments.filter(a=>a.attachmentId!==attachment.attachmentId)})}>Remove</Button></li>;})}</ul>
   <div className="flex flex-wrap items-center gap-2"><Input type="file" aria-label="Attach photo or file" data-testid="chat-attachment" accept="image/png,image/jpeg,image/webp,text/plain,application/pdf" disabled={busy||revoked||uploading} onChange={e=>{const file=e.target.files?.[0];e.target.value="";if(file)void stage(file);}} className="min-w-0 flex-1"/>{uploading&&<Button type="button" variant="ghost" onClick={()=>upload.current?.abort()}>Cancel upload</Button>}<Button type="submit" data-testid="chat-send" disabled={busy||revoked||uploading||(!draft.text.trim()&&!draft.attachments.length)}>{busy?"Sending...":"Send"}</Button></div>
  </form>
 </section>;
}
function RequestCard({callback,disabled,respond}:{callback:Callback;disabled:boolean;respond:(result:unknown)=>Promise<void>}){
 const [busy,setBusy]=useState(false);const [resolved,setResolved]=useState(false);const [error,setError]=useState<string>();const [answers,setAnswers]=useState<Record<string,string>>({});
 async function reply(result:unknown){if(busy||resolved||disabled)return;setBusy(true);try{await respond(result);setResolved(true);}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <fieldset disabled={disabled||busy||resolved} data-request-id={JSON.stringify(callback.id)} className="rounded-md border border-border bg-card p-3"><legend className="px-1 text-sm font-medium">{callback.kind==="approval"?"Approval required":"Question"}</legend><p className="select-text whitespace-pre-wrap break-words text-sm">{callback.text}</p>{callback.kind==="approval"?<div className="mt-3 flex gap-2"><Button data-testid="approval-accept" size="sm" onClick={()=>{void reply({decision:"accept"});}}>Accept</Button><Button data-testid="approval-decline" size="sm" variant="outline" onClick={()=>{void reply({decision:"decline"});}}>Decline</Button><Button size="sm" variant="ghost" onClick={()=>{void reply({decision:"cancel"});}}>Cancel turn</Button></div>:<div className="flex flex-col gap-2">{callback.questions?.map(q=><label key={q.id} className="text-sm">{q.question}{q.options?.map(option=><Button key={option.label} type="button" size="sm" variant={answers[q.id]===option.label?"secondary":"ghost"} onClick={()=>setAnswers(a=>({...a,[q.id]:option.label}))}>{option.label}</Button>)}<Input aria-label={q.question} type={q.isSecret?"password":"text"} value={answers[q.id]??""} onChange={e=>setAnswers(a=>({...a,[q.id]:e.target.value}))}/></label>)}<Button data-testid="question-submit" size="sm" disabled={!callback.questions?.every(q=>answers[q.id]?.trim())} onClick={()=>{void reply({answers:Object.fromEntries(Object.entries(answers).map(([id,answer])=>[id,{answers:[answer]}]))});}}>Submit answer</Button></div>}{resolved&&<p role="status" className="text-sm text-muted-foreground">Resolved</p>}{error&&<p role="alert" className="text-sm text-destructive">{error}</p>}</fieldset>;
}
