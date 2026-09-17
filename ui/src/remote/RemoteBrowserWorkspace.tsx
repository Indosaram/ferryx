/**
 * RemoteBrowserWorkspace Component (§4.4, §4.5, Phase 6)
 *
 * Full composition view wrapping useRemoteBrowser, useRemoteBrowserDriver,
 * and RemoteBrowser. Wires controls, mainframe point click, and mobile IME handling.
 */

import React, { useState } from "react";
import { RemoteBrowser, type RemoteBrowserPointClickEvent } from "./RemoteBrowser";
import { RemoteBrowserControls } from "./RemoteBrowserControls";
import type { BrowserSubscribeOptions } from "./browserProtocol";
import { useRemoteBrowser } from "./useRemoteBrowser";
import { useRemoteBrowserDriver } from "./useRemoteBrowserDriver";

export interface RemoteBrowserWorkspaceProps {
  baseUrl: string;
  browserId: string;
  deviceToken: string;
  options?: BrowserSubscribeOptions;
  capabilities?: {
    supportedCommands?: string[];
    browserAvailable?: boolean;
    pointClickSupported?: boolean;
  };
  onBack?: () => void;
  className?: string;
}

export const RemoteBrowserWorkspace: React.FC<RemoteBrowserWorkspaceProps> = ({
  baseUrl,
  browserId,
  deviceToken,
  options,
  capabilities,
  onBack,
  className = "",
}) => {
  // 1. Screencast viewer stream
  const {
    frame,
    browserState,
    hello,
    error: browserError,
    client,
  } = useRemoteBrowser({
    baseUrl,
    browserId,
    deviceToken,
    options,
  });

  // Extract browser identity guards for command dispatcher
  const browserInstanceId =
    frame?.metadata.browserInstanceId ?? hello?.browserInstanceId ?? "bi1";
  const desktopEpoch =
    frame?.metadata.desktopEpoch ?? hello?.desktopEpoch ?? "1";
  const documentGeneration =
    frame?.metadata.documentGeneration ?? browserState?.documentGeneration ?? "1";

  // 2. Remote driver lifecycle hook
  const driver = useRemoteBrowserDriver({
    client,
    browserId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
  });

  // Capability check for mainframe point click
  const canPointClick = Boolean(
    hello?.supportedCommands?.includes("click") ||
      capabilities?.supportedCommands?.includes("click") ||
      capabilities?.pointClickSupported
  );
  const [pointClickEnabled, setPointClickEnabled] = useState(true);

  // Mobile IME handling state
  const [imeText, setImeText] = useState("");
  const [isComposing, setIsComposing] = useState(false);
  const [imeTargetRef, setImeTargetRef] = useState("active");
  const [showImeBar, setShowImeBar] = useState(false);

  // Wire point click with normalized (u, v) coordinates when in driving mode
  const handlePointClick = (point: RemoteBrowserPointClickEvent) => {
    if (driver.driverState !== "driving" || !canPointClick || !pointClickEnabled) {
      return;
    }
    void driver.click({
      u: point.u,
      v: point.v,
    });
  };

  // Mobile IME submission: do NOT dispatch intermediate composition keystrokes; send confirmed text via fill
  const handleImeSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (isComposing) {
      // Do NOT send during composition
      return;
    }
    if (!imeText.trim() || driver.driverState !== "driving") return;

    void driver.fill(imeTargetRef || "active", imeText);
    setImeText("");
  };

  const controls = (
    <div className="flex flex-col">
      {/* Header with Back button if provided */}
      {onBack && (
        <div className="flex items-center justify-between px-2 py-1 bg-neutral-950 border-b border-neutral-800 text-xs">
          <button
            type="button"
            data-testid="remote-browser-workspace-back-btn"
            onClick={onBack}
            className="flex items-center gap-1 text-neutral-400 hover:text-white transition"
          >
            ← Back to Sessions
          </button>
          <span className="font-mono text-neutral-400 text-[11px] truncate max-w-[200px]">
            {browserId}
          </span>
          <button
            type="button"
            data-testid="remote-browser-ime-toggle-btn"
            onClick={() => setShowImeBar((prev) => !prev)}
            className={`text-[11px] px-1.5 py-0.5 rounded border transition ${
              showImeBar
                ? "bg-blue-900/60 border-blue-600 text-blue-200"
                : "bg-neutral-800 border-neutral-700 text-neutral-400"
            }`}
          >
            IME Input
          </button>
        </div>
      )}

      {/* Main RemoteBrowserControls toolbar */}
      <RemoteBrowserControls
        url={browserState?.url || ""}
        driverState={driver.driverState}
        canPointClick={canPointClick}
        pointClickEnabled={pointClickEnabled}
        onTogglePointClick={() => setPointClickEnabled((prev) => !prev)}
        onClaim={driver.claim}
        onRelease={driver.release}
        onNavigate={driver.navigate}
        onBack={driver.back}
        onForward={driver.forward}
        onReload={driver.reload}
        onFill={driver.fill}
        onKeypress={driver.keypress}
        onEval={driver.evalJs}
      />

      {/* Mobile IME input bar (safely buffers text and sends confirmed text via fill) */}
      {showImeBar && (
        <form
          onSubmit={handleImeSubmit}
          data-testid="remote-browser-ime-bar"
          className="flex items-center gap-2 p-1.5 bg-neutral-950 border-b border-neutral-800 text-xs"
        >
          <span className="text-neutral-400 text-[11px] shrink-0">Mobile IME:</span>
          <input
            type="text"
            data-testid="remote-browser-ime-ref-input"
            value={imeTargetRef}
            onChange={(e) => setImeTargetRef(e.target.value)}
            placeholder="Target Ref (e.g. e1)"
            className="w-20 px-1.5 py-0.5 rounded bg-neutral-900 border border-neutral-700 text-xs font-mono"
          />
          <input
            type="text"
            data-testid="remote-browser-ime-text-input"
            value={imeText}
            onChange={(e) => setImeText(e.target.value)}
            onCompositionStart={() => setIsComposing(true)}
            onCompositionEnd={(e) => {
              setIsComposing(false);
              setImeText(e.data);
            }}
            placeholder="Type confirmed text..."
            className="flex-1 px-2 py-0.5 rounded bg-neutral-900 border border-neutral-700 text-xs"
          />
          <button
            type="submit"
            data-testid="remote-browser-ime-send-btn"
            disabled={driver.driverState !== "driving" || !imeText.trim() || isComposing}
            className="px-2 py-0.5 bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white rounded text-xs font-medium transition"
          >
            Send Fill
          </button>
        </form>
      )}

      {/* Driver or browser error banner if error occurred */}
      {(driver.error || browserError) && (
        <div
          data-testid="remote-browser-driver-error"
          className="px-2 py-1 bg-red-950/80 border-b border-red-800 text-red-200 text-xs flex items-center justify-between"
        >
          <span>{driver.error ? driver.error.message : browserError?.message}</span>
        </div>
      )}
    </div>
  );

  return (
    <div
      data-testid="remote-browser-workspace"
      className={`relative flex flex-col w-full h-full overflow-hidden ${className}`}
    >
      <RemoteBrowser
        baseUrl={baseUrl}
        browserId={browserId}
        deviceToken={deviceToken}
        options={options}
        controls={controls}
        onPointClick={handlePointClick}
      />
    </div>
  );
};
