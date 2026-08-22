import React, { useEffect, useState } from "react";
import { PairingPage } from "./PairingPage";
import { RemoteSessionItem, RemoteSessionList } from "./RemoteSessionList";
import { RemoteTerminal } from "./RemoteTerminal";

const TOKEN_KEY = "ferryx_remote_token";
const LEGACY_TOKEN_KEY = "rorca_remote_token";

export const RemoteApp: React.FC = () => {
  const [token, setToken] = useState<string | null>(
    () => localStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(LEGACY_TOKEN_KEY)
  );
  const [sessions, setSessions] = useState<RemoteSessionItem[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

  useEffect(() => {
    // Check fragment #pair=<code >
    const hash = window.location.hash;
    if (hash.startsWith("#pair=")) {
      const code = hash.replace("#pair=", "");
      fetch("/api/v1/pair/exchange", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          code,
          deviceName: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser Device",
        }),
      })
        .then((res) => res.json())
        .then((data) => {
          if (data.token) {
            handlePaired(data.token);
            window.location.hash = "";
          }
        })
        .catch(() => {});
    }
  }, []);

  const handlePaired = (newToken: string) => {
    localStorage.setItem(TOKEN_KEY, newToken);
    localStorage.removeItem(LEGACY_TOKEN_KEY);
    setToken(newToken);
  };

  const loadSessions = async () => {
    if (!token) return;
    try {
      const res = await fetch(`/api/v1/sessions?token=${encodeURIComponent(token)}`);
      if (!res.ok) {
        localStorage.removeItem(TOKEN_KEY);
        localStorage.removeItem(LEGACY_TOKEN_KEY);
        setToken(null);
        return;
      }
      const data = await res.json();
      setSessions(data);
    } catch {
      // ignore
    }
  };

  useEffect(() => {
    if (token) {
      loadSessions();
    }
  }, [token]);

  if (!token) {
    return <PairingPage onPaired={handlePaired} />;
  }

  if (activeSessionId) {
    return (
      <RemoteTerminal
        sessionId={activeSessionId}
        token={token}
        onBack={() => {
          setActiveSessionId(null);
          loadSessions();
        }}
      />
    );
  }

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100 flex flex-col">
      <header className="px-4 py-3 bg-neutral-900 border-b border-neutral-800 flex justify-between items-center">
        <h1 className="text-base font-bold flex items-center gap-2">
          🦀 Ferryx Remote
        </h1>
        <button
          onClick={() => {
            localStorage.removeItem(TOKEN_KEY);
            localStorage.removeItem(LEGACY_TOKEN_KEY);
            setToken(null);
          }}
          className="text-xs text-neutral-500 hover:text-neutral-300"
        >
          Disconnect
        </button>
      </header>

      <main className="flex-1 p-4">
        <RemoteSessionList
          sessions={sessions}
          onSelect={(id) => setActiveSessionId(id)}
          onRefresh={loadSessions}
        />
      </main>
    </div>
  );
};
