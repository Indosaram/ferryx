import type {
  PreparedLocalSplit,
  PaneAttachTuple,
  SplitOperationRequest,
  SplitOperationResponse,
  StructuredIpcError,
  TerminalSession,
} from "./types";
import type {
  attachTerminal,
  SpawnTerminalRequest,
  spawnTerminalDetailed,
} from "./tauri";
import { toIpcError } from "./tauri";
import { registerDurableNativeBinding, subscribeNativeTerminalPresentation } from "./nativeTerminalLifecycle";
import { switchDebug } from "./switchDebug";
import { matchesAttachTuple, createAttemptBudget, ATTEMPT_TOTAL_BUDGET_MS, STAGE_CREATE_OR_STATUS_MAX_MS } from "./localSplitContract";

let persistBinding: ((session: LocalSplitSession) => Promise<void>) | undefined;
export function setLocalSplitPersistence(handler: ((session: LocalSplitSession) => Promise<void>) | undefined): void {
  persistBinding = handler;
}

export async function persistNativeBinding(session: TerminalSession): Promise<void> {
  if (!persistBinding) throw failure("missingDurablePersistence");
  await persistBinding(session);
}

export interface LocalSplitIntent {
  readonly requestId: string;
  readonly prepared: PreparedLocalSplit | null;
  readonly cancelRequested: boolean;
  readonly generation: number;
  readonly createSent: boolean;
  readonly ready?: boolean;
  readonly bindingPersisted?: boolean;
  readonly attachTuple?: PaneAttachTuple;
  readonly backendSessionId?: string | null;
  readonly daemonEpoch?: string | null;
  readonly incarnation?: string | null;
}

export type LocalSplitSession = TerminalSession & {
  spawnIntent?: LocalSplitIntent;
  incarnation?: string | null;
};

export function localSplitIntent(session: TerminalSession | undefined | null): LocalSplitIntent | undefined {
  return session ? (session as LocalSplitSession).spawnIntent : undefined;
}

const controllers = new Map<string, LocalSplitLifecycle>();

export function registerLocalSplit(id: string, controller: LocalSplitLifecycle): void {
  controllers.set(id, controller);
}

export function unregisterLocalSplit(id: string): void {
  controllers.delete(id);
}

export function hasLocalSplit(id: string): boolean {
  return controllers.has(id);
}

export function getLocalSplit(id: string): LocalSplitLifecycle | undefined {
  return controllers.get(id);
}

export function retryLocalSplit(id: string): Promise<void> {
  const controller = controllers.get(id);
  return controller ? controller.run() : Promise.reject(failure("missingCoordinator"));
}

export function cancelLocalSplit(id: string): Promise<void> {
  const controller = controllers.get(id);
  return controller ? controller.cancel() : Promise.reject(failure("missingCoordinator"));
}

export interface LocalSplitServices {
  readonly operation: (request: SplitOperationRequest) => Promise<SplitOperationResponse>;
  readonly create: typeof spawnTerminalDetailed;
  readonly attach: typeof attachTerminal;
  readonly ensureEvents: () => Promise<void>;
  readonly read: () => LocalSplitSession | undefined;
  readonly publish: (session: LocalSplitSession) => void;
  readonly remove: () => void;
  readonly persist?: () => Promise<void>;
  readonly visible?: () => boolean;
}

function failure(stage: string): StructuredIpcError {
  return {
    code: "SPAWN_ATTEMPT_TIMEOUT",
    message: "Shell startup was not confirmed. Retry to reconcile the same shell.",
    details: { stage, delivery: "ambiguous" },
  };
}

export class LocalSplitLifecycle {
  private generation: number;
  private unsubscribe: (() => void) | undefined;
  private stopped = false;
  private intent: LocalSplitIntent;
  private originalRequest: SpawnTerminalRequest | undefined;
  private preparing = false;

  constructor(private readonly services: LocalSplitServices, intent: LocalSplitIntent) {
    this.intent = intent;
    this.generation = intent.generation;
  }

  private publish(patch: Partial<LocalSplitSession>): void {
    const session = this.services.read();
    switchDebug("terminal.localSplit.stage", {
      requestId: this.intent.requestId,
      backendSessionId: patch.backendSessionId ?? session?.backendSessionId ?? null,
      originEpoch: this.intent.prepared?.identity.originEpoch ?? null,
      generation: this.intent.generation,
      stage: patch.reconnectLifecycle ?? "intent",
      delivery: this.intent.createSent ? "ambiguous" : "notSent",
    });
    if (session && localSplitIntent(session)?.requestId === this.intent.requestId &&
        (localSplitIntent(session)?.generation ?? 0) <= this.generation) {
      this.services.publish({ ...session, ...patch, spawnIntent: this.intent });
    }
  }

