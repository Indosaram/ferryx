/**
 * Remote Browser Controls Toolbar (§4.4, Phase 6)
 *
 * Designed to fit into RemoteBrowser's `controls` slot.
 * Includes URL bar, driver claim/release badge, mainframe point click toggle,
 * semantic reference fill controls, keypress inputs for allowlisted keys,
 * and a collapsible advanced eval UI with owner warning and 64 KiB truncation note.
 */

import React, { useState } from "react";
import type { RemoteDriverState } from "./useRemoteBrowserDriver";

export const ALLOWLISTED_KEYS = [
  { label: "Enter", key: "Enter" },
  { label: "Tab", key: "Tab" },
  { label: "Esc", key: "Escape" },
  { label: "←", key: "ArrowLeft" },
  { label: "→", key: "ArrowRight" },
  { label: "↑", key: "ArrowUp" },
  { label: "↓", key: "ArrowDown" },
  { label: "⌫", key: "Backspace" },
  { label: "Del", key: "Delete" },
] as const;

export interface RemoteBrowserControlsProps {
  url?: string;
  driverState: RemoteDriverState;
  canPointClick?: boolean;
  pointClickEnabled?: boolean;
  onTogglePointClick?: () => void;
  onClaim?: () => void;
  onRelease?: () => void;
  onNavigate?: (url: string) => void;
  onBack?: () => void;
  onForward?: () => void;
  onReload?: () => void;
  onFill?: (reference: string, value: string) => void;
  onKeypress?: (key: string) => void;
  onEval?: (script: string) => Promise<unknown> | void;
  className?: string;
}

