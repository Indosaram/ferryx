import { useState } from "react";
import { Bell, GitBranch, LogOut, Plus, Server } from "lucide-react";
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
  onOpenPhoneAlerts?: () => void;
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
  onOpenPhoneAlerts,
  onSignOut,
  connectionLabel,
  buildStamp,
  disabled = false,
}: MobileOverflowSheetProps) {
  // Sign out takes a second, explicit press; closing the sheet forgets the first one.
  const [confirmingSignOut, setConfirmingSignOut] = useState(false);
  const changeOpen = (next: boolean) => {
    if (!next) setConfirmingSignOut(false);
    onOpenChange(next);
  };

  return (
    <MobileSheet
      open={open}
      onOpenChange={changeOpen}
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
            changeOpen(false);
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
            changeOpen(false);
          }}
          testId="overflow-action-new-worktree"
        />
      ) : null}

      {onOpenMachines ? (
        <MobileSheetItem
          icon={Server}
          label="Computers"
          onSelect={() => {
            onOpenMachines();
            changeOpen(false);
          }}
          testId="overflow-action-machines"
        />
      ) : null}

      {onOpenPhoneAlerts ? (
        <MobileSheetItem
          icon={Bell}
          label="Phone alerts"
          onSelect={() => {
            onOpenPhoneAlerts();
            changeOpen(false);
          }}
          testId="overflow-action-phone-alerts"
        />
      ) : null}

      {onSignOut ? (
        <div className="mt-1 border-t border-chat-border pt-1">
          <MobileSheetItem
            icon={LogOut}
            label={confirmingSignOut ? "Tap again to sign out" : "Sign out"}
            description={confirmingSignOut ? "You'll need to sign in again on this phone." : undefined}
            danger
            onSelect={() => {
              if (!confirmingSignOut) {
                setConfirmingSignOut(true);
                return;
              }
              onSignOut();
              changeOpen(false);
            }}
            testId="overflow-action-sign-out"
          />
        </div>
      ) : null}

      {connectionLabel || buildStamp ? (
        <div className="mt-2 flex items-center justify-between px-3 py-2 font-mono text-xs text-chat-foreground-secondary">
          <span>{connectionLabel ?? ""}</span>
          <span>{buildStamp ?? ""}</span>
        </div>
      ) : null}
    </MobileSheet>
  );
}
