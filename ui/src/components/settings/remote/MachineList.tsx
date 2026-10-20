import type { ReactNode } from "react";
import { AlertCircle, Plus, Server } from "lucide-react";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";

export interface MachineListProps {
  total: number;
  shown: number;
  searchQuery: string;
  actionError: string | null;
  /** Enrollment codes are issued from the account bar, which exists only while signed in. */
  signedIn: boolean;
  onSearchChange: (value: string) => void;
  onAddMachine: () => void;
  onRefresh: () => void;
  children: ReactNode;
}

export function MachineList({ total, shown, searchQuery, actionError, signedIn, onSearchChange, onAddMachine, onRefresh, children }: MachineListProps) {
  return (
    <section aria-labelledby="remote-my-machines" className="space-y-3">
      <header className="flex flex-col gap-1 border-b border-border pb-2 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 id="remote-my-machines" className="text-[13px] font-semibold text-foreground">My machines</h2>
          <p className="text-[12px] text-muted-foreground">Connect to another machine.</p>
        </div>
        <p className="text-[11px] text-muted-foreground">{total} {total === 1 ? "machine" : "machines"}</p>
      </header>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          aria-label="Search machines"
          placeholder="Search machines…"
          value={searchQuery}
          onChange={e => onSearchChange(e.target.value)}
          className="h-8 min-w-0 flex-1 text-[12px] sm:max-w-64"
        />
        <Button type="button" size="sm" aria-label="Add Machine" onClick={onAddMachine}>
          <Plus className="mr-1 size-3.5" />
          Add Machine
        </Button>
        <Button type="button" variant="ghost" size="sm" aria-label="Refresh machines" onClick={onRefresh}>
          Refresh
        </Button>
      </div>
      {actionError ? (
        <div role="alert" className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-[12px] text-destructive">
          <AlertCircle className="size-4 shrink-0" />
          <span>{actionError}</span>
        </div>
      ) : null}
      {shown === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-8 text-center">
          <Server className="mb-2 size-8 text-muted-foreground/50" />
          <p className="text-[13px] font-medium text-foreground">{total === 0 ? "No remote machines configured" : "No matching machines found"}</p>
          <p className="mt-1 text-[12px] text-muted-foreground">
            {total === 0
              ? signedIn
                ? "The other machine has to enroll first. Issue an enrollment code above to enroll a machine, or use Add Machine to connect via SSH."
                : "The other machine has to enroll first. Sign in above to issue an enrollment code, or use Add Machine to connect via SSH."
              : "Try clearing your search query to see all machines."}
          </p>
        </div>
      ) : (
        <div role="list" aria-label="Remote machines" className="space-y-2">
          {children}
        </div>
      )}
    </section>
  );
}
