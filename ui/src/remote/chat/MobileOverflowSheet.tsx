import { GitBranch, LogOut, Plus, Server } from "lucide-react";
import { MobileSheet, MobileSheetItem } from "./MobileSheet";

/**
 * Phone overflow action sheet (plan todo 7f): exposes secondary actions like new terminal,
 * new worktree, host switcher or sign out, plus connection info.
 * Reference: paseo workspace-header-menu.tsx:62-132 (Apache-2.0).
 */

export interface MobileOverflowSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNewTerminal?: () => void;
  onNewWorktree?: () => void;
  onOpenMachines?: () => void;
  onSignOut?: () => void;
  connectionLabel?: string;
  buildStamp?: string;
  disabled?: boolean;
}

export function MobileOverflowSheet({
  open,
  onOpenChange,
  onNewTerminal,
  onNewWorktree,
  onOpenMachines,
  onSignOut,
  connectionLabel,
  buildStamp,
  disabled = false,
}: MobileOverflowSheetProps) {
  return (
    <MobileSheet
      open={open}
      onOpenChange={onOpenChange}
      title="Actions"
      ariaLabel="Actions"
      testId="mobile-overflow-sheet"
    >
      {onNewTerminal ? (
        <MobileSheetItem
          icon={Plus}
          label="New terminal"
          disabled={disabled}
          onSelect={() => {
            onNewTerminal();
            onOpenChange(false);
          }}
          testId="overflow-action-new-terminal"
        />
      ) : null}

      {onNewWorktree ? (
        <MobileSheetItem
          icon={GitBranch}
          label="New worktree"
          disabled={disabled}
          onSelect={() => {
            onNewWorktree();
            onOpenChange(false);
          }}
          testId="overflow-action-new-worktree"
        />
      ) : null}

      {onSignOut ? (
        <MobileSheetItem
          icon={LogOut}
          label="Sign out"
          onSelect={() => {
            onSignOut();
            onOpenChange(false);
          }}
          testId="overflow-action-sign-out"
        />
      ) : onOpenMachines ? (
        <MobileSheetItem
          icon={Server}
          label="Machines"
          onSelect={() => {
            onOpenMachines();
            onOpenChange(false);
          }}
          testId="overflow-action-machines"
        />
      ) : null}

      {connectionLabel || buildStamp ? (
        <div className="mt-2 flex items-center justify-between px-3 py-2 font-mono text-[11px] text-chat-foreground-secondary">
          <span>{connectionLabel ?? ""}</span>
          <span>{buildStamp ?? ""}</span>
        </div>
      ) : null}
    </MobileSheet>
  );
}

