import { useCallback, useEffect, useRef, useState } from "react";
import {
  ACCOUNT_SESSION_CHANGED_EVENT,
  getAccountContinuity,
  getConfiguredAccountOrigin,
  getStoredAccountId,
  getStoredAccountSessionToken,
  putAccountContinuity,
  resolveStoredAccountId,
} from "../remote/accountSession";
import { getRemoteStatus } from "./tauri";
import {
  createDesktopContinuityPublisher,
  type DesktopContinuityAccount,
  type DesktopContinuityIntent,
} from "./desktopContinuityPublisher";

export interface DesktopSelectionIntent {
  workspaceId: string;
  tabId?: string;
  leafId?: string;
  worktreePath?: string;
}

export interface CommittedDesktopSelection {
  workspaceId: string;
  tabId: string | null;
  leafId: string | null;
  worktreePath: string | null;
  worktreeSlug: string | null;
  sessionId: string | null;
  local: boolean;
}

function currentCredentials() {
  const accountOrigin = getConfiguredAccountOrigin();
  const sessionToken = getStoredAccountSessionToken(accountOrigin);
  const accountId = getStoredAccountId(accountOrigin);
  return { accountOrigin, sessionToken, accountId };
}

function currentAccount(): DesktopContinuityAccount | null {
  const { accountOrigin, sessionToken } = currentCredentials();
  const accountId = getStoredAccountId(accountOrigin);
  return sessionToken && accountId ? { accountOrigin, sessionToken, accountId } : null;
}

export function useDesktopContinuity(options: {
  enabled: boolean;
  leader: boolean;
  selection: CommittedDesktopSelection;
  onError(error: unknown): void;
}) {
  const latest = useRef(options);
  latest.current = options;
  const machineId = useRef<string | null>(null);
  const [, setIntentVersion] = useState(0);
  const pending = useRef<{
    intent: DesktopSelectionIntent;
    account: ReturnType<typeof currentCredentials>;
    publication: DesktopContinuityIntent | null;
  } | null>(null);
  const identityRequest = useRef<AbortController | null>(null);
  const machineRequest = useRef<AbortController | null>(null);
  const observedCredentials = useRef(currentCredentials());
  const publisher = useRef<ReturnType<typeof createDesktopContinuityPublisher> | null>(null);
  if (!publisher.current) {
    publisher.current = createDesktopContinuityPublisher({
      currentAccount,
      read: (account, signal) => getAccountContinuity(account.accountOrigin, account.sessionToken, signal),
      write: (account, target, revision, signal) =>
        putAccountContinuity(account.accountOrigin, account.sessionToken, revision, target, signal),
      onError: (error) => latest.current.onError(error),
      schedule(callback) {
        const timer = setTimeout(() => { void callback(); }, 150);
        return () => clearTimeout(timer);
      },
    });
  }

  useEffect(() => {
    if (!options.enabled) return;
    const invalidate = () => {
      pending.current = null;
      identityRequest.current?.abort();
      identityRequest.current = null;
      machineRequest.current?.abort();
      machineRequest.current = null;
      publisher.current?.invalidate();
    };
    const accountChanged = () => {
      const next = currentCredentials();
      const previous = observedCredentials.current;
      observedCredentials.current = next;
      if (next.accountOrigin !== previous.accountOrigin || next.sessionToken !== previous.sessionToken ||
        (previous.accountId !== null && next.accountId !== previous.accountId)) invalidate();
      else setIntentVersion((version) => version + 1);
    };
    window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, accountChanged);
    window.addEventListener("storage", accountChanged);
    return () => {
      machineId.current = null;
      invalidate();
      window.removeEventListener(ACCOUNT_SESSION_CHANGED_EVENT, accountChanged);
      window.removeEventListener("storage", accountChanged);
    };
  }, [options.enabled]);

  useEffect(() => {
    if (!options.enabled || !options.leader) {
      pending.current = null;
      identityRequest.current?.abort();
      machineRequest.current?.abort();
      publisher.current?.invalidate();
      return;
    }
    const requested = pending.current;
    if (!requested) return;
    const credentials = currentCredentials();
    if (credentials.accountOrigin !== requested.account.accountOrigin ||
      credentials.sessionToken !== requested.account.sessionToken ||
      (requested.account.accountId !== null && credentials.accountId !== requested.account.accountId)) {
      pending.current = null;
      identityRequest.current?.abort();
      machineRequest.current?.abort();
      publisher.current?.invalidate();
      return;
    }
    if (!currentAccount()) return;
    const { intent } = requested;
    const selected = options.selection;
    if (selected.workspaceId !== intent.workspaceId ||
      (intent.tabId !== undefined && intent.tabId !== selected.tabId) ||
      (intent.leafId !== undefined && intent.leafId !== selected.leafId) ||
      (intent.worktreePath !== undefined && intent.worktreePath !== selected.worktreePath)) return;
    // A workspace with no tab yet is not a committed nonlocal selection: keep the intent so the
    // first terminal opened after a project switch still publishes it.
    if (!selected.local) {
      if (selected.tabId !== null) pending.current = null;
      return;
    }
    if (!selected.sessionId || !machineId.current) return;
    const publication = requested.publication;
    pending.current = null;
    if (!publication) return;
    publisher.current?.select({
      machineId: machineId.current,
      workspaceId: selected.workspaceId,
      worktreeSlug: selected.worktreeSlug,
      sessionId: selected.sessionId,
    }, publication);
  });

  return useCallback((intent: DesktopSelectionIntent) => {
    identityRequest.current?.abort();
    identityRequest.current = null;
    machineRequest.current?.abort();
    machineRequest.current = null;
    const account = currentCredentials();
    observedCredentials.current = account;
    // A publication's baseline belongs to the credentials that were current when the user
    // selected, so the intent captures only those two fields; the account id is matched
    // separately and must not travel inside the snapshot.
    const credentials = account.sessionToken === null
      ? null
      : { accountOrigin: account.accountOrigin, sessionToken: account.sessionToken };
    pending.current = latest.current.enabled && latest.current.leader && credentials
      ? {
          intent,
          account,
          publication: publisher.current?.begin(credentials) ?? null,
        }
      : null;
    if (!pending.current) publisher.current?.invalidate();
    if (pending.current && !machineId.current) {
      const controller = new AbortController();
      machineRequest.current = controller;
      const requested = pending.current;
      void getRemoteStatus().then((status) => {
        if (controller.signal.aborted || pending.current !== requested) return;
        machineId.current = (status as typeof status & { machineId?: string | null }).machineId ?? null;
        setIntentVersion((version) => version + 1);
      }).catch((error: unknown) => {
        if (controller.signal.aborted || pending.current !== requested) return;
        latest.current.onError(error);
      });
    }
    if (pending.current && account.sessionToken && !getStoredAccountId(account.accountOrigin)) {
      const controller = new AbortController();
      identityRequest.current = controller;
      const requested = pending.current;
      void resolveStoredAccountId(account.accountOrigin, account.sessionToken, controller.signal).then((accountId) => {
        if (controller.signal.aborted || pending.current !== requested) return;
        if (!accountId) pending.current = null;
        setIntentVersion((version) => version + 1);
      }).catch((error: unknown) => {
        if (controller.signal.aborted || pending.current !== requested) return;
        pending.current = null;
        latest.current.onError(error);
      });
    }
    setIntentVersion((version) => version + 1);
  }, []);
}
