import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { MailCheck } from "lucide-react";
import {
  AccountSessionError,
  consumeLogin,
  isTerminalLoginPollError,
  pollLogin,
  requestLogin,
  resolveAccountOrigin,
  storeAccountSessionToken,
} from "./accountSession";
import { userFacingError } from "./userFacingError";

interface AccountLoginPageProps {
  relayUrl: string;
  onLoginSuccess: (token: string, email: string) => void;
  /** Opens the pairing-code entry for a phone paired straight to a computer. */
  onPairWithCode?: () => void;
  /** Why the user is here again, e.g. after a sign-out the server forced. */
  notice?: string | null;
}

const LOGIN_POLL_INTERVAL_MS = 2000;
const LOGIN_POLL_TIMEOUT_MS = 10 * 60 * 1000; // 10 minutes (TTL 600s)
const RESEND_AFTER_MS = 30 * 1000;
const TRANSIENT_FAILURES_BEFORE_HINT = 3;
const LINK_EXPIRED_MESSAGE = "This sign-in link expired. Send a new one.";

const LINK_CODE_COPY: Record<string, string> = {
  LOGIN_CODE_USED: "This sign-in link was already used. Send a new one.",
  LOGIN_CODE_EXPIRED: LINK_EXPIRED_MESSAGE,
  LOGIN_CODE_EXPIRED_OR_UNKNOWN: "This sign-in link was already used or expired. Send a new one.",
  LOGIN_HANDLE_INVALID: "This sign-in request is no longer valid. Send a new link.",
};

/** Sign-in failures as one plain sentence; codes and transport messages never reach the screen. */
export function loginErrorMessage(err: unknown, action: string): string {
  const code = err && typeof err === "object" && "code" in err ? (err as { code?: unknown }).code : undefined;
  if (typeof code === "string" && LINK_CODE_COPY[code]) return LINK_CODE_COPY[code];
  if (err instanceof TypeError) return "Couldn't reach Ferryx. Check your connection and try again.";
  if (code === "UNAUTHORIZED") return "This sign-in link was already used or expired. Send a new one.";
  return userFacingError(err, { action });
}

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
}

