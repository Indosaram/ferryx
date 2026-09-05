import { useEffect, useState } from "react";
import type { InventorySnapshot, ScopeEvent } from "../../../lib/scopedContracts";
import { ControlClient, InventoryClientState, type Agent } from "./client";
/** Subscribe before bootstrap. Transport must deliver reset/gap via onReset. */
export function useInventory(client:ControlClient, subscribe:(onEvent:(event:ScopeEvent<Agent>)=>void,onReset:()=>void)=>()=>void) {
  const [state]=useState(()=>new InventoryClientState());
  const [snapshot,setSnapshot]=useState<InventorySnapshot<Agent>>(state.snapshot);
  const [error,setError]=useState<string|null>(null);
  const [loading,setLoading]=useState(true);
  useEffect(()=>{
    const abort=new AbortController();
    let generation=0,bootstrapping=true;
    let queued:ScopeEvent<Agent>[]=[];
    const reset=()=>{
      const current=++generation;bootstrapping=true;queued=[];setLoading(true);
      void client.list(abort.signal).then(value=>{
        if(abort.signal.aborted||current!==generation)return;
        state.bootstrap(value);bootstrapping=false;
        const pending=queued;queued=[];
        for(const event of pending){if(event.revision<=state.snapshot.revision)continue;if(!state.delta(event)){reset();return;}}
        setSnapshot(state.snapshot);setError(null);setLoading(false);
      }).catch(e=>{if(!abort.signal.aborted&&current===generation){setError(e instanceof Error?e.message:"Inventory unavailable");setLoading(false);}});
    };
    const unsubscribe=subscribe(event=>{
      if(bootstrapping){queued.push(event);return;}
      if(event.revision<=state.snapshot.revision)return;
      if(!state.delta(event)){reset();return;}
      setSnapshot(state.snapshot);
    },reset);
    reset();
    return()=>{abort.abort();unsubscribe();};
  },[client,state,subscribe]);
  return {snapshot,error,loading,state};
}
