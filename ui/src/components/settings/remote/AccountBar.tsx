import { KeyRound, LogOut } from "lucide-react";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";

export interface AccountBarProps {
  accountOrigin: string;
  showNoMachinesHint: boolean;
  /** Opens the Add Machine wizard on its enrollment-code step. */
  onIssueCode: () => void;
  onSignOut: () => void;
}

/** Signed-in account strip. The issued code and its command live in the Add Machine wizard. */
export function AccountBar({
  accountOrigin,
  showNoMachinesHint,
  onIssueCode,
  onSignOut,
}: AccountBarProps) {
  let host = accountOrigin;
  try {
    host = new URL(accountOrigin).host;
  } catch {
    // Keep the raw origin when it is not a URL.
  }
  return (
    <section aria-labelledby="remote-account-heading" data-testid="account-status-bar" className="space-y-3 rounded-lg border border-border bg-card p-4">
      <h2 id="remote-account-heading" className="sr-only">Account</h2>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[12px] font-semibold text-foreground">
            <Badge variant="outline" className="h-4 border-primary/30 bg-primary/10 px-1.5 py-0 text-[11px] text-primary">
              Account Active
            </Badge>
            <span>Ferryx Account</span>
          </div>
          <p className="mt-0.5 truncate text-[11px] text-muted-foreground">
            {host} · Enrolled machines on this account are listed below.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm" onClick={onIssueCode}>
            <KeyRound className="mr-1 size-3.5" />
            Issue Enrollment Code
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={onSignOut} aria-label="Sign Out">
            <LogOut className="mr-1 size-3.5" />
            Sign Out
          </Button>
        </div>
      </div>

      {showNoMachinesHint ? (
        <p className="text-[11px] text-muted-foreground">No account machines enrolled yet. The other machine has to enroll first.</p>
      ) : null}
    </section>
  );
}
