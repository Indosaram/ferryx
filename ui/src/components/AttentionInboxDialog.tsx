import { useEffect } from "react";

import { AttentionInbox } from "../features/ferryx/control/AttentionInbox";
import { targetKey, type Agent } from "../features/ferryx/control/client";
import { buildDesktopInventory, isUnreadAgent, type DesktopWorkspace } from "../features/ferryx/control/desktopInventory";

export interface AttentionInboxDialogProps { workspaces: DesktopWorkspace[]; unavailableHosts?: readonly string[]; onSelect: (agent: Agent) => void; onClose: () => void; }

/**
 * Desktop cross-project attention list: every tracked session across the registered workspaces,
 * waiting or unread first. The remote web client already ships the same component.
 */
export function AttentionInboxDialog({ workspaces, unavailableHosts = [], onSelect, onClose }: AttentionInboxDialogProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const snapshot = buildDesktopInventory(workspaces, unavailableHosts);

  return (
    <>
      <div
        data-testid="attention-inbox-backdrop"
        className="fixed inset-0 z-40 bg-background/60"
        onClick={onClose}
      />
      <div
        role="dialog"
        aria-label="Agents needing attention"
        data-testid="attention-inbox-dialog"
        className="fixed top-1/2 left-1/2 z-50 flex max-h-[70vh] w-[420px] max-w-[90vw] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-md border border-border bg-card shadow-lg"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-3 py-2">
          <h2 className="text-sm font-medium">Agents needing attention</h2>
          <button type="button" className="rounded px-2 py-1 text-xs hover:bg-muted" onClick={onClose}>
            Close
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          <AttentionInbox
            snapshot={snapshot}
            onSelect={(target) => {
              const live = buildDesktopInventory(workspaces, unavailableHosts);
              const agent = live.items.find((item) => targetKey(item.target) === targetKey(target));
              if (agent) onSelect(agent);
            }}
            isUnread={(agent) => isUnreadAgent(agent, workspaces)}
          />
        </div>
      </div>
    </>
  );
}
