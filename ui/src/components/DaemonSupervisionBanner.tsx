import { AlertTriangle } from "lucide-react";
import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { isDesktopShell } from "../lib/clientRuntime";

/**
 * The crash supervisor's refusal, shown while it stands: the daemon died and was not restarted,
 * because supervision is off (no live keeper) or the restart budget is spent. Unlike the connection
 * banner there is no retry, because retrying is exactly what the supervisor just declined to do.
 */
export function DaemonSupervisionBanner() {
  const [banner, setBanner] = useState<string | null>(null);
  useEffect(() => {
    if (!isDesktopShell(isTauri())) return;
    let cancelled = false;
    invoke<{ banner?: string | null } | undefined>("cmd_daemon_supervision_status")
      .then((status) => {
        if (cancelled) return;
        setBanner(typeof status?.banner === "string" && status.banner.trim() !== "" ? status.banner : null);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  if (banner === null) return null;
  return (
    <div
      role="alert"
      data-testid="daemon-supervision-banner"
      className="flex items-center gap-2 bg-destructive px-4 py-2 text-[12px] font-medium text-destructive-foreground"
    >
      <AlertTriangle className="size-3.5 shrink-0" />
      <span>{banner}</span>
    </div>
  );
}
