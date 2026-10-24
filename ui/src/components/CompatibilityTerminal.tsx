import { RemoteTerminal } from "../remote/RemoteTerminal";
import { createLocalGridSocket } from "../lib/localGridSocket";

export const GPU_FALLBACK_BANNER = "GPU unavailable - using compatibility renderer";

/**
 * DOM grid renderer for a local session, used when no GPU adapter exists. Reuses the remote
 * client's renderer, fed by the local session's grid stream.
 */
export function CompatibilityTerminal({ backendSessionId }: { readonly backendSessionId: string }) {
  return (
    <div data-testid="terminal-compatibility-renderer" className="flex h-full min-h-0 w-full flex-col">
      <div
        role="status"
        data-testid="terminal-gpu-fallback-banner"
        className="shrink-0 truncate border-b border-border bg-muted/60 px-2 py-0.5 text-xs text-muted-foreground"
      >
        {GPU_FALLBACK_BANNER}
      </div>
      <RemoteTerminal
        sessionId={backendSessionId}
        token="local"
        embedded
        showTitleBar={false}
        showKeyDock={false}
        createLocalSocket={createLocalGridSocket}
      />
    </div>
  );
}
