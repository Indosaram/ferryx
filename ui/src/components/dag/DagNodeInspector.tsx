import { useEffect, useState, useMemo, useCallback } from "react";
import {
  X,
  Copy,
  Check,
  CloudOff,
  FileText,
  Terminal,
  Activity,
  AlertCircle,
  ChevronLeft,
  ChevronRight,
  Clock,
  Coins,
  Cpu,
} from "lucide-react";
import type { DagNodeSnapshot } from "../../lib/dagTypes";
import { dagReadNodeArtifact } from "../../lib/tauri";
import { formatRouteText, getNodeStateGlyph } from "./dagViewUtils";

export type DagNodeInspectorProps = {
  readonly node: DagNodeSnapshot | null;
  readonly projectPath?: string;
  readonly onClose: () => void;
  readonly allNodes?: readonly DagNodeSnapshot[];
  readonly onSelectNode?: (nodeId: string) => void;
};

type TabType = "artifact" | "prompt" | "stats" | "error";

/**
 * Remote DAG runs are stored under synthetic keys (`paired:<workspaceId>:<remotePath>`
 * and `ssh:<workspaceId>:<remotePath>`) that are not filesystem paths. Remote artifact
 * retrieval is out of scope for the current transport, so the local artifact IPC must
 * never receive one of these keys. Match on the transport prefix only - the remote path
 * suffix may itself contain colons and must not be parsed.
 */
export function isRemoteDagProjectKey(projectPath?: string | null): boolean {
  if (!projectPath) return false;
  return projectPath.startsWith("paired:") || projectPath.startsWith("ssh:");
}

export function formatDurationMs(ms?: number | null): string {
  if (ms === undefined || ms === null || !Number.isFinite(ms)) return "-";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  const remSec = sec % 60;
  return remSec > 0 ? `${min}m ${remSec}s` : `${min}m`;
}

export function formatTokenCount(num?: number | null): string {
  if (num === undefined || num === null || !Number.isFinite(num)) return "-";
  if (num < 1000) return `${num}`;
  if (num < 1000000) return `${(num / 1000).toFixed(1)}k`;
  return `${(num / 1000000).toFixed(2)}m`;
}

