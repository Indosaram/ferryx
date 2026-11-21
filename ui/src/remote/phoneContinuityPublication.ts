import type { AccountContinuityPointer } from "./accountSession";

type Selection = Pick<AccountContinuityPointer, "machineId" | "workspaceId" | "worktreeSlug" | "sessionId">;
type Target = { machineId: string; workspaceId: string; worktreeSlug: string | null; sessionId: string };

export interface PhoneContinuityPublication {
  isCurrent(): boolean;
  publish(target: Selection): Promise<boolean>;
}

type Revision = Pick<AccountContinuityPointer, "revision">;
type Scope = { readonly origin: string; readonly token: string; readonly accountId: string | null };
type Operations = {
  isCurrent(): boolean;
  read(signal: AbortSignal): Promise<Revision | null>;
  write(target: Target, revision: number, signal: AbortSignal): Promise<Revision>;
};

export function createPhoneContinuityPublisher() {
  let current: AbortController | null = null;
  let writing: { scope: Scope; controller: AbortController; expected: number; ack: Promise<number | null> } | null = null;

  function invalidate() {
    current?.abort();
    writing?.controller.abort();
    current = null;
    writing = null;
  }

  function begin(scope: Scope, ops: Operations): PhoneContinuityPublication {
    const prior = writing && writing.scope.origin === scope.origin && writing.scope.token === scope.token &&
      (writing.scope.accountId === null || writing.scope.accountId === scope.accountId) ? writing : null;
    if (!prior) invalidate();
    else if (current !== prior.controller) current?.abort();
    const controller = new AbortController();
    current = controller;
    const isCurrent = () => current === controller && !controller.signal.aborted && ops.isCurrent();
    // Read at selection time. Superseding a pick cannot undo a write already accepted by the server.
    const baseline = ops.read(controller.signal).then(
      (pointer) => ({ ok: true, revision: pointer?.revision ?? 0 } as const),
      (error: unknown) => ({ ok: false, error } as const),
    );
    let consumed = false;
    return { isCurrent, async publish({ machineId, workspaceId, worktreeSlug, sessionId }) {
      if (consumed) return false;
      consumed = true;
      if (!machineId?.trim() || !workspaceId?.trim() || !sessionId?.trim()) return false;
      const [result, ack] = await Promise.all([baseline, prior?.ack ?? null]);
      if (!isCurrent()) return false;
      if (!result.ok) throw result.error;
      // Advance only across our own acknowledged CAS, never across another device's write.
      const expected = prior?.expected === result.revision && ack === result.revision + 1 ? ack : result.revision;
      const write = ops.write({ machineId, workspaceId, worktreeSlug, sessionId }, expected, controller.signal);
      const pending = { scope: { ...scope }, controller, expected, ack: write.then(r => r.revision, () => null) };
      writing = pending;
      try { await write; } finally { if (writing === pending) writing = null; }
      return isCurrent();
    } };
  }
  return { begin, invalidate };
}
