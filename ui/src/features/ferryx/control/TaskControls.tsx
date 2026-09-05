import { useState } from "react";
import type { TargetRef, JsonValue } from "../../../lib/scopedContracts";
import { ControlClient } from "./client";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
/** Explicit target only. Failed requests keep the same request ID for a safe retry. */
export function TaskControls({client,target,hostId,workspaceId,canControl,onChanged}:{client:ControlClient;target:TargetRef|null;hostId:string;workspaceId:string;canControl:boolean;onChanged:()=>void}) {
  const [busy,setBusy]=useState(false),[error,setError]=useState<string|null>(null),[text,setText]=useState("");
  const [request,setRequest]=useState<{key:string;id:string}|null>(null);
  async function send(operation:"create"|"start"|"prompt"|"stop") {
    const params:JsonValue=operation==="create"?{hostId,workspaceId}:operation==="prompt"?{text}:{};
    const key=JSON.stringify([operation,target,params]);
    const id=request?.key===key?request.id:crypto.randomUUID();
    setRequest({key,id});setBusy(true);setError(null);
    try {await client.mutate(operation,id,params,operation==="create"?undefined:target!);setRequest(null);if(operation==="prompt")setText("");onChanged();}
    catch(e){setError(e instanceof Error?e.message:"Request failed");}
    finally{setBusy(false);}
  }
  return <section aria-label="Task controls" aria-busy={busy} className="space-y-2">
    {!canControl && <p className="text-xs text-muted-foreground">View-only access</p>}
    {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
    <div className="flex flex-wrap gap-2">
      <Button data-testid="task-create" size="sm" disabled={!canControl||busy||!hostId||!workspaceId} onClick={()=>void send("create")}>Create task</Button>
      <Button data-testid="agent-start" variant="outline" size="sm" disabled={!canControl||busy||!target} onClick={()=>void send("start")}>Start agent</Button>
      <Button data-testid="agent-stop" variant="outline" size="sm" disabled={!canControl||busy||!target} onClick={()=>void send("stop")}>Stop agent</Button>
    </div>
    <form className="flex gap-2" onSubmit={e=>{e.preventDefault();if(canControl&&!busy&&target&&text)void send("prompt");}}>
      <Input aria-label="Agent input" value={text} onChange={e=>setText(e.target.value)} disabled={!canControl||busy||!target} />
      <Button type="submit" disabled={!canControl||busy||!target||!text}>Send</Button>
    </form>
  </section>;
}
