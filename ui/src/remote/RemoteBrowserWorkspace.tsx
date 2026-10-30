/**
 * RemoteBrowserWorkspace Component (§4.4, §4.5, Phase 6)
 *
 * Full composition view wrapping useRemoteBrowser, useRemoteBrowserDriver,
 * and RemoteBrowser. Wires controls, mainframe point click, and mobile IME handling.
 */

import React, { useEffect, useRef, useState } from "react";
import { RemoteBrowser, type RemoteBrowserPointClickEvent } from "./RemoteBrowser";
import { RemoteBrowserControls } from "./RemoteBrowserControls";
import { reconcileMapRevision, type BrowserSubscribeOptions } from "./browserProtocol";
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

// The remote page tracks the highest fill revision per element (`__ferryx_tracked_revision`) and refuses
// anything older. A per-mount counter restarts at 1 and would be refused after a remount, so revisions
// come from one module-level, strictly increasing, clock-seeded counter shared by every mount.
let lastImeRevision = 0;
function nextImeRevision(): number {
  lastImeRevision = Math.max(lastImeRevision + 1, Date.now());
  return lastImeRevision;
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
  const documentGeneration =
    frame?.metadata.documentGeneration ?? browserState?.documentGeneration ?? "1";
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
  const [isComposing, setIsComposing] = useState(false);
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
    if (isComposing) {
      // Do NOT send during composition
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

  const controls = (
    <div className="flex flex-col">
      {/* Header with Back button if provided */}
      {onBack && (
        <div className="flex items-center justify-between px-2 py-1 bg-[#0a0a0a] border-b border-[#191919] text-xs">
          <button
            type="button"
            data-testid="remote-browser-workspace-back-btn"
            onClick={onBack}
            className="flex items-center gap-1 text-[#838383] hover:text-[#f5f5f5] transition"
          >
            ← Back to Sessions
          </button>
          <span className="font-mono text-[#838383] text-[11px] truncate max-w-[200px]">
            {browserId}
          </span>
          <button
            type="button"
            data-testid="remote-browser-ime-toggle-btn"
            onClick={() => setShowImeBar((prev) => !prev)}
            className={`text-[11px] px-1.5 py-0.5 rounded border transition ${
              showImeBar
                ? "bg-[#346bf1]/20 border-[#346bf1]/60 text-[#346bf1]"
                : "bg-[#1a1b1b] border-[#191919] text-[#838383]"
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

      {/* Mobile IME input bar (safely buffers text and sends confirmed text via fill) */}
      {showImeBar && (
        <form
          onSubmit={handleImeSubmit}
          data-testid="remote-browser-ime-bar"
          className="flex items-center gap-2 p-1.5 bg-[#0a0a0a] border-b border-[#191919] text-xs"
        >
          <span className="text-[#838383] text-[11px] shrink-0">Mobile IME:</span>
          <input
            type="text"
            data-testid="remote-browser-ime-ref-input"
            value={imeTargetRef}
            onChange={(e) => setImeTargetRef(e.target.value)}
            placeholder="Target Ref (e.g. e1)"
            className="w-20 px-1.5 py-0.5 rounded bg-[#111111] border border-[#191919] text-[#f5f5f5] text-xs font-mono"
          />
          <input
            type="text"
            data-testid="remote-browser-ime-text-input"
            value={imeText}
            onChange={(e) => setImeText(e.target.value)}
            onCompositionStart={() => setIsComposing(true)}
            onCompositionEnd={(e) => {
              setIsComposing(false);
              if (e.data) {
                // Prefer the field's real value: with the caret mid-text the composed text is already in place
                // and appending it again would duplicate it.
                const domValue = e.currentTarget.value;
                const composed = e.data;
                setImeText((prev) =>
                  domValue.includes(composed)
                    ? domValue
                    : prev.endsWith(composed)
                    ? prev
                    : `${prev}${composed}`
                );
              }
            }}
            placeholder="Type confirmed text..."
            className="flex-1 px-2 py-0.5 rounded bg-[#111111] border border-[#191919] text-[#f5f5f5] text-xs"
          />
          <button
            type="submit"
            data-testid="remote-browser-ime-send-btn"
            disabled={driver.driverState !== "driving" || !imeText.trim() || isComposing}
            className="px-2 py-0.5 bg-[#346bf1] hover:bg-[#346bf1]/90 disabled:opacity-40 text-[#f5f5f5] rounded text-xs font-medium transition"
          >
            Send Fill
          </button>
        </form>
      )}

      {/* Driver or browser error banner if error occurred */}
      {(driver.error || imeError || actionError || browserError) && (
        <div
          data-testid="remote-browser-driver-error"
          className="px-2 py-1 bg-[#ff6467]/15 border-b border-[#ff6467]/30 text-[#ff6467] text-xs flex items-center justify-between"
        >
          <span>{(driver.error ?? imeError ?? actionError ?? browserError)?.message}</span>
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
