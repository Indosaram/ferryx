import {
  answerAgentPrompt,
  getAgentPrompt,
  isStructuredIpcError,
  onAgentPromptChanged,
  type AgentPrompt,
  type AgentPromptAnswer,
} from "../../../lib/tauri";
import type { AttentionRow } from "./attentionModel";

export type AttentionPromptOutcome = "answered" | "stale";

/** How the Inbox reads and answers the prompt a needs-you row's pane is asking. */
export type AttentionPrompts = {
  /** Null when the pane holds no prompt the daemon can read: the row keeps open/dismiss only. */
  read: (row: AttentionRow) => Promise<AgentPrompt | null>;
  /** "stale": the screen moved since the prompt was read, and nothing was typed. */
  answer: (row: AttentionRow, prompt: AgentPrompt, answer: AgentPromptAnswer) => Promise<AttentionPromptOutcome>;
  /** The answer went out: the row is no longer waiting on the user. */
  onAnswered: (row: AttentionRow) => void;
  /** Calls `listener` whenever the daemon says the prompt behind `row`'s pane changed. */
  subscribe?: (row: AttentionRow, listener: () => void) => () => void;
};

/** The desktop's prompts, read from and answered on the daemon session that backs each row's pane. */
export function desktopAttentionPrompts(
  backendSessionOf: (row: AttentionRow) => string | null,
  onAnswered: (row: AttentionRow) => void,
): AttentionPrompts {
  const pushes = promptPushes();
  return {
    read: async (row) => {
      const sessionId = backendSessionOf(row);
      if (!sessionId) return null;
      try {
        return await getAgentPrompt(sessionId);
      } catch (error) {
        // An unreadable screen or an older daemon is not a prompt; the row still opens the pane.
        console.debug("[inbox] agent prompt unreadable", sessionId, error);
        return null;
      }
    },
    answer: async (row, prompt, answer) => {
      const sessionId = backendSessionOf(row);
      if (!sessionId) return "stale";
      try {
        await answerAgentPrompt({ sessionId, promptId: prompt.promptId, screenRevision: prompt.screenRevision, answer });
        return "answered";
      } catch (error) {
        if (isStructuredIpcError(error) && error.code === "REQUEST_CONFLICT") return "stale";
        throw error;
      }
    },
    onAnswered,
    subscribe: (row, listener) => {
      const sessionId = backendSessionOf(row);
      if (!sessionId) return () => undefined;
      return pushes.add(sessionId, listener);
    },
  };
}

/**
 * One `agentPromptChanged` listener for every row, dispatched by daemon session id. It is attached
 * while any row listens and detached when the last one leaves.
 */
function promptPushes() {
  const listeners = new Map<string, Set<() => void>>();
  let unlisten: (() => void) | null = null;
  let attaching = false;
  const dispatch = (sessionId: string) => {
    for (const listener of [...(listeners.get(sessionId) ?? [])]) listener();
  };
  const attach = () => {
    if (unlisten || attaching) return;
    attaching = true;
    void onAgentPromptChanged(dispatch).then(
      (stop) => {
        attaching = false;
        if (listeners.size > 0) unlisten = stop;
        else stop();
      },
      () => {
        attaching = false;
      },
    );
  };
  return {
    add(sessionId: string, listener: () => void): () => void {
      const set = listeners.get(sessionId) ?? new Set();
      set.add(listener);
      listeners.set(sessionId, set);
      attach();
      return () => {
        set.delete(listener);
        if (set.size === 0 && listeners.get(sessionId) === set) listeners.delete(sessionId);
        if (listeners.size === 0 && unlisten) {
          unlisten();
          unlisten = null;
        }
      };
    },
  };
}