export const RemoteBrowserControls: React.FC<RemoteBrowserControlsProps> = ({
  url = "",
  driverState,
  canPointClick = false,
  pointClickEnabled = false,
  onTogglePointClick,
  onClaim,
  onRelease,
  onNavigate,
  onBack,
  onForward,
  onReload,
  onFill,
  onKeypress,
  onEval,
  className = "",
}) => {
  const [inputUrl, setInputUrl] = useState(url);
  const [reference, setReference] = useState("");
  const [fillValue, setFillValue] = useState("");
  const [showAdvancedEval, setShowAdvancedEval] = useState(false);
  const [evalScript, setEvalScript] = useState("");
  const [evalResult, setEvalResult] = useState<string | null>(null);
  const [evalLoading, setEvalLoading] = useState(false);

  // Sync incoming url
  React.useEffect(() => {
    setInputUrl(url);
  }, [url]);

  const isDriving = driverState === "driving";

  const handleNavigateSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!isDriving || !inputUrl.trim() || !onNavigate) return;
    onNavigate(inputUrl.trim());
  };

  const handleFillSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!isDriving || !reference.trim() || !onFill) return;
    onFill(reference.trim(), fillValue);
  };

  const handleRunEval = async () => {
    if (!isDriving || !evalScript.trim() || !onEval) return;
    setEvalLoading(true);
    setEvalResult(null);
    try {
      const res = await onEval(evalScript);
      setEvalResult(typeof res === "object" ? JSON.stringify(res, null, 2) : String(res));
    } catch (err) {
      setEvalResult(`Error: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setEvalLoading(false);
    }
  };

  return (
    <div
      data-testid="remote-browser-controls"
      className={`flex flex-col gap-2 p-2 bg-neutral-900 border-b border-neutral-800 text-neutral-200 text-xs ${className}`}
    >
      {/* Top row: Navigation bar, Status badge, Claim/Release */}
      <div className="flex flex-wrap items-center gap-2">
        {/* Nav history buttons */}
        <div className="flex items-center gap-1">
          <button
            type="button"
            data-testid="remote-browser-back-btn"
            disabled={!isDriving}
            onClick={onBack}
            aria-label="Go back"
            className="p-1 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed transition"
          >
            ←
          </button>
          <button
            type="button"
            data-testid="remote-browser-forward-btn"
            disabled={!isDriving}
            onClick={onForward}
            aria-label="Go forward"
            className="p-1 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed transition"
          >
            →
          </button>
          <button
            type="button"
            data-testid="remote-browser-reload-btn"
            disabled={!isDriving}
            onClick={onReload}
            aria-label="Reload"
            className="p-1 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed transition"
          >
            ↻
          </button>
        </div>

        {/* URL input */}
        <form onSubmit={handleNavigateSubmit} className="flex-1 min-w-[180px] flex items-center gap-1">
          <input
            type="text"
            data-testid="remote-browser-url-input"
            value={inputUrl}
            onChange={(e) => setInputUrl(e.target.value)}
            disabled={!isDriving}
            placeholder={isDriving ? "https://example.com" : "Claim control to navigate"}
            className="w-full px-2 py-1 rounded bg-neutral-950 border border-neutral-700 focus:outline-none focus:border-blue-500 disabled:opacity-50 disabled:cursor-not-allowed text-xs font-mono"
          />
          <button
            type="submit"
            data-testid="remote-browser-navigate-btn"
            disabled={!isDriving || !inputUrl.trim()}
            className="px-2 py-1 rounded bg-blue-600 hover:bg-blue-500 disabled:opacity-40 disabled:cursor-not-allowed text-white font-medium transition"
          >
            Go
          </button>
        </form>

        {/* Driver Status Badge */}
        <div className="flex items-center gap-1.5 shrink-0">
          <span
            data-testid="remote-browser-driver-badge"
            className={`px-2 py-0.5 rounded text-[11px] font-semibold uppercase tracking-wider ${
              driverState === "driving"
                ? "bg-green-950 text-green-300 border border-green-800"
                : driverState === "claiming"
                ? "bg-amber-950 text-amber-300 border border-amber-800"
                : driverState === "occupied"
                ? "bg-red-950 text-red-300 border border-red-800"
                : driverState === "revoked"
                ? "bg-rose-950 text-rose-300 border border-rose-800"
                : "bg-neutral-800 text-neutral-400 border border-neutral-700"
            }`}
          >
            {driverState === "driving"
              ? "Driving"
              : driverState === "claiming"
              ? "Claiming..."
              : driverState === "occupied"
              ? "Occupied"
              : driverState === "revoked"
              ? "Revoked"
              : "Viewing"}
          </span>

          {/* Claim / Release Action Button */}
          {driverState === "driving" ? (
            <button
              type="button"
              data-testid="remote-browser-release-btn"
              onClick={onRelease}
              className="px-2 py-1 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-200 text-xs font-medium border border-neutral-700 transition"
            >
              Release Control
            </button>
          ) : (
            <button
              type="button"
              data-testid="remote-browser-claim-btn"
              disabled={driverState === "claiming"}
              onClick={onClaim}
              className="px-2 py-1 rounded bg-amber-600 hover:bg-amber-500 disabled:opacity-40 disabled:cursor-not-allowed text-white text-xs font-medium transition"
            >
              {driverState === "claiming" ? "Claiming..." : "Take Control"}
            </button>
          )}
        </div>
      </div>

      {/* Second row: Reference fill, point-click toggle, keypress quick actions */}
      <div className="flex flex-wrap items-center justify-between gap-2 pt-1 border-t border-neutral-800/80">
        {/* Semantic reference manipulation */}
        <form onSubmit={handleFillSubmit} className="flex items-center gap-1.5 flex-wrap">
          <span className="text-neutral-400 text-[11px] font-medium">Ref:</span>
          <input
            type="text"
            data-testid="remote-browser-ref-input"
            value={reference}
            onChange={(e) => setReference(e.target.value)}
            disabled={!isDriving}
            placeholder="e.g. e1"
            className="w-16 px-1.5 py-0.5 rounded bg-neutral-950 border border-neutral-700 focus:outline-none focus:border-blue-500 disabled:opacity-50 text-xs font-mono"
          />
          <input
            type="text"
            data-testid="remote-browser-fill-input"
            value={fillValue}
            onChange={(e) => setFillValue(e.target.value)}
            disabled={!isDriving}
            placeholder="Text to fill"
            className="w-28 sm:w-40 px-1.5 py-0.5 rounded bg-neutral-950 border border-neutral-700 focus:outline-none focus:border-blue-500 disabled:opacity-50 text-xs"
          />
          <button
            type="submit"
            data-testid="remote-browser-fill-btn"
            disabled={!isDriving || !reference.trim()}
            className="px-2 py-0.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed text-neutral-200 text-xs font-medium transition border border-neutral-700"
          >
            Fill
          </button>
        </form>

        {/* Point click toggle (visible ONLY when capability indicates point click is supported) */}
        {canPointClick && (
          <label
            data-testid="remote-browser-point-click-toggle"
            className="flex items-center gap-1.5 cursor-pointer select-none text-[11px]"
          >
            <input
              type="checkbox"
              checked={pointClickEnabled}
              disabled={!isDriving}
              onChange={onTogglePointClick}
              className="rounded bg-neutral-950 border-neutral-700 text-blue-600 focus:ring-0 focus:ring-offset-0"
            />
            <span className={isDriving ? "text-neutral-300" : "text-neutral-500"}>
              Point Click
            </span>
          </label>
        )}

        {/* Allowlisted keypress quick buttons */}
        <div className="flex items-center gap-1 flex-wrap">
          <span className="text-neutral-400 text-[11px]">Keys:</span>
          {ALLOWLISTED_KEYS.map(({ label, key }) => (
            <button
              key={key}
              type="button"
              data-testid={`remote-browser-key-${key}`}
              disabled={!isDriving}
              onClick={() => onKeypress?.(key)}
              className="px-1.5 py-0.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed text-neutral-300 text-[11px] font-mono transition border border-neutral-700"
            >
              {label}
            </button>
          ))}
        </div>

        {/* Collapsible advanced eval toggle */}
        <button
          type="button"
          data-testid="remote-browser-eval-toggle"
          onClick={() => setShowAdvancedEval((prev) => !prev)}
          className="text-[11px] text-neutral-400 hover:text-neutral-200 underline underline-offset-2 ml-auto transition"
        >
          {showAdvancedEval ? "Hide Advanced JS Eval" : "Advanced JS Eval"}
        </button>
      </div>

      {/* Collapsible Advanced Eval Section */}
      {showAdvancedEval && (
        <div
          data-testid="remote-browser-eval-panel"
          className="flex flex-col gap-1.5 p-2 bg-neutral-950 border border-neutral-800 rounded mt-1"
        >
          {/* Owner warning and truncation reminder */}
          <div className="flex flex-col gap-0.5 text-[11px] text-amber-400/90 font-medium">
            <p>Warning: Remote eval executes arbitrary JavaScript on the page with session privileges.</p>
            <p className="text-neutral-400">Result truncated to 64 KiB (65,536 UTF-8 bytes).</p>
          </div>

          <div className="flex gap-2">
            <textarea
              data-testid="remote-browser-eval-input"
              value={evalScript}
              onChange={(e) => setEvalScript(e.target.value)}
              disabled={!isDriving}
              placeholder='document.title or window.location.href'
              rows={2}
              className="flex-1 p-1.5 rounded bg-neutral-900 border border-neutral-700 text-xs font-mono focus:outline-none focus:border-blue-500 disabled:opacity-50"
            />
            <button
              type="button"
              data-testid="remote-browser-eval-run-btn"
              disabled={!isDriving || !evalScript.trim() || evalLoading}
              onClick={handleRunEval}
              className="px-3 py-1 bg-amber-700 hover:bg-amber-600 disabled:opacity-40 disabled:cursor-not-allowed text-white text-xs font-medium rounded self-start transition"
            >
              {evalLoading ? "Running..." : "Run Eval"}
            </button>
          </div>

          {evalResult && (
            <div
              data-testid="remote-browser-eval-result"
              className="p-1.5 bg-neutral-900 rounded border border-neutral-800 text-[11px] font-mono text-neutral-300 max-h-32 overflow-y-auto whitespace-pre-wrap break-all"
            >
              {evalResult}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
