import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AgentPrompt, AgentPromptAnswer } from "../lib/tauri";
import { buildRemoteAttentionRows, type AttentionRow } from "../features/ferryx/attention/attentionModel";
import type { AttentionPrompts } from "../features/ferryx/attention/attentionPrompts";
import type { RemoteTerminalTabInfo } from "./RemoteSessionList";
import {
  referenceNativeKindFromRegistryId,
  referenceNativeKindIsNative,
  referencePromptNeedsConfirmation,
  type ReferencePrompt,
  type ReferencePromptAnswerPayload,
} from "./chat/referenceTypes";
import { referencePromptErrorIsStale } from "./chat/referencePromptAnswer";

/** A successful prompt read: `prompt` null means the pane's screen holds no card now. */
export type RemoteInboxPromptRead = { prompt: ReferencePrompt | null; screenRevision: string | null };

export type RemoteInboxAnswerOutcome =
  | { ok: true }
  | { ok: false; code: string; message: string };

/** The owning host's reference-chat prompt routes, for one backend session. */
export type RemoteInboxTransport = {
  /** Null when no target can be named yet; throws when the read itself failed. */
  read: (sessionId: string, registryId: string) => Promise<RemoteInboxPromptRead | null>;
  answer: (
    sessionId: string,
    registryId: string,
    payload: ReferencePromptAnswerPayload,
  ) => Promise<RemoteInboxAnswerOutcome>;
};

/** The events socket's `agentPromptChanged` push. */
export type RemoteInboxPromptChange = { sessionId: string; promptId: string | null };

/**
 * The reference card as the shared Inbox reads it. The "type your own answer" row is free text,
 * not an option, and an option keeps its index on the card as its key, as the desktop's does. A
 * multiple choice is answered in the pane, never with one pick from here.
 */
export function agentPromptFromReference(prompt: ReferencePrompt, screenRevision: string): AgentPrompt | null {
  if (prompt.multiSelect) return null;
  const options = prompt.options
    .map((option, index) => ({ key: String(index), label: option.label }))
    .filter((_, index) => index !== prompt.customOptionIndex);
  const kind: AgentPrompt["kind"] =
    prompt.kind === "approval" || prompt.kind === "plan" ? "approval" : options.length === 0 ? "freeText" : "choice";
  return {
    promptId: prompt.id,
    screenRevision,
    question: prompt.question.trim() || prompt.title,
    options,
    kind,
    // The same rule the desktop's daemon reports for an option pick.
    needsConfirmation: referencePromptNeedsConfirmation(prompt, { optionIndex: 0 }),
  };
}

function referenceAnswerOf(answer: AgentPromptAnswer): ReferencePromptAnswerPayload["answer"] {
  return "key" in answer ? { optionIndex: Number(answer.key) } : { customText: answer.text };
}

function nativeRegistryId(tab: RemoteTerminalTabInfo | undefined): string | null {
  const agentType = tab?.agentType?.trim();
  if (!agentType) return null;
  return referenceNativeKindIsNative(referenceNativeKindFromRegistryId(agentType)) ? agentType : null;
}

export type RemoteInbox = {
  rows: AttentionRow[];
  /** The badge: sessions waiting on the user. */
  needsYou: number;
  prompts: AttentionPrompts;
  /** A pushed `agentPromptChanged` frame: re-read that row's prompt, drop it once answered elsewhere. */
  push: (change: RemoteInboxPromptChange) => void;
  /** Re-read every row's prompt (the events socket came back). */
  refreshAll: () => void;
};

/**
 * The phone's Inbox: one row per waiting or finished session of the mirrored list, each needs-you
 * row answered in place through the prompt routes the chat's card uses. A row answered here or on
 * another device leaves at once; it comes back only for a new prompt or a new waiting episode.
 */
