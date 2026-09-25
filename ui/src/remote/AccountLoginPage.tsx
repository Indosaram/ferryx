import React, { useEffect, useState } from "react";
import {
  consumeLogin,
  requestLogin,
} from "./accountSession";

interface AccountLoginPageProps {
  relayUrl: string;
  onLoginSuccess: (token: string, email: string) => void;
  onUseLegacyPin: () => void;
}

export const AccountLoginPage: React.FC<AccountLoginPageProps> = ({
  relayUrl,
  onLoginSuccess,
  onUseLegacyPin,
}) => {
  const [email, setEmail] = useState("");
  const [tokenInput, setTokenInput] = useState("");
  const [codeRequested, setCodeRequested] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let code: string | null = null;
    const hash = window.location.hash;
    if (hash.startsWith("#login=")) {
      code = new URLSearchParams(hash.slice(1)).get("login");
    } else if (hash.startsWith("#account_token=")) {
      code = new URLSearchParams(hash.slice(1)).get("account_token");
    }
    if (!code && window.location.search) {
      const searchParams = new URLSearchParams(window.location.search);
      code = searchParams.get("login") ?? searchParams.get("account_token");
    }

    if (code && code.trim()) {
      const trimmedCode = code.trim();
      setLoading(true);
      setError(null);
      consumeLogin(relayUrl, trimmedCode)
        .then((res) => {
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
          onLoginSuccess(res.token, res.email);
        })
        .catch((err) => {
          setError(err instanceof Error ? err.message : "Failed to consume login token");
          if (window.history && typeof window.history.replaceState === "function") {
            window.history.replaceState(null, "", window.location.pathname);
          }
          window.location.hash = "";
        })
        .finally(() => {
          setLoading(false);
        });
    }
  }, [onLoginSuccess, relayUrl]);

  const handleRequestLink = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!email.trim() || loading) return;

    setLoading(true);
    setError(null);
    try {
      await requestLogin(relayUrl, email.trim());
      setCodeRequested(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to request login link");
    } finally {
      setLoading(false);
    }
  };

  const handleConsumeCode = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!tokenInput.trim() || loading) return;

    setLoading(true);
    setError(null);
    try {
      const res = await consumeLogin(relayUrl, tokenInput.trim());
      onLoginSuccess(res.token, res.email);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Invalid or expired login code");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-[80vh] p-4 text-foreground">
      <div className="w-full max-w-sm bg-card border border-border rounded-lg p-6 shadow-xl space-y-5">
        <div>
          <h2 className="text-lg font-semibold tracking-tight text-foreground">
            Sign In to Ferryx
          </h2>
          <p className="text-xs text-muted-foreground mt-1">
            Access and control your remote machines securely over the encrypted tunnel.
          </p>
        </div>

        {error && (
          <div
            role="alert"
            data-testid="account-login-error"
            className="p-3 text-xs bg-destructive/10 border border-destructive/20 text-destructive rounded-md"
          >
            {error}
          </div>
        )}

        {!codeRequested ? (
          <form onSubmit={handleRequestLink} className="space-y-4">
            <div className="space-y-1.5">
              <label htmlFor="account-email-input" className="text-xs font-medium text-foreground">
                Email Address
              </label>
              <input
                id="account-email-input"
                data-testid="account-email-input"
                type="email"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="name@example.com"
                disabled={loading}
                className="w-full px-3 py-2 text-sm bg-background border border-input rounded-md focus:outline-none focus:ring-1 focus:ring-ring"
              />
            </div>

            <button
              type="submit"
              data-testid="request-magic-link-btn"
              disabled={loading || !email.trim()}
              className="w-full py-2 px-4 bg-primary text-primary-foreground text-sm font-medium rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50"
            >
              {loading ? "Sending link..." : "Send Magic Link"}
            </button>
          </form>
        ) : (
          <form onSubmit={handleConsumeCode} className="space-y-4">
            <p className="text-xs text-muted-foreground">
              A login link was sent to <strong className="text-foreground">{email}</strong>. Enter the code from your email below:
            </p>
            <div className="space-y-1.5">
              <label htmlFor="account-code-input" className="text-xs font-medium text-foreground">
                Login Code
              </label>
              <input
                id="account-code-input"
                data-testid="account-code-input"
                type="text"
                required
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
                placeholder="Enter 6-digit or login code"
                disabled={loading}
                className="w-full px-3 py-2 text-sm bg-background border border-input rounded-md focus:outline-none focus:ring-1 focus:ring-ring font-mono"
              />
            </div>

            <button
              type="submit"
              data-testid="consume-login-code-btn"
              disabled={loading || !tokenInput.trim()}
              className="w-full py-2 px-4 bg-primary text-primary-foreground text-sm font-medium rounded-md hover:bg-primary/90 transition-colors disabled:opacity-50"
            >
              {loading ? "Verifying..." : "Sign In"}
            </button>

            <button
              type="button"
              onClick={() => setCodeRequested(false)}
              className="w-full text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              Use a different email
            </button>
          </form>
        )}

        <div className="pt-2 border-t border-border flex flex-col items-center">
          <button
            type="button"
            data-testid="use-legacy-pin-btn"
            onClick={onUseLegacyPin}
            className="text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            Pair with device PIN instead
          </button>
        </div>
      </div>
    </div>
  );
};
