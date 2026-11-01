import React, { useState } from "react";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { suggestDeviceName } from "./deviceIdentity";
import { userFacingError } from "./userFacingError";

/** Path of the phone pairing entry; the desktop's QR code opens it. */
export const PAIR_ROUTE_PATH = "/pair";

export function isPairRoute(pathname: string): boolean {
  return pathname.replace(/\/+$/, "") === PAIR_ROUTE_PATH;
}

/** The structured fields of a refused pair exchange: the machine `code` and `details.remainingAttempts`. */
async function pairingFailure(res: Response): Promise<{ code: string | null; remainingAttempts: number | null }> {
  try {
    const body: unknown = await res.json();
    const row = body && typeof body === "object" ? (body as { code?: unknown; details?: unknown }) : {};
    const details = row.details;
    const value = details && typeof details === "object" ? (details as { remainingAttempts?: unknown }).remainingAttempts : undefined;
    return {
      code: typeof row.code === "string" ? row.code : null,
      remainingAttempts: typeof value === "number" && Number.isFinite(value) ? value : null,
    };
  } catch {
    return { code: null, remainingAttempts: null };
  }
}

/** The one sentence a refused exchange shows; the status and machine code stay out of the copy. */
function pairingRefusalMessage(status: number, code: string | null, remaining: number | null): string {
  if (status === 429 || code === "pairing_rate_limited") {
    return "Too many attempts. Wait a minute, then get a new code from Ferryx on your computer.";
  }
  if (code === "PIN_EXPIRED") return "This code expired. Get a new code from Ferryx on your computer.";
  if (code === "INVALID_PIN" && remaining !== null) {
    return `Wrong code. ${remaining} ${remaining === 1 ? "attempt" : "attempts"} left.`;
  }
  return "That code didn't work. Get a new code from Ferryx on your computer and try again.";
}

type PairingPageProps = {
  onPaired: (token: string, metadata?: { machineId?: unknown; displayName?: unknown }) => void;
  transportUrl?: string;
  /** Leaves the pairing entry for account sign-in. */
  onUseAccount?: () => void;
};

const FIELD_CLASS =
  "h-11 w-full rounded-lg border border-input bg-background px-3.5 text-base text-foreground placeholder:text-muted-foreground focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

export const PairingPage: React.FC<PairingPageProps> = ({ onPaired, transportUrl = window.location.origin, onUseAccount }) => {
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState(() => suggestDeviceName());
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!code.trim()) return;

    setLoading(true);
    setError(null);

    const installationId = getOrCreateInstallationId();
    try {
      const res = await fetch(transportUrl === window.location.origin ? "/api/v1/pair/exchange" : `${transportUrl}/api/v1/pair/exchange`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          code: code.trim(),
          deviceName: deviceName.trim() || suggestDeviceName(),
          installationId,
        }),
      });

      if (!res.ok) {
        const failure = await pairingFailure(res);
        setError(pairingRefusalMessage(res.status, failure.code, failure.remainingAttempts));
        return;
      }

      const data = await res.json();
      onPaired(data.token, { machineId: data.machineId, displayName: data.displayName });
    } catch (err: unknown) {
      console.warn("Pair exchange failed", err);
      setError(userFacingError(err, { action: "pair this phone" }));
    } finally {
      setLoading(false);
    }
  };

  return (
    <main className="flex min-h-dvh w-full flex-col items-center justify-center bg-background px-4 pt-[max(env(safe-area-inset-top),1rem)] pb-[max(env(safe-area-inset-bottom),1rem)] text-foreground">
      <div className="w-full max-w-sm space-y-6">
        <div className="space-y-2">
          <h1 className="text-xl font-semibold tracking-tight text-foreground">Pair this phone</h1>
          <p className="text-sm text-muted-foreground">
            Enter the 6-digit pairing code shown in Ferryx Desktop settings.
          </p>
        </div>

        {error && (
          <div role="alert" data-testid="pairing-error" className="rounded-lg bg-destructive/10 p-3.5 text-sm leading-relaxed text-foreground">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <label htmlFor="pairing-device-name" className="block text-sm font-medium text-foreground">
              Phone name
            </label>
            <input
              id="pairing-device-name"
              type="text"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder="Phone name"
              autoComplete="off"
              className={FIELD_CLASS}
              disabled={loading}
            />
          </div>

          <div className="space-y-2">
            <label htmlFor="pairing-code" className="block text-sm font-medium text-foreground">
              Pairing code
            </label>
            <input
              id="pairing-code"
              type="text"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="6-digit code"
              maxLength={6}
              inputMode="numeric"
              pattern="[0-9]*"
              autoComplete="one-time-code"
              className={`${FIELD_CLASS} font-mono tracking-widest`}
              disabled={loading}
            />
          </div>

          <button
            type="submit"
            disabled={loading || !code.trim()}
            className="flex h-11 w-full items-center justify-center rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
          >
            {loading ? "Connecting..." : "Connect"}
          </button>
        </form>

        {onUseAccount ? (
          <button
            type="button"
            onClick={onUseAccount}
            className="flex h-11 w-full items-center justify-center rounded-lg text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            Sign in with email instead
          </button>
        ) : null}
      </div>
    </main>
  );
};