  private async persist(): Promise<void> {
    const persist = this.services.persist ?? persistBinding;
    if (!persist) throw failure("missingDurablePersistence");
    const session = this.services.read();
    if (!session) throw failure("missingPersistenceOwner");
    await persist(session);
  }

  async run(request?: SpawnTerminalRequest): Promise<void> {
    if (!this.originalRequest && request) {
      this.originalRequest = structuredClone(request);
    }
    request = this.originalRequest;
    const generation = ++this.generation;
    this.unsubscribe?.();
    this.intent = { ...this.intent, generation, ready: false, bindingPersisted: false };
    const started = performance.now();
    const budget = createAttemptBudget(started);
    const overall = started + ATTEMPT_TOTAL_BUDGET_MS;
    const creation = started + budget.stageBudget(STAGE_CREATE_OR_STATUS_MAX_MS);
    const current = () => !this.stopped && generation === this.generation &&
      localSplitIntent(this.services.read())?.requestId === this.intent.requestId &&
      (localSplitIntent(this.services.read())?.generation ?? 0) <= generation;

    const bounded = async <T>(promise: Promise<T>, deadline: number, stage: string): Promise<T> => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const stageStarted = performance.now();
      const trace = (phase: "begin" | "end" | "failure", error?: unknown) => {
        const now = performance.now();
        const session = this.services.read();
        switchDebug("terminal.localSplit.attempt", {
          requestId: this.intent.requestId,
          frontendSessionId: session?.id ?? null,
          workspaceId: session?.workspaceId ?? null,
          generation,
          stage,
          phase,
          backendSessionId: session?.backendSessionId ?? null,
          epoch: session?.daemonEpoch ?? this.intent.prepared?.identity.originEpoch ?? null,
          elapsedMs: now - started,
          stageElapsedMs: now - stageStarted,
          remainingMs: Math.max(0, deadline - now),
          delivery: session?.backendSessionId ? "confirmed" : this.intent.createSent ? "ambiguous" : "notSent",
          errorCode: error === undefined ? null : toIpcError(error).code,
          // The Rust sink keeps only allowlisted detail keys, and `nested` is one of them, so the
          // failure message rides there. Without it a rejection whose value is not a structured
          // error (Tauri rejects an argument-deserialization failure with a plain string) is only
          // ever visible as the code `UNKNOWN`.
          nested: error === undefined
            ? null
            : { errorCode: String(toIpcError(error).message ?? "").slice(0, 300) },
        });
      };
      trace("begin");
      try {
        const result = await Promise.race([
          promise,
          new Promise<never>((_, reject) => {
            timer = setTimeout(() => reject(failure(stage)), Math.max(0, deadline - performance.now()));
          }),
        ]);
        trace("end");
        return result;
      } catch (error) {
        trace("failure", error);
        throw error;
      } finally {
        clearTimeout(timer);
      }
    };

