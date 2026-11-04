import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LayoutReply, DaemonLayout } from "../lib/layoutSync";

export function Detached({ scope }: { scope?: string }) {
  const [sessions, setSessions] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [offers, setOffers] = useState<Record<string, DaemonLayout>>({});
  useEffect(() => {
    const refreshOffers = async () => {
      try {
        const reply = await invoke<LayoutReply>("cmd_layout_get");
        if (reply?.type === "ready") setOffers(reply.view.layout.foreignOffers ?? {});
      } catch (failure) { setError(String(failure)); }
    };
    void refreshOffers();
    const subscription = listen("LayoutChanged", () => { void refreshOffers(); });
    return () => { void subscription.then(unlisten => unlisten(), failure => setError(String(failure))); };
  }, []);
  const openOffer = async (machineId: string, revision: number) => {
    try {
      await invoke("cmd_layout_import", { machineId, revision });
      setError(null);
    } catch (failure) { setError(String(failure)); }
  };
  const refresh = useCallback(async () => {
    try {
      const inventory = await invoke<string[]>("cmd_panes", { request: { action: "list" } });
      if (!Array.isArray(inventory)) throw new Error("Invalid detached inventory");
      setSessions(inventory);
      setError(null);
    } catch (failure) { setError(String(failure)); }
  }, []);
  useEffect(() => {

    const subscription = listen("LayoutChanged", () => { void refresh(); }).catch(failure => { setError(String(failure)); return () => {}; });
    void refresh();
    return () => { void subscription.then(unlisten => unlisten()); };
  }, [refresh]);
  const attach = async (session: string) => {
    try {
      setSessions(await invoke<string[]>("cmd_panes", { request: { action: "attach", session, window: null, split: null }, ...(scope ? { scope } : {}) }));
      setError(null);
    } catch (failure) { setError(String(failure)); }
  };
  return <section aria-label="Detached" className="px-3 py-2 text-xs">
    <div className="flex items-center justify-between"><h2>Detached</h2><button onClick={() => { void refresh(); }} aria-label="Refresh detached sessions">Refresh</button></div>
    {error && <p role="alert">{error}</p>}
    {Object.entries(offers).map(([machine, layout]) => <button key={machine}
      onClick={() => { void openOffer(machine, layout.revision); }}>Open layout from {machine}</button>)}
    {sessions.map(session => <div key={session} className="flex items-center justify-between gap-2">
      <span className="truncate" title={session}>{session}</span>
      <button aria-label={`Attach ${session}`} onClick={() => { void attach(session); }}>Attach</button>
    </div>)}
  </section>;
}
