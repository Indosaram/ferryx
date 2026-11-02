/**
 * RemoteBrowserWorkspace Component (§4.4, §4.5, Phase 6)
 *
 * Full composition view wrapping useRemoteBrowser, useRemoteBrowserDriver,
 * and RemoteBrowser. Wires controls, mainframe point click, and mobile IME handling.
 */

import React, { useEffect, useRef, useState } from "react";
import { ChevronLeft, Keyboard } from "lucide-react";
import {
  BROWSER_FOCUS_RING,
  RemoteBrowser,
  useCoarsePointer,
  type RemoteBrowserPointClickEvent,
} from "./RemoteBrowser";
import { RemoteBrowserControls } from "./RemoteBrowserControls";
import { reconcileMapRevision, type BrowserSubscribeOptions } from "./browserProtocol";
import { useRemoteBrowser } from "./useRemoteBrowser";
import { useRemoteBrowserDriver } from "./useRemoteBrowserDriver";
import { userFacingError } from "./userFacingError";

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

// The remote page tracks the highest fill revision per element (`__ferryx_tracked_revision`) and refuses
// anything older. A per-mount counter restarts at 1 and would be refused after a remount, so revisions
// come from one module-level, strictly increasing, clock-seeded counter shared by every mount.
let lastImeRevision = 0;
function nextImeRevision(): number {
  lastImeRevision = Math.max(lastImeRevision + 1, Date.now());
  return lastImeRevision;
}

function isImeConfirmingKey(e: React.KeyboardEvent, composing: boolean): boolean {
  return composing || e.nativeEvent.isComposing || e.keyCode === 229;
}

