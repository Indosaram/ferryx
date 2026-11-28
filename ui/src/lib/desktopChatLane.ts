import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState } from "react";
import type { MobileChatMessageProps } from "../remote/chat/MobileChatMessage";
import type { ReferenceOlderState } from "../remote/chat/MobileChatComponents";
import { referenceDrafts } from "../remote/chat/referenceDraft";
import { referenceAnswerFromText, referenceAnswerHint, type ReferenceTypedAnswer } from "../remote/chat/referencePromptAnswer";
import {
  REFERENCE_SUBMIT_MAX_CHARS,
  referenceAnswerIsSingleChoice,
  referenceHistoryDisclosure,
  referenceIsOutcomeUnknown,
  referencePromptNeedsConfirmation,
  referenceTargetKey,
  type ReferenceComposerCapabilities,
  type ReferenceHistoryCursor,
  type ReferenceHistoryPage,
  type ReferencePrompt,
  type ReferencePromptAnswerPayload,
  type ReferenceTargetRef,
  type ReferenceTurn,
} from "../remote/chat/referenceTypes";
import { userFacingError } from "../remote/userFacingError";
import {
  DESKTOP_CHAT_AMBIGUOUS,
  DESKTOP_CHAT_EXITED,
  DESKTOP_CHAT_HELD,
  DESKTOP_CHAT_NO_TARGET,
  DESKTOP_CHAT_OUTCOME_UNKNOWN,
  DESKTOP_CHAT_PROMPT_CHANGED,
  DESKTOP_CHAT_REFRESH_FAILED,
  DESKTOP_CHAT_SEND_FAILED,
  DESKTOP_CHAT_STOP_FAILED,
  DESKTOP_CHAT_STOP_REFUSED,
  DESKTOP_CHAT_TOO_LONG,
  DESKTOP_CHAT_VIEW_ONLY,
  DesktopChatRefusal,
  desktopChatMessagesFromTurns,
  desktopChatMutate,
  desktopChatRestoreDraft,
  desktopChatSessionExited,
  desktopChatStopCapability,
  desktopChatTargetFor,
  desktopChatUniqueIds,
  desktopChatUserTextCount,
  newDesktopChatRequestId,
  type DesktopChatIdentity,
  type DesktopChatTransport,
} from "./desktopChat";
import type { TerminalSession } from "./types";
import {
  ReferenceFileError,
  stageReferenceChatFile,
  referenceMentionInsertion,
  referenceMentionPathOf,
  type ReferenceFileStagingTransport,
} from "../remote/chat/referenceFiles";
import type { ReferenceFileReceipt } from "../remote/chat/referenceTypes";

/** Reuse the owning host's advertised upload capability and existing staging adapter. */
export interface DesktopChatFileStaging {
  readonly capabilities: readonly string[];
  readonly transport: ReferenceFileStagingTransport;
}
export const DESKTOP_CHAT_FILE_CAPABILITY = "pairedPasteUploadV1";

export const DESKTOP_CHAT_DEFAULT_POLL_MS = 2000;

export interface UseDesktopChatArgs {
  readonly session: TerminalSession;
  readonly identity: DesktopChatIdentity;
  readonly transport: DesktopChatTransport;
  /** Reads run only while visible; state survives being hidden. */
  readonly visible: boolean;
  /** History/prompt poll while visible and not exited; 0 disables polling. */
  readonly pollMs?: number;
  readonly fileStaging?: DesktopChatFileStaging;
}

