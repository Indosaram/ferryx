import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { ChevronRight } from "lucide-react";

import { cn } from "../../lib/cn";
import { rdErrorMessage } from "../../lib/remoteDesktop";
import {
  clampSidebarWidth,
  loadSidebarWidthForMode,
  saveSidebarWidthForMode,
  SIDEBAR_RANGE_BY_MODE,
} from "../../lib/sidebarPersistence";
import { remoteDesktopStore, useRemoteDesktopSession, type RemoteDesktopStatus } from "../../state/remoteDesktopStore";
import { remoteHostStore, selectHostList, type HostEndpoint } from "../../state/remoteHostStore";
import { RemoteDesktopHostSection } from "../settings/remote/RemoteDesktopHostSection";
import { Button } from "../ui/button";

const RANGE = SIDEBAR_RANGE_BY_MODE.remoteDesktop;
const KEYBOARD_STEP = 16;

const SESSION_LABEL: Record<RemoteDesktopStatus, string> = {
  idle: "Not connected",
  connecting: "Connecting",
  connected: "Connected",
  error: "Failed",
};

export type RemoteDesktopConnectBlock = {
  /** Structured code for the refusal, from the host's own fields - never from error text. */
  readonly code: string;
  readonly reason: string;
};

/**
 * Why this machine cannot be connected to, decided by structured inventory fields only. `null`
 * means Connect is available; the caller renders the reason next to the disabled button.
 */
export function remoteDesktopConnectBlock(
  host: Pick<HostEndpoint, "authStatus" | "grantScope" | "online">,
): RemoteDesktopConnectBlock | null {
  if (host.authStatus === "revoked") {
    return {
      code: "RD_PERMISSION_REQUIRED",
      reason: "Access to this machine was revoked. Pair it again from your Ferryx account.",
    };
  }
  if (host.authStatus !== "paired" || host.grantScope !== "machine") {
    return {
      code: "RD_PERMISSION_REQUIRED",
      reason: "This machine has not granted remote desktop access to this device.",
    };
  }
  if (!host.online) {
    return { code: "RD_HOST_OFFLINE", reason: "The machine is offline or unreachable." };
  }
  return null;
}

/** DOM ids and test ids cannot carry a relay host key's slashes. */
function safeId(hostId: string): string {
  return hostId.replace(/[^a-zA-Z0-9_-]/g, "-");
}

export type RemoteDesktopActivitySidebarProps = {
  /** The machine the global canvas shows; the drawer highlights its row. */
  activeHostId: string | null;
  onSelectHost: (hostId: string) => void;
};

/**
 * Remote Desktop mode's drawer: paired machines from `remoteHostStore` with reachability and
 * remote-desktop grant state, one-click connect/disconnect against the keep-alive session store,
 * and a collapsible host-sharing section (Settings > Remote keeps its own copy).
 */
export function RemoteDesktopActivitySidebar({ activeHostId, onSelectHost }: RemoteDesktopActivitySidebarProps) {
  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const hosts = selectHostList(remoteState);
  const [width, setWidth] = useState(() => loadSidebarWidthForMode("remoteDesktop"));
  const [sharingOpen, setSharingOpen] = useState(false);
  const widthRef = useRef(width);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  widthRef.current = width;

  const endResize = useCallback(() => {
    if (!dragRef.current) return;
    dragRef.current = null;
    saveSidebarWidthForMode("remoteDesktop", widthRef.current);
    document.body.style.cursor = "";
  }, []);

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      setWidth(clampSidebarWidth(drag.startWidth + event.clientX - drag.startX, RANGE));
    };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", endResize);
      window.removeEventListener("pointercancel", endResize);
      document.body.style.cursor = "";
    };
  }, [endResize]);

  const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    document.body.style.cursor = "col-resize";
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // The pointer may already be gone; the window listeners still end the drag.
    }
  };

  const resizeWithKeyboard = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const delta = event.key === "ArrowLeft" ? -KEYBOARD_STEP : KEYBOARD_STEP;
    const nextWidth = clampSidebarWidth(widthRef.current + delta, RANGE);
    widthRef.current = nextWidth;
    setWidth(nextWidth);
    saveSidebarWidthForMode("remoteDesktop", nextWidth);
  };

  const connect = (host: HostEndpoint) => {
    const name = host.displayName ?? host.name;
    onSelectHost(host.hostId);
    void remoteDesktopStore.connect(host.hostId, name);
  };

  return (
    <aside
      data-testid="activity-drawer-remote-desktop"
      className="relative flex h-full shrink-0 flex-col overflow-hidden border-r border-border bg-worktree-sidebar text-worktree-sidebar-foreground"
      style={{ width: `${width}px`, minWidth: `${RANGE.min}px`, maxWidth: `${RANGE.max}px` }}
    >
      <div data-tauri-drag-region className="h-titlebar shrink-0" />
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3 py-2">
        <header className="border-b border-border pb-2">
          <h2 className="text-[13px] font-semibold text-foreground">Remote desktop</h2>
          <p className="text-[11px] text-muted-foreground">
            {hosts.length === 0
              ? "No paired machines yet."
              : `${hosts.length} paired ${hosts.length === 1 ? "machine" : "machines"}`}
          </p>
        </header>
        {hosts.length === 0 ? (
          <p data-testid="rd-no-machines" className="text-[11px] text-muted-foreground">
            Pair a machine from Settings &gt; Remote to control it here.
          </p>
        ) : (
          <ul aria-label="Paired machines" className="space-y-1.5">
            {hosts.map((host) => (
              <PairedMachineRow
                key={host.hostId}
                host={host}
                active={activeHostId === host.hostId}
                onConnect={connect}
                onSelect={onSelectHost}
              />
            ))}
          </ul>
        )}
        <section className="mt-auto border-t border-border pt-2" data-testid="rd-host-sharing">
          <button
            type="button"
            aria-expanded={sharingOpen}
            aria-controls="rd-host-sharing-panel"
            data-testid="rd-host-sharing-toggle"
            onClick={() => setSharingOpen((open) => !open)}
            className="flex w-full items-center gap-1 rounded-md px-1 py-1 text-left text-[12px] font-medium text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ChevronRight
              aria-hidden="true"
              className={cn("size-3 transition-transform", sharingOpen && "rotate-90")}
            />
            Host sharing
          </button>
          {sharingOpen ? (
            <div id="rd-host-sharing-panel" data-testid="rd-host-sharing-panel" className="px-1 pt-2">
              <RemoteDesktopHostSection />
            </div>
          ) : null}
        </section>
      </div>
      <div
        role="separator"
        aria-label="Resize remote desktop machines"
        aria-orientation="vertical"
        aria-valuemin={RANGE.min}
        aria-valuemax={RANGE.max}
        aria-valuenow={width}
        tabIndex={0}
        onPointerDown={startResize}
        onLostPointerCapture={endResize}
        onKeyDown={resizeWithKeyboard}
        data-testid="rd-drawer-resize"
        className="no-drag absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none"
      />
    </aside>
  );
}

