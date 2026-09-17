/**
 * RemoteBrowserSharingIndicator Component (§6.1, Phase 6)
 *
 * Desktop status indicator showing active remote browser screencast sessions
 * and current driver status with an immediate "Reclaim Control" / "Revoke Remote Driver"
 * button calling desktop Tauri commands (`cmd_browser_remote_reclaim` / `cmd_browser_remote_revoke`).
 */

import React, { useState } from "react";
import { browserRemoteReclaim, browserRemoteRevoke, isTauriRuntime } from "../lib/tauri";
import {
  emitSharingState,
  parseSharingState,
  REMOTE_BROWSER_SHARING_EVENT,
  REMOTE_BROWSER_SHARING_TAURI_EVENT,
  type RemoteBrowserSharingState,
} from "../remote/browserProtocol";

export interface RemoteBrowserSharingIndicatorProps {
  isSharing?: boolean;
  activeSessionsCount?: number;
  driverStatus?: "driving" | "viewing" | "idle" | null;
  driverDeviceId?: string | null;
  onReclaim?: () => Promise<void> | void;
  onRevoke?: () => Promise<void> | void;
  className?: string;
}

export const RemoteBrowserSharingIndicator: React.FC<RemoteBrowserSharingIndicatorProps> = ({
  isSharing = false,
  activeSessionsCount = 0,
  driverStatus = null,
  driverDeviceId = null,
  onReclaim,
  onRevoke,
  className = "",
}) => {
  const [internalSharing, setInternalSharing] = useState(isSharing);
  const [internalDriverStatus, setInternalDriverStatus] = useState(driverStatus);
  const [internalSessionsCount, setInternalSessionsCount] = useState(activeSessionsCount);
  const [internalDriverDeviceId, setInternalDriverDeviceId] = useState(driverDeviceId);
  const [reclaiming, setReclaiming] = useState(false);
  const [feedbackMessage, setFeedbackMessage] = useState<string | null>(null);

  const applySharingState = React.useCallback((state: RemoteBrowserSharingState) => {
    setInternalSharing(state.isSharing);
    setInternalDriverStatus(state.driverStatus);
    setInternalSessionsCount(state.activeSessionsCount);
    setInternalDriverDeviceId(state.driverDeviceId);
  }, []);

  // Sync props to internal state if props change
  React.useEffect(() => {
    setInternalSharing(isSharing);
  }, [isSharing]);

  React.useEffect(() => {
    setInternalDriverStatus(driverStatus);
  }, [driverStatus]);

  React.useEffect(() => {
    setInternalSessionsCount(activeSessionsCount);
  }, [activeSessionsCount]);

  React.useEffect(() => {
    setInternalDriverDeviceId(driverDeviceId);
  }, [driverDeviceId]);

  // Authoritative sharing state published by the daemon/desktop shell (R4-5).
  React.useEffect(() => {
    const handleSharingEvent = (e: Event) => {
      const state = parseSharingState((e as CustomEvent).detail);
      if (state) applySharingState(state);
    };
    window.addEventListener(REMOTE_BROWSER_SHARING_EVENT, handleSharingEvent);
    window.addEventListener("remote-browser-sharing-change", handleSharingEvent);
    return () => {
      window.removeEventListener(REMOTE_BROWSER_SHARING_EVENT, handleSharingEvent);
      window.removeEventListener("remote-browser-sharing-change", handleSharingEvent);
    };
  }, [applySharingState]);

  // Desktop shell bridge: daemon admission/revocation broadcasts arrive as Tauri events
  // and are republished on the window channel so every listener converges (R4-5).
  React.useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<unknown>(REMOTE_BROWSER_SHARING_TAURI_EVENT, (event) => {
          const state = parseSharingState(event.payload);
          if (!state) return;
          applySharingState(state);
          emitSharingState(state);
        }),
      )
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {
        // Desktop event bridge unavailable; window-channel updates still apply.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applySharingState]);

  const active = internalSharing || internalDriverStatus === "driving" || internalSessionsCount > 0;

  if (!active && !feedbackMessage) {
    return null;
  }

  const handleReclaim = async () => {
    setReclaiming(true);
    setFeedbackMessage(null);
    try {
      await browserRemoteReclaim();
      if (onReclaim) {
        await onReclaim();
      }
      // Publish the reclaimed state so every listener stops showing remote control.
      const reclaimed: RemoteBrowserSharingState = {
        isSharing: false,
        activeSessionsCount: 0,
        driverStatus: "idle",
        driverDeviceId: null,
      };
      applySharingState(reclaimed);
      emitSharingState(reclaimed);
      setFeedbackMessage("Control reclaimed by desktop owner");
    } catch (err) {
      setFeedbackMessage(`Reclaim error: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setReclaiming(false);
    }
  };

  const handleRevoke = async () => {
    setReclaiming(true);
    setFeedbackMessage(null);
    try {
      await browserRemoteRevoke();
      if (onRevoke) {
        await onRevoke();
      }
      // The lease is gone, but viewers may remain: publish viewing, not idle.
      const revoked: RemoteBrowserSharingState = {
        isSharing: internalSharing,
        activeSessionsCount: internalSessionsCount,
        driverStatus: internalSharing || internalSessionsCount > 0 ? "viewing" : "idle",
        driverDeviceId: null,
      };
      applySharingState(revoked);
      emitSharingState(revoked);
      setFeedbackMessage("Remote driver lease revoked");
    } catch (err) {
      setFeedbackMessage(`Revoke error: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setReclaiming(false);
    }
  };

  return (
    <aside
      role="status"
      aria-label="Remote Browser Sharing Status"
      data-testid="remote-browser-sharing-indicator"
      className={`flex items-center justify-between px-3 py-1 bg-amber-950/40 border-b border-amber-600/40 text-amber-200 text-xs select-none z-30 shrink-0 ${className}`}
    >
      <div className="flex items-center gap-2">
        <span className="inline-block size-2 rounded-full bg-amber-400 animate-pulse" />
        <span className="font-medium text-[11px]">
          Remote Browser Screencast Active
          {internalSessionsCount > 0 ? ` (${internalSessionsCount} active)` : ""}
        </span>
        {internalDriverStatus === "driving" && (
          <span
            data-testid="remote-driver-badge"
            className="px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-300 font-mono text-[10px] border border-amber-500/40"
          >
            Driver Active{internalDriverDeviceId ? `: ${internalDriverDeviceId}` : ""}
          </span>
        )}
        {feedbackMessage && (
          <span
            data-testid="reclaim-feedback-msg"
            className="text-[11px] text-amber-300/80 italic ml-2"
          >
            {feedbackMessage}
          </span>
        )}
      </div>

      <div className="flex items-center gap-1.5">
        <button
          type="button"
          data-testid="reclaim-control-btn"
          disabled={reclaiming}
          onClick={handleReclaim}
          className="px-2 py-0.5 rounded bg-amber-700 hover:bg-amber-600 active:bg-amber-800 disabled:opacity-50 text-white text-[11px] font-medium transition shadow-sm"
        >
          {reclaiming ? "Reclaiming..." : "Reclaim Control"}
        </button>
        <button
          type="button"
          data-testid="revoke-driver-btn"
          disabled={reclaiming}
          onClick={handleRevoke}
          className="px-2 py-0.5 rounded bg-neutral-800 hover:bg-neutral-700 active:bg-neutral-900 disabled:opacity-50 text-neutral-300 text-[11px] font-medium transition border border-neutral-700"
        >
          Revoke Remote Driver
        </button>
      </div>
    </aside>
  );
};
