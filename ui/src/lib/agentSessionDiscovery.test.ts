import { describe, expect, it, vi } from "vitest";
import {
  discoverAgentSessionId,
  extractSessionIdFromPath,
  findAgentPid,
  UNSUPPORTED_DISCOVERY_AGENTS,
  type OpenFileProbe,
  type ProcessSnapshot,
} from "./agentSessionDiscovery";

describe("agentSessionDiscovery", () => {
  describe("findAgentPid", () => {
    it("finds the agent pid when the agent is a grandchild (pty -> zsh -> claude)", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 100, ppid: 1, command: "/bin/pty-host" },
        { pid: 101, ppid: 100, command: "/bin/zsh" },
        { pid: 102, ppid: 101, command: "/usr/local/bin/claude --verbose" },
        { pid: 103, ppid: 102, command: "node worker.js" },
      ];

      const agentPid = findAgentPid(snapshot, 100, "claude");
      expect(agentPid).toBe(102);
    });

    it("matches cursor-agent for agentType cursor", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 200, ppid: 1, command: "zsh" },
        { pid: 201, ppid: 200, command: "/Users/user/.cursor/cursor-agent run" },
      ];

      const agentPid = findAgentPid(snapshot, 200, "cursor");
      expect(agentPid).toBe(201);
    });

    it("returns null when no matching descendant exists in the subtree", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 300, ppid: 1, command: "zsh" },
        { pid: 301, ppid: 300, command: "git status" },
        { pid: 400, ppid: 1, command: "claude" }, // different tree
      ];

      const agentPid = findAgentPid(snapshot, 300, "claude");
      expect(agentPid).toBeNull();
    });

    it("terminates safely and returns null when process tree contains cycles", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 500, ppid: 1, command: "zsh" },
        { pid: 501, ppid: 500, command: "sh" },
        { pid: 502, ppid: 501, command: "nested-sh" },
        { pid: 501, ppid: 502, command: "cycle" },
      ];

      const agentPid = findAgentPid(snapshot, 500, "claude");
      expect(agentPid).toBeNull();
    });
  });

  describe("extractSessionIdFromPath", () => {
    const UUID = "550e8400-e29b-41d4-a716-446655440000";

    it("extracts session id for claude with valid and invalid paths", () => {
      const validPath = `/Users/x/.claude/projects/-Users-x-code-project/${UUID}.jsonl`;
      const invalidPath = "/Users/x/.claude/projects/-Users-x-code-project/not-a-uuid.jsonl";
      const wrongDir = `/Users/x/.other/projects/${UUID}.jsonl`;

      expect(extractSessionIdFromPath("claude", validPath)).toBe(UUID);
      expect(extractSessionIdFromPath("claude", invalidPath)).toBeNull();
      expect(extractSessionIdFromPath("claude", wrongDir)).toBeNull();
    });

    it("extracts session id for codex with valid and invalid paths", () => {
      const validPath = `/Users/x/.codex/sessions/2026/08/24/rollout-2026-08-24T00-20-13-${UUID}.jsonl`;
      const invalidPath = "/Users/x/.codex/sessions/2026/08/24/rollout-2026-08-24T00-20-13-invalid.jsonl";
      const wrongDir = `/Users/x/.not-codex/sessions/2026/08/24/rollout-2026-08-24T00-20-13-${UUID}.jsonl`;

      expect(extractSessionIdFromPath("codex", validPath)).toBe(UUID);
      expect(extractSessionIdFromPath("codex", invalidPath)).toBeNull();
      expect(extractSessionIdFromPath("codex", wrongDir)).toBeNull();
    });

    it("extracts session id for copilot with valid and invalid paths", () => {
      const validPath = `/Users/x/.copilot/session-state/${UUID}.jsonl`;
      const invalidPath = "/Users/x/.copilot/session-state/invalid-id.jsonl";
      const wrongDir = `/Users/x/.copilot/other-folder/${UUID}.jsonl`;

      expect(extractSessionIdFromPath("copilot", validPath)).toBe(UUID);
      expect(extractSessionIdFromPath("copilot", invalidPath)).toBeNull();
      expect(extractSessionIdFromPath("copilot", wrongDir)).toBeNull();
    });

    it("extracts session id for cursor with store.db, store.db-wal, and store.db-shm variants", () => {
      const projectHash = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
      const dbPath = `/Users/x/.cursor/chats/${projectHash}/${UUID}/store.db`;
      const walPath = `/Users/x/.cursor/chats/${projectHash}/${UUID}/store.db-wal`;
      const shmPath = `/Users/x/.cursor/chats/${projectHash}/${UUID}/store.db-shm`;

      expect(extractSessionIdFromPath("cursor", dbPath)).toBe(UUID);
      expect(extractSessionIdFromPath("cursor", walPath)).toBe(UUID);
      expect(extractSessionIdFromPath("cursor", shmPath)).toBe(UUID);
    });

    it("returns null for cursor when the UUID segment is missing or invalid", () => {
      const projectHash = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
      const missingUuidPath = `/Users/x/.cursor/chats/${projectHash}/store.db`;
      const invalidUuidPath = `/Users/x/.cursor/chats/${projectHash}/not-a-uuid/store.db`;
      const wrongFileNamePath = `/Users/x/.cursor/chats/${projectHash}/${UUID}/other.db`;
      const wrongDir = `/Users/x/.other/chats/${projectHash}/${UUID}/store.db`;

      expect(extractSessionIdFromPath("cursor", missingUuidPath)).toBeNull();
      expect(extractSessionIdFromPath("cursor", invalidUuidPath)).toBeNull();
      expect(extractSessionIdFromPath("cursor", wrongFileNamePath)).toBeNull();
      expect(extractSessionIdFromPath("cursor", wrongDir)).toBeNull();
    });

    it("extracts session id for kimi with valid and invalid paths", () => {
      const validPath = `/Users/x/.kimi/sessions/a1b2c3d4e5/${UUID}/conversation.json`;
      const validDirPath = `/Users/x/.kimi/sessions/a1b2c3d4e5/${UUID}`;
      const invalidPath = "/Users/x/.kimi/sessions/a1b2c3d4e5/not-a-uuid/conversation.json";
      const wrongDir = `/Users/x/.kimi/other/${UUID}/conversation.json`;

      expect(extractSessionIdFromPath("kimi", validPath)).toBe(UUID);
      expect(extractSessionIdFromPath("kimi", validDirPath)).toBe(UUID);
      expect(extractSessionIdFromPath("kimi", invalidPath)).toBeNull();
      expect(extractSessionIdFromPath("kimi", wrongDir)).toBeNull();
    });

    it("extracts session id for omo from real verified session path", () => {
      const realPath =
        "/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_01a04339-8665-7cf7-864f-2b0dd9f6678c.jsonl";
      expect(extractSessionIdFromPath("omo", realPath)).toBe(
        "01a04339-8665-7cf7-864f-2b0dd9f6678c",
      );
    });

    it("returns null for omo when path is in -artifacts or missing uuid", () => {
      const artifactsDirPath =
        "/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_01a04339-8665-7cf7-864f-2b0dd9f6678c-artifacts";
      const artifactsSubfilePath =
        "/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_01a04339-8665-7cf7-864f-2b0dd9f6678c-artifacts/data.json";
      const noUuidPath =
        "/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z.jsonl";
      const timestampOnlyPath =
        "/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_not-a-uuid.jsonl";
      const wrongDir =
        `/Users/indo/.other/sessions/--slug--/2026-08-27T12-37-18-566Z_${UUID}.jsonl`;

      expect(extractSessionIdFromPath("omo", artifactsDirPath)).toBeNull();
      expect(extractSessionIdFromPath("omo", artifactsSubfilePath)).toBeNull();
      expect(extractSessionIdFromPath("omo", noUuidPath)).toBeNull();
      expect(extractSessionIdFromPath("omo", timestampOnlyPath)).toBeNull();
      expect(extractSessionIdFromPath("omo", wrongDir)).toBeNull();
    });

    it("extracts session id for pi from real verified session path", () => {
      const realPath =
        "/Users/indo/.pi/agent/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_01a04339-8665-7cf7-864f-2b0dd9f6678c.jsonl";
      expect(extractSessionIdFromPath("pi", realPath)).toBe(
        "01a04339-8665-7cf7-864f-2b0dd9f6678c",
      );
    });

    it("returns null for unknown agent type", () => {
      const path = `/Users/x/.unknown/sessions/${UUID}.jsonl`;
      expect(extractSessionIdFromPath("unknown-agent", path)).toBeNull();
      expect(extractSessionIdFromPath("grok", path)).toBeNull();
    });
  });

  describe("discoverAgentSessionId", () => {
    it("proves that two panes running claude in the SAME cwd with DIFFERENT root pids resolve to DIFFERENT session ids", () => {
      const pane1RootPid = 1000;
      const pane1ZshPid = 1001;
      const pane1ClaudePid = 1002;
      const pane1SessionId = "11111111-1111-1111-1111-111111111111";

      const pane2RootPid = 2000;
      const pane2ZshPid = 2001;
      const pane2ClaudePid = 2002;
      const pane2SessionId = "22222222-2222-2222-2222-222222222222";

      const sameCwdEscaped = "-Users-indo-code-project-orca-lite";

      const snapshot: ProcessSnapshot = [
        // Pane 1 tree
        { pid: pane1RootPid, ppid: 1, command: "/bin/zsh" },
        { pid: pane1ZshPid, ppid: pane1RootPid, command: "zsh" },
        { pid: pane1ClaudePid, ppid: pane1ZshPid, command: "claude" },

        // Pane 2 tree
        { pid: pane2RootPid, ppid: 1, command: "/bin/zsh" },
        { pid: pane2ZshPid, ppid: pane2RootPid, command: "zsh" },
        { pid: pane2ClaudePid, ppid: pane2ZshPid, command: "claude" },
      ];

      const openFilesMap: Record<number, string[]> = {
        [pane1ClaudePid]: [
          "/dev/ttys001",
          `/Users/indo/.claude/projects/${sameCwdEscaped}/${pane1SessionId}.jsonl`,
        ],
        [pane2ClaudePid]: [
          "/dev/ttys002",
          `/Users/indo/.claude/projects/${sameCwdEscaped}/${pane2SessionId}.jsonl`,
        ],
      };

      const openFiles: OpenFileProbe = (pid: number) => openFilesMap[pid] ?? [];

      const discovered1 = discoverAgentSessionId({
        snapshot,
        rootPid: pane1RootPid,
        agentType: "claude",
        openFiles,
      });

      const discovered2 = discoverAgentSessionId({
        snapshot,
        rootPid: pane2RootPid,
        agentType: "claude",
        openFiles,
      });

      expect(discovered1).toBe(pane1SessionId);
      expect(discovered2).toBe(pane2SessionId);
      expect(discovered1).not.toBe(discovered2);
    });

    it("discovers session id end-to-end for cursor", () => {
      const cursorPid = 502;
      const cursorChatId = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
      const snapshot: ProcessSnapshot = [
        { pid: 500, ppid: 1, command: "zsh" },
        { pid: 501, ppid: 500, command: "node runner.js" },
        { pid: cursorPid, ppid: 501, command: "/usr/local/bin/cursor-agent" },
      ];
      const openFiles: OpenFileProbe = (pid: number) =>
        pid === cursorPid
          ? [
              "/dev/null",
              `/Users/indo/.cursor/chats/project-abc/${cursorChatId}/store.db-wal`,
            ]
          : [];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 500,
        agentType: "cursor",
        openFiles,
      });

      expect(discovered).toBe(cursorChatId);
    });

    it("discovers session id end-to-end for omo", () => {
      const omoPid = 602;
      const omoSessionId = "01a04339-8665-7cf7-864f-2b0dd9f6678c";
      const snapshot: ProcessSnapshot = [
        { pid: 600, ppid: 1, command: "zsh" },
        { pid: omoPid, ppid: 600, command: "omo run" },
      ];
      const openFiles: OpenFileProbe = (pid: number) =>
        pid === omoPid
          ? [
              "/dev/ttys003",
              `/Users/indo/.omo/sessions/--Users-indo-code-project-orca-lite--/2026-08-27T12-37-18-566Z_${omoSessionId}.jsonl`,
            ]
          : [];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 600,
        agentType: "omo",
        openFiles,
      });

      expect(discovered).toBe(omoSessionId);
    });

    it("returns null when the agent pid is not found and does not warn", () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const snapshot: ProcessSnapshot = [
        { pid: 100, ppid: 1, command: "zsh" },
      ];
      const openFiles: OpenFileProbe = () => ["/some/file.jsonl"];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 100,
        agentType: "claude",
        openFiles,
      });

      expect(discovered).toBeNull();
      expect(warnSpy).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("warns about possible vendor layout drift when agent process has non-matching open files", () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const snapshot: ProcessSnapshot = [
        { pid: 100, ppid: 1, command: "zsh" },
        { pid: 101, ppid: 100, command: "claude" },
      ];
      const openFiles: OpenFileProbe = () => ["/dev/null", "/Users/x/.config/claude/config.json"];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 100,
        agentType: "claude",
        openFiles,
      });

      expect(discovered).toBeNull();
      expect(warnSpy).toHaveBeenCalledTimes(1);
      expect(warnSpy).toHaveBeenCalledWith(
        expect.stringMatching(/^\[agent-session-discovery\] Possible vendor layout drift: no session ID found for agent "claude" across 2 open file\(s\)\.$/),
      );
      warnSpy.mockRestore();
    });

    it("does not warn when session extraction succeeds", () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const snapshot: ProcessSnapshot = [
        { pid: 100, ppid: 1, command: "zsh" },
        { pid: 101, ppid: 100, command: "claude" },
      ];
      const sessionId = "550e8400-e29b-41d4-a716-446655440000";
      const openFiles: OpenFileProbe = () => [
        `/Users/x/.claude/projects/proj/${sessionId}.jsonl`,
      ];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 100,
        agentType: "claude",
        openFiles,
      });

      expect(discovered).toBe(sessionId);
      expect(warnSpy).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("regression guard: returns null and never invents or generates a session ID when probe returns empty list", () => {
      const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
      const snapshot: ProcessSnapshot = [
        { pid: 100, ppid: 1, command: "zsh" },
        { pid: 101, ppid: 100, command: "claude" },
      ];
      const openFiles: OpenFileProbe = () => [];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 100,
        agentType: "claude",
        openFiles,
      });

      expect(discovered).toBeNull();
      expect(warnSpy).not.toHaveBeenCalled();
      warnSpy.mockRestore();
    });

    it("exports UNSUPPORTED_DISCOVERY_AGENTS set not containing opencode or pi", () => {
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("opencode")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("claude")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("antigravity")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("pi")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("cursor")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("omo")).toBe(false);
      expect(UNSUPPORTED_DISCOVERY_AGENTS.has("unknown-agent")).toBe(false);
    });
  });

  describe("gjc short-hex session stores", () => {
    it("matches the gjc binary in the process tree", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 300, ppid: 1, command: "zsh" },
        { pid: 301, ppid: 300, command: "/usr/local/bin/gjc" },
      ];
      expect(findAgentPid(snapshot, 300, "gjc")).toBe(301);
    });

    it("extracts the session id from default and XDG gjc layouts", () => {
      expect(
        extractSessionIdFromPath(
          "gjc",
          "/Users/me/.gjc/agent/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_1f9d2a6b9c0d1234.jsonl",
        ),
      ).toBe("1f9d2a6b9c0d1234");
      expect(
        extractSessionIdFromPath(
          "gjc",
          "/Users/me/.local/share/gjc/profiles/work/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_1f9d2a6b9c0d1234.jsonl",
        ),
      ).toBe("1f9d2a6b9c0d1234");
    });

    it("returns null outside gjc session stores and for degenerate ids", () => {
      expect(extractSessionIdFromPath("gjc", "/tmp/unrelated_1f9d2a6b9c0d1234.jsonl")).toBeNull();
      expect(
        extractSessionIdFromPath(
          "gjc",
          "/Users/me/.gjc/agent/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_abcd.jsonl",
        ),
      ).toBeNull();
    });
  });

  describe("antigravity (agy) session discovery", () => {
    it("matches the agy binary in the process tree", () => {
      const snapshot: ProcessSnapshot = [
        { pid: 400, ppid: 1, command: "zsh" },
        { pid: 401, ppid: 400, command: "/usr/local/bin/agy" },
      ];
      expect(findAgentPid(snapshot, 400, "antigravity")).toBe(401);
    });

    it("extracts the session id from antigravity conversations db path", () => {
      const uuid = "12345678-1234-1234-1234-123456789abc";
      expect(
        extractSessionIdFromPath(
          "antigravity",
          `/Users/me/.gemini/antigravity-cli/conversations/${uuid}.db`,
        ),
      ).toBe(uuid);
      expect(
        extractSessionIdFromPath(
          "antigravity",
          `/Users/me/.gemini/antigravity-cli/conversations/${uuid}.db-wal`,
        ),
      ).toBe(uuid);
      expect(
        extractSessionIdFromPath(
          "antigravity",
          `/Users/me/.gemini/antigravity-cli/conversations/${uuid}.db-shm`,
        ),
      ).toBe(uuid);
    });

    it("returns null outside antigravity session store or for non-uuid db names", () => {
      expect(
        extractSessionIdFromPath("antigravity", "/tmp/conversations/12345678-1234-1234-1234-123456789abc.db"),
      ).toBeNull();
      expect(
        extractSessionIdFromPath(
          "antigravity",
          "/Users/me/.gemini/antigravity-cli/conversations/not-a-uuid.db",
        ),
      ).toBeNull();
    });

    it("discovers antigravity session id from open db file", () => {
      const uuid = "abcdef12-3456-7890-abcd-ef1234567890";
      const snapshot: ProcessSnapshot = [
        { pid: 400, ppid: 1, command: "zsh" },
        { pid: 401, ppid: 400, command: "agy" },
      ];
      const openFiles: OpenFileProbe = (pid) =>
        pid === 401 ? [`/Users/me/.gemini/antigravity-cli/conversations/${uuid}.db`] : [];

      const discovered = discoverAgentSessionId({
        snapshot,
        rootPid: 400,
        agentType: "antigravity",
        openFiles,
      });

      expect(discovered).toBe(uuid);
    });
  });
});
