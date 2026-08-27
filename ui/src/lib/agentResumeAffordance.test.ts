import { describe, expect, it } from "vitest";

import type { TerminalSession } from "./types";
import {
  collectResumableAgentPanes,
  resumableAgentPane,
  type ResumableAgentPane,
} from "./agentResumeAffordance";

function createSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "session-1",
    cwd: "/repo/workspace",
    worktreePath: "/repo/workspace",
    workspaceId: "ws-1",
    worktree: null,
    backendSessionId: null,
    lifecycle: "exited",
    agentType: "claude",
    agentSessionId: "claude-uuid-123",
    ...overrides,
  };
}

describe("agentResumeAffordance", () => {
  describe("resumableAgentPane", () => {
    it("(a) yields ResumableAgentPane with exact argv for an exited claude pane with an agentSessionId", () => {
      const session = createSession({
        id: "pane-term-1",
        cwd: "/repo/workspace/sub",
        lifecycle: "exited",
        backendSessionId: null,
        agentType: "claude",
        agentSessionId: "claude-session-abc",
      });

      const result = resumableAgentPane(session);

      expect(result).toEqual({
        sessionId: "pane-term-1",
        agentType: "claude",
        agentSessionId: "claude-session-abc",
        cwd: "/repo/workspace/sub",
        argv: ["claude", "--resume", "claude-session-abc"],
      } satisfies ResumableAgentPane);
    });

    it("(b) yields null for a pane that is still live (lifecycle 'working' with backendSessionId set)", () => {
      const session = createSession({
        id: "pane-live-1",
        lifecycle: "working",
        backendSessionId: "backend-pty-999",
        agentType: "claude",
        agentSessionId: "claude-session-abc",
      });

      expect(resumableAgentPane(session)).toBeNull();
    });

    it("(b2) yields null when lifecycle is exited but backendSessionId is unexpectedly present", () => {
      const session = createSession({
        id: "pane-mismatch",
        lifecycle: "exited",
        backendSessionId: "still-alive-backend-id",
        agentType: "claude",
        agentSessionId: "claude-session-abc",
      });

      expect(resumableAgentPane(session)).toBeNull();
    });

    it("(b3) yields null when backendSessionId is null but lifecycle is not 'exited'", () => {
      const sessionRunning = createSession({
        lifecycle: "running",
        backendSessionId: null,
      });
      const sessionStarting = createSession({
        lifecycle: "starting",
        backendSessionId: null,
      });
      const sessionWaiting = createSession({
        lifecycle: "waiting",
        backendSessionId: null,
      });
      const sessionFailed = createSession({
        lifecycle: "failed",
        backendSessionId: null,
      });

      expect(resumableAgentPane(sessionRunning)).toBeNull();
      expect(resumableAgentPane(sessionStarting)).toBeNull();
      expect(resumableAgentPane(sessionWaiting)).toBeNull();
      expect(resumableAgentPane(sessionFailed)).toBeNull();
    });

    it("(c) yields null for an exited pane with no agentType (plain shell)", () => {
      const sessionUndefined = createSession({
        agentType: undefined,
        agentSessionId: "some-id",
      });
      const sessionNull = createSession({
        agentType: null,
        agentSessionId: "some-id",
      });
      const sessionEmpty = createSession({
        agentType: "",
        agentSessionId: "some-id",
      });

      expect(resumableAgentPane(sessionUndefined)).toBeNull();
      expect(resumableAgentPane(sessionNull)).toBeNull();
      expect(resumableAgentPane(sessionEmpty)).toBeNull();
    });

    it("(d) yields null for an exited pane with agentType 'grok' (agent has no resume support)", () => {
      const session = createSession({
        agentType: "grok",
        agentSessionId: "grok-sess-1",
      });

      expect(resumableAgentPane(session)).toBeNull();
    });

    it("(e) yields null for an exited pane with an empty-string or whitespace agentSessionId", () => {
      const sessionEmpty = createSession({
        agentType: "claude",
        agentSessionId: "",
      });
      const sessionWhitespace = createSession({
        agentType: "claude",
        agentSessionId: "   \t \n  ",
      });
      const sessionNull = createSession({
        agentType: "claude",
        agentSessionId: null,
      });
      const sessionUndefined = createSession({
        agentType: "claude",
        agentSessionId: undefined,
      });

      expect(resumableAgentPane(sessionEmpty)).toBeNull();
      expect(resumableAgentPane(sessionWhitespace)).toBeNull();
      expect(resumableAgentPane(sessionNull)).toBeNull();
      expect(resumableAgentPane(sessionUndefined)).toBeNull();
    });

    it("supports other resumable agents such as codex and omo", () => {
      const codexSession = createSession({
        id: "codex-1",
        cwd: "/repo/codex-work",
        agentType: "codex",
        agentSessionId: "sess-codex-99",
      });
      expect(resumableAgentPane(codexSession)).toEqual({
        sessionId: "codex-1",
        agentType: "codex",
        agentSessionId: "sess-codex-99",
        cwd: "/repo/codex-work",
        argv: ["codex", "resume", "sess-codex-99"],
      });

      const omoSession = createSession({
        id: "omo-1",
        cwd: "/repo/omo-work",
        agentType: "omo",
        agentSessionId: "omo-sess-42",
      });
      expect(resumableAgentPane(omoSession)).toEqual({
        sessionId: "omo-1",
        agentType: "omo",
        agentSessionId: "omo-sess-42",
        cwd: "/repo/omo-work",
        argv: ["omo", "--session", "omo-sess-42"],
      });
    });
  });

  describe("collectResumableAgentPanes", () => {
    it("(f) returns only resumable panes from a mixed record in insertion order", () => {
      const sessions: Record<string, TerminalSession> = {
        "sess-live": createSession({
          id: "sess-live",
          lifecycle: "working",
          backendSessionId: "pty-1",
          agentType: "claude",
          agentSessionId: "id-1",
        }),
        "sess-resumable-1": createSession({
          id: "sess-resumable-1",
          cwd: "/repo/a",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: "claude",
          agentSessionId: "claude-sess-1",
        }),
        "sess-plain-shell": createSession({
          id: "sess-plain-shell",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: null,
          agentSessionId: null,
        }),
        "sess-grok": createSession({
          id: "sess-grok",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: "grok",
          agentSessionId: "grok-1",
        }),
        "sess-resumable-2": createSession({
          id: "sess-resumable-2",
          cwd: "/repo/b",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: "omo",
          agentSessionId: "omo-sess-2",
        }),
        "sess-empty-id": createSession({
          id: "sess-empty-id",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: "claude",
          agentSessionId: "",
        }),
        "sess-resumable-3": createSession({
          id: "sess-resumable-3",
          cwd: "/repo/c",
          lifecycle: "exited",
          backendSessionId: null,
          agentType: "copilot",
          agentSessionId: "copilot-sess-3",
        }),
      };

      const results = collectResumableAgentPanes(sessions);

      expect(results).toEqual([
        {
          sessionId: "sess-resumable-1",
          agentType: "claude",
          agentSessionId: "claude-sess-1",
          cwd: "/repo/a",
          argv: ["claude", "--resume", "claude-sess-1"],
        },
        {
          sessionId: "sess-resumable-2",
          agentType: "omo",
          agentSessionId: "omo-sess-2",
          cwd: "/repo/b",
          argv: ["omo", "--session", "omo-sess-2"],
        },
        {
          sessionId: "sess-resumable-3",
          agentType: "copilot",
          agentSessionId: "copilot-sess-3",
          cwd: "/repo/c",
          argv: ["copilot", "--resume", "copilot-sess-3"],
        },
      ]);
    });

    it("returns an empty array when given an empty record or no resumable sessions", () => {
      expect(collectResumableAgentPanes({})).toEqual([]);

      const nonResumableSessions: Record<string, TerminalSession> = {
        "sess-1": createSession({
          id: "sess-1",
          lifecycle: "working",
          backendSessionId: "pty-1",
        }),
        "sess-2": createSession({
          id: "sess-2",
          agentType: null,
        }),
      };
      expect(collectResumableAgentPanes(nonResumableSessions)).toEqual([]);
    });
  });
});
