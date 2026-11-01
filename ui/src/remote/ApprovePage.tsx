import React, { useEffect, useRef, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { AccountLoginPage } from "./AccountLoginPage";
import {
  AccountSessionError,
  approveMachineRequest,
  clearStoredAccountSessionToken,
  denyMachineRequest,
  getStoredAccountSessionToken,
  lookupMachineRequest,
  normalizeUserCode,
  resolveAccountOrigin,
  type MachineRequestView,
} from "./accountSession";
import { MachineApprovalCard, machineApprovalErrorMessage } from "../components/settings/remote/MachineApprovalCard";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";

export const APPROVE_ROUTE_PATH = "/approve";
/** Holds the approval code across the sign-in detour; the URL copy is removed on entry. */
export const APPROVE_PENDING_CODE_KEY = "ferryx.approve.pendingCode";

export function isApproveRoute(pathname: string): boolean {
  return pathname.replace(/\/+$/, "") === APPROVE_ROUTE_PATH;
}

function sessionStore(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.sessionStorage ?? null : null;
  } catch {
    return null;
  }
}

/**
 * Reads ?code= once, keeps it in sessionStorage and strips it from the URL: AccountLoginPage
 * treats a ?code= query as a magic-link login code, so it must never see the approval code.
 */
function takeApprovalCode(): string {
  const storage = sessionStore();
  let code = "";
  try {
    code = new URLSearchParams(window.location.search).get("code")?.trim() ?? "";
  } catch {
    code = "";
  }
  if (code) {
    try {
      storage?.setItem(APPROVE_PENDING_CODE_KEY, code);
    } catch {}
    if (window.history && typeof window.history.replaceState === "function") {
      window.history.replaceState(null, "", window.location.pathname);
    }
    return code;
  }
  try {
    return storage?.getItem(APPROVE_PENDING_CODE_KEY) ?? "";
  } catch {
    return "";
  }
}

function clearPendingCode(): void {
  try {
    sessionStore()?.removeItem(APPROVE_PENDING_CODE_KEY);
  } catch {}
}

interface ApprovePageProps {
  relayUrl: string;
}

/** Web approval page for /approve?code=XXXX-XXXX: sign in if needed, then approve or deny. */
export const ApprovePage: React.FC<ApprovePageProps> = ({ relayUrl }) => {
  const [initialCode] = useState(takeApprovalCode);
  const [origin, setOrigin] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [codeDraft, setCodeDraft] = useState(initialCode);
  const [userCode, setUserCode] = useState(initialCode ? normalizeUserCode(initialCode) : "");
  const [request, setRequest] = useState<MachineRequestView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void resolveAccountOrigin(relayUrl).then((resolved) => {
      if (!active) return;
      setOrigin(resolved);
      setToken(getStoredAccountSessionToken(resolved));
    });
    return () => {
      active = false;
    };
  }, [relayUrl]);

  useEffect(() => {
    if (!origin || !token || !userCode) return;
    let active = true;
    setLoading(true);
    setError(null);
    setRequest(null);
    lookupMachineRequest(origin, token, userCode)
      .then((found) => {
        if (active) setRequest(found);
      })
      .catch((err: unknown) => {
        if (!active) return;
        if (err instanceof AccountSessionError && err.code === "UNAUTHORIZED") {
          clearStoredAccountSessionToken();
          setToken(null);
          return;
        }
        setError(machineApprovalErrorMessage(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [origin, token, userCode]);

  if (!origin) {
    return (
      <main className="flex min-h-dvh items-center justify-center bg-background pt-[env(safe-area-inset-top)] pb-[env(safe-area-inset-bottom)] text-foreground">
        <LoaderCircle className="size-5 animate-spin motion-reduce:animate-none text-muted-foreground" aria-label="Loading" />
      </main>
    );
  }

  if (!token) {
    return (
      <AccountLoginPage
        relayUrl={relayUrl}
        onLoginSuccess={(tok) => {
          if (mountedRef.current) setToken(tok);
        }}
      />
    );
  }

  const submitCode = (e: React.FormEvent) => {
    e.preventDefault();
    const next = normalizeUserCode(codeDraft);
    if (!next) return;
    try {
      sessionStore()?.setItem(APPROVE_PENDING_CODE_KEY, next);
    } catch {}
    setUserCode(next);
  };

  const signOut = () => {
    clearStoredAccountSessionToken();
    setRequest(null);
    setError(null);
    setToken(null);
  };

  return (
    <main className="flex min-h-dvh items-start justify-center bg-background px-4 pt-[max(env(safe-area-inset-top),1rem)] pb-[max(env(safe-area-inset-bottom),1rem)] text-foreground sm:items-center sm:p-6">
      <div className="w-full max-w-sm space-y-4">
        <div className="space-y-1">
          <h1 className="text-xl font-semibold">Approve a computer</h1>
          <p className="text-sm text-muted-foreground">
            A computer asked to join your Ferryx account. Check its details before you decide.
          </p>
        </div>

        {request ? (
          <MachineApprovalCard
            request={request}
            onApprove={() => approveMachineRequest(origin, token, request.requestId, userCode)}
            onDeny={() => denyMachineRequest(origin, token, request.requestId, userCode)}
            onDecided={clearPendingCode}
          />
        ) : (
          <form className="space-y-3" onSubmit={submitCode}>
            <label htmlFor="approve-page-code" className="block text-sm font-medium">Approval code</label>
            <Input
              id="approve-page-code"
              aria-label="Approval code"
              autoComplete="off"
              autoCapitalize="characters"
              spellCheck={false}
              maxLength={9}
              placeholder="XXXX-XXXX"
              className="h-11 font-mono text-base uppercase tracking-wider"
              disabled={loading}
              value={codeDraft}
              onChange={(e) => setCodeDraft(e.target.value)}
            />
            {loading ? (
              <p role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
                <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                Looking up the computer...
              </p>
            ) : null}
            {error ? (
              <div role="alert" className="rounded-lg bg-destructive/10 p-3.5 text-sm leading-relaxed text-foreground">
                <span className="break-words">{error}</span>
              </div>
            ) : null}
            <Button type="submit" className="h-11 w-full text-sm" disabled={loading || !codeDraft.trim()}>
              Find computer
            </Button>
          </form>
        )}
        <button
          type="button"
          data-testid="approve-page-sign-out"
          onClick={signOut}
          className="flex h-11 w-full items-center justify-center rounded-lg text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Not your account? Sign out
        </button>
      </div>
    </main>
  );
};
