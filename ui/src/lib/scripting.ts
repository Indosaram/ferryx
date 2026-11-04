import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ScriptStatus = { enabled: boolean; log: string; bindings: { keys: string; override: boolean }[] };
export type ScriptAction = { action: "status" | "reload" } | { action: "setEnabled"; enabled: boolean } | { action: "key"; keys: string; session: string };
let status: ScriptStatus = { enabled: false, log: "", bindings: [] };
let currentSession: string | null = null;
export async function scriptControl(action: ScriptAction): Promise<ScriptStatus> {
  status = await invoke<ScriptStatus>("cmd_script", { action });
  return status;
}
export function scriptChord(event: KeyboardEvent): string {
  const modifiers = [event.altKey && "alt", event.metaKey && "cmd", event.ctrlKey && "ctrl", event.shiftKey && "shift"].filter(Boolean);
  return [...modifiers, event.key.toLowerCase()].join("+");
}
export function dispatchScriptKey(event: KeyboardEvent, override: boolean): boolean {
  if (!currentSession || !status.enabled || event.defaultPrevented || event.isComposing || event.repeat) return false;
  if (event.target instanceof Element && event.target.closest("input,textarea,select,[contenteditable=true],[data-shortcut-recorder]")) return false;
  const keys = scriptChord(event);
  if (!status.bindings.some((binding) => binding.keys === keys && binding.override === override)) return false;
  event.preventDefault();
  event.stopPropagation();
  void scriptControl({ action: "key", keys, session: currentSession }).catch((error: unknown) => {
    window.dispatchEvent(new CustomEvent("ferryx-script-error", { detail: error }));
  });
  return true;
}
export function useScripting(session: string | null) {
  useEffect(() => { currentSession = session; return () => { currentSession = null; }; }, [session]);
  useEffect(() => {
    const refresh = () => { void scriptControl({ action: "status" }).catch((error: unknown) => {
      window.dispatchEvent(new CustomEvent("ferryx-script-error", { detail: error }));
    }); };
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
}
