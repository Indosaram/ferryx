import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AgentPrompt, AgentPromptAnswer } from "../lib/tauri";
import { formatAttentionLocation, sortAttentionRows, type AttentionRow } from "../features/ferryx/attention/attentionModel";
import type { AttentionPrompts } from "../features/ferryx/attention/attentionPrompts";
import type { RemoteWorkspaceModel } from "./RemoteSessionList";
import { INBOX_WHY, type InboxRow } from "./home/phoneHomeModel";
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

/**
 * One waiting or finished session on the host, wherever it runs: a pane of the mirrored view, a
 * session row of another worktree or project, or (account mode) any machine's inventory row.
 */
export type RemoteInboxEntry = {
  /** Row identity: machine (empty for a direct pairing) and session. */
  key: string;
  sessionId: string;
  /** The mirrored pane that shows this session, when the current view has one. */
  tabId: string | null;
  workspaceId: string;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  state: "waiting" | "done";
  who: string;
  text?: string;
  location?: string;
  at?: number;
  agentType: string | null;
  supervisorLabel?: string;
  /** Account machine owning the session; null on a direct pairing. */
  machineId: string | null;
  /** The prompt routes of the connected host can read this session's card. */
  readable: boolean;
};

function inboxKey(machineId: string | null, sessionId: string): string {
  return `${machineId ?? ""}\u0000${sessionId}`;
}

function inboxState(value: string | null | undefined): "waiting" | "done" | null {
  return value === "waiting" || value === "done" ? value : null;
}

function whoAndText(agentType: string | null | undefined, label: string): { who: string; text?: string } {
  const agent = agentType?.trim();
  return { who: agent || label, ...(agent && label !== agent ? { text: label } : {}) };
}

/**
 * Every waiting or finished session the phone knows of on the host, not only the desktop's active
 * selection: the mirrored panes first (their activity is the freshest), then every session row of
 * any worktree or project, then (account mode) every machine's inventory. One row per session.
 */
export function remoteInboxEntries(input: {
  model: RemoteWorkspaceModel;
  /** The account machine the mirrored view belongs to; null on a direct pairing. */
  machineId: string | null;
  accountRows: readonly InboxRow[];
}): RemoteInboxEntry[] {
  const { model, machineId } = input;
  const context = model.context;
  const labelOf = (workspaceId: string) => {
    const option = model.options.find(
      (candidate) => candidate.workspaceId === workspaceId && (candidate.projectLabel || candidate.workspaceLabel),
    );
    return option?.projectLabel ?? option?.workspaceLabel ?? workspaceId;
  };
  const entries: RemoteInboxEntry[] = [];
  const seen = new Set<string>();
  const add = (entry: RemoteInboxEntry) => {
    if (seen.has(entry.key)) return;
    seen.add(entry.key);
    entries.push(entry);
  };

  for (const tab of context.terminalTabs ?? []) {
    const state = inboxState(tab.activityState);
    const workspaceId = tab.workspaceId ?? context.workspaceId;
    if (!state || !tab.sessionId || !workspaceId) continue;
    const worktreeLabel = tab.worktreeLabel ?? context.worktreeLabel ?? null;
    add({
      key: inboxKey(machineId, tab.sessionId),
      sessionId: tab.sessionId,
      tabId: tab.id,
      workspaceId,
      worktreeSlug: tab.worktreeSlug !== undefined ? tab.worktreeSlug : context.worktreeSlug,
      worktreeLabel,
      state,
      ...whoAndText(tab.agentType, tab.label),
      location: formatAttentionLocation(labelOf(workspaceId), worktreeLabel ?? undefined),
      agentType: tab.agentType ?? null,
      machineId,
      readable: true,
    });
  }

  // Every project's waiting sessions the desktop publishes, beyond the mirrored view.
  for (const item of model.sessionAttention ?? []) {
    const option = model.options.find(
      (candidate) => candidate.workspaceId === item.workspaceId && candidate.worktreeSlug === item.worktreeSlug && !candidate.sessionId,
    );
    const worktreeLabel = option?.worktreeLabel ?? item.worktreeSlug;
    add({
      key: inboxKey(machineId, item.sessionId),
      sessionId: item.sessionId,
      tabId: null,
      workspaceId: item.workspaceId,
      worktreeSlug: item.worktreeSlug,
      worktreeLabel,
      state: item.state,
      ...whoAndText(item.agentType, `Session ${item.sessionId.slice(0, 8)}`),
      location: formatAttentionLocation(labelOf(item.workspaceId), worktreeLabel ?? undefined),
      agentType: item.agentType,
      machineId,
      readable: true,
    });
  }

  for (const option of model.options) {
    // Account options are listed through their inventory rows below.
    if (option.machineId || option.unavailable || !option.sessionId) continue;
    const state = inboxState(option.attention ?? option.activityState);
    if (!state) continue;
    const label = option.sessionLabel ?? `Session ${option.sessionId.slice(0, 8)}`;
    add({
      key: inboxKey(null, option.sessionId),
      sessionId: option.sessionId,
      tabId: null,
      workspaceId: option.workspaceId,
      worktreeSlug: option.worktreeSlug,
      worktreeLabel: option.worktreeLabel,
      state,
      ...whoAndText(option.agentType, label),
      location: formatAttentionLocation(labelOf(option.workspaceId), option.worktreeLabel ?? undefined),
      agentType: option.agentType ?? null,
      machineId: null,
      readable: true,
    });
  }

  for (const row of input.accountRows) {
    add({
      key: inboxKey(row.machineId, row.sessionId),
      sessionId: row.sessionId,
      tabId: null,
      workspaceId: row.option.workspaceId,
      worktreeSlug: row.option.worktreeSlug,
      worktreeLabel: row.option.worktreeLabel,
      state: row.kind === "done" ? "done" : "waiting",
      ...whoAndText(row.agentType, row.title),
      location: row.where,
      ...(row.observedAt !== null ? { at: row.observedAt } : {}),
      agentType: row.agentType,
      ...(row.kind === "held" ? { supervisorLabel: INBOX_WHY.held } : {}),
      machineId: row.machineId,
      readable: row.machineId === machineId,
    });
  }
  return entries;
}

