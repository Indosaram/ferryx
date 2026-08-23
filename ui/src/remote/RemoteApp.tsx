import React, { useCallback, useEffect, useState } from "react";
import { PairingPage } from "./PairingPage";
import {
  normalizeRemoteWorkspaceState,
  RemoteWorkspaceMirror,
  type RemoteContextOption,
  type RemoteWorkspaceModel,
} from "./RemoteSessionList";
import { RemoteTerminal } from "./RemoteTerminal";

const TOKEN_KEY = "ferryx_remote_token";
const LEGACY_TOKEN_KEY = "rorca_remote_token";

const EMPTY_MODEL: RemoteWorkspaceModel = {
  context: {
    workspaceId: null,
    worktreeSlug: null,
    worktreeLabel: null,
    activeTerminal: null,
  },
  options: [],
};

export const RemoteApp: React.FC = () => {
  const [token, setToken] = useState<string | null>(
    () => localStorage.getItem(TOKEN_KEY) ?? localStorage.getItem(LEGACY_TOKEN_KEY),
  );
  const [model, setModel] = useState<RemoteWorkspaceModel>(EMPTY_MODEL);
  const [pending, setPending] = useState<RemoteContextOption | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);

  const disconnect = useCallback(() => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(LEGACY_TOKEN_KEY);
    setToken(null);
    setModel(EMPTY_MODEL);
    setPending(null);
    setStatusMessage(null);
  }, []);

  const handlePaired = useCallback((newToken: string) => {
    localStorage.setItem(TOKEN_KEY, newToken);
    localStorage.removeItem(LEGACY_TOKEN_KEY);
    setToken(newToken);
  }, []);

  const refreshWorkspace = useCallback(async (): Promise<RemoteWorkspaceModel | null> => {
    if (!token) return null;
    try {
      const response = await fetch(
        `/api/v1/workspace/state?token=${encodeURIComponent(token)}`,
      );
      if (!response.ok) {
        disconnect();
        return null;
      }
      const next = normalizeRemoteWorkspaceState(await response.json());
      setModel(next);
      return next;
    } catch {
      return null;
    }
  }, [disconnect, token]);

  useEffect(() => {
    const hash = window.location.hash;
    if (!hash.startsWith("#pair=")) return;

    const code = hash.slice("#pair=".length);
    fetch("/api/v1/pair/exchange", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        code,
        deviceName: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser Device",
      }),
    })
      .then((response) => response.json())
      .then((data) => {
        if (typeof data.token !== "string") return;
        handlePaired(data.token);
        window.location.hash = "";
      })
      .catch(() => undefined);
  }, [handlePaired]);

  useEffect(() => {
    if (token) void refreshWorkspace();
  }, [refreshWorkspace, token]);

  const selectContext = useCallback(async (option: RemoteContextOption) => {
    if (!token || pending) return;
    const target = option.worktreeLabel ?? option.worktreeSlug;
    setPending(option);
    setStatusMessage(
      `Switching to ${option.workspaceId}${target ? ` / ${target}` : ""}...`,
    );

    try {
      const response = await fetch(
        `/api/v1/workspace/select?token=${encodeURIComponent(token)}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            workspaceId: option.workspaceId,
            ...(option.worktreeSlug ? { worktreeSlug: option.worktreeSlug } : {}),
          }),
        },
      );
      if (!response.ok) throw new Error(`Selection failed (${response.status})`);

      const confirmed = await refreshWorkspace();
      if (!confirmed) throw new Error("Desktop context could not be refreshed");
      const confirmedWorktree =
        confirmed.context.worktreeSlug ?? confirmed.context.worktreeLabel;
      const requestedWorktree = option.worktreeSlug ?? option.worktreeLabel;
      if (
        confirmed.context.workspaceId !== option.workspaceId ||
        (requestedWorktree !== null && confirmedWorktree !== requestedWorktree)
      ) {
        throw new Error("Ferryx Desktop has not confirmed this context yet");
      }
      setStatusMessage("Desktop context confirmed");
    } catch (error) {
      setStatusMessage(error instanceof Error ? error.message : "Context selection failed");
    } finally {
      setPending(null);
    }
  }, [pending, refreshWorkspace, token]);

  if (!token) return <PairingPage onPaired={handlePaired} />;

  const activeTerminal = model.context.activeTerminal;

  return (
    <div className="flex h-[100dvh] min-h-screen flex-col overflow-hidden bg-background text-foreground">
      <header className="flex h-11 shrink-0 items-center justify-between border-b border-border bg-card px-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="flex size-6 items-center justify-center rounded-md bg-primary text-xs font-bold text-primary-foreground" aria-hidden="true">
            F
          </span>
          <div className="min-w-0">
            <h1 className="truncate text-sm font-semibold">Ferryx Remote</h1>
            <p className="text-xs text-muted-foreground">Following Ferryx Desktop</p>
          </div>
        </div>
        <button
          type="button"
          onClick={disconnect}
          className="min-h-9 rounded-md px-3 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Disconnect
        </button>
      </header>

      <RemoteWorkspaceMirror
        model={model}
        pending={pending}
        statusMessage={statusMessage}
        onSelect={(option) => void selectContext(option)}
      >
        {activeTerminal ? (
          <RemoteTerminal
            sessionId={activeTerminal.sessionId}
            token={token}
            onBack={() => undefined}
            embedded
          />
        ) : null}
      </RemoteWorkspaceMirror>
    </div>
  );
};