export interface DesktopChatLane {
  readonly stagedFiles: readonly ReferenceFileReceipt[];
  readonly attaching: boolean;
  readonly attachError: ReferenceFileError | null;
  readonly attachFiles: (files: readonly File[], caret: number) => Promise<void>;
  /**
   * Stage files for a prompt's answer field (G6's `ReferencePromptCard.onAttachAnswerFiles`).
   * Nothing here touches the draft or the composer's staged files: an answer set is the card's
   * own, so the same host route returns receipts and the card decides what the answer text says.
   * A refusal throws, and the card reports it against the files that asked for it.
   */
  readonly attachAnswerFiles: (files: readonly File[]) => Promise<readonly ReferenceFileReceipt[]>;
  readonly removeStagedFile: (attachmentId: string) => Promise<void>;
  readonly target: ReferenceTargetRef | null;
  readonly exited: boolean;
  readonly messages: readonly MobileChatMessageProps[];
  readonly pageDisclosure: string | null;
  readonly composerCapabilities: ReferenceComposerCapabilities | null;
  readonly hasOlderPage: boolean;
  readonly loadedOlder: boolean;
  readonly olderState: ReferenceOlderState;
  readonly warnings: readonly string[];
  readonly composerWarning: string | null;
  readonly composerWarningAction: { readonly label: string; readonly onAction: () => void } | null;
  readonly composerPlaceholder: string | undefined;
  readonly draft: string;
  readonly draftUnsaved: boolean;
  readonly isRunning: boolean;
  readonly prompt: ReferencePrompt | null;
  readonly screenRevision: string | null;
  readonly promptGeneration: number;
  readonly promptError: string | null;
  readonly typedAnswer: ReferenceTypedAnswer | null;
  readonly setDraft: (text: string) => void;
  /** The composer's send. `false` keeps the text in the box. */
  readonly send: (text: string) => boolean;
  readonly stop: () => Promise<void>;
  /** For `ReferencePromptCard.onAnswer`: throws `DesktopChatRefusal` carrying the wire code. */
  readonly answerPrompt: (payload: ReferencePromptAnswerPayload) => Promise<boolean>;
  readonly dismissAmbiguousPrompt: () => Promise<void>;
  readonly onPromptChanged: () => void;
  readonly clearTypedAnswer: () => void;
  readonly loadOlder: () => Promise<void>;
  readonly refresh: () => Promise<void>;
}

interface PendingEcho {
  readonly id: string;
  readonly text: string;
  readonly baseline: number;
  readonly at: number;
}

interface LaneState {
  readonly key: string | null;
  readonly stagedFiles: readonly ReferenceFileReceipt[];
  readonly attaching: boolean;
  readonly attachError: ReferenceFileError | null;
  readonly page: ReferenceHistoryPage | null;
  readonly older: readonly ReferenceTurn[];
  readonly olderCursor: ReferenceHistoryCursor | null;
  readonly olderHasMore: boolean;
  readonly olderState: ReferenceOlderState;
  readonly pending: readonly PendingEcho[];
  readonly unknown: { readonly text: string; readonly baseline: number } | null;
  readonly prompt: ReferencePrompt | null;
  readonly screenRevision: string | null;
  readonly promptGeneration: number;
  readonly promptError: string | null;
  readonly typedAnswer: ReferenceTypedAnswer | null;
  readonly warnings: readonly string[];
}

function emptyLane(key: string | null): LaneState {
  return {
    key,
    stagedFiles: [],
    attaching: false,
    attachError: null,
    page: null,
    older: [],
    olderCursor: null,
    olderHasMore: false,
    olderState: "idle",
    pending: [],
    unknown: null,
    prompt: null,
    screenRevision: null,
    promptGeneration: 0,
    promptError: null,
    typedAnswer: null,
    warnings: [],
  };
}

function withWarning(warnings: readonly string[], warning: string): readonly string[] {
  return warnings.includes(warning) ? warnings : [...warnings, warning];
}

function sameJsonValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (left === null || right === null || typeof left !== "object" || typeof right !== "object") return false;
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) return false;
    for (let index = 0; index < left.length; index += 1) {
      if (!sameJsonValue(left[index], right[index])) return false;
    }
    return true;
  }
  const leftObj = left as Record<string, unknown>;
  const rightObj = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftObj);
  const rightKeys = Object.keys(rightObj);
  if (leftKeys.length !== rightKeys.length) return false;
  for (const key of leftKeys) {
    if (!Object.prototype.hasOwnProperty.call(rightObj, key)) return false;
    if (!sameJsonValue(leftObj[key], rightObj[key])) return false;
  }
  return true;
}

function sameHistoryPage(previous: ReferenceHistoryPage | null, next: ReferenceHistoryPage): boolean {
  if (previous === next) return true;
  if (previous === null) return false;
  if (
    previous.generation !== next.generation ||
    previous.turns.length !== next.turns.length ||
    previous.hasMore !== next.hasMore ||
    previous.source !== next.source ||
    previous.availability !== next.availability ||
    previous.unavailableReason !== next.unavailableReason
  ) {
    return false;
  }
  return (
    sameJsonValue(previous.cursor ?? null, next.cursor ?? null) &&
    sameJsonValue(previous.composer ?? null, next.composer ?? null) &&
    sameJsonValue(previous.turns, next.turns)
  );
}

