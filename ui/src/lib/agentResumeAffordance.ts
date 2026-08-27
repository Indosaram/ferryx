import { buildResumeArgv } from "./agentResume";
import type { TerminalSession } from "./types";

export type ResumableAgentPane = {
  sessionId: string;
  agentType: string;
  agentSessionId: string;
  cwd: string;
  argv: string[];
};

export function resumableAgentPane(session: TerminalSession): ResumableAgentPane | null {
  if (session.lifecycle !== "exited") {
    return null;
  }

  if (session.backendSessionId !== null) {
    return null;
  }

  if (typeof session.agentType !== "string" || session.agentType.trim() === "") {
    return null;
  }

  if (typeof session.agentSessionId !== "string" || session.agentSessionId.trim() === "") {
    return null;
  }

  const argv = buildResumeArgv({
    agentType: session.agentType,
    sessionId: session.agentSessionId,
  });

  if (!argv) {
    return null;
  }

  return {
    sessionId: session.id,
    agentType: session.agentType,
    agentSessionId: session.agentSessionId,
    cwd: session.cwd,
    argv,
  };
}

export function collectResumableAgentPanes(
  sessions: Readonly<Record<string, TerminalSession>>,
): ResumableAgentPane[] {
  const result: ResumableAgentPane[] = [];
  for (const session of Object.values(sessions)) {
    const pane = resumableAgentPane(session);
    if (pane !== null) {
      result.push(pane);
    }
  }
  return result;
}
