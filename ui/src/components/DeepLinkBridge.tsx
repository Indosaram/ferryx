import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { routeDeepLink, type DeepLinkActions, type DeepLinkRoute } from "../lib/deepLinks";

export function DeepLinkBridge({ actions }: { actions: Omit<DeepLinkActions, "confirm"> }) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;
  const [pending, setPending] = useState<DeepLinkRoute | null>(null);
  const resolveRef = useRef<((accepted: boolean) => void) | null>(null);
  useEffect(() => {
    let disposed = false;
    let chain = Promise.resolve();
    const drain = () => {
      chain = chain.then(async () => {
        if (disposed) return;
        const links = await invoke<string[]>("cmd_deep_link_take_pending");
        for (const link of links) {
          if (disposed) return;
          await routeDeepLink(link, {
            ...actionsRef.current,
            confirm: (route) => new Promise<boolean>((resolve) => {
              resolveRef.current = resolve;
              setPending(route);
            }),
          });
        }
      }).catch((error: unknown) => {
        if (!disposed) actionsRef.current.toast(`Could not receive Ferryx link: ${String(error)}`);
      });
    };
    // Listen before draining so cold-start and warm links cannot fall between the two.
    const subscription = listen("deep_link_received", drain);
    void subscription.then((unlisten) => {
      if (disposed) unlisten();
      else drain();
    }).catch((error: unknown) => {
      if (!disposed) actionsRef.current.toast(`Could not listen for Ferryx links: ${String(error)}`);
    });
    return () => {
      disposed = true;
      resolveRef.current?.(false);
      void subscription.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, []);
  const answer = (accepted: boolean) => {
    const resolve = resolveRef.current;
    resolveRef.current = null;
    setPending(null);
    resolve?.(accepted);
  };
  useEffect(() => {
    if (!pending) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); answer(false); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [pending]);
  if (!pending) return null;
  const destination = pending.kind === "settings" ? `Settings / ${pending.section}`
    : pending.kind === "onboarding" ? `Onboarding / ${pending.step}` : `Workspace / ${pending.workspaceId}`;
  return <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
    <section role="dialog" aria-modal="true" aria-labelledby="deep-link-title" className="max-w-md rounded-xl border border-border bg-card p-4 text-foreground shadow-xl">
      <h2 id="deep-link-title">Open Ferryx link?</h2>
      <p className="my-4 break-all">{destination}</p>
      <div className="flex justify-end gap-4">
        <button type="button" autoFocus onClick={() => answer(false)}>Cancel</button>
        <button type="button" onClick={() => answer(true)}>Open</button>
      </div>
    </section>
  </div>;
}
