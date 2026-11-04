import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export function Detached({ scope }: { scope?: string }) {
  const [sessions, setSessions] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      setSessions(await invoke<string[]>("cmd_panes", { request: { action: "list" } }));
      setError(null);
    } catch (failure) { setError(String(failure)); }
  }, []);
  useEffect(() => {

    const subscription = listen("LayoutChanged", () => { void refresh(); });
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
    {sessions.map(session => <div key={session} className="flex items-center justify-between gap-2">
      <span className="truncate" title={session}>{session}</span>
      <button aria-label={`Attach ${session}`} onClick={() => { void attach(session); }}>Attach</button>
    </div>)}
  </section>;
}
