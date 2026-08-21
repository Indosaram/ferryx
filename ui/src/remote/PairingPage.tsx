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
      <div className="w-full max-w-sm bg-neutral-900 border border-neutral-800 rounded-lg p-6 shadow-xl">
        <h2 className="text-xl font-bold text-neutral-100 mb-2">Pair Device</h2>
        <p className="text-xs text-neutral-400 mb-6">
          Enter the one-time pairing code from your rorca Desktop settings to connect.
        </p>

        {error && (
          <div className="p-3 mb-4 text-xs bg-red-950 border border-red-800 text-red-300 rounded">
            {error}
          </div>
        )}

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <input
              type="text"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="Pairing Code"
              className="w-full px-3 py-2 bg-neutral-950 border border-neutral-800 rounded font-mono text-sm text-neutral-100 focus:outline-none focus:border-blue-500"
              disabled={loading}
              autoFocus
            />
          </div>

          <button
            type="submit"
            disabled={loading || !code.trim()}
            className="w-full py-2 px-4 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white rounded text-sm font-medium transition-colors"
          >
            {loading ? "Connecting..." : "Connect"}
          </button>
        </form>
      </div>
    </div>
  );
};