export const AccountLoginPage: React.FC<AccountLoginPageProps> = ({
  relayUrl,
  onLoginSuccess,
  onPairWithCode,
  notice = null,
}) => {
  const [email, setEmail] = useState("");
  const [codeRequested, setCodeRequested] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sentAt, setSentAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [pollTrouble, setPollTrouble] = useState(false);

  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const isMountedRef = useRef(true);
  const onLoginSuccessRef = useRef(onLoginSuccess);
  const consumedCodesRef = useRef<Set<string>>(new Set());

  useLayoutEffect(() => {
    onLoginSuccessRef.current = onLoginSuccess;
  }, [onLoginSuccess]);

  const stopPolling = () => {
    if (pollTimerRef.current !== null) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  };

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      stopPolling();
    };
  }, []);

  useEffect(() => {
    if (sentAt === null) return;
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, [sentAt]);

  const startPolling = (origin: string, loginHandle: string) => {
    stopPolling();
    const startTime = Date.now();
    let inFlight = false;
    let transientFailures = 0;
    setPollTrouble(false);

    pollTimerRef.current = setInterval(async () => {
      if (!isMountedRef.current) return;
      if (inFlight) return;

      if (Date.now() - startTime >= LOGIN_POLL_TIMEOUT_MS) {
        stopPolling();
        if (!isMountedRef.current) return;
        setSentAt(null);
        setError(LINK_EXPIRED_MESSAGE);
        return;
      }

      inFlight = true;
      try {
        const res = await pollLogin(origin, loginHandle);
        if (!isMountedRef.current) return;
        transientFailures = 0;
        setPollTrouble(false);

        if (res.status === "approved" && res.token) {
          stopPolling();
          storeAccountSessionToken(res.token, origin);
          onLoginSuccessRef.current(res.token, res.email ?? "");
        }
      } catch (err: unknown) {
        if (!isMountedRef.current) return;

        // Distinguish terminal from transient errors: if transient (network failure,
        // fetch TypeError, 5xx server error), leave the timer running and retry silently.
        // Stopping on a transient error would abandon a login code that may already be
        // consumed server-side once the user opened the magic link.
        if (isTerminalLoginPollError(err)) {
          stopPolling();
          setSentAt(null);
          setError(loginErrorMessage(err, "check your sign-in"));
        } else {
          transientFailures += 1;
          if (transientFailures >= TRANSIENT_FAILURES_BEFORE_HINT) setPollTrouble(true);
        }
        // Transient errors keep polling; only a run of them shows the quiet "still trying" hint.
      } finally {
        inFlight = false;
      }
    }, LOGIN_POLL_INTERVAL_MS);
  };

  useEffect(() => {
    let code: string | null = null;
    const hash = window.location.hash;
    if (hash.startsWith("#code=")) {
      code = new URLSearchParams(hash.slice(1)).get("code");
    } else if (hash.startsWith("#login=")) {
      code = new URLSearchParams(hash.slice(1)).get("login");
    } else if (hash.startsWith("#account_token=")) {
      code = new URLSearchParams(hash.slice(1)).get("account_token");
    } else if (hash.length > 1) {
      const hashParams = new URLSearchParams(hash.slice(1));
      code = hashParams.get("code") ?? hashParams.get("login") ?? hashParams.get("account_token");
    }
    if (!code && window.location.search) {
      const searchParams = new URLSearchParams(window.location.search);
      code = searchParams.get("code") ?? searchParams.get("login") ?? searchParams.get("account_token");
    }

    if (code && code.trim()) {
      const trimmedCode = code.trim();
      if (consumedCodesRef.current.has(trimmedCode)) return;
      consumedCodesRef.current.add(trimmedCode);

      stopPolling();
      setLoading(true);
      setError(null);
      // The account API is not necessarily on the page origin (the desktop app
      // serves this client without an account router), so resolve it first.
      resolveAccountOrigin(relayUrl)
        .then(async (origin) => ({ origin, res: await consumeLogin(origin, trimmedCode) }))
        .then(({ origin, res }) => {
          storeAccountSessionToken(res.token, origin);
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
          onLoginSuccessRef.current(res.token, res.email);
        })
        .catch((err) => {
          console.warn("Sign-in link consume failed", err);
          setError(loginErrorMessage(err, "sign you in"));
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
        })
        .finally(() => {
          setLoading(false);
        });
    }
  }, [relayUrl]);

  const handleRequestLink = async (e: React.SyntheticEvent) => {
    e.preventDefault();
    if (!email.trim() || loading) return;

    stopPolling();
    setLoading(true);
    setError(null);
    try {
      const origin = await resolveAccountOrigin(relayUrl);
      const res = await requestLogin(origin, email.trim());
      setCodeRequested(true);
      setSentAt(Date.now());
      setNow(Date.now());
      startPolling(origin, res.loginHandle);
    } catch (err: unknown) {
      console.warn("Sign-in link request failed", err instanceof AccountSessionError ? err.code : err);
      setError(loginErrorMessage(err, "send the sign-in link"));
    } finally {
      setLoading(false);
    }
  };

  const elapsed = sentAt === null ? 0 : now - sentAt;
  const canResend = sentAt !== null && elapsed >= RESEND_AFTER_MS && !loading;
  const consuming = loading && !codeRequested && !email.trim();

  return (
    <div className="flex min-h-dvh w-full flex-col bg-background text-foreground px-4 pt-[max(env(safe-area-inset-top),1.5rem)] pb-[max(env(safe-area-inset-bottom),1.5rem)] sm:items-center sm:justify-center sm:p-6">
      <main className="flex w-full flex-1 flex-col justify-center sm:max-w-sm sm:flex-none">
        <div className="space-y-6 sm:rounded-2xl sm:border sm:border-border sm:bg-card sm:p-8">
          <div className="flex flex-col items-center space-y-3 text-center">
            <img
              src="/icon-192.png"
              alt=""
              aria-hidden="true"
              width={48}
              height={48}
              className="size-12 rounded-xl object-contain"
            />
            <div>
              <h1 className="text-2xl font-semibold leading-tight tracking-tight text-foreground">
                Sign in to Ferryx
              </h1>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                Follow and answer the agents running on your computer.
              </p>
            </div>
          </div>

          {notice && !error ? (
            <div role="status" data-testid="account-login-notice" className="rounded-lg bg-muted p-3.5 text-sm leading-relaxed text-foreground">
              {notice}
            </div>
          ) : null}

          {error && (
            <div
              role="alert"
              data-testid="account-login-error"
              className="rounded-lg bg-destructive/10 p-3.5 text-sm leading-relaxed text-foreground"
            >
              {error}
            </div>
          )}

          {consuming ? (
            <p role="status" className="text-center text-sm text-muted-foreground">Signing you in...</p>
          ) : null}

          {!codeRequested ? (
            <form onSubmit={handleRequestLink} className="space-y-5">
              <div className="space-y-2">
                <label
                  htmlFor="account-email-input"
                  className="block text-sm font-medium text-foreground"
                >
                  Email address
                </label>
                <input
                  id="account-email-input"
                  data-testid="account-email-input"
                  type="email"
                  required
                  autoComplete="email"
                  autoCapitalize="none"
                  spellCheck={false}
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  placeholder="name@example.com"
                  disabled={loading}
                  className="h-11 w-full rounded-lg border border-input bg-background px-3.5 py-2.5 text-base text-foreground placeholder:text-muted-foreground transition-colors focus:border-border focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
                />
              </div>

              <button
                type="submit"
                data-testid="request-magic-link-btn"
                disabled={loading || !email.trim()}
                className="flex h-11 w-full items-center justify-center rounded-lg bg-primary px-4 py-2.5 font-sans text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
              >
                {loading ? "Sending link..." : "Email me a sign-in link"}
              </button>
            </form>
          ) : (
            <div data-testid="magic-link-waiting" className="space-y-4 pt-1">
              <div role="status" className="flex items-start gap-3 text-left">
                <MailCheck className="mt-0.5 size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <p className="text-sm leading-relaxed text-muted-foreground">
                  We sent a sign-in link to <strong className="break-all font-medium text-foreground">{email}</strong>. Open it on any device and this page signs in by itself. Check spam if it doesn't arrive.
                </p>
              </div>
              {sentAt !== null ? (
                <p data-testid="magic-link-expiry" className="text-sm text-muted-foreground">
                  Link expires in {formatRemaining(LOGIN_POLL_TIMEOUT_MS - elapsed)}
                </p>
              ) : null}
              {pollTrouble ? (
                <p role="status" data-testid="magic-link-poll-trouble" className="text-sm text-muted-foreground">
                  Having trouble reaching Ferryx. Still trying...
                </p>
              ) : null}

              <button
                type="button"
                data-testid="resend-magic-link-btn"
                disabled={!canResend}
                onClick={(event) => void handleRequestLink(event)}
                className="flex h-11 w-full items-center justify-center rounded-lg border border-border text-sm font-medium text-foreground transition-colors hover:bg-muted focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
              >
                {canResend || sentAt === null ? "Send a new link" : `Send a new link in ${Math.ceil((RESEND_AFTER_MS - elapsed) / 1000)} s`}
              </button>
              <button
                type="button"
                onClick={() => {
                  stopPolling();
                  setCodeRequested(false);
                  setSentAt(null);
                  setPollTrouble(false);
                  setError(null);
                }}
                className="flex h-11 w-full items-center justify-center rounded-lg text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              >
                Use a different email
              </button>
            </div>
          )}

          {onPairWithCode ? (
            <button
              type="button"
              data-testid="account-login-pair-with-code"
              onClick={onPairWithCode}
              className="flex h-11 w-full items-center justify-center rounded-lg text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              Pair with a code instead
            </button>
          ) : null}
        </div>
      </main>
    </div>
  );
};
