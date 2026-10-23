import { useEffect, useRef, useState } from "react";
import { AlertCircle, CheckCircle2, Monitor, ShieldAlert, XCircle } from "lucide-react";
import {
  ACCOUNT_MACHINE_CLAIMED,
  MACHINE_REQUEST_DECIDED,
  MACHINE_REQUEST_EXPIRED,
  MACHINE_REQUEST_FOREIGN_ACCOUNT,
  PLAN_LIMIT_REACHED,
  type MachineRequestView,
} from "../../../remote/accountSession";
import { Button } from "../../ui/button";

const APPROVAL_ERROR_MESSAGES: Record<string, string> = {
  [MACHINE_REQUEST_FOREIGN_ACCOUNT]: "This request is addressed to a different Ferryx account. Sign in with the email the machine asked for.",
  [MACHINE_REQUEST_DECIDED]: "This request was already approved or denied.",
  [MACHINE_REQUEST_EXPIRED]: "This request expired. Run ferryx-cli up on the machine again to get a new code.",
  [ACCOUNT_MACHINE_CLAIMED]: "This machine already belongs to another Ferryx account.",
  [PLAN_LIMIT_REACHED]: "Your plan's machine limit is reached. Remove a machine or upgrade, then approve again.",
  UNAUTHORIZED: "Your account session expired. Sign in again.",
  RATE_LIMITED: "Too many attempts. Wait a minute and try again.",
};

/** Maps a structured account error to an approval message by its code; never by message text. */
export function machineApprovalErrorMessage(err: unknown): string {
  if (err && typeof err === "object" && "code" in err && typeof (err as { code: unknown }).code === "string") {
    const typed = err as { code: string; message?: unknown };
    const known = APPROVAL_ERROR_MESSAGES[typed.code];
    if (known) return known;
    if (typeof typed.message === "string" && typed.message.trim()) return typed.message;
    return typed.code;
  }
  return err instanceof Error && err.message ? err.message : "The request could not be completed.";
}

/** Error codes after which neither decision can succeed, so both buttons stay disabled. */
const FINAL_ERROR_CODES = new Set<string>([
  MACHINE_REQUEST_FOREIGN_ACCOUNT,
  MACHINE_REQUEST_DECIDED,
  MACHINE_REQUEST_EXPIRED,
  ACCOUNT_MACHINE_CLAIMED,
]);

function errorCode(err: unknown): string | null {
  return err && typeof err === "object" && "code" in err && typeof (err as { code: unknown }).code === "string"
    ? (err as { code: string }).code
    : null;
}

function formatUnixSeconds(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "unknown";
  return new Date(seconds * 1000).toLocaleString();
}

export type MachineApprovalOutcome = "approved" | "denied";

export interface MachineApprovalCardProps {
  request: MachineRequestView;
  onApprove: () => Promise<unknown>;
  onDeny: () => Promise<unknown>;
  /** Called once after a decision succeeds. */
  onDecided?: (outcome: MachineApprovalOutcome) => void;
  /**
   * Focus Deny on mount (default). Cards that appear on their own from background polling pass
   * false so they never steal focus; Deny stays the primary action either way.
   */
  autoFocus?: boolean;
  /** Injected clock for tests (milliseconds). */
  now?: () => number;
}

/**
 * Owner-side approval of a machine that asked to join the account. Deny is the default
 * (focused, primary) action: approving is the deliberate, secondary choice.
 */
export function MachineApprovalCard({ request, onApprove, onDeny, onDecided, autoFocus = true, now = Date.now }: MachineApprovalCardProps) {
  const [busy, setBusy] = useState<MachineApprovalOutcome | null>(null);
  const [outcome, setOutcome] = useState<MachineApprovalOutcome | null>(null);
  const [error, setError] = useState<{ code: string | null; message: string } | null>(null);
  const denyRef = useRef<HTMLButtonElement>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    if (autoFocus) denyRef.current?.focus();
    return () => {
      mountedRef.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const expired = request.expiresAt * 1000 <= now();
  const finalError = error?.code ? FINAL_ERROR_CODES.has(error.code) : false;
  const locked = busy !== null || outcome !== null || finalError;

  const decide = async (kind: MachineApprovalOutcome) => {
    if (locked) return;
    setBusy(kind);
    setError(null);
    try {
      await (kind === "approved" ? onApprove() : onDeny());
      if (!mountedRef.current) return;
      setOutcome(kind);
      onDecided?.(kind);
    } catch (err) {
      if (mountedRef.current) setError({ code: errorCode(err), message: machineApprovalErrorMessage(err) });
    } finally {
      if (mountedRef.current) setBusy(null);
    }
  };

  const name = request.displayName || request.machineId;
  return (
    <section
      aria-label={`Approve machine ${name}`}
      data-testid="machine-approval-card"
      className="space-y-3 rounded-lg border border-border bg-muted/20 p-3.5"
    >
      <div className="flex items-start gap-2.5">
        <span className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground">
          <Monitor className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium text-foreground">{name}</p>
          <p className="text-[11px] text-muted-foreground">wants to join your Ferryx account</p>
        </div>
      </div>

      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
        <dt className="text-muted-foreground">OS</dt>
        <dd data-testid="approval-platform" className="break-words text-foreground">{request.platform || "unknown"}</dd>
        <dt className="text-muted-foreground">IP address</dt>
        <dd data-testid="approval-ip" className="break-all font-mono text-foreground">{request.requesterIp || "unknown"}</dd>
        <dt className="text-muted-foreground">App version</dt>
        <dd data-testid="approval-version" className="break-words text-foreground">{request.appVersion || "unknown"}</dd>
        <dt className="text-muted-foreground">Expires</dt>
        <dd data-testid="approval-expiry" className="text-foreground">
          {expired ? "Expired" : formatUnixSeconds(request.expiresAt)}
        </dd>
      </dl>

      <p className="flex items-start gap-1.5 text-[11px] text-muted-foreground">
        <ShieldAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
        Approve only if you just started this machine yourself and the details match.
      </p>

      {error ? (
        <div
          role="alert"
          data-code={error.code ?? undefined}
          className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-[12px] text-destructive"
        >
          <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span className="break-words">{error.message}</span>
        </div>
      ) : null}

      {outcome ? (
        <p role="status" className="flex items-center gap-2 text-[12px] text-foreground">
          {outcome === "approved" ? (
            <>
              <CheckCircle2 className="size-4 text-status-success" aria-hidden="true" />
              Approved. The machine finishes enrolling on its own.
            </>
          ) : (
            <>
              <XCircle className="size-4 text-muted-foreground" aria-hidden="true" />
              Denied. The machine was not added.
            </>
          )}
        </p>
      ) : (
        <div className="flex flex-wrap justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="text-[12px]"
            disabled={locked || expired}
            onClick={() => void decide("approved")}
          >
            {busy === "approved" ? "Approving…" : "Approve"}
          </Button>
          <Button
            ref={denyRef}
            type="button"
            size="sm"
            className="text-[12px]"
            disabled={locked}
            onClick={() => void decide("denied")}
          >
            {busy === "denied" ? "Denying…" : "Deny"}
          </Button>
        </div>
      )}
    </section>
  );
}
