import { useEffect, useRef, useState } from "react";
import { AlertCircle, Check, Copy, Info, LoaderCircle, X } from "lucide-react";
import { copyTextToClipboard } from "../../../lib/clipboard";
import { extractIpcErrorMessage } from "../../../lib/sshHosts";
import { Button } from "../../ui/button";

export function enrollmentCommand(code: string, origin: string): string {
  return `ferryx-cli account enroll --code ${code} --origin ${origin}`;
}

export interface EnrollmentStepProps {
  accountOrigin: string;
  /** Issues a fresh code for the signed-in account. Absent while signed out. */
  onIssueEnrollmentCode?: () => Promise<string>;
  onBack: () => void;
  onDone: () => void;
}

/** Add machine step 2 for the "Enrollment code" method: issues a code and shows the enroll command. */
export function EnrollmentStep({ accountOrigin, onIssueEnrollmentCode, onBack, onDone }: EnrollmentStepProps) {
  const [code, setCode] = useState<string | null>(null);
  const [issuing, setIssuing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const mountedRef = useRef(false);
  const requestedRef = useRef(false);
  const issuingRef = useRef(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const issue = async () => {
    if (!onIssueEnrollmentCode || issuingRef.current) return;
    issuingRef.current = true;
    setIssuing(true);
    setError(null);
    try {
      const next = await onIssueEnrollmentCode();
      if (mountedRef.current) {
        setCode(next);
        setCopyState("idle");
      }
    } catch (err) {
      if (mountedRef.current) setError(extractIpcErrorMessage(err, "Failed to issue enrollment code"));
    } finally {
      issuingRef.current = false;
      if (mountedRef.current) setIssuing(false);
    }
  };

  useEffect(() => {
    mountedRef.current = true;
    // One code per visit to this step. The ref survives StrictMode's simulated remount, so
    // development builds do not issue two codes.
    if (!requestedRef.current) {
      requestedRef.current = true;
      void issue();
    }
    return () => {
      mountedRef.current = false;
      clearTimeout(copyTimerRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleCopy = () => {
    if (!code) return;
    void copyTextToClipboard(enrollmentCommand(code, accountOrigin)).then((copied) => {
      if (!mountedRef.current) return;
      setCopyState(copied ? "copied" : "failed");
      clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => setCopyState("idle"), 2000);
    });
  };

  if (!onIssueEnrollmentCode) {
    return (
      <div className="space-y-4">
        <p role="status" className="flex items-start gap-2 rounded-md border border-border bg-muted/40 p-3 text-[12px] text-muted-foreground">
          <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>Sign in to issue an enrollment code. Sign in from the account panel in Remote settings, then add the machine again.</span>
        </p>
        <div className="flex items-center gap-2 border-t border-border pt-3">
          <Button type="button" variant="ghost" size="sm" className="text-[12px]" onClick={onBack}>Back</Button>
          <span className="flex-1" />
          <Button type="button" size="sm" className="text-[12px]" onClick={onDone}>Done</Button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {error ? (
        <div role="alert" className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-[12px] text-destructive">
          <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span className="break-words">{error}</span>
        </div>
      ) : null}

      {issuing && !code ? (
        <p role="status" className="flex items-center gap-2 text-[12px] text-muted-foreground">
          <LoaderCircle className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />
          Issuing an enrollment code…
        </p>
      ) : null}

      {code ? (
        <div data-testid="issued-enrollment-card" className="space-y-2">
          <div className="flex items-start justify-between gap-3">
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
              <span className="font-mono text-[20px] font-semibold tracking-wider text-foreground">{code}</span>
              <span className="text-[11px] text-muted-foreground">Single use</span>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="size-6 p-0"
              aria-label="Dismiss enrollment code"
              onClick={() => {
                setCode(null);
                setCopyState("idle");
              }}
            >
              <X className="size-3.5" />
            </Button>
          </div>
          <p className="text-[11px] font-medium text-muted-foreground">Run this command on the machine you want to add:</p>
          <div className="flex items-start gap-2 rounded-md border border-border bg-muted/60 py-1.5 pl-2.5 pr-1.5">
            <code
              data-testid="enrollment-command"
              className="min-w-0 flex-1 select-all break-all pt-1 font-mono text-[11px] leading-relaxed text-foreground"
            >
              {enrollmentCommand(code, accountOrigin)}
            </code>
            <Button type="button" variant="ghost" size="sm" className="h-7 shrink-0 px-2 text-[12px]" onClick={handleCopy}>
              {copyState === "failed" ? (
                <>
                  <AlertCircle className="size-3" /> Copy failed
                </>
              ) : copyState === "copied" ? (
                <>
                  <Check className="size-3" /> Copied
                </>
              ) : (
                <>
                  <Copy className="size-3" /> Copy
                </>
              )}
            </Button>
          </div>
          <p className="text-[12px] text-muted-foreground">
            The machine appears under My machines after it enrolls. Use Refresh to update the list.
          </p>
        </div>
      ) : null}

      <div className="flex items-center gap-2 border-t border-border pt-3">
        <Button type="button" variant="ghost" size="sm" className="text-[12px]" onClick={onBack}>Back</Button>
        <span className="flex-1" />
        <Button type="button" variant="outline" size="sm" className="text-[12px]" disabled={issuing} onClick={() => void issue()}>
          {issuing ? "Issuing…" : "New code"}
        </Button>
        <Button type="button" size="sm" className="text-[12px]" onClick={onDone}>Done</Button>
      </div>
    </div>
  );
}
