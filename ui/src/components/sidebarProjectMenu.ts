import { toast } from "sonner";

import { isExternalWorktreePromptMuted, setExternalWorktreePromptMuted } from "../lib/externalWorktrees";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { revealPath, type RegisteredProject } from "../lib/tauri";
import { fileManagerActionLabel } from "./WorktreeList";

export type ProjectMenuActions = {
  readonly onCreateWorktree?: () => void;
  readonly onManageDisk?: () => void;
  readonly onOpenHistory?: () => void;
  readonly onRemoveProject?: () => void;
};

/** Opens the desktop-native project context menu; the returned signal owner aborts a stale one. */
export function openProjectMenu(
  project: RegisteredProject,
  position: { x: number; y: number },
  actions: ProjectMenuActions,
  signal: AbortSignal,
) {
  const remote = Boolean(project.target && project.target.kind !== "local");
  const promptMuted = !remote && isExternalWorktreePromptMuted(project.workspaceId);
  const items: NativeMenuEntry[] = [
    { kind: "item", id: "add-worktree", label: "Add Worktree", enabled: project.gitRoot !== null, icon: "add" },
    { kind: "item", id: "manage-disk", label: "Manage Worktree Disk…", enabled: !remote && project.gitRoot !== null, icon: "disk" },
    {
      kind: "item",
      id: "toggle-external-prompt",
      label: promptMuted ? "Ask About Outside Worktrees on Launch" : "Don't Ask About Outside Worktrees",
      enabled: !remote && project.gitRoot !== null,
    },
    { kind: "item", id: "past-conversations", label: "Past Conversations…", enabled: Boolean(actions.onOpenHistory), icon: "history" },
    { kind: "item", id: "reveal", label: remote ? "Local reveal unavailable over SSH" : fileManagerActionLabel(), enabled: !remote, icon: "reveal" },
    { kind: "item", id: "copy-path", label: "Copy Project Path" },
    { kind: "separator" },
    { kind: "item", id: "remove", label: "Remove Project", enabled: Boolean(actions.onRemoveProject), icon: "trash" },
  ];
  void openNativePopupMenu(
    "cmd_native_sidebar_context_menu",
    items,
    position,
    (id) => {
      if (id === "add-worktree") actions.onCreateWorktree?.();
      else if (id === "manage-disk") actions.onManageDisk?.();
      else if (id === "toggle-external-prompt" && !remote) setExternalWorktreePromptMuted(project.workspaceId, !promptMuted);
      else if (id === "past-conversations") actions.onOpenHistory?.();
      else if (id === "reveal" && !remote) {
        revealPath(project.repoRoot).catch((err: unknown) => {
          toast.error(`Failed to reveal path: ${err instanceof Error ? err.message : String(err)}`);
        });
      } else if (id === "copy-path") {
        if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
          navigator.clipboard
            .writeText(project.repoRoot)
            .then(() => toast.success("Path copied to clipboard"))
            .catch(() => toast.error("Failed to copy to clipboard"));
        } else {
          toast.error("Clipboard API unavailable");
        }
      } else if (id === "remove") actions.onRemoveProject?.();
    },
    signal,
  ).catch((error: unknown) => console.warn("Could not open native project menu", error));
}