function PairedMachineRow({ host, active, onConnect, onSelect }: {
  host: HostEndpoint;
  active: boolean;
  onConnect: (host: HostEndpoint) => void;
  onSelect: (hostId: string) => void;
}) {
  const session = useRemoteDesktopSession(host.hostId);
  const name = host.displayName ?? host.name;
  const id = safeId(host.hostId);
  const reasonId = `rd-connect-reason-${id}`;
  const block = remoteDesktopConnectBlock(host);
  const granted = host.authStatus === "paired" && host.grantScope === "machine";
  const status: RemoteDesktopStatus = session?.status ?? "idle";
  const live = status === "connected" || status === "connecting";

  return (
    <li
      role="listitem"
      data-testid={`rd-host-${id}`}
      data-active={active ? "true" : "false"}
      data-session={status}
      className={cn(
        "space-y-1 rounded-lg border p-2",
        active ? "border-primary/60 bg-accent/40" : "border-border bg-card/40",
      )}
    >
      <div className="flex min-w-0 items-center gap-1.5">
        <button
          type="button"
          onClick={() => onSelect(host.hostId)}
          disabled={!live}
          aria-pressed={active}
          aria-label={`Show ${name}`}
          data-testid={`rd-host-select-${id}`}
          className="min-w-0 flex-1 truncate rounded text-left text-[13px] font-medium text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-default"
        >
          {name}
        </button>
        {live ? (
          <span
            data-testid={`rd-host-badge-${id}`}
            className="shrink-0 rounded border border-border/70 bg-muted/50 px-1.5 text-[10px] font-medium text-foreground"
          >
            {SESSION_LABEL[status]}
          </span>
        ) : null}
      </div>
      <p className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
        <span
          aria-hidden="true"
          className={cn("size-1.5 shrink-0 rounded-full", host.online ? "bg-status-success" : "bg-muted-foreground/50")}
        />
        <span data-testid={`rd-host-status-${id}`} data-online={host.online ? "true" : "false"}>
          {host.online ? "Online" : "Offline"}
        </span>
        <span aria-hidden="true">-</span>
        <span data-testid={`rd-host-grant-${id}`} data-granted={granted ? "true" : "false"}>
          {granted ? "Remote desktop granted" : "Remote desktop not granted"}
        </span>
      </p>
      {session?.error ? (
        <p
          role="alert"
          data-testid={`rd-session-error-${id}`}
          data-error-code={session.error.code}
          className="break-words text-[11px] text-destructive"
        >
          {rdErrorMessage(session.error)}
        </p>
      ) : null}
      {block && !live ? (
        <p id={reasonId} data-testid={`rd-connect-reason-${id}`} data-reason-code={block.code} className="text-[11px] text-muted-foreground">
          {block.reason}
        </p>
      ) : null}
      <div className="flex gap-1.5">
        {live ? (
          <Button
            type="button"
            size="sm"
            variant="outline"
            aria-label={`Disconnect ${name}`}
            data-testid={`rd-disconnect-${id}`}
            onClick={() => void remoteDesktopStore.disconnect(host.hostId)}
          >
            Disconnect
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            variant={block ? "outline" : "default"}
            aria-label={`${status === "error" ? "Reconnect" : "Connect"} ${name}`}
            data-testid={`rd-connect-${id}`}
            disabled={block !== null}
            aria-describedby={block ? reasonId : undefined}
            onClick={() => onConnect(host)}
          >
            {status === "error" ? "Reconnect" : "Connect"}
          </Button>
        )}
      </div>
    </li>
  );
}