    try {
      let session = this.services.read();
      let binding: Partial<LocalSplitSession> = {};
      if (!session || this.intent.cancelRequested || this.stopped) {
        // A silent return here is indistinguishable from a split that never started: the caller
        // (`splitPane`) has already logged `split.pane.dispatched` unconditionally, and nothing
        // downstream of this point emits anything. Name the reason so a stalled split is
        // attributable instead of being read as "the backend call went missing".
        switchDebug("terminal.localSplit.earlyReturn", {
          requestId: this.intent.requestId,
          stage: "pre-flow",
          reason: !session ? "session-not-readable" : this.intent.cancelRequested ? "cancel-requested" : "stopped",
        });
        return;
      }

      {
        this.publish({ reconnectLifecycle: session.backendSessionId ? "validating" : "spawning", reconnectError: null });
        if (!this.intent.prepared) {
          if (!request) throw failure("missingPreparation");
          this.preparing = true;
          const preparation = this.services.operation({
              action: "prepare",
              requestId: this.intent.requestId,
              request,
              remainingMs: Math.max(0, Math.floor(Math.min(creation, overall) - performance.now())),
            });
          void preparation.then(async (response) => {
            this.preparing = false;
            if (response.action === "prepare" && this.intent.cancelRequested) {
              this.intent = { ...this.intent, prepared: response.prepared };
              this.publish({});
              await this.persist();
              await this.cancel();
            }
          }).catch((error: unknown) => {
            this.preparing = false;
            this.publish({ reconnectLifecycle: "failed", reconnectError: toIpcError(error) });
          });
          const response = await bounded(preparation, creation, "prepare");
          this.preparing = false;
          if (this.intent.cancelRequested) return;
          if (!current()) return;
          if (performance.now() >= creation) throw failure("prepare");
          if (response.action !== "prepare") throw failure("prepareProtocol");
          this.intent = { ...this.intent, prepared: response.prepared };
          this.publish({});
          await bounded(this.persist(), creation, "persistPreparation");
        }

        const prepared = this.intent.prepared;
        if (!prepared) {
          switchDebug("terminal.localSplit.earlyReturn", {
            requestId: this.intent.requestId,
            stage: "post-prepare",
            reason: "no-prepared-identity",
          });
          return;
        }
        let canCreate = !session.backendSessionId && !this.intent.createSent && request !== undefined && generation === 1;
        if (!canCreate) {
          if (!current() || performance.now() >= creation) throw failure("status");
          const response = await bounded(
            this.services.operation({
              action: "status",
              identity: prepared.identity,
              remainingMs: Math.max(0, Math.floor(Math.min(creation, overall) - performance.now())),
            }),
            creation,
            "status",
          );
          if (!current()) return;
          if (response.action === "prepare") throw failure("statusProtocol");
          const operation = response.operation;
          switch (operation.state) {
            case "created":
              binding = {
                backendSessionId: operation.sessionId,
                daemonEpoch: operation.daemonEpoch,
                cwd: operation.session.cwd ?? prepared.cwd,
                incarnation: operation.session.incarnation ?? null,
              };
              break;
            case "absent":
              canCreate = operation.canCreate && !session.backendSessionId;
              break;
            case "cancelled":
            case "exited":
              this.publish({ lifecycle: "exited", reconnectLifecycle: "idle" });
              return;
            case "failed":
              throw operation.error;
            case "pending":
            case "unknown":
              throw failure(operation.state);
          }
          if (!canCreate && !binding.backendSessionId) throw failure("unconfirmed");
        }

        if (canCreate) {
          if (!current() || performance.now() >= creation) throw failure("create");
          this.intent = { ...this.intent, createSent: true };
          this.publish({});
          await bounded(this.persist(), creation, "persistCreateIntent");
          if (!current()) return;

          const createPromise = this.services.create(
            {
              workspaceId: prepared.workspaceId,
              worktree: prepared.worktree,
              cwd: prepared.cwd,
              shell: prepared.shell,
              clientRequestId: prepared.identity.requestId,
            },
            {
              createOnly: true,
              preparedLocalSplit: prepared,
              remainingMs: Math.max(0, Math.floor(Math.min(creation, overall) - performance.now())),
            },
          );

          void createPromise.then(
            () => {
              if (this.intent.cancelRequested) void this.cancel();
            },
            () => undefined,
          );

          const result = await bounded(createPromise, creation, "create");
          if (!current()) return;
          binding = {
            backendSessionId: result.sessionId,
            daemonEpoch: result.daemonEpoch,
            cwd: result.session?.cwd ?? prepared.cwd,
            incarnation: result.session.incarnation ?? null,
          };
          this.intent = {
            ...this.intent,
            backendSessionId: result.sessionId,
            daemonEpoch: result.daemonEpoch,
            incarnation: result.session.incarnation ?? null,
          };
          this.publish(binding);
        }
        if (binding.backendSessionId) {
          if (session.backendSessionId && session.backendSessionId !== binding.backendSessionId) throw failure("statusIdentityMismatch");
          if (session.daemonEpoch && session.daemonEpoch !== binding.daemonEpoch &&
              (!session.incarnation || session.incarnation !== binding.incarnation)) throw failure("statusIncarnationUnconfirmed");
          this.intent = { ...this.intent, backendSessionId: binding.backendSessionId,
            daemonEpoch: binding.daemonEpoch, incarnation: binding.incarnation };
          this.publish(binding);
          await bounded(this.persist(), overall, "persistBinding");
          if (!current()) return;
        }
      }

      const latest = this.services.read();
      session = latest ? { ...latest, ...binding } : undefined;
      const prepared = this.intent.prepared;
      if (!session?.backendSessionId || !prepared || !current() || session.lifecycle === "exited") return;

      const backendSessionId = session.backendSessionId;
      let resolvePresented: () => void = () => undefined;
      const presentation = new Promise<void>((resolve) => {
        resolvePresented = resolve;
      });

      const bindingKey = `${backendSessionId}:${session.daemonEpoch ?? ""}:${session.remoteGeneration ?? 0}:${session.remoteConnectionState ?? ""}`;
      const tuple: PaneAttachTuple = {
        backendSessionId, incarnation: session.incarnation ?? null,
        daemonEpoch: session.daemonEpoch ?? "", frontendSessionId: session.id,
        paneIdentity: session.id, bindingKey, attemptGeneration: generation,
      };
      this.intent = { ...this.intent, attachTuple: tuple, bindingPersisted: false };
      this.publish({});
      await bounded(this.persist(), overall, "persistAttempt");
      if (!current()) return;
      if (!registerDurableNativeBinding(tuple, started)) throw failure("staleAttachTuple");
      this.intent = { ...this.intent, bindingPersisted: true };

      this.unsubscribe = subscribeNativeTerminalPresentation(
        {
          frontendSessionId: session.id,
          paneIdentity: session.id,
          backendSessionId,
          bindingKey,
          attemptGeneration: generation,
          incarnation: tuple.incarnation,
          daemonEpoch: tuple.daemonEpoch,
        },
        () => resolvePresented(),
      );

      this.publish({ ...binding, reconnectLifecycle: "binding", reconnectError: null });

      const attachDeadline = Math.min(overall, performance.now() + 4_000);
      await bounded(this.services.ensureEvents(), attachDeadline, "listeners");
      if (!current()) return;
      if (performance.now() >= attachDeadline) throw failure("listeners");

      const afterSequence =
        !binding.backendSessionId && session.daemonEpoch === prepared.identity.originEpoch
          ? session.lastOutputSequence ?? null
          : null;

      const attached = await bounded(
        this.services.attach(backendSessionId, afterSequence, {
          identity: prepared.identity,
          frontendSessionId: session.id,
          generation,
          remainingMs: Math.max(0, Math.floor(attachDeadline - performance.now())),
        }),
        attachDeadline,
        "attach",
      );
      if (!current()) return;
      if (!attached.attachTuple || !matchesAttachTuple(tuple, attached.attachTuple)) {
        throw failure("attachTupleMismatch");
      }
      if (this.services.visible?.() === false) {
        this.publish({ reconnectLifecycle: "idle", reconnectError: null });
        return;
      }

      const presentationDeadline = Math.min(overall, performance.now() + 2_000);
      await bounded(presentation, presentationDeadline, "presentation");

      if (current() && this.services.read()?.lifecycle !== "exited") {
        this.intent = { ...this.intent, ready: true };
        this.publish({ lifecycle: "running", reconnectLifecycle: "idle", reconnectError: null });
      }
    } catch (error) {
      if (current() && this.services.read()?.lifecycle !== "exited") {
        this.publish({ reconnectLifecycle: "failed", reconnectError: toIpcError(error) });
      }
    } finally {
      if (generation === this.generation) {
        this.unsubscribe?.();
        this.unsubscribe = undefined;
      }
    }
  }

  async cancel(): Promise<void> {
    const owner = this.services.read();
    if (!owner || localSplitIntent(owner)?.requestId !== this.intent.requestId ||
      (localSplitIntent(owner)?.generation ?? 0) > this.generation) return;
    ++this.generation;
    const cancelGeneration = this.generation;
    this.unsubscribe?.();
    this.unsubscribe = undefined;
    this.intent = { ...this.intent, generation: this.generation, cancelRequested: true };
    this.publish({});
    await this.persist();
    if (!this.intent.prepared) {
      if (this.preparing || this.intent.createSent) {
        this.publish({ reconnectLifecycle: "failed", reconnectError: failure("cancelPendingPreparation") });
        return;
      }
      this.services.remove();
      this.dispose();
      return;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const response = await Promise.race([
        this.services.operation({
          action: "cancel",
          identity: this.intent.prepared.identity,
          remainingMs: 3_000,
        }),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(failure("cancel")), 3_000);
        }),
      ]);
      if (response.action === "prepare") throw failure("cancelProtocol");
      const operation = response.operation;
      if (this.generation !== cancelGeneration || localSplitIntent(this.services.read())?.generation !== cancelGeneration) return;
      if (operation.state === "cancelled" || operation.state === "exited" || (operation.state === "failed" && operation.noChild)) {
        this.services.remove();
        this.dispose();
      } else {
        this.publish({ reconnectLifecycle: "failed", reconnectError: failure("cancelUnconfirmed") });
      }
    } catch (error) {
      this.publish({ reconnectLifecycle: "failed", reconnectError: toIpcError(error) });
    } finally {
      clearTimeout(timer);
    }
  }

  dispose(): void {
    for (const [id, owner] of controllers) {
      if (owner === this) controllers.delete(id);
    }
    this.stopped = true;
    ++this.generation;
    this.unsubscribe?.();
    this.unsubscribe = undefined;
  }
}
