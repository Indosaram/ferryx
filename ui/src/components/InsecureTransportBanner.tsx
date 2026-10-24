import { ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { isDesktopShell } from "../lib/clientRuntime";

/**
 * Persistent red warning while any insecure transport opt-in (`FERRYX_ALLOW_INSECURE_DIRECT`,
 * `FERRYX_ALLOW_INSECURE_LAN`, `FERRYX_ALLOW_INSECURE_RELAY`) is in effect in the gateway. It has
 * no dismiss control: the state is a process-wide property, not a notice to acknowledge.
 *
 * Reads the status through its own `invoke` rather than `lib/tauri`, so screens whose tests mock
 * that module wholesale do not have to know about this banner.
 */
export function InsecureTransportBanner() {
  const [modes, setModes] = useState<string[]>([]);
  useEffect(() => {
    if (!isDesktopShell(isTauri())) return;
    let cancelled = false;
    invoke<{ insecureModes?: string[] }>("cmd_remote_status")
      .then((status) => {
        if (!cancelled) setModes(Array.isArray(status.insecureModes) ? status.insecureModes : []);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  if (modes.length === 0) return null;
  return (
    <div
      role="alert"
      data-testid="insecure-transport-banner"
      className="flex items-center gap-2 bg-destructive px-4 py-2 text-[12px] font-medium text-destructive-foreground"
    >
      <ShieldAlert className="size-3.5 shrink-0" />
      <span>Insecure transport enabled ({modes.join(", ")}). Remote traffic may be unencrypted.</span>
    </div>
  );
}