function pageLabel(title: string | undefined, url: string | undefined): string {
  const trimmedTitle = title?.trim();
  if (trimmedTitle) return trimmedTitle;
  if (url) {
    try {
      return new URL(url).host || "Browser";
    } catch {
      return "Browser";
    }
  }
  return "Browser";
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
  const phone = useCoarsePointer();
  // 1. Screencast viewer stream
  const session = useRemoteBrowser({
    baseUrl,
    browserId,
    deviceToken,
    options,
  });

  const {
    frame,
    browserState,
    hello,
    error: browserError,
    client,
    subscriptionId,
  } = session;

  // Extract browser identity guards for command dispatcher
  // No invented instance/epoch fallbacks: a missing identity must make the driver's guard checks refuse the command.
  const browserInstanceId =
    frame?.metadata.browserInstanceId ?? hello?.browserInstanceId ?? null;
  const desktopEpoch =
    frame?.metadata.desktopEpoch ?? hello?.desktopEpoch ?? null;
  const frameGeneration = frame?.metadata.documentGeneration;
  const stateGeneration = browserState?.documentGeneration;
  const documentGeneration = frameGeneration && stateGeneration
    ? (BigInt(frameGeneration) > BigInt(stateGeneration) ? frameGeneration : stateGeneration)
    : frameGeneration ?? stateGeneration ?? "1";
  const viewportRevision = frame?.metadata.viewportRevision;

  // Track snapshotId and mapRevision (reconciled to decimal string)
  const [workspaceSnapshotId, setWorkspaceSnapshotId] = useState<string | null>(
    browserState?.snapshotId ?? null
  );
  const [workspaceMapRevision, setWorkspaceMapRevision] = useState<string | null>(
    reconcileMapRevision(browserState?.mapRevision) ?? null
  );

  useEffect(() => {
    if (browserState?.snapshotId) {
      setWorkspaceSnapshotId(browserState.snapshotId);
    }
    if (browserState?.mapRevision !== undefined) {
      setWorkspaceMapRevision(reconcileMapRevision(browserState.mapRevision) ?? null);
    }
  }, [browserState?.snapshotId, browserState?.mapRevision]);

  // 2. Remote driver lifecycle hook
  const driver = useRemoteBrowserDriver({
    client,
    browserId,
    subscriptionId,
    browserInstanceId,
    desktopEpoch,
    documentGeneration,
    viewportRevision,
    snapshotId: workspaceSnapshotId ?? browserState?.snapshotId,
    mapRevision: workspaceMapRevision ?? reconcileMapRevision(browserState?.mapRevision),
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
  const composingRef = useRef(false);
  const [imeTargetRef, setImeTargetRef] = useState("active");
  const [showImeBar, setShowImeBar] = useState(false);

  // Revision-Keyed Submission Queue for IME (R4-12, R5-13)
  interface ImeSubmissionRecord {
    revision: number;
    submittedChunk: string;
    fullValue: string;
    targetRef: string;
    queueKey: string;
    completed: boolean;
    superseded?: boolean;
    executing?: boolean;
  }
  const pendingImeSubmissionsRef = useRef<ImeSubmissionRecord[]>([]);
  // Text the remote field already holds per (target, lease). The fill command replaces the whole field value,
  // so every fill must carry committed + still-unsubmitted text, never just the unsubmitted tail.
  const committedImeRef = useRef<Map<string, string>>(new Map());
  const [imeError, setImeError] = useState<Error | null>(null);
  const [actionError, setActionError] = useState<Error | null>(null);
  const surfaceAction = (action: Promise<unknown>) => {
    setActionError(null);
    action.catch((err: unknown) => {
      setActionError(err instanceof Error ? err : new Error(String(err)));
    });
  };
  // Per (target, lease) latest queued revision tracking (R5-13)
  const latestQueuedRevRef = useRef<Map<string, number>>(new Map());

  // Wire point click with normalized (u, v) coordinates when in driving mode
  const handlePointClick = (point: RemoteBrowserPointClickEvent) => {
    if (driver.driverState !== "driving" || !canPointClick || !pointClickEnabled) {
      return;
    }
    return driver.click({
      ...point,
      u: point.u,
      v: point.v,
      streamId: point.streamId,
      sequenceNumber: point.sequenceNumber ?? point.seq,
      documentGeneration: point.documentGeneration,
      viewportRevision: point.viewportRevision,
      browserInstanceId: point.browserInstanceId,
    });
  };

  // Mobile IME submission: do NOT dispatch intermediate composition keystrokes; send confirmed text via fill
  // Revision-Keyed Submission Queue: monotonic revision counter, ordered completion, and coherent full-value replacement (R4-12, R5-13)
  const handleImeSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (composingRef.current) {
      return;
    }
    if (driver.driverState !== "driving") return;

    // Calculate characters currently in flight across pending submissions
    let inFlightLength = 0;
    for (const record of pendingImeSubmissionsRef.current) {
      if (!record.completed) {
        inFlightLength += record.submittedChunk.length;
      }
    }

    const unsubmittedText = imeText.slice(inFlightLength);
    if (!unsubmittedText.trim()) return;

    // Monotonic submission revision counter
    const targetRef = imeTargetRef || "active";
    const queueKey = `${targetRef}:${driver.leaseEpoch || "none"}`;
    setImeError(null);
    const submissionRev = nextImeRevision();
    latestQueuedRevRef.current.set(queueKey, submissionRev);

    // Cancel any older non-executing pending submissions for the same target and lease (R5-13)
    for (const older of pendingImeSubmissionsRef.current) {
      if (older.targetRef === targetRef && older.revision < submissionRev && !older.executing) {
        older.superseded = true;
      }
    }

    // Coherent full-value replacement: the remote already holds the committed text, the buffer holds the rest.
    const fullValue = `${committedImeRef.current.get(queueKey) ?? ""}${imeText}`;
    const record: ImeSubmissionRecord = {
      revision: submissionRev,
      submittedChunk: unsubmittedText,
      fullValue,
      targetRef,
      queueKey,
      completed: false,
    };
    pendingImeSubmissionsRef.current.push(record);

    const retireCompleted = () => {
      while (
        pendingImeSubmissionsRef.current.length > 0 &&
        pendingImeSubmissionsRef.current[0].completed
      ) {
        const head = pendingImeSubmissionsRef.current.shift()!;
        committedImeRef.current.set(
          head.queueKey,
          `${committedImeRef.current.get(head.queueKey) ?? ""}${head.submittedChunk}`
        );
        setImeText((current) => {
          // Only slice if current buffer starts with this chunk, preserving unrelated edits
          if (current.startsWith(head.submittedChunk)) {
            return current.slice(head.submittedChunk.length);
          }
          return current;
        });
      }
    };

    // Check if superseded before execution starts (R5-13)
    if (record.superseded || (latestQueuedRevRef.current.get(queueKey) ?? 0) > record.revision) {
      record.completed = true;
      retireCompleted();
      return;
    }

    record.executing = true;

    // Distinct fill flows (R6-10):
    // 'active'-element fills and CSS-selector fills must NOT require/attach a snapshot reference;
    // only snapshot-id fills attach snapshotId+mapRevision.
    const trimmedTarget = (record.targetRef || "").trim();
    const isActiveTarget = !trimmedTarget || trimmedTarget === "active";
    const isCssTarget =
      !isActiveTarget &&
      (trimmedTarget.startsWith("#") ||
        trimmedTarget.startsWith(".") ||
        trimmedTarget.startsWith("["));
    const isSnapshotRef = !isActiveTarget && !isCssTarget;

    let effectiveSnapId = workspaceSnapshotId ?? driver.snapshotId;
    let effectiveMapRev = workspaceMapRevision ?? driver.mapRevision;
    if (isSnapshotRef && (!effectiveSnapId || !effectiveMapRev) && client && typeof client.takeSnapshot === "function") {
      try {
        const snap = await client.takeSnapshot(browserId);
        effectiveSnapId = snap.snapshotId;
        effectiveMapRev = reconcileMapRevision(snap.mapRevision) ?? null;
        setWorkspaceSnapshotId(effectiveSnapId);
        setWorkspaceMapRevision(effectiveMapRev);
      } catch {
        // Fall through to driver.fill which handles/reports
      }
    }

    // Recheck supersession before remote write (R5-13)
    if ((latestQueuedRevRef.current.get(queueKey) ?? 0) > record.revision) {
      record.completed = true;
      retireCompleted();
      return;
    }

    try {
      if (client && driver.leaseEpoch && browserInstanceId && desktopEpoch && documentGeneration) {
        const fillParams: Record<string, unknown> = {
          value: record.fullValue,
          text: record.fullValue,
          revision: record.revision,
          imeRevision: record.revision,
        };

        if (isSnapshotRef) {
          fillParams.reference = record.targetRef;
          if (effectiveSnapId) {
            fillParams.snapshotId = effectiveSnapId;
          }
          if (effectiveMapRev) {
            fillParams.mapRevision = effectiveMapRev;
          }
        } else if (isCssTarget) {
          fillParams.selector = record.targetRef;
        }
        // Active element targets attach neither reference nor selector nor snapshot metadata

        await client.sendCommand({
          browserId,
          leaseEpoch: driver.leaseEpoch,
          browserInstanceId,
          desktopEpoch,
          documentGeneration,
          command: "fill",
          params: fillParams,
        });
      } else {
        if (isSnapshotRef) {
          await driver.fill(
            record.targetRef,
            record.fullValue,
            {
              snapshotId: effectiveSnapId ?? undefined,
              mapRevision: effectiveMapRev ?? undefined,
            }
          );
        } else {
          await driver.fill(
            record.targetRef,
            record.fullValue
          );
        }
      }
      // Mark this submission record completed
      record.completed = true;
      retireCompleted();
    } catch (err) {
      // On failure, remove from queue without slicing so user does not lose typed text, and tell the user
      setImeError(err instanceof Error ? err : new Error(String(err)));
      const idx = pendingImeSubmissionsRef.current.indexOf(record);
      if (idx !== -1) {
        pendingImeSubmissionsRef.current.splice(idx, 1);
      }
    }
  };

  const focus = BROWSER_FOCUS_RING;
  const touchTarget = phone ? "min-h-11" : "min-h-7";
  const imeInputCls = `rounded bg-chat-surface border border-chat-border text-chat-foreground text-base sm:text-xs ${focus} ${
    phone ? "min-h-11 px-3" : "px-2 py-1"
  }`;

  const controls = (
    <div className="flex flex-col">
      {onBack && (
        <div className="flex items-center justify-between gap-2 px-2 py-1 bg-chat-screen border-b border-chat-border text-xs">
          <button
            type="button"
            data-testid="remote-browser-workspace-back-btn"
            onClick={onBack}
            className={`flex items-center gap-1 pr-2 rounded text-chat-foreground-secondary hover:text-chat-foreground transition-colors ${touchTarget} ${focus}`}
          >
            <ChevronLeft className="size-4" aria-hidden="true" />
            Back to sessions
          </button>
          <span data-testid="remote-browser-page-label" className="text-chat-foreground-secondary text-xs truncate min-w-0 flex-1 text-center">
            {pageLabel(browserState?.title, browserState?.url)}
          </span>
          <button
            type="button"
            data-testid="remote-browser-ime-toggle-btn"
            aria-pressed={showImeBar}
            onClick={() => setShowImeBar((prev) => !prev)}
            className={`flex items-center gap-1.5 px-3 rounded border transition-colors ${touchTarget} ${focus} ${
              showImeBar
                ? "bg-chat-surface-raised border-chat-primary text-chat-primary"
                : "bg-chat-surface-raised border-chat-border text-chat-foreground-secondary hover:text-chat-foreground"
            }`}
          >
            <Keyboard className="size-4" aria-hidden="true" />
            Type text
          </button>
        </div>
      )}

      <RemoteBrowserControls
        url={browserState?.url || ""}
        driverState={driver.driverState}
        phone={phone}
        canPointClick={canPointClick}
        pointClickEnabled={pointClickEnabled}
        onTogglePointClick={() => setPointClickEnabled((prev) => !prev)}
        onClaim={() => surfaceAction(driver.claim())}
        onRelease={driver.release}
        onNavigate={(url) => surfaceAction(driver.navigate(url))}
        onBack={() => surfaceAction(driver.back())}
        onForward={() => surfaceAction(driver.forward())}
        onReload={() => surfaceAction(driver.reload())}
        onFill={driver.fill}
        onKeypress={driver.keypress}
        onEval={driver.evalJs}
      />

      {showImeBar && (
        <form
          onSubmit={handleImeSubmit}
          data-testid="remote-browser-ime-bar"
          className="flex items-center gap-2 p-1.5 bg-chat-screen border-b border-chat-border text-xs"
        >
          <input
            type="text"
            data-testid="remote-browser-ime-ref-input"
            value={imeTargetRef}
            onChange={(e) => setImeTargetRef(e.target.value)}
            aria-label="Field to type into"
            placeholder="Field (e.g. e1)"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            className={`w-24 font-mono ${imeInputCls}`}
          />
          <input
            type="text"
            data-testid="remote-browser-ime-text-input"
            value={imeText}
            onChange={(e) => setImeText(e.target.value)}
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={(e) => {
              composingRef.current = false;
              setImeText(e.currentTarget.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && isImeConfirmingKey(e, composingRef.current)) {
                e.preventDefault();
              }
            }}
            aria-label="Text to type"
            placeholder="Type text..."
            enterKeyHint="send"
            className={`flex-1 min-w-0 ${imeInputCls}`}
          />
          <button
            type="submit"
            data-testid="remote-browser-ime-send-btn"
            disabled={driver.driverState !== "driving" || !imeText.trim()}
            className={`px-4 bg-chat-primary text-chat-primary-foreground hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed rounded text-sm font-medium transition-colors ${touchTarget} ${focus}`}
          >
            Send
          </button>
        </form>
      )}

      {(driver.error || imeError || actionError || browserError) && (
        <div
          data-testid="remote-browser-driver-error"
          role="alert"
          className="px-2 py-1.5 bg-chat-surface-raised border-b border-chat-border text-chat-danger text-xs flex flex-col gap-1"
        >
          <span>{userFacingError(driver.error ?? imeError ?? actionError ?? browserError)}</span>
          <details className="text-chat-foreground-secondary">
            <summary className={`cursor-pointer rounded ${focus}`}>Details</summary>
            <span className="font-mono break-words">{(driver.error ?? imeError ?? actionError ?? browserError)?.message}</span>
          </details>
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
        session={session}
        controls={controls}
        onPointClick={handlePointClick}
      />
    </div>
  );
};
