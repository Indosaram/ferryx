import React, { useState } from "react";

type PairingPageProps = {
  onPaired: (token: string) => void;
};

export const PairingPage: React.FC<PairingPageProps> = ({ onPaired }) => {
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!code.trim()) return;

    setLoading(true);
    setError(null);

    try {
      const res = await fetch("/api/v1/pair/exchange", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          code: code.trim(),
          deviceName: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser Device",
        }),
      });

      if (!res.ok) {
        throw new Error(`Pairing failed (${res.status})`);
      }

      const data = await res.json();
      onPaired(data.token);
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
          Enter the 6-digit PIN from your Ferryx Desktop settings (valid for 1 minute).
        </p>

        {error && (
          <div className="p-3 mb-4 text-xs bg-destructive/15 border border-destructive text-destructive-foreground rounded">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <input
              type="text"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="6-digit PIN" maxLength={6} inputMode="numeric" pattern="[0-9]*"
              className="w-full px-3 py-2 bg-background border border-border text-foreground rounded font-mono text-sm text-foreground focus:outline-none focus:border-ring"
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
