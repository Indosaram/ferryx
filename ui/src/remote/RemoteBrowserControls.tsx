/**
 * Remote Browser Controls Toolbar (§4.4, Phase 6)
 *
 * Designed to fit into RemoteBrowser's `controls` slot.
 * Desktop: URL bar, claim/release, point click toggle, reference fill, allowlisted keys and a
 * confirm-gated advanced eval. Phone: one navigation row plus claim and a Tools sheet; eval is not offered.
 */

import React, { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Delete,
  RotateCw,
  SlidersHorizontal,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  BROWSER_FOCUS_RING,
  buildRemoteInputRefusal,
  describeRemoteInputRefusal,
  useCoarsePointer,
  type RemoteInputRefusal,
} from "./RemoteBrowser";
import { MobileSheet } from "./chat/MobileSheet";
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

interface AllowlistedKey {
  label: string;
  key: string;
  icon?: LucideIcon;
}

export const ALLOWLISTED_KEYS: readonly AllowlistedKey[] = [
  { label: "Enter", key: "Enter" },
  { label: "Tab", key: "Tab" },
  { label: "Esc", key: "Escape" },
  { label: "Arrow left", key: "ArrowLeft", icon: ArrowLeft },
  { label: "Arrow right", key: "ArrowRight", icon: ArrowRight },
  { label: "Arrow up", key: "ArrowUp", icon: ArrowUp },
  { label: "Arrow down", key: "ArrowDown", icon: ArrowDown },
  { label: "Backspace", key: "Backspace", icon: Delete },
  { label: "Del", key: "Delete" },
];

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
  /** Overrides the touch-device check: true renders the phone layout, false the desktop one. */
  phone?: boolean;
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
  phone: phoneProp,
  className = "",
}) => {
  const coarsePointer = useCoarsePointer();
  const phone = phoneProp ?? coarsePointer;
  const [inputUrl, setInputUrl] = useState(url);
  const [reference, setReference] = useState("");
  const [fillValue, setFillValue] = useState("");
  const [customKey, setCustomKey] = useState("");
  const [showAdvancedEval, setShowAdvancedEval] = useState(false);
  const [evalConfirming, setEvalConfirming] = useState(false);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [claimConfirming, setClaimConfirming] = useState(false);
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

  useEffect(() => {
    setClaimConfirming(false);
  }, [driverState]);

  const handleClaimClick = () => {
    if (driverState === "occupied") {
      setClaimConfirming(true);
      return;
    }
    onClaim?.();
  };

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

  const handleRunEvalClick = () => {
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
    setEvalConfirming(true);
  };

  const handleConfirmEval = async () => {
    setEvalConfirming(false);
    if (!isDriving || !evalScript.trim() || !onEval) return;
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

  const focus = BROWSER_FOCUS_RING;
  const iconBtn = `flex items-center justify-center rounded bg-chat-surface-raised hover:bg-chat-surface-hover text-chat-foreground disabled:opacity-40 disabled:cursor-not-allowed transition-colors ${focus} ${
    phone ? "size-11" : "size-7"
  }`;
  const secondaryBtn = `rounded border border-chat-border bg-chat-surface-raised hover:bg-chat-surface-hover text-chat-foreground font-medium disabled:opacity-40 disabled:cursor-not-allowed transition-colors ${focus} ${
    phone ? "min-h-11 px-4 text-sm" : "px-2 py-1 text-xs"
  }`;
  const primaryBtn = `rounded bg-chat-primary text-chat-primary-foreground hover:brightness-110 font-medium disabled:opacity-40 disabled:cursor-not-allowed transition-colors ${focus} ${
    phone ? "min-h-11 px-4 text-sm" : "px-2 py-1 text-xs"
  }`;
  const inputCls = `rounded bg-chat-screen border border-chat-border text-chat-foreground disabled:opacity-50 disabled:cursor-not-allowed ${focus} ${
    phone ? "min-h-11 px-3 text-base" : "px-1.5 py-1 text-xs"
  }`;

  const navButtons = (
    <div className="flex items-center gap-1">
      <button
        type="button"
        data-testid="remote-browser-back-btn"
        disabled={!isDriving}
        onClick={onBack}
        aria-label="Go back"
        className={iconBtn}
      >
        <ArrowLeft className="size-4" aria-hidden="true" />
      </button>
      <button
        type="button"
        data-testid="remote-browser-forward-btn"
        disabled={!isDriving}
        onClick={onForward}
        aria-label="Go forward"
        className={iconBtn}
      >
        <ArrowRight className="size-4" aria-hidden="true" />
      </button>
      <button
        type="button"
        data-testid="remote-browser-reload-btn"
        disabled={!isDriving}
        onClick={onReload}
        aria-label="Reload"
        className={iconBtn}
      >
        <RotateCw className="size-4" aria-hidden="true" />
      </button>
    </div>
  );

  const urlForm = (
    <form onSubmit={handleNavigateSubmit} className={`flex-1 flex items-center gap-1 ${phone ? "min-w-0" : "min-w-[180px]"}`}>
      <input
        type="text"
        inputMode="url"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        enterKeyHint="go"
        data-testid="remote-browser-url-input"
        value={inputUrl}
        onChange={(e) => {
          urlEditedRef.current = true;
          setInputUrl(e.target.value);
        }}
        disabled={!isDriving}
        aria-label="Page URL"
        placeholder={isDriving ? "https://example.com" : "Take control to navigate"}
        className={`w-full min-w-0 font-mono ${inputCls}`}
      />
      {!phone && (
        <button
          type="submit"
          data-testid="remote-browser-navigate-btn"
          disabled={!isDriving || !inputUrl.trim()}
          className={primaryBtn}
        >
          Go
        </button>
      )}
    </form>
  );

  const driverLabel =
    driverState === "driving"
      ? "Driving"
      : driverState === "claiming"
      ? "Taking control..."
      : driverState === "occupied"
      ? "Occupied"
      : driverState === "revoked"
      ? "Revoked"
      : "Viewing";

  const driverBadge = (
    <span
      data-testid="remote-browser-driver-badge"
      role="status"
      className={`rounded px-2 py-0.5 text-xs font-medium bg-chat-surface-raised ${
        phone ? "sr-only" : ""
      } ${
        driverState === "driving"
          ? "text-status-success"
          : driverState === "claiming"
          ? "text-status-warning"
          : driverState === "occupied" || driverState === "revoked"
          ? "text-chat-danger"
          : "text-chat-foreground-secondary"
      }`}
    >
      {driverLabel}
    </span>
  );

  const claimButton =
    driverState === "driving" ? (
      <button
        type="button"
        data-testid="remote-browser-release-btn"
        onClick={onRelease}
        className={`${secondaryBtn} ${phone ? "flex-1" : ""}`}
      >
        Release control
      </button>
    ) : (
      <button
        type="button"
        data-testid="remote-browser-claim-btn"
        disabled={driverState === "claiming"}
        onClick={handleClaimClick}
        className={`${primaryBtn} ${phone ? "flex-1" : ""}`}
      >
        {driverState === "claiming" ? "Taking control..." : "Take control"}
      </button>
    );

  const occupiedNotice =
    driverState === "occupied" ? (
      <div data-testid="remote-browser-occupied-notice" className="flex flex-col gap-2 text-xs text-chat-foreground-secondary">
        <p>Someone else is controlling this browser. Taking control will stop them.</p>
        {claimConfirming && (
          <div className="flex items-center gap-2">
            <button
              type="button"
              data-testid="remote-browser-claim-confirm-btn"
              onClick={() => {
                setClaimConfirming(false);
                onClaim?.();
              }}
              className={primaryBtn}
            >
              Take control anyway
            </button>
            <button
              type="button"
              data-testid="remote-browser-claim-cancel-btn"
              onClick={() => setClaimConfirming(false)}
              className={secondaryBtn}
            >
              Cancel
            </button>
          </div>
        )}
      </div>
    ) : null;

  const fillForm = (
    <form onSubmit={handleFillSubmit} className={`flex items-center gap-1.5 flex-wrap ${phone ? "w-full" : ""}`}>
      <span className="text-chat-foreground-secondary text-xs font-medium">Ref:</span>
      <input
        type="text"
        data-testid="remote-browser-ref-input"
        value={reference}
        onChange={(e) => setReference(e.target.value)}
        disabled={!isDriving}
        aria-label="Element reference"
        placeholder="e.g. e1"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        className={`font-mono ${phone ? "w-20" : "w-16"} ${inputCls}`}
      />
      <input
        type="text"
        data-testid="remote-browser-fill-input"
        value={fillValue}
        onChange={(e) => setFillValue(e.target.value)}
        disabled={!isDriving}
        aria-label="Text to fill"
        placeholder="Text to fill"
        className={`${phone ? "flex-1 min-w-0" : "w-40"} ${inputCls}`}
      />
      <button
        type="submit"
        data-testid="remote-browser-fill-btn"
        disabled={!isDriving || !reference.trim()}
        className={secondaryBtn}
      >
        Fill
      </button>
    </form>
  );

  const showPointClick = canPointClick && !pointClickDisabledReason;
  const pointClickToggle = showPointClick ? (
    <label
      data-testid="remote-browser-point-click-toggle"
      title={isDriving ? "Toggle coordinate point click" : "Take control to enable point click"}
      className={`flex items-center gap-2 select-none text-xs cursor-pointer ${phone ? "min-h-11" : ""}`}
    >
      <input
        type="checkbox"
        checked={pointClickEnabled}
        disabled={!isDriving}
        onChange={onTogglePointClick}
        className={`rounded accent-chat-primary disabled:opacity-40 ${focus} ${phone ? "size-5" : ""}`}
      />
      <span className={isDriving ? "text-chat-foreground" : "text-chat-foreground-secondary"}>Point click</span>
    </label>
  ) : null;

  const keysRow = (
    <div className="flex items-center gap-1 flex-wrap">
      <span className="text-chat-foreground-secondary text-xs">Keys:</span>
      {ALLOWLISTED_KEYS.map(({ label, key, icon: Icon }) => (
        <button
          key={key}
          type="button"
          data-testid={`remote-browser-key-${key}`}
          disabled={!isDriving}
          onClick={() => dispatchKeypress(key)}
          aria-label={Icon ? label : undefined}
          className={`flex items-center justify-center rounded border border-chat-border bg-chat-surface-raised hover:bg-chat-surface-hover text-chat-foreground-secondary text-xs font-mono disabled:opacity-40 disabled:cursor-not-allowed transition-colors ${focus} ${
            phone ? "min-h-11 min-w-11 px-3" : "px-1.5 py-0.5"
          }`}
        >
          {Icon ? <Icon className="size-4" aria-hidden="true" /> : label}
        </button>
      ))}
      <form onSubmit={handleCustomKeySubmit} className="inline-flex items-center gap-1 ml-1">
        <input
          type="text"
          data-testid="remote-browser-custom-key-input"
          value={customKey}
          onChange={(e) => setCustomKey(e.target.value)}
          disabled={!isDriving}
          aria-label="Custom key"
          placeholder="Key"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          className={`font-mono ${phone ? "w-24" : "w-14"} ${inputCls}`}
        />
        <button
          type="submit"
          data-testid="remote-browser-send-key-btn"
          disabled={!isDriving || !customKey.trim()}
          className={secondaryBtn}
        >
          Send
        </button>
      </form>
    </div>
  );

  const refusalBanner = activeRefusal ? (
    <div
      data-testid="remote-browser-controls-refusal-banner"
      role="alert"
      className="flex items-start justify-between gap-2 pl-2.5 pr-1 py-1 bg-chat-surface-raised border border-chat-border text-chat-foreground rounded text-xs"
    >
      <div className="flex flex-col flex-1 min-w-0 gap-1 py-1.5">
        <span data-testid="remote-browser-controls-refusal-message" className="font-medium">
          {describeRemoteInputRefusal(activeRefusal)}
        </span>
        <details className="text-chat-foreground-secondary">
          <summary className={`cursor-pointer w-fit rounded ${focus}`}>Details</summary>
          <div className="mt-1 flex flex-col gap-0.5 font-mono break-words">
            <span data-testid="remote-browser-controls-refusal-code">{activeRefusal.code}</span>
            <span data-testid="remote-browser-controls-refusal-reason">{activeRefusal.reason}</span>
          </div>
        </details>
      </div>
      <button
        type="button"
        data-testid="remote-browser-controls-refusal-dismiss"
        onClick={() => {
          setLocalRefusal(null);
          onClearRefusal?.();
        }}
        className={`shrink-0 flex items-center justify-center rounded text-chat-foreground-secondary hover:text-chat-foreground hover:bg-chat-surface-hover transition-colors ${focus} ${
          phone ? "size-11" : "size-7"
        }`}
        aria-label="Dismiss notice"
      >
        <X className="size-4" aria-hidden="true" />
      </button>
    </div>
  ) : null;

  const rootCls = `flex flex-col gap-2 p-2 bg-chat-surface border-b border-chat-border text-chat-foreground text-xs ${className}`;

  if (phone) {
    return (
      <div data-testid="remote-browser-controls" className={rootCls}>
        <div className="flex items-center gap-1">
          {navButtons}
          {urlForm}
        </div>
        <div className="flex items-center gap-2">
          {driverBadge}
          {claimButton}
          <button
            type="button"
            data-testid="remote-browser-tools-btn"
            onClick={() => setToolsOpen(true)}
            aria-haspopup="dialog"
            className={`${secondaryBtn} flex items-center gap-2`}
          >
            <SlidersHorizontal className="size-4" aria-hidden="true" />
            Tools
          </button>
        </div>
        {occupiedNotice}
        {!toolsOpen && refusalBanner}
        <MobileSheet
          open={toolsOpen}
          onOpenChange={setToolsOpen}
          title="Tools"
          ariaLabel="Browser tools"
          testId="remote-browser-tools-sheet"
        >
          <div className="flex flex-col gap-4 p-2">
            {refusalBanner}
            {fillForm}
            {pointClickToggle}
            {keysRow}
          </div>
        </MobileSheet>
      </div>
    );
  }

  return (
    <div data-testid="remote-browser-controls" className={rootCls}>
      <div className="flex flex-wrap items-center gap-2">
        {navButtons}
        {urlForm}
        <div className="flex items-center gap-1.5 shrink-0">
          {driverBadge}
          {claimButton}
        </div>
      </div>

      {occupiedNotice}

      <div className="flex flex-wrap items-center justify-between gap-2 pt-1 border-t border-chat-border">
        {fillForm}
        {pointClickToggle}
        {keysRow}
        <button
          type="button"
          data-testid="remote-browser-eval-toggle"
          onClick={() => {
            setShowAdvancedEval((prev) => !prev);
            setEvalConfirming(false);
          }}
          className={`text-xs text-chat-foreground-secondary hover:text-chat-foreground underline underline-offset-2 ml-auto rounded transition-colors ${focus}`}
        >
          {showAdvancedEval ? "Hide advanced JS eval" : "Advanced JS eval"}
        </button>
      </div>

      {refusalBanner}

      {showAdvancedEval && (
        <div
          data-testid="remote-browser-eval-panel"
          className="flex flex-col gap-1.5 p-2 bg-chat-screen border border-chat-border rounded mt-1"
        >
          <div className="flex flex-col gap-0.5 text-xs font-medium">
            <p className="text-status-warning">
              Warning: Remote eval executes arbitrary JavaScript on the page with session privileges.
            </p>
            <p className="text-chat-foreground-secondary">Result truncated to 64 KiB (65,536 UTF-8 bytes).</p>
          </div>

          <div className="flex gap-2">
            <textarea
              data-testid="remote-browser-eval-input"
              value={evalScript}
              onChange={(e) => {
                setEvalScript(e.target.value);
                setEvalConfirming(false);
              }}
              disabled={!isDriving}
              aria-label="Script to run"
              placeholder="document.title or window.location.href"
              rows={2}
              className={`flex-1 p-1.5 rounded bg-chat-surface border border-chat-border text-chat-foreground text-base sm:text-xs font-mono disabled:opacity-50 ${focus}`}
            />
            <button
              type="button"
              data-testid="remote-browser-eval-run-btn"
              disabled={!isDriving || !evalScript.trim() || evalLoading || evalConfirming}
              onClick={handleRunEvalClick}
              className={`${secondaryBtn} self-start [@media(pointer:coarse)]:min-h-11`}
            >
              {evalLoading ? "Running..." : "Run eval"}
            </button>
          </div>

          {evalConfirming && (
            <div
              data-testid="remote-browser-eval-confirm"
              role="alertdialog"
              aria-label="Confirm script"
              className="flex flex-col gap-2 p-2 bg-chat-surface-raised border border-chat-border rounded"
            >
              <p className="text-chat-foreground">Run this script on the page? It can read and change anything you can see.</p>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  data-testid="remote-browser-eval-confirm-btn"
                  onClick={handleConfirmEval}
                  className={`${primaryBtn} [@media(pointer:coarse)]:min-h-11`}
                >
                  Run script
                </button>
                <button
                  type="button"
                  data-testid="remote-browser-eval-cancel-btn"
                  onClick={() => setEvalConfirming(false)}
                  className={`${secondaryBtn} [@media(pointer:coarse)]:min-h-11`}
                >
                  Cancel
                </button>
              </div>
            </div>
          )}

          {evalResult && (
            <div
              data-testid="remote-browser-eval-result"
              className="p-1.5 bg-chat-surface rounded border border-chat-border text-xs font-mono text-chat-foreground max-h-32 overflow-y-auto whitespace-pre-wrap break-all"
            >
              {evalResult}
            </div>
          )}
        </div>
      )}
    </div>
  );
};
