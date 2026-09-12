import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import ReactDOM from "react-dom/client";

import "../index.css";
import { dagStore } from "../state/dagStore";
import { createDagPaneContent, createTerminalPaneContent } from "../lib/types";
import { createLayoutState } from "../state/layout";
import { createLeafNode } from "../state/paneTree";
import { DagPaneBadge } from "../components/dag/DagPaneBadge";
import { TerminalSplitView } from "../components/TerminalSplitView";
import type { LayoutState } from "../lib/types";
import type { TerminalSession } from "../lib/types";
import {
  buildBigDagRun,
  buildEmptyDagRun,
  buildQaDagRunA,
  buildQaDagRunAUpdated,
  buildQaDagRunB,
  buildTallDagRun,
  QA_OWNER_SESSION_ID,
  QA_PROJECT_PATH,
} from "./dagViewportQaFixtures";

/**
 * Browser-rendered QA harness for DAG viewport navigation. It mounts the REAL
 * DagGraphView, DagPaneBadge and TerminalSplitView (DAG leaf plus terminal sibling)
 * and seeds them through the REAL dagStore APIs. Only PTY/native IPC is absent here
 * (this page runs in a plain browser), which explicitly disqualifies it as native
 * evidence; native verification happens through the debug app, never this page.
 */

const qaSession: TerminalSession = {
  id: "sess-qa",
  cwd: QA_PROJECT_PATH,
  worktreePath: QA_PROJECT_PATH,
  workspaceId: "qa-ws",
  worktree: null,
  backendSessionId: "backend-qa",
  lifecycle: "working",
  providerSession: null,
};

function buildStandaloneLayout(runId: string, ratio = 0.5): LayoutState {
  const base = createLayoutState([
    { id: "tab-qa", kind: "terminal", label: "qa", sessionId: qaSession.id },
  ]);
  const tabLayout = base.layoutsByTabId["tab-qa"];
  return {
    ...base,
    layoutsByTabId: {
      "tab-qa": {
        ...tabLayout,
        root: {
          type: "split" as const,
          direction: "horizontal" as const,
          first: createLeafNode("leaf-qa-term"),
          second: createLeafNode("leaf-qa-dag", "dag"),
          ratio,
        },
        sessionIdsByLeafId: {
          "leaf-qa-term": qaSession.id,
          "leaf-qa-dag": "",
        },
        contentsByLeafId: {
          "leaf-qa-term": createTerminalPaneContent(qaSession.id),
          "leaf-qa-dag": createDagPaneContent({ runId }),
        },
      },
    },
  };
}

function seedStore(): void {
  dagStore.reset();
  // Runs without the qa-owner root session never appear in the badge modal tabs.
  dagStore.applySnapshot(QA_PROJECT_PATH, buildBigDagRun());
  dagStore.applySnapshot(QA_PROJECT_PATH, buildTallDagRun());
  dagStore.applySnapshot(QA_PROJECT_PATH, buildEmptyDagRun());
  // Both owned runs are running; A carries the later updatedAt so it opens first.
  dagStore.applySnapshot(QA_PROJECT_PATH, buildQaDagRunB());
  dagStore.applySnapshot(QA_PROJECT_PATH, buildQaDagRunA());
}

function DagViewportQaHarness(): JSX.Element {
  const seededRef = useRef(false);
  const [standaloneRunId, setStandaloneRunId] = useState("qa-dag-a");
  const [standaloneRatio, setStandaloneRatio] = useState(0.5);
  const [receipt, setReceipt] = useState<unknown>(null);

  // QA-only commit barrier. MessageChannel runs after the native dispatch stack,
  // unlike a capture-listener microtask. The layout effect acknowledges an actual
  // committed subtree render, including queued camera updates, even for no-ops.
  useLayoutEffect(() => {
    if (receipt !== null) window.dispatchEvent(new CustomEvent("qa-render-receipt", { detail: receipt }));
  }, [receipt]);
  useEffect(() => {
    const channel = new MessageChannel();
    channel.port1.onmessage = (event) => flushSync(() => setReceipt(event.data));
    const request = (event: Event) => channel.port2.postMessage((event as CustomEvent).detail);
    window.addEventListener("qa-request-receipt", request);
    return () => {
      window.removeEventListener("qa-request-receipt", request);
      channel.port1.close();
      channel.port2.close();
    };
  }, []);

  useEffect(() => {
    if (seededRef.current) return;
    seededRef.current = true;
    seedStore();
  }, []);

  const layout = useMemo(
    () => buildStandaloneLayout(standaloneRunId, standaloneRatio),
    [standaloneRunId, standaloneRatio],
  );

  const handleSetRatio = useCallback((_tabId: string, _path: string, ratio: number) => {
    setStandaloneRatio(ratio);
  }, []);

  const actionButton = (id: string, label: string, onClick: () => void) => (
    <button
      key={id}
      type="button"
      data-testid={id}
      onClick={onClick}
      className="rounded border border-border px-2 py-1 text-xs"
    >
      {label}
    </button>
  );

  return (
    <div className="flex min-h-screen flex-col gap-3 bg-background p-4 text-foreground">
      <div className="flex flex-wrap items-center gap-2">
        {actionButton("qa-load-a", "standalone: run A", () => setStandaloneRunId("qa-dag-a"))}
        {actionButton("qa-load-big", "standalone: big", () => setStandaloneRunId("qa-dag-big"))}
        {actionButton("qa-load-tall", "standalone: tall", () => setStandaloneRunId("qa-dag-tall"))}
        {actionButton("qa-load-empty", "standalone: empty", () => setStandaloneRunId("qa-dag-empty"))}
        {actionButton("qa-update-status-a", "update status A (same run)", () =>
          dagStore.applySnapshot(QA_PROJECT_PATH, buildQaDagRunAUpdated()))}
        <span className="text-xs text-muted-foreground">
          standalone leaf run: <span data-testid="qa-standalone-run">{standaloneRunId}</span>
        </span>
      </div>

      <section
        data-testid="qa-modal-host"
        className="relative h-28 overflow-hidden rounded border border-border"
      >
        <span className="text-xs text-muted-foreground">modal host: real DagPaneBadge</span>
        <DagPaneBadge
          projectPath={QA_PROJECT_PATH}
          providerSessionId={QA_OWNER_SESSION_ID}
        />
      </section>

      <section
        data-testid="qa-modal-host-wrong-owner"
        className="relative h-20 overflow-hidden rounded border border-border"
      >
        <span className="text-xs text-muted-foreground">wrong owner: badge must stay absent</span>
        <DagPaneBadge
          projectPath={QA_PROJECT_PATH}
          providerSessionId="qa-someone-else"
        />
      </section>

      <section
        data-testid="qa-standalone-host"
        className="flex h-[520px] w-full max-w-[640px] flex-col overflow-hidden rounded border border-border"
      >
        <TerminalSplitView
          layout={layout}
          sessions={{ [qaSession.id]: qaSession }}
          onSetRatio={handleSetRatio}
        />
      </section>
    </div>
  );
}

const el = document.getElementById("root");
if (el) {
  ReactDOM.createRoot(el).render(
    <React.StrictMode>
      <DagViewportQaHarness />
    </React.StrictMode>,
  );
}
