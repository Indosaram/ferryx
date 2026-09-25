import { useEffect, useState, type FormEvent } from "react";
import { AlertCircle, CheckCircle2, Mail } from "lucide-react";
import {
  AccountSessionError,
  consumeLogin,
  requestLogin,
  storeAccountSessionToken,
} from "../../remote/accountSession";
import { DEFAULT_RELAY_ORIGIN } from "../../lib/pairedHostInventory";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";

export interface AccountSignInProps {
  origin?: string;
  onSignIn?: (token: string, email: string) => void;
  className?: string;
}

export function AccountSignIn({
  origin = DEFAULT_RELAY_ORIGIN,
  onSignIn,
  className = "",
}: AccountSignInProps) {
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  const [codeRequested, setCodeRequested] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [infoMessage, setInfoMessage] = useState<string | null>(null);

  useEffect(() => {
    let tokenFromUrl: string | null = null;
    const hash = window.location.hash;
    if (hash.startsWith("#login=")) {
      tokenFromUrl = new URLSearchParams(hash.slice(1)).get("login");
    } else if (hash.startsWith("#account_token=")) {
      tokenFromUrl = new URLSearchParams(hash.slice(1)).get("account_token");
    }
    if (!tokenFromUrl && window.location.search) {
      const searchParams = new URLSearchParams(window.location.search);
      tokenFromUrl =
        searchParams.get("login") ??
        searchParams.get("account_token") ??
        searchParams.get("code");
    }

    if (tokenFromUrl && tokenFromUrl.trim()) {
      const trimmed = tokenFromUrl.trim();
      setLoading(true);
      setError(null);
      setErrorCode(null);
      consumeLogin(origin, trimmed)
        .then((res) => {
          storeAccountSessionToken(res.token);
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
          onSignIn?.(res.token, res.email);
        })
        .catch((err: unknown) => {
          if (err instanceof AccountSessionError) {
            setErrorCode(err.code);
            setError(err.message);
          } else if (
            err &&
            typeof err === "object" &&
            "code" in err &&
            typeof (err as { code: string }).code === "string"
          ) {
            const typed = err as { code: string; message?: string };
            setErrorCode(typed.code);
            setError(typed.message || typed.code);
          } else if (err instanceof Error) {
            setError(err.message);
          } else {
            setError("Failed to consume magic link token");
          }
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
        })
        .finally(() => {
          setLoading(false);
        });
    }
  }, [onSignIn, origin]);

  const handleRequestMagicLink = async (e: FormEvent) => {
    e.preventDefault();
    const cleanEmail = email.trim();
    if (!cleanEmail || loading) return;

    setLoading(true);
    setError(null);
    setErrorCode(null);
    try {
      await requestLogin(origin, cleanEmail);
      setCodeRequested(true);
      setInfoMessage(
        `Magic sign-in link sent to ${cleanEmail}. Click the link in your email or enter the code below.`,
      );
    } catch (err: unknown) {
      if (err instanceof AccountSessionError) {
        setErrorCode(err.code);
        setError(err.message);
      } else if (
        err &&
        typeof err === "object" &&
        "code" in err &&
        typeof (err as { code: string }).code === "string"
      ) {
        const typed = err as { code: string; message?: string };
        setErrorCode(typed.code);
        setError(typed.message || typed.code);
      } else if (err instanceof Error) {
        setError(err.message);
      } else {
        setError("Failed to request magic link");
      }
    } finally {
      setLoading(false);
    }
  };

  const handleConsumeCode = async (e: FormEvent) => {
    e.preventDefault();
    const cleanCode = code.trim();
    if (!cleanCode || loading) return;

    setLoading(true);
    setError(null);
    setErrorCode(null);
    try {
      const res = await consumeLogin(origin, cleanCode);
      storeAccountSessionToken(res.token);
      onSignIn?.(res.token, res.email);
    } catch (err: unknown) {
      if (err instanceof AccountSessionError) {
        setErrorCode(err.code);
        setError(err.message);
      } else if (
        err &&
        typeof err === "object" &&
        "code" in err &&
        typeof (err as { code: string }).code === "string"
      ) {
        const typed = err as { code: string; message?: string };
        setErrorCode(typed.code);
        setError(typed.message || typed.code);
      } else if (err instanceof Error) {
        setError(err.message);
      } else {
        setError("Invalid or expired sign-in code");
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      data-testid="account-sign-in"
      className={`rounded-lg border border-border bg-card p-5 space-y-4 ${className}`}
    >
      <div className="space-y-1">
        <h3 className="text-sm font-semibold text-foreground flex items-center gap-2">
          <Mail className="size-4 text-primary" />
          Sign in to Ferryx
        </h3>
        <p className="text-xs text-muted-foreground">
          Sign in with your email to view, enroll, and manage remote machines.
        </p>
      </div>

      {error ? (
        <div
          role="alert"
          data-testid="account-signin-error"
          data-code={errorCode ?? undefined}
          className="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2.5 text-xs text-destructive"
        >
          <AlertCircle className="size-4 shrink-0" />
          <span>{error}</span>
        </div>
      ) : null}

      {infoMessage && !error ? (
        <div
          role="status"
          data-testid="account-signin-info"
          className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/10 p-2.5 text-xs text-foreground"
        >
          <CheckCircle2 className="size-4 shrink-0 text-primary" />
          <span>{infoMessage}</span>
        </div>
      ) : null}

      {!codeRequested ? (
        <form onSubmit={handleRequestMagicLink} className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="account-email" className="text-xs font-medium">
              Email Address
            </Label>
            <Input
              id="account-email"
              aria-label="Email Address"
              type="email"
              placeholder="name@example.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={loading}
              required
              className="h-8 text-xs max-w-sm"
            />
          </div>
          <Button
            type="submit"
            size="sm"
            disabled={loading || !email.trim()}
          >
            {loading ? "Sending link…" : "Send Magic Link"}
          </Button>
        </form>
      ) : (
        <form onSubmit={handleConsumeCode} className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="account-code" className="text-xs font-medium">
              Verification Code
            </Label>
            <Input
              id="account-code"
              aria-label="Verification Code"
              type="text"
              placeholder="Paste code from magic link"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              disabled={loading}
              required
              className="h-8 text-xs max-w-sm font-mono"
            />
          </div>
          <div className="flex items-center gap-2">
            <Button
              type="submit"
              size="sm"
              disabled={loading || !code.trim()}
            >
              {loading ? "Signing in…" : "Sign In"}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={loading}
              onClick={() => {
                setCodeRequested(false);
                setCode("");
                setError(null);
                setErrorCode(null);
                setInfoMessage(null);
              }}
            >
              Use a different email
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
