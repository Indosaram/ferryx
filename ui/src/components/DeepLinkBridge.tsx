import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { routeDeepLink, type DeepLinkActions, type DeepLinkRoute } from "../lib/deepLinks";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "./ui/dialog";

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
    void subscription.then(() => {
      if (!disposed) drain();
    }).catch((error: unknown) => {
      if (!disposed) actionsRef.current.toast(`Could not listen for Ferryx links: ${String(error)}`);
    });
    return () => {
      disposed = true;
      resolveRef.current?.(false);
      void subscription.then((unlisten) => unlisten()).catch((error: unknown) => {
        console.error("Could not release deep-link listener", error);
      });
    };
  }, []);
  const answer = (accepted: boolean) => {
    const resolve = resolveRef.current;
    resolveRef.current = null;
    setPending(null);
    resolve?.(accepted);
  };

  if (!pending) return null;
  const destination = pending.kind === "settings" ? `Settings / ${pending.section}`
    : pending.kind === "onboarding" ? `Onboarding / ${pending.step}` : `Workspace / ${pending.workspaceId}`;
  return <Dialog open onOpenChange={(open) => { if (!open) answer(false); }}>
    <DialogContent>
      <DialogTitle>Open Ferryx link?</DialogTitle>
      <DialogDescription className="break-all">{destination}</DialogDescription>
      <div className="flex justify-end gap-4">
        <button type="button" onClick={() => answer(false)}>Cancel</button>
        <button type="button" onClick={() => answer(true)}>Open</button>
      </div>
    </DialogContent>
  </Dialog>;
}