export function useRemoteInbox({
  workspaceId,
  workspaceLabel,
  tabs,
  transport,
}: {
  workspaceId: string | null | undefined;
  workspaceLabel: string | null | undefined;
  tabs: readonly RemoteTerminalTabInfo[];
  transport: RemoteInboxTransport;
}): RemoteInbox {
  const transportRef = useRef(transport);
  transportRef.current = transport;
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  // Sessions whose prompt went away (answered, here or elsewhere) while their state still says waiting.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(() => new Set());
  // Bumped per session on a push, so the row's prompt panel reads again.
  const [revisions, setRevisions] = useState<Readonly<Record<string, number>>>({});
  // The prompt each session was last known to show.
  const knownRef = useRef(new Map<string, string>());

  const hide = useCallback((sessionId: string) => {
    setHidden((prev) => (prev.has(sessionId) ? prev : new Set(prev).add(sessionId)));
  }, []);
  const unhide = useCallback((sessionId: string) => {
    setHidden((prev) => {
      if (!prev.has(sessionId)) return prev;
      const next = new Set(prev);
      next.delete(sessionId);
      return next;
    });
  }, []);

  // A hidden row's waiting episode is over once its state moves on: the next one shows again.
  useEffect(() => {
    if (hidden.size === 0) return;
    for (const sessionId of hidden) {
      const tab = tabs.find((candidate) => candidate.sessionId === sessionId);
      if (tab?.activityState === "waiting") continue;
      knownRef.current.delete(sessionId);
      unhide(sessionId);
    }
  }, [hidden, tabs, unhide]);

  const push = useCallback(
    (change: RemoteInboxPromptChange) => {
      const known = knownRef.current.get(change.sessionId);
      if (change.promptId === null) {
        // Only a prompt this Inbox saw can be answered elsewhere; a pane no reader knows keeps its row.
        if (known !== undefined) {
          knownRef.current.delete(change.sessionId);
          hide(change.sessionId);
        }
      } else if (change.promptId !== known) {
        knownRef.current.delete(change.sessionId);
        unhide(change.sessionId);
      }
      setRevisions((prev) => ({ ...prev, [change.sessionId]: (prev[change.sessionId] ?? 0) + 1 }));
    },
    [hide, unhide],
  );

  const refreshAll = useCallback(() => {
    setRevisions((prev) => {
      const next: Record<string, number> = { ...prev };
      for (const tab of tabsRef.current) {
        if (tab.sessionId) next[tab.sessionId] = (next[tab.sessionId] ?? 0) + 1;
      }
      return next;
    });
  }, []);

  const rows = useMemo(() => {
    if (!workspaceId) return [];
    const bySession = new Map(tabs.map((tab) => [tab.id, tab] as const));
    // Every waiting session is listed, the one on screen included: the Inbox is its own view.
    return buildRemoteAttentionRows(workspaceId, workspaceLabel ?? workspaceId, tabs, null).flatMap((row) => {
      const sessionId = bySession.get(row.id)?.sessionId;
      if (!sessionId || hidden.has(sessionId)) return [];
      return [{ ...row, sessionId, revision: revisions[sessionId] ?? 0 }];
    });
  }, [hidden, revisions, tabs, workspaceId, workspaceLabel]);

  const prompts = useMemo<AttentionPrompts>(() => {
    const tabOf = (row: AttentionRow) => tabsRef.current.find((tab) => tab.id === row.id);
    return {
      read: async (row) => {
        const registryId = nativeRegistryId(tabOf(row));
        if (!registryId) return null;
        let read: RemoteInboxPromptRead | null;
        try {
          read = await transportRef.current.read(row.sessionId, registryId);
        } catch (error) {
          // An unreadable screen is not a prompt; the row still opens the pane.
          console.debug("[inbox] remote prompt unreadable", row.sessionId, error);
          return null;
        }
        if (read === null) return null;
        if (read.prompt === null || read.screenRevision === null) {
          // The card this Inbox saw is gone: it was answered somewhere.
          if (read.prompt === null && knownRef.current.delete(row.sessionId)) hide(row.sessionId);
          return null;
        }
        knownRef.current.set(row.sessionId, read.prompt.id);
        return agentPromptFromReference(read.prompt, read.screenRevision);
      },
      answer: async (row, prompt, answer) => {
        const registryId = nativeRegistryId(tabOf(row));
        if (!registryId) return "stale";
        const outcome = await transportRef.current.answer(row.sessionId, registryId, {
          promptId: prompt.promptId,
          screenRevision: prompt.screenRevision,
          answer: referenceAnswerOf(answer),
        });
        if (outcome.ok) return "answered";
        if (referencePromptErrorIsStale(outcome)) return "stale";
        throw outcome;
      },
      onAnswered: (row) => hide(row.sessionId),
    };
  }, [hide]);

  let needsYou = 0;
  for (const row of rows) if (row.state === "needs-you") needsYou += 1;

  return { rows, needsYou, prompts, push, refreshAll };
}
