import { describe, expect, it } from "vitest";

import {
  agentProviderSessionsEqual,
  agentResumeCapability,
  buildResumeArgv,
  canResumeAgent,
  extractAgentProviderSession,
  getAgentResumeArgv,
  hasUnsafeProviderSessionIdChars,
  isResumableTuiAgent,
  normalizeAgentProviderSession,
  normalizeSessionId,
  PROVIDER_SESSION_ID_MAX_LENGTH,
  RESUMABLE_TUI_AGENTS,
  type AgentSessionRef,
} from "./agentResume";
import type { AgentProviderSession } from "./types";

describe("agentResume adapter module", () => {
  describe("constants and validation primitives", () => {
    it("exports max ID length of 512 and resumable TUI agents list", () => {
      expect(PROVIDER_SESSION_ID_MAX_LENGTH).toBe(512);
      expect(RESUMABLE_TUI_AGENTS).toContain("claude");
      expect(RESUMABLE_TUI_AGENTS).toContain("codex");
      expect(RESUMABLE_TUI_AGENTS).toContain("antigravity");
      expect(RESUMABLE_TUI_AGENTS).toContain("opencode");
      expect(RESUMABLE_TUI_AGENTS).toContain("pi");
      expect(RESUMABLE_TUI_AGENTS).toContain("prime-agent");
      expect(RESUMABLE_TUI_AGENTS).toContain("mimo-code");
      expect(RESUMABLE_TUI_AGENTS).toContain("droid");
      expect(RESUMABLE_TUI_AGENTS).toContain("grok");
      expect(RESUMABLE_TUI_AGENTS).toContain("devin");
      expect(RESUMABLE_TUI_AGENTS).toContain("omp");
      expect(RESUMABLE_TUI_AGENTS).toContain("omo");
      expect(RESUMABLE_TUI_AGENTS).toContain("kimi");
      expect(RESUMABLE_TUI_AGENTS).toContain("gjc");
      expect(RESUMABLE_TUI_AGENTS).toContain("copilot");
      expect(RESUMABLE_TUI_AGENTS).toContain("cursor");
      expect(RESUMABLE_TUI_AGENTS).toContain("cursor-agent");
    });

    it("hasUnsafeProviderSessionIdChars detects ASCII control chars and DEL", () => {
      expect(hasUnsafeProviderSessionIdChars("normal-id-123")).toBe(false);
      expect(hasUnsafeProviderSessionIdChars("id\x00null")).toBe(true);
      expect(hasUnsafeProviderSessionIdChars("id\x1funit-sep")).toBe(true);
      expect(hasUnsafeProviderSessionIdChars("id\x7fdel")).toBe(true);
      expect(hasUnsafeProviderSessionIdChars("id\nnewline")).toBe(true);
      expect(hasUnsafeProviderSessionIdChars("id\rcarriage")).toBe(true);
      expect(hasUnsafeProviderSessionIdChars("id\ttab")).toBe(true);
    });

    it("normalizeSessionId validates and trims valid IDs", () => {
      expect(normalizeSessionId("  valid-session-id-123  ")).toBe("valid-session-id-123");
      const max512 = "a".repeat(512);
      expect(normalizeSessionId(max512)).toBe(max512);
    });

    it("normalizeSessionId rejects empty, whitespace, non-string, leading dash, control bytes, >512 chars, and 'latest'", () => {
      // Given: invalid inputs
      // When / Then:
      expect(normalizeSessionId("")).toBeNull();
      expect(normalizeSessionId("   \t \n  ")).toBeNull();
      expect(normalizeSessionId(null)).toBeNull();
      expect(normalizeSessionId(undefined)).toBeNull();
      expect(normalizeSessionId(12345)).toBeNull();
      // Leading dash / option injection
      expect(normalizeSessionId("-leading-dash")).toBeNull();
      expect(normalizeSessionId("--resume")).toBeNull();
      expect(normalizeSessionId(" -trimmed-leading-dash ")).toBeNull();
      // Control byte
      expect(normalizeSessionId("bad\x00id")).toBeNull();
      expect(normalizeSessionId("bad\x1bid")).toBeNull();
      // 513 bytes (>512)
      expect(normalizeSessionId("a".repeat(513))).toBeNull();
      // 'latest' keyword must NOT be accepted as a session ID
      expect(normalizeSessionId("latest")).toBeNull();
      expect(normalizeSessionId("LATEST")).toBeNull();
      expect(normalizeSessionId("  latest  ")).toBeNull();
    });

    it("normalizeAgentProviderSession validates and normalizes session records", () => {
      expect(normalizeAgentProviderSession({ key: "session_id", id: "  c18f-uuid  " })).toEqual({
        key: "session_id",
        id: "c18f-uuid",
      });

      expect(
        normalizeAgentProviderSession({
          key: "conversation_id",
          id: "conv-101",
          transcriptPath: " /var/log/transcript.jsonl ",
        }),
      ).toEqual({
        key: "conversation_id",
        id: "conv-101",
        transcriptPath: "/var/log/transcript.jsonl",
      });

      // Invalid key
      expect(normalizeAgentProviderSession({ key: "invalid_key", id: "valid-id" })).toBeNull();
      // Leading dash in id
      expect(normalizeAgentProviderSession({ key: "session_id", id: "-malicious" })).toBeNull();
      // Control byte in id
      expect(normalizeAgentProviderSession({ key: "session_id", id: "bad\x00id" })).toBeNull();
      // >512 length
      expect(normalizeAgentProviderSession({ key: "session_id", id: "x".repeat(513) })).toBeNull();
      // 'latest'
      expect(normalizeAgentProviderSession({ key: "session_id", id: "latest" })).toBeNull();
      // Null / non-object
      expect(normalizeAgentProviderSession(null)).toBeNull();
      expect(normalizeAgentProviderSession("string")).toBeNull();
    });

    it("bounds transcript paths and accepts only absolute path forms", () => {
      const base = { key: "session_id", id: "provider-id" } as const;
      expect(normalizeAgentProviderSession({ ...base, transcriptPath: "relative/session.json" })).toEqual(base);
      expect(normalizeAgentProviderSession({ ...base, transcriptPath: "--session" })).toEqual(base);
      expect(normalizeAgentProviderSession({ ...base, transcriptPath: `/${"x".repeat(4096)}` })).toEqual(base);
      expect(normalizeAgentProviderSession({ ...base, transcriptPath: "/tmp/session.json" })).toEqual({ ...base, transcriptPath: "/tmp/session.json" });
      expect(normalizeAgentProviderSession({ ...base, transcriptPath: "C:\\sessions\\run.json" })).toEqual({ ...base, transcriptPath: "C:\\sessions\\run.json" });
    });

    it("agentProviderSessionsEqual compares session references accurately", () => {
      const s1: AgentProviderSession = { key: "session_id", id: "id-1" };
      const s2: AgentProviderSession = { key: "session_id", id: "id-1" };
      const s3: AgentProviderSession = { key: "session_id", id: "id-2" };
      const s4: AgentProviderSession = { key: "conversation_id", id: "id-1" };

      expect(agentProviderSessionsEqual("claude", s1, s2)).toBe(true);
      expect(agentProviderSessionsEqual("claude", s1, s3)).toBe(false);
      expect(agentProviderSessionsEqual("claude", s1, s4)).toBe(false);
      expect(agentProviderSessionsEqual("claude", undefined, undefined)).toBe(true);
      expect(agentProviderSessionsEqual("claude", s1, undefined)).toBe(false);

      // Pi / Prime compares transcriptPath
      const pi1: AgentProviderSession = { key: "session_id", id: "id-1", transcriptPath: "/a.json" };
      const pi2: AgentProviderSession = { key: "session_id", id: "id-1", transcriptPath: "/a.json" };
      const pi3: AgentProviderSession = { key: "session_id", id: "id-1", transcriptPath: "/b.json" };
      expect(agentProviderSessionsEqual("pi", pi1, pi2)).toBe(true);
      expect(agentProviderSessionsEqual("pi", pi1, pi3)).toBe(false);
    });
  });

  describe("extractAgentProviderSession", () => {
    it("extracts session_id and optional transcript_path for claude and codex", () => {
      expect(extractAgentProviderSession("claude", { session_id: "claude-uuid-1" })).toEqual({
        key: "session_id",
        id: "claude-uuid-1",
      });
      expect(
        extractAgentProviderSession("claude", {
          session_id: "claude-uuid-1",
          transcript_path: "/path/to/transcript.jsonl",
        }),
      ).toEqual({
        key: "session_id",
        id: "claude-uuid-1",
        transcriptPath: "/path/to/transcript.jsonl",
      });
      expect(extractAgentProviderSession("codex", { session_id: "codex-uuid-2" })).toEqual({
        key: "session_id",
        id: "codex-uuid-2",
      });
    });

    it("extracts session_id for opencode, mimo-code, antigravity, droid, grok, devin, omp, omo, kimi, copilot, cursor, cursor-agent", () => {
      expect(extractAgentProviderSession("codex", { session_id: "s-1" })).toEqual({
        key: "session_id",
        id: "s-1",
      });
      expect(extractAgentProviderSession("opencode", { session_id: "s-1" })).toEqual({
        key: "session_id",
        id: "s-1",
      });
      expect(extractAgentProviderSession("antigravity", { session_id: "agy-sess-1" })).toEqual({
        key: "session_id",
        id: "agy-sess-1",
      });
      expect(extractAgentProviderSession("opencode", { sessionID: "opencode-sess-1" })).toEqual({
        key: "session_id",
        id: "opencode-sess-1",
      });
      expect(extractAgentProviderSession("mimo-code", { sessionID: "mimo-sess-1" })).toEqual({
        key: "session_id",
        id: "mimo-sess-1",
      });
      expect(extractAgentProviderSession("copilot", { session_id: "copilot-sess-1" })).toEqual({
        key: "session_id",
        id: "copilot-sess-1",
      });
      expect(extractAgentProviderSession("cursor", { session_id: "cursor-sess-1" })).toEqual({
        key: "session_id",
        id: "cursor-sess-1",
      });
      expect(extractAgentProviderSession("cursor-agent", { session_id: "cursor-agent-sess-1" })).toEqual({
        key: "session_id",
        id: "cursor-agent-sess-1",
      });
      expect(extractAgentProviderSession("grok", { sessionId: "grok-sess-1" })).toEqual({
        key: "session_id",
        id: "grok-sess-1",
      });
      expect(extractAgentProviderSession("devin", { session_id: "devin-sess-1" })).toEqual({
        key: "session_id",
        id: "devin-sess-1",
      });
      expect(extractAgentProviderSession("omp", { session_id: "omp-sess-1" })).toEqual({
        key: "session_id",
        id: "omp-sess-1",
      });
      expect(extractAgentProviderSession("omo", { session_id: "omo-sess-1" })).toEqual({
        key: "session_id",
        id: "omo-sess-1",
      });
      expect(extractAgentProviderSession("kimi", { session_id: "kimi-sess-1" })).toEqual({
        key: "session_id",
        id: "kimi-sess-1",
      });
    });

    it("requires transcript path (session_file) for pi and prime-agent", () => {
      expect(extractAgentProviderSession("pi", { session_id: "pi-sess-1" })).toBeNull();
      expect(
        extractAgentProviderSession("pi", {
          session_id: "pi-sess-1",
          session_file: "/home/user/.pi/sessions/pi-sess-1.json",
        }),
      ).toEqual({
        key: "session_id",
        id: "pi-sess-1",
        transcriptPath: "/home/user/.pi/sessions/pi-sess-1.json",
      });
      expect(extractAgentProviderSession("prime-agent", { session_id: "prime-1" })).toBeNull();
      expect(
        extractAgentProviderSession("prime-agent", {
          session_id: "prime-1",
          session_file: "/home/user/.prime/session.json",
        }),
      ).toEqual({
        key: "session_id",
        id: "prime-1",
        transcriptPath: "/home/user/.prime/session.json",
      });
    });

    it("returns null for unsupported / unidentifiable agents (cline, windsurf, aider, unknown, etc.)", () => {
      expect(extractAgentProviderSession("cline", { session_id: "cline-1" })).toBeNull();
      expect(extractAgentProviderSession("windsurf", { session_id: "windsurf-1" })).toBeNull();
      expect(extractAgentProviderSession("aider", { session_id: "aider-1" })).toBeNull();
      expect(extractAgentProviderSession("unknown", { session_id: "unk-1" })).toBeNull();
    });
  });

  describe("agentResumeCapability and canResumeAgent", () => {
    it("identifies supported resumable agents", () => {
      expect(isResumableTuiAgent("claude")).toBe(true);
      expect(isResumableTuiAgent("codex")).toBe(true);
      expect(isResumableTuiAgent("antigravity")).toBe(true);
      expect(isResumableTuiAgent("opencode")).toBe(true);
      expect(isResumableTuiAgent("pi")).toBe(true);
      expect(isResumableTuiAgent("prime-agent")).toBe(true);
      expect(isResumableTuiAgent("mimo-code")).toBe(true);
      expect(isResumableTuiAgent("droid")).toBe(true);
      expect(isResumableTuiAgent("grok")).toBe(true);
      expect(isResumableTuiAgent("devin")).toBe(true);
      expect(isResumableTuiAgent("omp")).toBe(true);
      expect(isResumableTuiAgent("omo")).toBe(true);
      expect(isResumableTuiAgent("kimi")).toBe(true);
      expect(isResumableTuiAgent("copilot")).toBe(true);
      expect(isResumableTuiAgent("cursor")).toBe(true);
      expect(isResumableTuiAgent("cursor-agent")).toBe(true);

      expect(agentResumeCapability("claude")).toBe("uuid");
      expect(agentResumeCapability("omo")).toBe("uuid");
      expect(agentResumeCapability("copilot")).toBe("uuid");
      expect(agentResumeCapability("cursor")).toBe("uuid");
      expect(agentResumeCapability("unknown")).toBe("none");

      expect(canResumeAgent("claude")).toBe(true);
      expect(canResumeAgent("omo")).toBe(true);
      expect(canResumeAgent("copilot")).toBe(true);
      expect(canResumeAgent("cursor")).toBe(true);
      expect(canResumeAgent("cursor-agent")).toBe(true);
      expect(canResumeAgent("cline")).toBe(false);
      expect(canResumeAgent("windsurf")).toBe(false);
      expect(canResumeAgent("aider")).toBe(false);
      expect(canResumeAgent("notanagent")).toBe(false);
    });
  });

  describe("getAgentResumeArgv & buildResumeArgv", () => {
    describe("builds exact provider-native reconnect argv", () => {
      it("produces exact argv for claude: claude --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "c18f-uuid-456" };
        expect(getAgentResumeArgv("claude", providerSession)).toEqual(["claude", "--resume", "c18f-uuid-456"]);
        expect(buildResumeArgv({ agentType: "claude", providerSession })).toEqual([
          "claude",
          "--resume",
          "c18f-uuid-456",
        ]);
      });

      it("produces exact argv for codex: codex resume <SESSION_ID>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "session-name-or-uuid" };
        expect(getAgentResumeArgv("codex", providerSession)).toEqual(["codex", "resume", "session-name-or-uuid"]);
      });

      it("produces exact argv for antigravity: agy --conversation <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "conv-agy-789" };
        expect(getAgentResumeArgv("antigravity", providerSession)).toEqual(["agy", "--conversation", "conv-agy-789"]);
      });

      it("produces exact argv for opencode: opencode --session <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "opencode-id-202" };
        expect(getAgentResumeArgv("opencode", providerSession)).toEqual(["opencode", "--session", "opencode-id-202"]);
      });

      it("produces exact argv for pi: pi --session <transcriptPath>", () => {
        const providerSession: AgentProviderSession = {
          key: "session_id",
          id: "pi-id-1",
          transcriptPath: "/path/to/pi-session.json",
        };
        expect(getAgentResumeArgv("pi", providerSession)).toEqual(["pi", "--session", "/path/to/pi-session.json"]);
      });

      it("produces exact argv for prime-agent: prime-agent --resume <transcriptPath>", () => {
        const providerSession: AgentProviderSession = {
          key: "session_id",
          id: "prime-id-1",
          transcriptPath: "/path/to/prime-session.json",
        };
        expect(getAgentResumeArgv("prime-agent", providerSession)).toEqual([
          "prime-agent",
          "--resume",
          "/path/to/prime-session.json",
        ]);
      });

      it("produces exact argv for mimo-code: mimo --session <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "mimo-id-303" };
        expect(getAgentResumeArgv("mimo-code", providerSession)).toEqual(["mimo", "--session", "mimo-id-303"]);
      });

      it("produces exact argv for droid: droid --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "droid-id-404" };
        expect(getAgentResumeArgv("droid", providerSession)).toEqual(["droid", "--resume", "droid-id-404"]);
      });

      it("produces exact argv for grok: grok --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "grok-id-505" };
        expect(getAgentResumeArgv("grok", providerSession)).toEqual(["grok", "--resume", "grok-id-505"]);
      });

      it("produces exact argv for devin: devin --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "devin-id-606" };
        expect(getAgentResumeArgv("devin", providerSession)).toEqual(["devin", "--resume", "devin-id-606"]);
      });

      it("produces exact argv for omp: omp --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "omp-id-707" };
        expect(getAgentResumeArgv("omp", providerSession)).toEqual(["omp", "--resume", "omp-id-707"]);
      });

      it("produces exact argv for omo: omo --session <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "omo-sess-404" };
        expect(getAgentResumeArgv("omo", providerSession)).toEqual(["omo", "--session", "omo-sess-404"]);
        expect(buildResumeArgv({ agentType: "omo", providerSession })).toEqual(["omo", "--session", "omo-sess-404"]);
      });

      it("never resumes omo through the id-minting --session-id flag", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "omo-sess-404" };
        const argv = getAgentResumeArgv("omo", providerSession);
        expect(argv).toBeDefined();
        expect(argv).not.toContain("--session-id");
      });

      it("produces exact argv for kimi: kimi --session <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "kimi-sess-101" };
        expect(getAgentResumeArgv("kimi", providerSession)).toEqual(["kimi", "--session", "kimi-sess-101"]);
      });

      it("produces exact argv for copilot: copilot --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "copilot-sess-202" };
        expect(getAgentResumeArgv("copilot", providerSession)).toEqual(["copilot", "--resume", "copilot-sess-202"]);
        expect(buildResumeArgv({ agentType: "copilot", providerSession })).toEqual([
          "copilot",
          "--resume",
          "copilot-sess-202",
        ]);
      });

      it("produces exact argv for cursor: cursor-agent --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "cursor-sess-303" };
        expect(getAgentResumeArgv("cursor", providerSession)).toEqual(["cursor-agent", "--resume", "cursor-sess-303"]);
        expect(buildResumeArgv({ agentType: "cursor", providerSession })).toEqual([
          "cursor-agent",
          "--resume",
          "cursor-sess-303",
        ]);
      });

      it("produces exact argv for cursor-agent: cursor-agent --resume <id>", () => {
        const providerSession: AgentProviderSession = { key: "session_id", id: "cursor-agent-sess-404" };
        expect(getAgentResumeArgv("cursor-agent", providerSession)).toEqual([
          "cursor-agent",
          "--resume",
          "cursor-agent-sess-404",
        ]);
      });
    });

    describe("rejections: malformed IDs, wrong keys, missing transcripts, unsupported agents", () => {
      it("rejects leading dash in ID for all providers", () => {
        const badSession: AgentProviderSession = { key: "session_id", id: "-danger-flag" };
        expect(getAgentResumeArgv("claude", badSession)).toBeNull();
        expect(getAgentResumeArgv("codex", badSession)).toBeNull();
        expect(getAgentResumeArgv("omo", badSession)).toBeNull();
        expect(getAgentResumeArgv("grok", badSession)).toBeNull();
      });

      it("rejects control byte in ID", () => {
        const badSession: AgentProviderSession = { key: "session_id", id: "id\x00injection" };
        expect(getAgentResumeArgv("claude", badSession)).toBeNull();
        expect(getAgentResumeArgv("omo", badSession)).toBeNull();
      });

      it("rejects 513-byte ID (> 512 chars)", () => {
        const badSession: AgentProviderSession = { key: "session_id", id: "a".repeat(513) };
        expect(getAgentResumeArgv("claude", badSession)).toBeNull();
        expect(getAgentResumeArgv("omo", badSession)).toBeNull();
      });

      it("rejects wrong provider key (e.g. conversation_id for claude or antigravity)", () => {
        const convForClaude: AgentProviderSession = { key: "conversation_id", id: "conv-123" };
        expect(getAgentResumeArgv("claude", convForClaude)).toBeNull();

        const convForAgy: AgentProviderSession = { key: "conversation_id", id: "conv-123" };
        expect(getAgentResumeArgv("antigravity", convForAgy)).toBeNull();
      });

      it("rejects pi and prime-agent when transcriptPath is missing", () => {
        const missingTranscript: AgentProviderSession = { key: "session_id", id: "pi-id-1" };
        expect(getAgentResumeArgv("pi", missingTranscript)).toBeNull();
        expect(getAgentResumeArgv("prime-agent", missingTranscript)).toBeNull();
      });

      it("rejects 'latest' as a session ID", () => {
        const latestSession: AgentProviderSession = { key: "session_id", id: "latest" };
        expect(getAgentResumeArgv("antigravity", latestSession)).toBeNull();
        expect(getAgentResumeArgv("claude", latestSession)).toBeNull();
      });

      it("builds gjc resume argv with --resume and the agent-minted id", () => {
        const gjcSession: AgentProviderSession = { key: "session_id", id: "1f9d2a6b9c0d1234" };
        expect(getAgentResumeArgv("gjc", gjcSession)).toEqual([
          "gjc",
          "--resume",
          "1f9d2a6b9c0d1234",
        ]);
      });

      it("returns null for unsupported agents (cline, windsurf, aider, notanagent)", () => {
        const validSession: AgentProviderSession = { key: "session_id", id: "session-123" };
        expect(getAgentResumeArgv("cline", validSession)).toBeNull();
        expect(getAgentResumeArgv("windsurf", validSession)).toBeNull();
        expect(getAgentResumeArgv("aider", validSession)).toBeNull();
        expect(getAgentResumeArgv("notanagent", validSession)).toBeNull();
      });
    });

    describe("backward compatibility for legacy AgentSessionRef", () => {
      it("supports legacy { agentType, sessionId } in buildResumeArgv with normalization", () => {
        const ref: AgentSessionRef = { agentType: "claude", sessionId: "c18f-uuid-456" };
        expect(buildResumeArgv(ref)).toEqual(["claude", "--resume", "c18f-uuid-456"]);
      });

    });
  });

});