export function DagNodeInspector({
  node,
  projectPath,
  onClose,
  allNodes = [],
  onSelectNode,
}: DagNodeInspectorProps): JSX.Element | null {
  const [activeTab, setActiveTab] = useState<TabType>("artifact");
  const [artifactContent, setArtifactContent] = useState<string | null>(null);
  const [artifactLoading, setArtifactLoading] = useState(false);
  const [artifactError, setArtifactError] = useState<string | null>(null);
  const [copiedTab, setCopiedTab] = useState<string | null>(null);
  const artifactIsRemote = isRemoteDagProjectKey(projectPath);

  useEffect(() => {
    if (!node) return;
    if (node.state === "failed" || node.error) {
      setActiveTab("error");
    } else if (node.resultArtifact) {
      setActiveTab("artifact");
    } else {
      setActiveTab("prompt");
    }
  }, [node?.id, node?.state, node?.resultArtifact, node?.error]);

  useEffect(() => {
    if (!node || !node.resultArtifact?.relativePath || !projectPath || artifactIsRemote) {
      setArtifactContent(null);
      setArtifactError(null);
      setArtifactLoading(false);
      return;
    }

    let active = true;
    setArtifactLoading(true);
    setArtifactError(null);

    dagReadNodeArtifact(projectPath, node.resultArtifact.relativePath)
      .then((text: string) => {
        if (active) {
          setArtifactContent(text);
          setArtifactLoading(false);
        }
      })
      .catch((err: unknown) => {
        if (active) {
          setArtifactError(err instanceof Error ? err.message : String(err));
          setArtifactLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [node?.id, node?.resultArtifact?.relativePath, projectPath, artifactIsRemote]);

  const copyToClipboard = useCallback((text: string, tabName: string) => {
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(() => {
        setCopiedTab(tabName);
        setTimeout(() => setCopiedTab(null), 1800);
      });
    }
  }, []);

  const { prevNode, nextNode } = useMemo(() => {
    if (!node || allNodes.length <= 1) return { prevNode: null, nextNode: null };
    const currentIndex = allNodes.findIndex((n) => n.id === node.id);
    if (currentIndex === -1) return { prevNode: null, nextNode: null };
    return {
      prevNode: currentIndex > 0 ? allNodes[currentIndex - 1] : null,
      nextNode: currentIndex < allNodes.length - 1 ? allNodes[currentIndex + 1] : null,
    };
  }, [node, allNodes]);

  if (!node) return null;

  const displayLabel = node.label || node.id;
  const routeText = formatRouteText(node.route);
  const glyph = getNodeStateGlyph(node.state);

  return (
    <aside
      data-testid="dag-node-inspector"
      aria-label={`Node inspector: ${displayLabel}`}
      className="absolute top-0 right-0 z-20 flex h-full w-[440px] max-w-[90vw] flex-col border-l border-border/80 bg-background/95 shadow-2xl backdrop-blur-md transition-transform duration-200 ease-out select-text"
    >
      <div className="flex items-center justify-between border-b border-border/60 px-4 py-3 bg-muted/20">
        <div className="flex items-center gap-2 min-w-0">
          <span className="font-mono text-base font-bold shrink-0 text-indigo-500">
            {glyph}
          </span>
          <div className="min-w-0">
            <h2 className="truncate font-semibold text-sm text-foreground" title={displayLabel}>
              {displayLabel}
            </h2>
            <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground font-mono">
              <span>{routeText}</span>
              {node.taskId && (
                <>
                  <span>•</span>
                  <span className="text-muted-foreground/80">{node.taskId}</span>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {prevNode && (
            <button
              type="button"
              aria-label="Previous node"
              onClick={() => onSelectNode?.(prevNode.id)}
              className="flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              title={`Previous: ${prevNode.label || prevNode.id}`}
            >
              <ChevronLeft className="size-4" />
            </button>
          )}
          {nextNode && (
            <button
              type="button"
              aria-label="Next node"
              onClick={() => onSelectNode?.(nextNode.id)}
              className="flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              title={`Next: ${nextNode.label || nextNode.id}`}
            >
              <ChevronRight className="size-4" />
            </button>
          )}
          <button
            type="button"
            aria-label="Close inspector"
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground transition-colors ml-1"
          >
            <X className="size-4" />
          </button>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-2 border-b border-border/40 px-4 py-2 bg-muted/10 text-[11px]">
        <div>
          <span className="text-muted-foreground block text-[10px]">State</span>
          <span className="font-medium capitalize text-foreground">{node.state}</span>
        </div>
        <div>
          <span className="text-muted-foreground block text-[10px]">Duration</span>
          <span className="font-mono font-medium text-foreground">
            {formatDurationMs(node.runStats?.runtimeMs)}
          </span>
        </div>
        <div>
          <span className="text-muted-foreground block text-[10px]">Tokens</span>
          <span className="font-mono font-medium text-foreground">
            {formatTokenCount(node.runStats?.totalTokens)}
          </span>
        </div>
      </div>

      <div
        className="flex border-b border-border/60 px-3 pt-1 gap-1 bg-muted/15"
        role="tablist"
      >
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === "artifact"}
          onClick={() => setActiveTab("artifact")}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
            activeTab === "artifact"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <FileText className="size-3.5" />
          <span>Deliverable</span>
        </button>

        <button
          type="button"
          role="tab"
          aria-selected={activeTab === "prompt"}
          onClick={() => setActiveTab("prompt")}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
            activeTab === "prompt"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <Terminal className="size-3.5" />
          <span>Prompt</span>
        </button>

        <button
          type="button"
          role="tab"
          aria-selected={activeTab === "stats"}
          onClick={() => setActiveTab("stats")}
          className={`flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
            activeTab === "stats"
              ? "border-primary text-foreground"
              : "border-transparent text-muted-foreground hover:text-foreground"
          }`}
        >
          <Activity className="size-3.5" />
          <span>Stats</span>
        </button>

        {(node.error || node.state === "failed") && (
          <button
            type="button"
            role="tab"
            aria-selected={activeTab === "error"}
            onClick={() => setActiveTab("error")}
            className={`flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === "error"
                ? "border-rose-500 text-rose-500"
                : "border-transparent text-rose-400/80 hover:text-rose-500"
            }`}
          >
            <AlertCircle className="size-3.5" />
            <span>Error</span>
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto p-4 text-xs">
        {activeTab === "artifact" && (
          <div className="space-y-3">
            {node.resultArtifact ? (
              <>
                <div className="flex items-center justify-between rounded-md border border-border/60 bg-muted/20 px-3 py-2">
                  <div className="min-w-0 pr-2 font-mono text-[11px] text-muted-foreground truncate">
                    {node.resultArtifact.relativePath}
                  </div>
                  {artifactContent && (
                    <button
                      type="button"
                      onClick={() => copyToClipboard(artifactContent, "artifact")}
                      className="flex items-center gap-1 rounded bg-background px-2 py-1 text-[10px] font-medium border border-border/60 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0"
                    >
                      {copiedTab === "artifact" ? (
                        <>
                          <Check className="size-3 text-emerald-500" />
                          <span>Copied</span>
                        </>
                      ) : (
                        <>
                          <Copy className="size-3" />
                          <span>Copy</span>
                        </>
                      )}
                    </button>
                  )}
                </div>

                {artifactIsRemote ? (
                  <div
                    data-testid="dag-artifact-remote-unavailable"
                    className="rounded-md border border-border/60 bg-muted/20 p-3 text-muted-foreground space-y-1"
                  >
                    <div className="flex items-center gap-1.5 font-medium text-foreground">
                      <CloudOff className="size-4 text-muted-foreground/70" />
                      <span>Artifact stays on the remote machine</span>
                    </div>
                    <p className="text-[11px] leading-relaxed">
                      This run was recorded on a remote session, so its deliverable file is not
                      reachable from this machine. Graph, prompt and stats remain available.
                    </p>
                  </div>
                ) : artifactLoading ? (
                  <div className="py-8 text-center text-muted-foreground">Loading artifact...</div>
                ) : artifactError ? (
                  <div className="rounded-md border border-rose-500/30 bg-rose-500/10 p-3 text-rose-400">
                    Failed to load artifact: {artifactError}
                  </div>
                ) : artifactContent ? (
                  <pre className="overflow-x-auto rounded-md border border-border/60 bg-card p-3 font-mono text-[11px] leading-relaxed text-foreground whitespace-pre-wrap select-text">
                    {artifactContent}
                  </pre>
                ) : (
                  <div className="py-8 text-center text-muted-foreground">Artifact is empty</div>
                )}
              </>
            ) : (
              <div className="py-12 text-center text-muted-foreground">
                <FileText className="mx-auto mb-2 size-8 text-muted-foreground/40" />
                <span>No deliverable artifact recorded for this node yet</span>
              </div>
            )}
          </div>
        )}

        {activeTab === "prompt" && (
          <div className="space-y-3">
            {node.prompt ? (
              <>
                <div className="flex justify-end">
                  <button
                    type="button"
                    onClick={() => copyToClipboard(node.prompt!, "prompt")}
                    className="flex items-center gap-1 rounded bg-background px-2.5 py-1 text-[11px] font-medium border border-border/60 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                  >
                    {copiedTab === "prompt" ? (
                      <>
                        <Check className="size-3 text-emerald-500" />
                        <span>Copied</span>
                      </>
                    ) : (
                      <>
                        <Copy className="size-3" />
                        <span>Copy prompt</span>
                      </>
                    )}
                  </button>
                </div>
                <pre className="overflow-x-auto rounded-md border border-border/60 bg-card p-3 font-mono text-[11px] leading-relaxed text-foreground whitespace-pre-wrap select-text">
                  {node.prompt}
                </pre>
              </>
            ) : (
              <div className="py-12 text-center text-muted-foreground">
                <Terminal className="mx-auto mb-2 size-8 text-muted-foreground/40" />
                <span>No prompt available</span>
              </div>
            )}
          </div>
        )}

        {activeTab === "stats" && (
          <div className="space-y-4">
            {node.runStats ? (
              <>
                <div className="rounded-lg border border-border/60 bg-card p-3 space-y-2">
                  <div className="flex items-center gap-1.5 font-medium text-foreground">
                    <Cpu className="size-4 text-indigo-400" />
                    <span>Token Consumption</span>
                  </div>
                  <div className="grid grid-cols-2 gap-2 text-[11px]">
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Input Tokens</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.inputTokens?.toLocaleString() ?? "-"}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Output Tokens</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.outputTokens?.toLocaleString() ?? "-"}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Total Tokens</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.totalTokens?.toLocaleString() ?? "-"}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Cache Read</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.cacheReadTokens?.toLocaleString() ?? "-"}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="rounded-lg border border-border/60 bg-card p-3 space-y-2">
                  <div className="flex items-center gap-1.5 font-medium text-foreground">
                    <Clock className="size-4 text-indigo-400" />
                    <span>Execution & Speed</span>
                  </div>
                  <div className="grid grid-cols-2 gap-2 text-[11px]">
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Runtime</span>
                      <span className="font-mono font-medium text-foreground">
                        {formatDurationMs(node.runStats.runtimeMs)}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Throughput</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.tokensPerSecond
                          ? `${Math.round(node.runStats.tokensPerSecond)} tok/s`
                          : "-"}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Turns</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.turns ?? "-"}
                      </span>
                    </div>
                    <div className="rounded bg-muted/30 p-2">
                      <span className="text-muted-foreground block text-[10px]">Tool Calls</span>
                      <span className="font-mono font-medium text-foreground">
                        {node.runStats.toolCalls ?? "-"}
                      </span>
                    </div>
                  </div>
                </div>

                {node.runStats.costUsd !== undefined && node.runStats.costUsd !== null && (
                  <div className="rounded-lg border border-border/60 bg-card p-3 space-y-1">
                    <div className="flex items-center gap-1.5 font-medium text-foreground">
                      <Coins className="size-4 text-amber-400" />
                      <span>Cost Estimate</span>
                    </div>
                    <div className="font-mono text-sm font-semibold text-foreground">
                      ${node.runStats.costUsd.toFixed(4)} USD
                    </div>
                  </div>
                )}
              </>
            ) : (
              <div className="py-12 text-center text-muted-foreground">
                <Activity className="mx-auto mb-2 size-8 text-muted-foreground/40" />
                <span>No runtime performance statistics available</span>
              </div>
            )}
          </div>
        )}

        {activeTab === "error" && (
          <div className="space-y-3">
            {node.error ? (
              <div className="rounded-lg border border-rose-500/40 bg-rose-500/10 p-3 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="font-mono text-xs font-semibold text-rose-500">
                    {node.error.code}
                  </span>
                  {node.error.at && (
                    <span className="font-mono text-[10px] text-muted-foreground">
                      {node.error.at}
                    </span>
                  )}
                </div>
                <div className="rounded bg-background/60 p-2.5 font-mono text-[11px] leading-relaxed text-rose-400 whitespace-pre-wrap select-text">
                  {node.error.message}
                </div>
              </div>
            ) : (
              <div className="py-12 text-center text-muted-foreground">
                <span>No error recorded for this node</span>
              </div>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
