/**
 * Remote Browser Controls Toolbar (§4.4, Phase 6)
 *
 * Designed to fit into RemoteBrowser's `controls` slot.
 * Includes URL bar, driver claim/release badge, mainframe point click toggle,
 * semantic reference fill controls, keypress inputs for allowlisted keys,
 * and a collapsible advanced eval UI with owner warning and 64 KiB truncation note.
 */

import React, { useState } from "react";
import { buildRemoteInputRefusal, type RemoteInputRefusal } from "./RemoteBrowser";
import type { BrowserErrorDetails } from "./browserProtocol";
import type { RemoteDriverState } from "./useRemoteBrowserDriver";

export const FORBIDDEN_APP_CHROME_KEYS = new Set([
  "tabclose",
  "newtab",
  "appquit",
  "commandpalette",
  "settings",
  "sidebartoggle",
  "super",
  "meta",
  "alt+f4",
  "cmd+q",
  "cmd+w",
  "cmd+t",
  "ctrl+w",
  "ctrl+t",
]);

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
  pointClickDisabledReason?: string | null;
  onTogglePointClick?: () => void;
  onClaim?: () => void;
  onRelease?: () => void;
  onNavigate?: (url: string) => void;
  onBack?: () => void;
  onForward?: () => void;
  onReload?: () => void;
  onFill?: (reference: string, value: string) => Promise<unknown> | void;
  onKeypress?: (key: string) => Promise<unknown> | void;
  onEval?: (script: string) => Promise<unknown> | void;
  refusal?: RemoteInputRefusal | null;
  onRefusal?: (refusal: RemoteInputRefusal) => void;
  onClearRefusal?: () => void;
  className?: string;
}

/** Refusal for a rejected fill/key command: keeps the server's structured `details` when present. */
function commandFailureRefusal(err: unknown, inputClass: "fill" | "key"): RemoteInputRefusal {
  const details = (err as { details?: BrowserErrorDetails })?.details;
  const base = buildRemoteInputRefusal(err);
  return {
    ...base,
    code: details?.code || (err as { code?: string })?.code || "COMMAND_FAILED",
    inputClass: details ? base.inputClass : inputClass,
  };
}

