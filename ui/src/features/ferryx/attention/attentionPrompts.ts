import {
  answerAgentPrompt,
  getAgentPrompt,
  isStructuredIpcError,
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
};

/** The desktop's prompts, read from and answered on the daemon session that backs each row's pane. */
export function desktopAttentionPrompts(
  backendSessionOf: (row: AttentionRow) => string | null,
  onAnswered: (row: AttentionRow) => void,
): AttentionPrompts {
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
  };
}
