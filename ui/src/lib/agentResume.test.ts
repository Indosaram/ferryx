import { describe, expect, it } from "vitest";

import {
  AGENT_RESUME_SPECS,
  agentResumeCapability,
  buildResumeArgv,
  canResumeAgent,
  type AgentSessionRef,
} from "./agentResume";

describe("agentResume adapter module", () => {
  describe("AGENT_RESUME_SPECS table", () => {
    it("exports a resume spec table mapping supported agent types", () => {
      expect(AGENT_RESUME_SPECS).toBeDefined();
      expect(typeof AGENT_RESUME_SPECS).toBe("object");
      expect(AGENT_RESUME_SPECS.claude).toBeDefined();
      expect(AGENT_RESUME_SPECS.codex).toBeDefined();
      expect(AGENT_RESUME_SPECS.copilot).toBeDefined();
      expect(AGENT_RESUME_SPECS.kimi).toBeDefined();
      expect(AGENT_RESUME_SPECS.opencode).toBeDefined();
      expect(AGENT_RESUME_SPECS.gemini).toBeDefined();
      expect(AGENT_RESUME_SPECS.cursor).toBeDefined();
      expect(AGENT_RESUME_SPECS.omo).toBeDefined();
    });
  });

  describe("agentResumeCapability", () => {
    it("returns 'uuid' for agents resuming via UUID / session identifier", () => {
      expect(agentResumeCapability("claude")).toBe("uuid");
      expect(agentResumeCapability("codex")).toBe("uuid");
      expect(agentResumeCapability("copilot")).toBe("uuid");
      expect(agentResumeCapability("kimi")).toBe("uuid");
      expect(agentResumeCapability("opencode")).toBe("uuid");
      expect(agentResumeCapability("omo")).toBe("uuid");
    });

    it("returns 'positional-index' for gemini", () => {
      expect(agentResumeCapability("gemini")).toBe("positional-index");
    });

    it("returns 'chat-id' for cursor", () => {
      expect(agentResumeCapability("cursor")).toBe("chat-id");
    });

    it("returns 'none' for grok and unknown/unsupported agents", () => {
      expect(agentResumeCapability("grok")).toBe("none");
      expect(agentResumeCapability("antigravity")).toBe("none");
      expect(agentResumeCapability("pi")).toBe("none");
      expect(agentResumeCapability("cline")).toBe("none");
      expect(agentResumeCapability("notanagent")).toBe("none");
      expect(agentResumeCapability("")).toBe("none");
    });

    it("normalizes agentType by trimming and lowercasing", () => {
      expect(agentResumeCapability("  ClAuDe  ")).toBe("uuid");
      expect(agentResumeCapability("  GEMINI ")).toBe("positional-index");
      expect(agentResumeCapability("Cursor")).toBe("chat-id");
      expect(agentResumeCapability("  GROK ")).toBe("none");
    });
  });

  describe("canResumeAgent", () => {
    it("returns true for supported resumable agents", () => {
      expect(canResumeAgent("claude")).toBe(true);
      expect(canResumeAgent("codex")).toBe(true);
      expect(canResumeAgent("copilot")).toBe(true);
      expect(canResumeAgent("kimi")).toBe(true);
      expect(canResumeAgent("opencode")).toBe(true);
      expect(canResumeAgent("gemini")).toBe(true);
      expect(canResumeAgent("cursor")).toBe(true);
      expect(canResumeAgent("omo")).toBe(true);
    });

    it("returns false for non-resumable, unknown, or empty agent types", () => {
      expect(canResumeAgent("grok")).toBe(false);
      expect(canResumeAgent("antigravity")).toBe(false);
      expect(canResumeAgent("pi")).toBe(false);
      expect(canResumeAgent("cline")).toBe(false);
      expect(canResumeAgent("notanagent")).toBe(false);
      expect(canResumeAgent("")).toBe(false);
    });

    it("normalizes agentType case and whitespace", () => {
      expect(canResumeAgent("  Claude ")).toBe(true);
      expect(canResumeAgent("CODEX")).toBe(true);
      expect(canResumeAgent("  NOTANAGENT  ")).toBe(false);
    });
  });

  describe("buildResumeArgv", () => {
    describe("verified empirical matrix agents", () => {
      it("produces exact argv for claude: claude --resume <id>", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "c18f-uuid-456" };
        expect(buildResumeArgv(ref)).toEqual(["claude", "--resume", "c18f-uuid-456"]);
      });

      it("produces exact argv for codex: codex resume <SESSION_ID>", () => {
        const ref: AgentSessionRef = { agentType: "codex", sessionId: "session-name-or-uuid" };
        expect(buildResumeArgv(ref)).toEqual(["codex", "resume", "session-name-or-uuid"]);
      });

      it("produces exact argv for copilot: copilot --resume <sessionId>", () => {
        const ref: AgentSessionRef = { agentType: "copilot", sessionId: "copilot-sess-789" };
        expect(buildResumeArgv(ref)).toEqual(["copilot", "--resume", "copilot-sess-789"]);
      });

      it("produces exact argv for kimi: kimi --session <id>", () => {
        const ref: AgentSessionRef = { agentType: "kimi", sessionId: "kimi-sess-101" };
        expect(buildResumeArgv(ref)).toEqual(["kimi", "--session", "kimi-sess-101"]);
      });

      it("produces exact argv for opencode: opencode -s <id> (TUI, not the `run` one-shot)", () => {
        const ref: AgentSessionRef = { agentType: "opencode", sessionId: "opencode-id-202" };
        expect(buildResumeArgv(ref)).toEqual(["opencode", "-s", "opencode-id-202"]);
      });

      it("produces exact argv for cursor: cursor-agent --resume <chatId>", () => {
        const ref: AgentSessionRef = { agentType: "cursor", sessionId: "chatId-303" };
        expect(buildResumeArgv(ref)).toEqual(["cursor-agent", "--resume", "chatId-303"]);
      });

      it("produces exact argv for omo: omo --session <id> (--resume takes no value)", () => {
        const ref: AgentSessionRef = { agentType: "omo", sessionId: "omo-sess-404" };
        expect(buildResumeArgv(ref)).toEqual(["omo", "--session", "omo-sess-404"]);
      });

      it("never resumes omo through the id-minting --session-id flag", () => {
        const ref: AgentSessionRef = { agentType: "omo", sessionId: "omo-sess-404" };
        expect(buildResumeArgv(ref)).not.toContain("--session-id");
      });

      it("produces exact argv for gemini with 'latest'", () => {
        const ref: AgentSessionRef = { agentType: "gemini", sessionId: "latest" };
        expect(buildResumeArgv(ref)).toEqual(["gemini", "-r", "latest"]);
      });

      it("produces exact argv for gemini with an index like '5'", () => {
        const ref: AgentSessionRef = { agentType: "gemini", sessionId: "5" };
        expect(buildResumeArgv(ref)).toEqual(["gemini", "-r", "5"]);
      });
    });

    describe("unsupported, non-resumable, and invalid inputs", () => {
      it("returns null for grok (no resume support)", () => {
        const ref: AgentSessionRef = { agentType: "grok", sessionId: "session-123" };
        expect(buildResumeArgv(ref)).toBeNull();
      });

      it("returns null for unknown agent type ('notanagent')", () => {
        const ref: AgentSessionRef = { agentType: "notanagent", sessionId: "session-123" };
        expect(buildResumeArgv(ref)).toBeNull();
      });

      it("returns null for empty string sessionId", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "" };
        expect(buildResumeArgv(ref)).toBeNull();
      });

      it("returns null for whitespace-only sessionId", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "   \t  \n " };
        expect(buildResumeArgv(ref)).toBeNull();
      });

      it("returns null when agentType is empty or whitespace", () => {
        const ref: AgentSessionRef = { agentType: "  ", sessionId: "session-123" };
        expect(buildResumeArgv(ref)).toBeNull();
      });
    });

    describe("normalization and baseCommand overrides", () => {
      it("normalizes mixed-case agent type ('ClAuDe')", () => {
        const ref: AgentSessionRef = { agentType: "ClAuDe", sessionId: "abc-123" };
        expect(buildResumeArgv(ref)).toEqual(["claude", "--resume", "abc-123"]);
      });

      it("normalizes agent type with surrounding whitespace", () => {
        const ref: AgentSessionRef = { agentType: "  codex  ", sessionId: "xyz-789" };
        expect(buildResumeArgv(ref)).toEqual(["codex", "resume", "xyz-789"]);
      });

      it("overrides argv[0] when baseCommand is provided", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "sess-1" };
        const override = "/opt/homebrew/bin/claude-custom";
        expect(buildResumeArgv(ref, override)).toEqual(["/opt/homebrew/bin/claude-custom", "--resume", "sess-1"]);
      });

      it("overrides argv[0] for opencode while keeping its resume flags", () => {
        const ref: AgentSessionRef = { agentType: "opencode", sessionId: "sess-2" };
        const override = "opencode-beta";
        expect(buildResumeArgv(ref, override)).toEqual(["opencode-beta", "-s", "sess-2"]);
      });

      it("overrides argv[0] for cursor with custom path", () => {
        const ref: AgentSessionRef = { agentType: "cursor", sessionId: "chat-456" };
        const override = "/usr/local/bin/cursor-agent";
        expect(buildResumeArgv(ref, override)).toEqual(["/usr/local/bin/cursor-agent", "--resume", "chat-456"]);
      });

      it("falls back to default command if baseCommand is whitespace only", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "sess-1" };
        expect(buildResumeArgv(ref, "   ")).toEqual(["claude", "--resume", "sess-1"]);
      });
    });
  });
});
