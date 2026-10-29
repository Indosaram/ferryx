import React, { useState } from "react";
import { getOrCreateInstallationId } from "../lib/storageKeys";
import { suggestDeviceName } from "./deviceIdentity";

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

type PairingPageProps = {
  onPaired: (token: string, metadata?: { machineId?: unknown; displayName?: unknown }) => void;
  transportUrl?: string;
};

export const PairingPage: React.FC<PairingPageProps> = ({ onPaired, transportUrl = window.location.origin }) => {
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
        if (res.status === 429 || failure.code === "pairing_rate_limited") {
          throw new Error("Too many attempts. Wait a moment, then request a new code in Ferryx Desktop.");
        }
        if (failure.code === "PIN_EXPIRED") {
          throw new Error("Code expired. Ask for a new code in Ferryx Desktop settings.");
        }
        const remaining = failure.remainingAttempts;
        if (failure.code === "INVALID_PIN" && remaining !== null) {
          throw new Error(`Wrong code. ${remaining} ${remaining === 1 ? "attempt" : "attempts"} left.`);
        }
        throw new Error(`Pairing failed (${res.status})`);
      }

      const data = await res.json();
      onPaired(data.token, { machineId: data.machineId, displayName: data.displayName });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to pair device");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-[80vh] p-4">
      <div className="w-full max-w-sm bg-card border border-border rounded-lg p-6 shadow-xl">
        <h2 className="text-xl font-bold text-foreground mb-2">Pair Device</h2>
        <p className="text-xs text-muted-foreground mb-6">
          Enter the 6-digit code shown in Ferryx Desktop settings.
        </p>

        {error && (
          <div className="p-3 mb-4 text-xs bg-destructive/15 border border-destructive text-destructive-foreground rounded">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs font-medium text-muted-foreground mb-1">
              Device Name
            </label>
            <input
              type="text"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder="Device name"
              className="w-full px-3 py-2 bg-background border border-border text-foreground rounded text-sm focus:outline-none focus:border-ring"
              disabled={loading}
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-muted-foreground mb-1">
              Pairing PIN
            </label>
            <input
              type="text"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="6-digit PIN" maxLength={6} inputMode="numeric" pattern="[0-9]*"
              className="w-full px-3 py-2 bg-background border border-border text-foreground rounded font-mono text-sm focus:outline-none focus:border-ring"
              disabled={loading}
              autoFocus
            />
          </div>

          <button
            type="submit"
            disabled={loading || !code.trim()}
            className="w-full py-2 px-4 bg-primary hover:bg-primary/90 disabled:opacity-50 text-primary-foreground rounded text-sm font-medium transition-colors"
          >
            {loading ? "Connecting..." : "Connect"}
          </button>
        </form>
      </div>
    </div>
  );
};