function nativeRegistryId(entry: RemoteInboxEntry | undefined): string | null {
  if (!entry?.readable) return null;
  const agentType = entry.agentType?.trim();
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
 * The phone's one Inbox, shown both on the home and in the session sheet: one row per waiting or
 * finished session on the host, each needs-you row answered in place through the prompt routes
 * the chat's card uses. A row answered here or on another device leaves at once; it comes back
 * only for a new prompt or a new waiting episode.
 */
export function useRemoteInbox({
  entries,
  transport,
}: {
  entries: readonly RemoteInboxEntry[];
  transport: RemoteInboxTransport;
}): RemoteInbox {
  const transportRef = useRef(transport);
  transportRef.current = transport;
  const entriesRef = useRef(entries);
  entriesRef.current = entries;
  // Rows (machine plus session) whose prompt went away (answered, here or elsewhere) while their state still says waiting.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(() => new Set());
  // Bumped per row on a push, so the row's prompt panel reads again.
  const [revisions, setRevisions] = useState<Readonly<Record<string, number>>>({});
  // The prompt each row was last known to show.
  const knownRef = useRef(new Map<string, string>());

  const hide = useCallback((key: string) => {
    setHidden((prev) => (prev.has(key) ? prev : new Set(prev).add(key)));
  }, []);
  const unhide = useCallback((key: string) => {
    setHidden((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  }, []);

  // A hidden row's waiting episode is over once its state moves on: the next one shows again.
  useEffect(() => {
    if (hidden.size === 0) return;
    for (const key of hidden) {
      const entry = entries.find((candidate) => candidate.key === key);
      if (entry?.state === "waiting") continue;
      knownRef.current.delete(key);
      unhide(key);
    }
  }, [entries, hidden, unhide]);

  const push = useCallback(
    (change: RemoteInboxPromptChange) => {
      // The push comes from the connected host: it names only that host's readable rows.
      const keys = entriesRef.current
        .filter((entry) => entry.readable && entry.sessionId === change.sessionId)
        .map((entry) => entry.key);
      for (const key of keys) {
        const known = knownRef.current.get(key);
        if (change.promptId === null) {
          // Only a prompt this Inbox saw can be answered elsewhere; a pane no reader knows keeps its row.
          if (known !== undefined) {
            knownRef.current.delete(key);
            hide(key);
          }
        } else if (change.promptId !== known) {
          knownRef.current.delete(key);
          unhide(key);
        }
      }
      setRevisions((prev) => {
        const next: Record<string, number> = { ...prev };
        for (const key of keys) next[key] = (next[key] ?? 0) + 1;
        return next;
      });
    },
    [hide, unhide],
  );

  const refreshAll = useCallback(() => {
    setRevisions((prev) => {
      const next: Record<string, number> = { ...prev };
      for (const entry of entriesRef.current) next[entry.key] = (next[entry.key] ?? 0) + 1;
      return next;
    });
  }, []);

  const rows = useMemo(
    () =>
      // Every waiting session is listed, the one on screen included: the Inbox is its own view.
      sortAttentionRows(
        entries.flatMap((entry): AttentionRow[] =>
          hidden.has(entry.key)
            ? []
            : [
                {
                  id: entry.key,
                  revision: revisions[entry.key] ?? 0,
                  workspaceId: entry.workspaceId,
                  sessionId: entry.sessionId,
                  state: entry.state === "waiting" ? "needs-you" : "done",
                  who: entry.who,
                  ...(entry.location ? { location: entry.location } : {}),
                  ...(entry.text ? { text: entry.text } : {}),
                  ...(entry.at !== undefined ? { at: entry.at } : {}),
                  ...(entry.supervisorLabel ? { supervisorLabel: entry.supervisorLabel } : {}),
                },
              ],
        ),
      ),
    [entries, hidden, revisions],
  );

  const prompts = useMemo<AttentionPrompts>(() => {
    const entryOf = (row: AttentionRow) => entriesRef.current.find((entry) => entry.key === row.id);
    return {
      read: async (row) => {
        const registryId = nativeRegistryId(entryOf(row));
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
          // The card this Inbox saw is gone: it was answered somewhere. A waiting pane whose card
          // this Inbox never saw keeps its row even on a null read: no reader may know its prompt
          // (an unknown menu, a bell), and hiding it would lose the only sign it needs the user.
          if (read.prompt === null && knownRef.current.delete(row.id)) hide(row.id);
          return null;
        }
        knownRef.current.set(row.id, read.prompt.id);
        return agentPromptFromReference(read.prompt, read.screenRevision);
      },
      answer: async (row, prompt, answer) => {
        const registryId = nativeRegistryId(entryOf(row));
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
      onAnswered: (row) => hide(row.id),
    };
  }, [hide]);

  let needsYou = 0;
  for (const row of rows) if (row.state === "needs-you") needsYou += 1;

  return { rows, needsYou, prompts, push, refreshAll };
}
