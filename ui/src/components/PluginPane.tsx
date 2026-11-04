import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** An opaque-origin, scriptless child browsing context. No app IPC or navigation privileges. */
export function PluginPane({ plugin, view, visible }: { plugin: string; view: string; visible: boolean }) {
  const [document, setDocument] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      try {
        const html = await invoke<string>("cmd_plugin_view", { plugin, view });
        if (!disposed) { setDocument(html); setError(null); }
      } catch (cause) { if (!disposed) { setDocument(""); setError(String(cause)); } }
    };
    void listen<{ plugin: string; view: string }>("PluginViewChanged", ({ payload }) => {
      if (payload.plugin === plugin && payload.view === view) void refresh();
    }).then((stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      void refresh();
    }).catch((cause: unknown) => { if (!disposed) setError(String(cause)); });
    return () => { disposed = true; unlisten?.(); };
  }, [plugin, view]);
  return <div style={{ width: "100%", height: "100%", visibility: visible ? "visible" : "hidden" }}>
    {error && <div role="status">{error}</div>}
    <iframe title={`${plugin}: ${view}`} sandbox="" referrerPolicy="no-referrer" srcDoc={document}
      style={{ border: 0, width: "100%", height: "100%", pointerEvents: visible ? "auto" : "none" }} />
  </div>;
}