export const RemoteBrowserControls: React.FC<RemoteBrowserControlsProps> = ({
  url = "",
  driverState,
  canPointClick = false,
  pointClickEnabled = false,
  pointClickDisabledReason = null,
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
  refusal: propRefusal,
  onRefusal,
  onClearRefusal,
  className = "",
}) => {
  const [inputUrl, setInputUrl] = useState(url);
  const [reference, setReference] = useState("");
  const [fillValue, setFillValue] = useState("");
  const [customKey, setCustomKey] = useState("");
  const [showAdvancedEval, setShowAdvancedEval] = useState(false);
  const [evalScript, setEvalScript] = useState("");
  const [evalResult, setEvalResult] = useState<string | null>(null);
  const [evalLoading, setEvalLoading] = useState(false);
  const [localRefusal, setLocalRefusal] = useState<RemoteInputRefusal | null>(null);
  const activeRefusal = propRefusal ?? localRefusal;

  // Sync incoming url, but never overwrite a URL the user is in the middle of typing
  const urlEditedRef = React.useRef(false);
  React.useEffect(() => {
    if (!urlEditedRef.current) setInputUrl(url);
  }, [url]);

  const isDriving = driverState === "driving";

  const handleNavigateSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!isDriving || !inputUrl.trim() || !onNavigate) return;
    urlEditedRef.current = false;
    onNavigate(inputUrl.trim());
  };

  const handleFillSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!isDriving) return;
    const trimmedRef = reference.trim();
    if (!trimmedRef) {
      const refusal: RemoteInputRefusal = {
        code: "BROWSER_INVALID_REFERENCE",
        reason: "invalid element reference: reference cannot be empty",
        inputClass: "fill",
        remediation: "Specify a non-empty element reference from the DOM snapshot.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      return;
    }
    const byteLength = new TextEncoder().encode(fillValue).length;
    if (byteLength > 16 * 1024) {
      const refusal: RemoteInputRefusal = {
        code: "PAYLOAD_TOO_LARGE",
        reason: `fill text too large: ${byteLength} bytes (max 16384)`,
        inputClass: "fill",
        remediation: "Split input into chunks smaller than 16 KiB.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      return;
    }
    if (!onFill) return;
    try {
      const res: unknown = onFill(trimmedRef, fillValue);
      if (res && typeof (res as Promise<unknown>).catch === "function") {
        (res as Promise<unknown>).catch((err: unknown) => {
          const refusal = commandFailureRefusal(err, "fill");
          setLocalRefusal(refusal);
          onRefusal?.(refusal);
        });
      }
    } catch (err: unknown) {
      const refusal = commandFailureRefusal(err, "fill");
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
    }
  };

  const dispatchKeypress = (key: string) => {
    if (!isDriving) return;
    const trimmed = key.trim();
    if (!trimmed) {
      const refusal: RemoteInputRefusal = {
        code: "KEY_NOT_ALLOWED",
        reason: "keypress '' not allowed (empty key)",
        inputClass: "key",
        remediation: "Provide a valid key name or character.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      return;
    }

    const lower = trimmed.toLowerCase();
    if (FORBIDDEN_APP_CHROME_KEYS.has(lower)) {
      const refusal: RemoteInputRefusal = {
        code: "KEY_NOT_ALLOWED",
        reason: `keypress '${trimmed}' not allowed (page-scoped allowlist; app-chrome shortcuts are refused)`,
        inputClass: "key",
        remediation: "App-chrome shortcuts cannot be sent to remote page sessions.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      return;
    }

    if (!onKeypress) return;
    try {
      const res: unknown = onKeypress(trimmed);
      if (res && typeof (res as Promise<unknown>).catch === "function") {
        (res as Promise<unknown>).catch((err: unknown) => {
          const refusal = commandFailureRefusal(err, "key");
          setLocalRefusal(refusal);
          onRefusal?.(refusal);
        });
      }
    } catch (err: unknown) {
      const refusal = commandFailureRefusal(err, "key");
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
    }
  };

  const handleCustomKeySubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!isDriving || !customKey.trim()) return;
    dispatchKeypress(customKey);
    setCustomKey("");
  };

  const handleRunEval = async () => {
    if (!isDriving || !evalScript.trim()) return;
    const scriptBytes = new TextEncoder().encode(evalScript).length;
    if (scriptBytes > 32 * 1024) {
      const refusal: RemoteInputRefusal = {
        code: "PAYLOAD_TOO_LARGE",
        reason: `eval script too large: ${scriptBytes} bytes (max 32768)`,
        inputClass: "eval",
        remediation: "Reduce script size to under 32 KiB.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      setEvalResult(`Refused: eval script too large (${scriptBytes} bytes > 32 KiB limit)`);
      return;
    }

    if (!onEval) return;
    setEvalLoading(true);
    setEvalResult(null);
    try {
      const res = await onEval(evalScript);
      setEvalResult(typeof res === "object" ? JSON.stringify(res, null, 2) : String(res));
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      // Structured code only: the backend maps an eval approval refusal to PERMISSION_DENIED.
      // Never infer the cause from the message text.
      const code = (err as { code?: string })?.code;
      if (code === "PERMISSION_DENIED") {
        const refusal: RemoteInputRefusal = {
          code: "PERMISSION_DENIED",
          reason: msg,
          inputClass: "eval",
          remediation: "Remote JavaScript evaluation requires explicit driver approval.",
        };
        setLocalRefusal(refusal);
        onRefusal?.(refusal);
      }
      setEvalResult(`Error: ${msg}`);
    } finally {
      setEvalLoading(false);
    }
  };

  return (
    <div
      data-testid="remote-browser-controls"
      className={`flex flex-col gap-2 p-2 bg-[#111111] border-b border-[#191919] text-[#f5f5f5] text-xs ${className}`}
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
            className="p-1 rounded bg-[#1a1b1b] hover:bg-[#141414] text-[#f5f5f5] disabled:opacity-40 disabled:cursor-not-allowed transition"
          >
            ←
          </button>
          <button
            type="button"
            data-testid="remote-browser-forward-btn"
            disabled={!isDriving}
            onClick={onForward}
            aria-label="Go forward"
            className="p-1 rounded bg-[#1a1b1b] hover:bg-[#141414] text-[#f5f5f5] disabled:opacity-40 disabled:cursor-not-allowed transition"
          >
            →
          </button>
          <button
            type="button"
            data-testid="remote-browser-reload-btn"
            disabled={!isDriving}
            onClick={onReload}
            aria-label="Reload"
            className="p-1 rounded bg-[#1a1b1b] hover:bg-[#141414] text-[#f5f5f5] disabled:opacity-40 disabled:cursor-not-allowed transition"
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
            onChange={(e) => {
              urlEditedRef.current = true;
              setInputUrl(e.target.value);
            }}
            disabled={!isDriving}
            aria-label="Page URL"
            placeholder={isDriving ? "https://example.com" : "Claim control to navigate"}
            className="w-full px-2 py-1 rounded bg-[#0a0a0a] border border-[#191919] focus:outline-none focus:border-[#346bf1] disabled:opacity-50 disabled:cursor-not-allowed text-[#f5f5f5] text-base sm:text-xs font-mono"
          />
          <button
            type="submit"
            data-testid="remote-browser-navigate-btn"
            disabled={!isDriving || !inputUrl.trim()}
            className="px-2 py-1 rounded bg-[#346bf1] hover:bg-[#346bf1]/90 disabled:opacity-40 disabled:cursor-not-allowed text-[#ffffff] font-medium transition"
          >
            Go
          </button>
        </form>

        {/* Driver Status Badge */}
        <div className="flex items-center gap-1.5 shrink-0">
          <span
            data-testid="remote-browser-driver-badge"
            role="status"
            className={`px-2 py-0.5 rounded text-[11px] font-semibold uppercase tracking-wider ${
              driverState === "driving"
                ? "bg-[#4bb8f0]/15 text-[#4bb8f0] border border-[#4bb8f0]/30"
                : driverState === "claiming"
                ? "bg-[#ffb900]/15 text-[#ffb900] border border-[#ffb900]/30"
                : driverState === "occupied"
                ? "bg-[#ff6467]/15 text-[#ff6467] border border-[#ff6467]/30"
                : driverState === "revoked"
                ? "bg-[#ff6467]/15 text-[#ff6467] border border-[#ff6467]/30"
                : "bg-[#1a1b1b] text-[#838383] border border-[#191919]"
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
              className="px-2 py-1 rounded bg-[#1a1b1b] hover:bg-[#141414] text-[#f5f5f5] text-xs font-medium border border-[#191919] transition"
            >
              Release Control
            </button>
          ) : (
            <button
              type="button"
              data-testid="remote-browser-claim-btn"
              disabled={driverState === "claiming"}
              onClick={onClaim}
              className="px-2 py-1 rounded bg-[#ffb900] hover:bg-[#ffb900]/90 disabled:opacity-40 disabled:cursor-not-allowed text-[#0a0a0a] text-xs font-medium transition"
            >
              {driverState === "claiming" ? "Claiming..." : "Take Control"}
            </button>
          )}
        </div>
      </div>

      {/* Second row: Reference fill, point-click toggle, keypress quick actions */}
      <div className="flex flex-wrap items-center justify-between gap-2 pt-1 border-t border-[#191919]">
        {/* Semantic reference manipulation */}
        <form onSubmit={handleFillSubmit} className="flex items-center gap-1.5 flex-wrap">
          <span className="text-[#838383] text-[11px] font-medium">Ref:</span>
          <input
            type="text"
            data-testid="remote-browser-ref-input"
            value={reference}
            onChange={(e) => setReference(e.target.value)}
            disabled={!isDriving}
            aria-label="Element reference"
            placeholder="e.g. e1"
            className="w-16 px-1.5 py-0.5 rounded bg-[#0a0a0a] border border-[#191919] focus:outline-none focus:border-[#346bf1] disabled:opacity-50 text-[#f5f5f5] text-base sm:text-xs font-mono"
          />
          <input
            type="text"
            data-testid="remote-browser-fill-input"
            value={fillValue}
            onChange={(e) => setFillValue(e.target.value)}
            disabled={!isDriving}
            aria-label="Text to fill"
            placeholder="Text to fill"
            className="w-28 sm:w-40 px-1.5 py-0.5 rounded bg-[#0a0a0a] border border-[#191919] focus:outline-none focus:border-[#346bf1] disabled:opacity-50 text-[#f5f5f5] text-base sm:text-xs"
          />
          <button
            type="submit"
            data-testid="remote-browser-fill-btn"
            disabled={!isDriving || !reference.trim()}
            className="px-2 py-0.5 rounded bg-[#1a1b1b] hover:bg-[#141414] disabled:opacity-40 disabled:cursor-not-allowed text-[#f5f5f5] text-xs font-medium transition border border-[#191919]"
          >
            Fill
          </button>
        </form>

        {/* Point click toggle (visible ONLY when capability indicates point click is supported) */}
        {canPointClick ? (
          <label
            data-testid="remote-browser-point-click-toggle"
            title={
              pointClickDisabledReason ||
              (isDriving ? "Toggle coordinate point click" : "Claim control to enable point click")
            }
            className={`flex items-center gap-1.5 select-none text-[11px] ${
              pointClickDisabledReason ? "cursor-not-allowed opacity-60" : "cursor-pointer"
            }`}
          >
            <input
              type="checkbox"
              checked={pointClickEnabled && !pointClickDisabledReason}
              disabled={!isDriving || Boolean(pointClickDisabledReason)}
              onChange={onTogglePointClick}
              className="rounded bg-[#0a0a0a] border-[#191919] text-[#346bf1] focus:ring-0 focus:ring-offset-0 disabled:opacity-40"
            />
            <span
              className={
                pointClickDisabledReason
                  ? "text-[#ff6467] line-through"
                  : isDriving
                  ? "text-[#f5f5f5]"
                  : "text-[#818181]"
              }
            >
              Point Click
            </span>
            {pointClickDisabledReason && (
              <span
                data-testid="remote-browser-point-click-disabled-badge"
                className="text-[10px] text-[#ff6467] bg-[#ff6467]/10 border border-[#ff6467]/30 px-1 rounded ml-0.5"
              >
                Refused
              </span>
            )}
          </label>
        ) : (
          <div
            data-testid="remote-browser-point-click-unsupported"
            className="flex items-center gap-1 text-[11px] text-[#818181] opacity-50 cursor-not-allowed select-none"
            title="Point click unsupported in v1; use reference-based interaction instead"
          >
            <span className="line-through">Point Click</span>
            <span className="text-[10px] text-[#838383] bg-[#1a1b1b] border border-[#191919] px-1 rounded">
              Unsupported
            </span>
          </div>
        )}

        {/* Allowlisted keypress quick buttons & custom key input */}
        <div className="flex items-center gap-1 flex-wrap">
          <span className="text-[#838383] text-[11px]">Keys:</span>
          {ALLOWLISTED_KEYS.map(({ label, key }) => (
            <button
              key={key}
              type="button"
              data-testid={`remote-browser-key-${key}`}
              disabled={!isDriving}
              onClick={() => dispatchKeypress(key)}
              className="px-1.5 py-0.5 rounded bg-[#1a1b1b] hover:bg-[#141414] disabled:opacity-40 disabled:cursor-not-allowed text-[#838383] text-[11px] font-mono transition border border-[#191919]"
            >
              {label}
            </button>
          ))}
          <form onSubmit={handleCustomKeySubmit} className="inline-flex items-center gap-1 ml-1">
            <input
              type="text"
              data-testid="remote-browser-custom-key-input"
              value={customKey}
              onChange={(e) => setCustomKey(e.target.value)}
              disabled={!isDriving}
              placeholder="Key"
              className="w-14 px-1 py-0.5 rounded bg-[#0a0a0a] border border-[#191919] text-[#f5f5f5] text-[11px] font-mono focus:outline-none focus:border-[#346bf1] disabled:opacity-50"
            />
            <button
              type="submit"
              data-testid="remote-browser-send-key-btn"
              disabled={!isDriving || !customKey.trim()}
              className="px-1.5 py-0.5 rounded bg-[#1a1b1b] hover:bg-[#141414] disabled:opacity-40 disabled:cursor-not-allowed text-[#f5f5f5] text-[11px] font-medium transition border border-[#191919]"
            >
              Send
            </button>
          </form>
        </div>

        {/* Collapsible advanced eval toggle */}
        <button
          type="button"
          data-testid="remote-browser-eval-toggle"
          onClick={() => setShowAdvancedEval((prev) => !prev)}
          className="text-[11px] text-[#838383] hover:text-[#f5f5f5] underline underline-offset-2 ml-auto transition"
        >
          {showAdvancedEval ? "Hide Advanced JS Eval" : "Advanced JS Eval"}
        </button>
      </div>

      {/* Controls Refusal Explanation Banner (T22) */}
      {activeRefusal && (
        <div
          data-testid="remote-browser-controls-refusal-banner"
          role="alert"
          className="flex items-center justify-between gap-2 px-2.5 py-1.5 bg-[#ff6467]/15 border border-[#ff6467]/30 text-[#ff6467] rounded text-xs"
        >
          <div className="flex items-center gap-1.5 flex-1 min-w-0">
            <span
              data-testid="remote-browser-controls-refusal-code"
              className="font-semibold text-[10px] uppercase tracking-wider px-1 py-0.5 bg-[#ff6467]/20 border border-[#ff6467]/30 rounded shrink-0"
            >
              {activeRefusal.code}
            </span>
            <span
              data-testid="remote-browser-controls-refusal-reason"
              className="text-[#f5f5f5] font-mono text-[11px] truncate"
            >
              {activeRefusal.reason}
            </span>
          </div>
          <button
            type="button"
            data-testid="remote-browser-controls-refusal-dismiss"
            onClick={() => {
              setLocalRefusal(null);
              onClearRefusal?.();
            }}
            className="text-[#838383] hover:text-[#f5f5f5] text-xs px-1 hover:bg-[#ff6467]/10 rounded"
            aria-label="Dismiss refusal notice"
          >
            ×
          </button>
        </div>
      )}

      {/* Collapsible Advanced Eval Section */}
      {showAdvancedEval && (
        <div
          data-testid="remote-browser-eval-panel"
          className="flex flex-col gap-1.5 p-2 bg-[#0a0a0a] border border-[#191919] rounded mt-1"
        >
          {/* Owner warning and truncation reminder */}
          <div className="flex flex-col gap-0.5 text-[11px] text-[#ffb900] font-medium">
            <p>Warning: Remote eval executes arbitrary JavaScript on the page with session privileges.</p>
            <p className="text-[#838383]">Result truncated to 64 KiB (65,536 UTF-8 bytes).</p>
          </div>

          <div className="flex gap-2">
            <textarea
              data-testid="remote-browser-eval-input"
              value={evalScript}
              onChange={(e) => setEvalScript(e.target.value)}
              disabled={!isDriving}
              placeholder='document.title or window.location.href'
              rows={2}
              className="flex-1 p-1.5 rounded bg-[#111111] border border-[#191919] text-[#f5f5f5] text-xs font-mono focus:outline-none focus:border-[#346bf1] disabled:opacity-50"
            />
            <button
              type="button"
              data-testid="remote-browser-eval-run-btn"
              disabled={!isDriving || !evalScript.trim() || evalLoading}
              onClick={handleRunEval}
              className="px-3 py-1 bg-[#ffb900]/20 hover:bg-[#ffb900]/30 border border-[#ffb900]/40 disabled:opacity-40 disabled:cursor-not-allowed text-[#ffb900] text-xs font-medium rounded self-start transition"
            >
              {evalLoading ? "Running..." : "Run Eval"}
            </button>
          </div>

          {evalResult && (
            <div
              data-testid="remote-browser-eval-result"
              className="p-1.5 bg-[#111111] rounded border border-[#191919] text-[11px] font-mono text-[#f5f5f5] max-h-32 overflow-y-auto whitespace-pre-wrap break-all"
            >
              {evalResult}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
