import { AlertCircle, Check, Copy, KeyRound, LogOut, X } from "lucide-react";
import { Badge } from "../../ui/badge";
import { Button } from "../../ui/button";

export function enrollmentCommand(code: string, origin: string): string {
  return `ferryx-cli account enroll --code ${code} --origin ${origin}`;
}

export interface AccountBarProps {
  accountOrigin: string;
  isIssuingCode: boolean;
  issuedCode: string | null;
  copyState: "idle" | "copied" | "failed";
  showNoMachinesHint: boolean;
  onIssueCode: () => void;
  onCopyCommand: () => void;
  onDismissCode: () => void;
  onSignOut: () => void;
}

/** Signed-in account strip. The enrollment card stays here until the Add machine wizard owns it. */
export function AccountBar({
  accountOrigin,
  isIssuingCode,
  issuedCode,
  copyState,
  showNoMachinesHint,
  onIssueCode,
  onCopyCommand,
  onDismissCode,
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
          <Button type="button" variant="outline" size="sm" disabled={isIssuingCode} onClick={onIssueCode}>
            <KeyRound className="mr-1 size-3.5" />
            {isIssuingCode ? "Issuing…" : "Issue Enrollment Code"}
          </Button>
          <Button type="button" variant="ghost" size="sm" onClick={onSignOut} aria-label="Sign Out">
            <LogOut className="mr-1 size-3.5" />
            Sign Out
          </Button>
        </div>
      </div>

      {issuedCode ? (
        <div data-testid="issued-enrollment-card" className="space-y-2 rounded-md border border-primary/40 bg-accent/20 p-3 text-[12px]">
          <div className="flex items-center justify-between">
            <span className="font-semibold text-foreground">
              Enrollment Code Issued: <span className="font-mono font-bold text-primary">{issuedCode}</span>
            </span>
            <Button type="button" variant="ghost" size="sm" className="size-6 p-0" aria-label="Dismiss enrollment code" onClick={onDismissCode}>
              <X className="size-3.5" />
            </Button>
          </div>
          <p className="text-[11px] text-muted-foreground">Run this command on the headless machine to enroll it:</p>
          <div className="flex items-center gap-2">
            <code
              data-testid="enrollment-command"
              className="block flex-1 select-all break-all rounded border border-border bg-muted/80 px-2.5 py-1.5 font-mono text-[11px] text-foreground"
            >
              {enrollmentCommand(issuedCode, accountOrigin)}
            </code>
            <Button type="button" variant="outline" size="sm" className="h-7 shrink-0 text-[12px]" onClick={onCopyCommand}>
              {copyState === "failed" ? (
                <>
                  <AlertCircle className="mr-1 size-3" /> Copy failed
                </>
              ) : copyState === "copied" ? (
                <>
                  <Check className="mr-1 size-3" /> Copied
                </>
              ) : (
                <>
                  <Copy className="mr-1 size-3" /> Copy
                </>
              )}
            </Button>
          </div>
        </div>
      ) : null}

      {showNoMachinesHint ? (
        <p className="text-[11px] text-muted-foreground">No account machines enrolled yet. The other machine has to enroll first.</p>
      ) : null}
    </section>
  );
}