const SEND_WARNINGS = [DESKTOP_CHAT_SEND_FAILED, DESKTOP_CHAT_OUTCOME_UNKNOWN, DESKTOP_CHAT_VIEW_ONLY];

/** Mutations are never aborted: an aborted submit could still have landed. They are fenced instead. */
const MUTATION_SIGNAL = new AbortController().signal;

interface Lane {
  readonly key: string | null;
  readonly controller: AbortController;
  reading: boolean;
}

export function useDesktopChat({
  session,
  identity,
  transport,
  visible,
  pollMs = DESKTOP_CHAT_DEFAULT_POLL_MS,
  fileStaging,
}: UseDesktopChatArgs): DesktopChatLane {
  const exited = desktopChatSessionExited(session);
  const { hostId, ownerId, epoch } = identity;
  const target = useMemo(
    () => desktopChatTargetFor(session, { hostId, ownerId, epoch }),
    [hostId, ownerId, epoch, session],
  );
  const targetKey = target === null ? null : referenceTargetKey(target);
  const registryId = session.agentType ?? null;

  const [state, setState] = useState<LaneState>(() => emptyLane(targetKey));
  const live = state.key === targetKey ? state : emptyLane(targetKey);
  const [looseDraft, setLooseDraft] = useState("");
  const [, bumpDrafts] = useReducer((tick: number) => tick + 1, 0);

  const transportRef = useRef(transport);
  transportRef.current = transport;
  const fileStagingRef = useRef(fileStaging);
  fileStagingRef.current = fileStaging;
  const attachingKeys = useRef(new Set<string>());
  const targetRef = useRef(target);
  const registryRef = useRef(registryId);
  registryRef.current = registryId;
  const exitedRef = useRef(exited);
  exitedRef.current = exited;
  const stateRef = useRef(live);
  stateRef.current = live;
  const laneRef = useRef<Lane>({ key: targetKey, controller: new AbortController(), reading: false });
  const stopInFlightRef = useRef(new Set<string>());

  useLayoutEffect(() => {
    targetRef.current = target;
    if (laneRef.current.key === targetKey) return;
    laneRef.current.controller.abort();
    laneRef.current = { key: targetKey, controller: new AbortController(), reading: false };
  }, [target, targetKey]);

  useEffect(() => {
    if (laneRef.current.controller.signal.aborted) {
      laneRef.current = { key: targetKey, controller: new AbortController(), reading: false };
    }
    const lane = laneRef.current;
    return () => lane.controller.abort();
  }, [targetKey]);
  useEffect(() => referenceDrafts.subscribe(bumpDrafts), []);

  const isCurrent = useCallback((key: string) => laneRef.current.key === key, []);

  const patch = useCallback((key: string, update: (prev: LaneState) => LaneState) => {
    if (!isCurrent(key)) return;
    setState((prev) => update(prev.key === key ? prev : emptyLane(key)));
  }, [isCurrent]);

  const read = useCallback(async (): Promise<void> => {
    const t = targetRef.current;
    const lane = laneRef.current;
    if (t === null || lane.reading) return;
    const key = referenceTargetKey(t);
    if (lane.key !== key) return;
    lane.reading = true;
    const signal = lane.controller.signal;
    const request = { target: t, registryId: registryRef.current };
    try {
      const [history, promptRead] = await Promise.all([
        transportRef.current(
          { ...request, kind: "history", requestId: newDesktopChatRequestId(), params: { cursor: null } },
          signal,
        ),
        exitedRef.current
          ? null
          : transportRef.current({ ...request, kind: "prompt", requestId: newDesktopChatRequestId(), params: {} }, signal),
      ]);
      if (signal.aborted || !isCurrent(key)) return;
      if (history.ok) {
        const page = history.data;
        const unknown = stateRef.current.unknown;
        if (unknown !== null && desktopChatUserTextCount(page.turns, unknown.text) > unknown.baseline) {
          referenceDrafts.settle(t, unknown.text);
          referenceDrafts.end(t);
        }
      }
      patch(key, (prev) => {
        let next: LaneState = prev;
        if (history.ok) {
          const page = history.data;
          const sameGeneration = prev.page === null || prev.page.generation === page.generation;
          const pageUnchanged = sameHistoryPage(prev.page, page);
          const confirmed =
            prev.unknown !== null && desktopChatUserTextCount(page.turns, prev.unknown.text) > prev.unknown.baseline;
          const nextPending = prev.pending.length === 0
            ? prev.pending
            : prev.pending.filter((echo) => desktopChatUserTextCount(page.turns, echo.text) <= echo.baseline);
          const pendingChanged = nextPending.length !== prev.pending.length;
          const nextWarnings = next.warnings.filter(
            (warning) => warning !== DESKTOP_CHAT_REFRESH_FAILED && !(confirmed && warning === DESKTOP_CHAT_OUTCOME_UNKNOWN),
          );
          const warningsChanged = nextWarnings.length !== next.warnings.length;
          if (!pageUnchanged || !sameGeneration || pendingChanged || confirmed || warningsChanged) {
            next = {
              ...next,
              page: pageUnchanged ? prev.page : page,
              ...(sameGeneration ? {} : { older: [], olderCursor: null, olderHasMore: false, olderState: "idle" as const }),
              pending: pendingChanged ? nextPending : prev.pending,
              unknown: confirmed ? null : prev.unknown,
              warnings: warningsChanged ? nextWarnings : next.warnings,
            };
          }
        } else {
          const nextWarnings = withWarning(next.warnings, DESKTOP_CHAT_REFRESH_FAILED);
          if (nextWarnings !== next.warnings) {
            next = { ...next, warnings: nextWarnings };
          }
        }
        if (promptRead !== null && promptRead.ok) {
          const { prompt, screenRevision } = promptRead.data;
          const promptWithStatus = prompt === null || promptRead.data.answerStatus == null
            ? prompt
            : { ...prompt, answerStatus: promptRead.data.answerStatus };
          const moved = promptWithStatus?.id !== prev.prompt?.id || screenRevision !== prev.screenRevision;
          if (promptWithStatus === null || screenRevision === null) {
            if (next.prompt !== null || next.screenRevision !== null || next.typedAnswer !== null || next.promptError !== null) {
              next = { ...next, prompt: null, screenRevision: null, typedAnswer: null, promptError: null };
            }
          } else if (moved) {
            next = {
              ...next,
              prompt: promptWithStatus,
              screenRevision,
              promptGeneration: prev.promptGeneration + 1,
              typedAnswer: promptWithStatus.id === prev.prompt?.id ? prev.typedAnswer : null,
            };
          }
        } else if (promptRead !== null) {
          const nextWarnings = withWarning(next.warnings, DESKTOP_CHAT_REFRESH_FAILED);
          if (next.prompt !== null || next.screenRevision !== null || next.typedAnswer !== null || nextWarnings !== next.warnings) {
            next = {
              ...next,
              prompt: null,
              screenRevision: null,
              typedAnswer: null,
              warnings: nextWarnings,
            };
          }
        }
        return next;
      });
    } catch {
      if (signal.aborted || !isCurrent(key)) return;
      patch(key, (prev) => ({ ...prev, warnings: withWarning(prev.warnings, DESKTOP_CHAT_REFRESH_FAILED) }));
    } finally {
      lane.reading = false;
    }
  }, [isCurrent, patch]);

  useEffect(() => {
    if (!visible || targetKey === null) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const hidden = () => typeof document !== "undefined" && document.hidden;
    const clear = () => {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    };
    const schedule = () => {
      clear();
      if (cancelled || exited || pollMs <= 0 || hidden()) return;
      timer = setTimeout(() => {
        timer = null;
        void poll();
      }, pollMs);
    };
    const poll = async (): Promise<void> => {
      if (cancelled || hidden()) return;
      await read();
      schedule();
    };
    const onVisibilityChange = () => {
      if (cancelled) return;
      if (hidden()) {
        clear();
        return;
      }
      void poll();
    };
    if (!hidden()) void poll();
    if (typeof document !== "undefined") document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      cancelled = true;
      clear();
      if (typeof document !== "undefined") document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [visible, targetKey, exited, pollMs, read]);

  const loadOlder = useCallback(async (): Promise<void> => {
    const t = targetRef.current;
    const current = stateRef.current;
    const cursor = current.older.length > 0 ? current.olderCursor : current.page?.cursor ?? null;
    if (t === null || cursor === null || current.olderState === "loading" || current.page === null) return;
    const key = referenceTargetKey(t);
    const generation = current.page.generation;
    const signal = laneRef.current.controller.signal;
    patch(key, (prev) => ({ ...prev, olderState: "loading" }));
    try {
      const result = await transportRef.current(
        { kind: "history", requestId: newDesktopChatRequestId(), target: t, registryId: registryRef.current, params: { cursor } },
        signal,
      );
      if (signal.aborted) return;
      patch(key, (prev) => {
        if (!result.ok || result.data.generation !== generation || prev.page?.generation !== generation) {
          return { ...prev, olderState: "failed" };
        }
        return {
          ...prev,
          older: [...result.data.turns, ...prev.older],
          olderCursor: result.data.cursor ?? null,
          olderHasMore: result.data.hasMore && result.data.cursor != null,
          olderState: "idle",
        };
      });
    } catch {
      if (!signal.aborted) patch(key, (prev) => ({ ...prev, olderState: "failed" }));
    }
  }, [patch]);

  const attachFiles = useCallback(async (files: readonly File[], caret: number): Promise<void> => {
    const t = targetRef.current;
    if (t === null || exitedRef.current || files.length === 0) return;
    const key = referenceTargetKey(t);
    if (attachingKeys.current.has(key)) return;
    const staging = fileStagingRef.current;
    if (!staging) {
      patch(key, (prev) => ({ ...prev, attachError: new ReferenceFileError(
        "UNSUPPORTED", `Local chat has no file staging route yet; attach the file by path in the terminal instead.`,
      ) }));
      return;
    }
    if (!staging.capabilities.includes(DESKTOP_CHAT_FILE_CAPABILITY)) {
      patch(key, (prev) => ({ ...prev, attachError: new ReferenceFileError(
        "UNSUPPORTED", `This host does not advertise ${DESKTOP_CHAT_FILE_CAPABILITY}; files cannot be staged.`,
      ) }));
      return;
    }
    attachingKeys.current.add(key);
    patch(key, (prev) => ({ ...prev, attaching: true, attachError: null }));
    const signal = laneRef.current.controller.signal;
    const staged = [...stateRef.current.stagedFiles];
    let at = caret;
    try {
      for (const file of files) {
        const receipt = await stageReferenceChatFile({ transport: staging.transport }, t, file, signal, {
          fileCount: staged.length,
          turnBytes: staged.reduce((sum, entry) => sum + entry.receipt.sizeBytes, 0),
        });
        if (signal.aborted || !isCurrent(key)) {
          const cleaned = await staging.transport.cancel(t.target, receipt.receipt.attachmentId);
          if (!cleaned) throw new ReferenceFileError("TIMEOUT", "The host did not confirm abandoned upload cleanup.");
          return;
        }
        staged.push(receipt);
        const path = referenceMentionPathOf(receipt.mentionText)!;
        const inserted = referenceMentionInsertion(referenceDrafts.read(t).text, path, at);
        at = inserted.caret;
        referenceDrafts.set(t, inserted.text);
        patch(key, (prev) => ({ ...prev, stagedFiles: [...prev.stagedFiles, receipt] }));
      }
    } catch (error) {
      patch(key, (prev) => ({ ...prev, attachError: error instanceof ReferenceFileError
        ? error : new ReferenceFileError("UNSUPPORTED", error instanceof Error ? error.message : "File staging failed.") }));
    } finally {
      attachingKeys.current.delete(key);
      patch(key, (prev) => ({ ...prev, attaching: false }));
    }
  }, [isCurrent, patch]);

  const removeStagedFile = useCallback(async (attachmentId: string): Promise<void> => {
    const t = targetRef.current;
    const staging = fileStagingRef.current;
    if (t === null || staging === undefined) return;
    const key = referenceTargetKey(t);
    const receipt = stateRef.current.stagedFiles.find((entry) => entry.receipt.attachmentId === attachmentId);
    if (!receipt || referenceDrafts.isUnknownHeld(t)) return;
    try {
      if (!await staging.transport.remove(t.target, attachmentId)) {
        throw new ReferenceFileError("TIMEOUT", "The host did not confirm file removal.");
      }
      referenceDrafts.set(t, referenceDrafts.read(t).text.split(receipt.mentionText.trimEnd()).join(""));
      patch(key, (prev) => ({ ...prev, stagedFiles: prev.stagedFiles.filter((entry) => entry !== receipt), attachError: null }));
    } catch (error) {
      patch(key, (prev) => ({ ...prev, attachError: error instanceof ReferenceFileError
        ? error : new ReferenceFileError("UNSUPPORTED", error instanceof Error ? error.message : "File removal failed.") }));
    }
  }, [patch]);

  // A prompt's answer files: the same host route and receipt as the composer's, with none of
  // the composer's state. The card owns the answer set; a refusal reaches it as a throw.
  const attachAnswerFiles = useCallback(async (files: readonly File[]): Promise<readonly ReferenceFileReceipt[]> => {
    const t = targetRef.current;
    if (t === null || exitedRef.current || files.length === 0) return [];
    const staging = fileStagingRef.current;
    if (!staging) {
      throw new ReferenceFileError(
        "UNSUPPORTED",
        "Local chat has no file staging route yet; attach the file by path in the terminal instead.",
      );
    }
    if (!staging.capabilities.includes(DESKTOP_CHAT_FILE_CAPABILITY)) {
      throw new ReferenceFileError(
        "UNSUPPORTED",
        `This host does not advertise ${DESKTOP_CHAT_FILE_CAPABILITY}; files cannot be staged.`,
      );
    }
    const signal = laneRef.current.controller.signal;
    const receipts: ReferenceFileReceipt[] = [];
    for (const file of files) {
      signal.throwIfAborted();
      receipts.push(await stageReferenceChatFile({ transport: staging.transport }, t, file, signal, {
        fileCount: receipts.length,
        turnBytes: receipts.reduce((sum, entry) => sum + entry.receipt.sizeBytes, 0),
      }));
    }
    return receipts;
  }, []);

  const submit = useCallback(async (t: ReferenceTargetRef, text: string): Promise<void> => {
    const key = referenceTargetKey(t);
    const requestId = newDesktopChatRequestId();
    const turns = [...stateRef.current.older, ...(stateRef.current.page?.turns ?? [])];
    const echo: PendingEcho = {
      id: `pending-${requestId}`,
      text,
      baseline: desktopChatUserTextCount(turns, text),
      at: Date.now(),
    };
    patch(key, (prev) => ({ ...prev, pending: [...prev.pending, echo] }));
    // Files already live on the owning host. Editable path mentions travel in text;
    // attachmentIds is intentionally empty, never an unfinished upload-id lane.
    const outcome = await desktopChatMutate(() =>
      transportRef.current(
        { kind: "submit", requestId, target: t, registryId: registryRef.current, params: { text, attachmentIds: [], origin: "chat" } },
        MUTATION_SIGNAL,
      ),
    );
    if (outcome.ok) {
      referenceDrafts.settle(t, text);
      referenceDrafts.end(t, text);
      patch(key, (prev) => ({ ...prev,
        stagedFiles: prev.stagedFiles.filter((entry) => !text.includes(entry.mentionText.trimEnd())),
        warnings: prev.warnings.filter((warning) => !SEND_WARNINGS.includes(warning)),
      }));
      if (isCurrent(key)) void read();
      return;
    }
    const unknown = referenceIsOutcomeUnknown(outcome.code);
    referenceDrafts.set(t, (current) => desktopChatRestoreDraft(current, text));
    if (unknown) referenceDrafts.holdUnknown(t);
    else referenceDrafts.end(t, text);
    const refusal =
      outcome.code === "FORBIDDEN" ? DESKTOP_CHAT_VIEW_ONLY : unknown ? DESKTOP_CHAT_OUTCOME_UNKNOWN : DESKTOP_CHAT_SEND_FAILED;
    patch(key, (prev) => ({
      ...prev,
      pending: prev.pending.filter((entry) => entry.id !== echo.id),
      unknown: unknown ? { text, baseline: echo.baseline } : prev.unknown,
      warnings: withWarning(prev.warnings, refusal),
    }));
  }, [isCurrent, patch, read]);

  const answerPrompt = useCallback(async (payload: ReferencePromptAnswerPayload): Promise<boolean> => {
    const t = targetRef.current;
    if (t === null) throw new DesktopChatRefusal("UNAUTHORIZED", DESKTOP_CHAT_NO_TARGET);
    const key = referenceTargetKey(t);
    const generation = stateRef.current.promptGeneration;
    const outcome = await desktopChatMutate(() =>
      transportRef.current(
        { kind: "answer", requestId: newDesktopChatRequestId(), target: t, registryId: registryRef.current, params: payload },
        MUTATION_SIGNAL,
      ),
    );
    const latest = stateRef.current;
    const stillThisCard =
      isCurrent(key) &&
      latest.promptGeneration === generation &&
      latest.prompt?.id === payload.promptId &&
      latest.screenRevision === payload.screenRevision;
    if (!outcome.ok) {
      if (stillThisCard) {
        const message = userFacingError({ code: outcome.code }, { action: "send your answer" });
        patch(key, (prev) => ({ ...prev, promptError: message }));
      }
      throw new DesktopChatRefusal(outcome.code, outcome.message);
    }
    if (!stillThisCard) return true;
    patch(key, (prev) => ({
      ...prev,
      prompt: null,
      screenRevision: null,
      typedAnswer: null,
      promptError: null,
      promptGeneration: prev.promptGeneration + 1,
    }));
    void read();
    return true;
  }, [isCurrent, patch, read]);

  const dismissAmbiguousPrompt = useCallback(async (): Promise<void> => {
    const t = targetRef.current;
    const current = stateRef.current;
    if (t === null || current.prompt === null || current.screenRevision === null) return;
    const key = referenceTargetKey(t);
    const promptId = current.prompt.id;
    const screenRevision = current.screenRevision;
    const response = await transportRef.current(
      { kind: "resolvePrompt", requestId: newDesktopChatRequestId(), target: t, registryId: registryRef.current,
        params: { promptId, screenRevision } },
      MUTATION_SIGNAL,
    );
    if (!response.ok) throw new DesktopChatRefusal(response.code, response.message);
    patch(key, (prev) => ({ ...prev, prompt: null, screenRevision: null, promptGeneration: prev.promptGeneration + 1 }));
    void read();
  }, [patch, read]);

  const send = useCallback((text: string): boolean => {
    const t = targetRef.current;
    const key = t === null ? null : referenceTargetKey(t);
    const warn = (warning: string): false => {
      if (key !== null) patch(key, (prev) => ({ ...prev, warnings: withWarning(prev.warnings, warning) }));
      return false;
    };
    if (exitedRef.current) return warn(DESKTOP_CHAT_EXITED);
    if (t === null || key === null) return false;
    if (attachingKeys.current.has(key) || stateRef.current.attachError !== null) return false;
    const { prompt, screenRevision } = stateRef.current;
    if (prompt !== null) {
      if (stateRef.current.stagedFiles.length > 0) return warn(DESKTOP_CHAT_HELD);
      const typed = referenceAnswerFromText(prompt, text);
      if (typed === null) return warn(DESKTOP_CHAT_HELD);
      if (referencePromptNeedsConfirmation(prompt, typed)) {
        patch(key, (prev) => ({ ...prev, typedAnswer: typed }));
        return true;
      }
      if (!referenceAnswerIsSingleChoice(typed)) return warn(DESKTOP_CHAT_AMBIGUOUS);
      if (screenRevision === null) {
        patch(key, (prev) => ({ ...prev, promptError: DESKTOP_CHAT_PROMPT_CHANGED }));
        return false;
      }
      void answerPrompt({ promptId: prompt.id, screenRevision, answer: typed }).catch(() => {
        referenceDrafts.set(t, (current) => desktopChatRestoreDraft(current, text));
      });
      return true;
    }
    const trimmed = text.trim();
    if (!trimmed) return false;
    if (trimmed.length > REFERENCE_SUBMIT_MAX_CHARS) return warn(DESKTOP_CHAT_TOO_LONG);
    if (!referenceDrafts.begin(t, trimmed)) return false;
    void submit(t, trimmed);
    return true;
  }, [answerPrompt, patch, submit]);

  const stop = useCallback(async (): Promise<void> => {
    const t = targetRef.current;
    if (t === null || exitedRef.current) return;
    const key = referenceTargetKey(t);
    const capability = desktopChatStopCapability(registryRef.current);
    if (capability === "refused") {
      patch(key, (prev) => ({ ...prev, warnings: withWarning(prev.warnings, DESKTOP_CHAT_STOP_REFUSED) }));
      return;
    }
    if (stopInFlightRef.current.has(key)) return;
    stopInFlightRef.current.add(key);
    try {
      const outcome = await desktopChatMutate(() =>
        transportRef.current(
          { kind: "stop", requestId: newDesktopChatRequestId(), target: t, registryId: registryRef.current, params: { capability } },
          MUTATION_SIGNAL,
        ),
      );
      if (!outcome.ok) patch(key, (prev) => ({ ...prev, warnings: withWarning(prev.warnings, DESKTOP_CHAT_STOP_FAILED) }));
    } finally {
      stopInFlightRef.current.delete(key);
    }
  }, [patch]);

  const setDraft = useCallback((text: string) => {
    const t = targetRef.current;
    if (t === null) setLooseDraft(text);
    else referenceDrafts.set(t, text);
  }, []);

  const onPromptChanged = useCallback(() => {
    if (targetKey === null) return;
    patch(targetKey, (prev) => ({ ...prev, promptError: DESKTOP_CHAT_PROMPT_CHANGED }));
    void read();
  }, [patch, read, targetKey]);

  const clearTypedAnswer = useCallback(() => {
    if (targetKey !== null) patch(targetKey, (prev) => ({ ...prev, typedAnswer: null }));
  }, [patch, targetKey]);

  const releaseUnknown = useCallback(() => {
    const t = targetRef.current;
    if (t === null) return;
    referenceDrafts.end(t);
    patch(referenceTargetKey(t), (prev) => ({
      ...prev,
      unknown: null,
      warnings: prev.warnings.filter((warning) => warning !== DESKTOP_CHAT_OUTCOME_UNKNOWN),
    }));
  }, [patch]);

  const messages = useMemo(() => {
    const turns = desktopChatMessagesFromTurns([...live.older, ...(live.page?.turns ?? [])]);
    const echoes: MobileChatMessageProps[] = live.pending.map((echo) => ({
      id: echo.id,
      role: "user",
      content: echo.text,
      timestamp: echo.at,
      pending: true,
    }));
    return desktopChatUniqueIds([...turns, ...echoes]);
  }, [live.older, live.page, live.pending]);

  const unknownHeld = target !== null && referenceDrafts.isUnknownHeld(target);
  const draft = target === null ? looseDraft : referenceDrafts.read(target).text;
  const prompt = exited ? null : live.prompt;

  return {
    stagedFiles: live.stagedFiles,
    attaching: live.attaching,
    attachError: live.attachError,
    attachFiles,
    attachAnswerFiles,
    removeStagedFile,
    target,
    exited,
    messages,
    pageDisclosure: live.page === null ? null : referenceHistoryDisclosure(live.page),
    composerCapabilities: exited ? null : live.page?.composer ?? null,
    hasOlderPage: live.older.length > 0 ? live.olderHasMore : Boolean(live.page?.hasMore && live.page.cursor),
    loadedOlder: live.older.length > 0,
    olderState: live.olderState,
    warnings: live.warnings,
    composerWarning: exited
      ? DESKTOP_CHAT_EXITED
      : target === null
        ? DESKTOP_CHAT_NO_TARGET
        : unknownHeld
          ? DESKTOP_CHAT_OUTCOME_UNKNOWN
          : null,
    composerWarningAction: unknownHeld && !exited ? { label: "Allow sending again", onAction: releaseUnknown } : null,
    composerPlaceholder: prompt !== null ? referenceAnswerHint(prompt) : undefined,
    draft,
    draftUnsaved: target !== null && referenceDrafts.isUnsaved(target),
    isRunning: !exited && (session.lifecycle === "working" || prompt !== null),
    prompt,
    screenRevision: exited ? null : live.screenRevision,
    promptGeneration: live.promptGeneration,
    promptError: live.promptError,
    typedAnswer: live.typedAnswer,
    setDraft,
    send,
    stop,
    answerPrompt,
    dismissAmbiguousPrompt,
    onPromptChanged,
    clearTypedAnswer,
    loadOlder,
    refresh: read,
  };
}
