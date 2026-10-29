import { beforeEach, describe, expect, it, vi } from "vitest";

import { splitAgentArgs } from "./agentsSettings";

describe("splitAgentArgs", () => {
  it("keeps quoted arguments whole and backslashes literal", () => {
    expect(splitAgentArgs(`--append-system-prompt "a b" --name 'c d'  ""`)).toEqual([
      "--append-system-prompt", "a b", "--name", "c d", "",
    ]);
    expect(splitAgentArgs(`-c "say \\"hi\\"" 'it'"'"'s'`)).toEqual(["-c", `say "hi"`, "it's"]);
    expect(splitAgentArgs(`--config C:\\Users\\me\\cfg.toml --dir "C:\\Program Files\\X\\"`)).toEqual([
      "--config", "C:\\Users\\me\\cfg.toml", "--dir", "C:\\Program Files\\X\\",
    ]);
    expect(splitAgentArgs("  ")).toEqual([]);
  });
});

import {
  AGENT_CANDIDATES,
  AGENTS_SETTINGS_CHANGED_EVENT,
  AGENTS_SETTINGS_STORAGE_KEY,
  DEFAULT_AGENT_SETTINGS,
  detectionTargets,
  getLaunchableAgents,
  loadAgentSettings,
  mergeDetections,
  normalizeCustomAgentName,
  removeCustomAgent,
  saveAgentSettings,
  upsertCustomAgent,
  validateCustomAgent,
  type AgentSettings,
} from "./agentsSettings";

describe("agentsSettings persistence and pure helpers", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  describe("loadAgentSettings and saveAgentSettings", () => {
    it("returns default settings when storage is empty", () => {
      const settings = loadAgentSettings();
      expect(settings).toEqual(DEFAULT_AGENT_SETTINGS);
    });

    it("loads and saves settings accurately with localStorage", () => {
      const customSettings: AgentSettings = {
        version: 1,
        defaultAgentId: "claude",
        overrides: {
          claude: { enabled: true, command: "claude-code", args: "--dangerously-skip-permissions" },
          aider: { enabled: false },
        },
        custom: [],
      };

      saveAgentSettings(customSettings);
      expect(localStorage.getItem(AGENTS_SETTINGS_STORAGE_KEY)).toBe(JSON.stringify(customSettings));

      const loaded = loadAgentSettings();
      expect(loaded).toEqual(customSettings);
    });

    it("falls back safely on corrupted or invalid JSON in storage", () => {
      localStorage.setItem(AGENTS_SETTINGS_STORAGE_KEY, "invalid-json{{");
      expect(loadAgentSettings()).toEqual(DEFAULT_AGENT_SETTINGS);

      localStorage.setItem(AGENTS_SETTINGS_STORAGE_KEY, JSON.stringify("not-an-object"));
      expect(loadAgentSettings()).toEqual(DEFAULT_AGENT_SETTINGS);
    });

    it("dispatches AGENTS_SETTINGS_CHANGED_EVENT on save", () => {
      const listener = vi.fn();
      window.addEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);

      const payload: AgentSettings = {
        version: 1,
        defaultAgentId: "codex",
        overrides: {},
        custom: [],
      };
      saveAgentSettings(payload);

      expect(listener).toHaveBeenCalled();
      window.removeEventListener(AGENTS_SETTINGS_CHANGED_EVENT, listener);
    });
  });

  describe("mergeDetections", () => {
    it("defaults enabled to available when no overrides are set", () => {
      const detections = [
        { name: "claude", available: true },
        { name: "agy", available: false },
      ];

      const resolved = mergeDetections(DEFAULT_AGENT_SETTINGS, detections);

      expect(resolved).toHaveLength(AGENT_CANDIDATES.length);

      const claude = resolved.find((r) => r.name === "claude")!;
      expect(claude).toEqual({
        name: "claude",
        available: true,
        enabled: true,
        command: "claude",
        args: "",
        custom: false,
      });

      const antigravity = resolved.find((r) => r.name === "antigravity")!;
      expect(antigravity).toEqual({
        name: "antigravity",
        available: false,
        enabled: false,
        command: "agy",
        args: "",
        custom: false,
      });
    });

    it("applies command, args, and enabled overrides correctly", () => {
      const settings: AgentSettings = {
        version: 1,
        defaultAgentId: "claude",
        overrides: {
          claude: { enabled: false, command: "/usr/local/bin/claude", args: "--model sonnet" },
          opencode: { enabled: true, command: "opencode-custom" },
        },
        custom: [],
      };

      const detections = [
        { name: "/usr/local/bin/claude", available: true },
        { name: "opencode-custom", available: false },
      ];

      const resolved = mergeDetections(settings, detections);

      const claude = resolved.find((r) => r.name === "claude")!;
      expect(claude).toEqual({
        name: "claude",
        available: true,
        enabled: false, // overridden to false despite available
        command: "/usr/local/bin/claude",
        args: "--model sonnet",
        custom: false,
      });

      const opencode = resolved.find((r) => r.name === "opencode")!;
      expect(opencode).toEqual({
        name: "opencode",
        available: false,
        enabled: true, // overridden to true despite not available
        command: "opencode-custom",
        args: "",
        custom: false,
      });
    });

    it("resolves registered custom agents alongside built-in candidates", () => {
      const settings: AgentSettings = {
        version: 1,
        defaultAgentId: null,
        overrides: {},
        custom: [{ name: "my-agent", command: "my-agent-cli", args: "--resume" }],
      };

      const resolved = mergeDetections(settings, [
        { name: "my-agent-cli", available: true },
      ]);

      expect(resolved).toHaveLength(AGENT_CANDIDATES.length + 1);
      expect(resolved.find((r) => r.name === "my-agent")).toEqual({
        name: "my-agent",
        available: true,
        enabled: true,
        command: "my-agent-cli",
        args: "--resume",
        custom: true,
      });
    });
  });

  describe("launch mode flags", () => {
    const argsFor = (name: string, overrides: AgentSettings["overrides"]) =>
      mergeDetections({ ...DEFAULT_AGENT_SETTINGS, overrides }, []).find((r) => r.name === name)!
        .args;

    it("appends --permission-mode for claude when a permission mode is set", () => {
      expect(argsFor("claude", { claude: { permissionMode: "plan" } })).toBe(
        "--permission-mode plan",
      );
      expect(
        argsFor("claude", { claude: { args: "--model opus", permissionMode: "plan" } }),
      ).toMatch(/--permission-mode plan$/);
    });

    it("keeps a --permission-mode the user already put in args", () => {
      expect(
        argsFor("claude", {
          claude: { args: "--permission-mode acceptEdits", permissionMode: "plan" },
        }),
      ).toBe("--permission-mode acceptEdits");
    });

    it("appends --sandbox for codex when a sandbox is set", () => {
      expect(argsFor("codex", { codex: { sandbox: "read-only" } })).toBe("--sandbox read-only");
    });

    it("leaves args unchanged when no mode is set or for other agents", () => {
      expect(argsFor("claude", { claude: { args: "--model opus" } })).toBe("--model opus");
      expect(argsFor("codex", {})).toBe("");
      expect(
        argsFor("aider", { aider: { args: "--chat", permissionMode: "plan", sandbox: "read-only" } }),
      ).toBe("--chat");
    });
  });

  describe("custom agent registration", () => {
    it("normalizes names to a lowercase dashed slug", () => {
      expect(normalizeCustomAgentName("  My Agent  ")).toBe("my-agent");
    });

    it("rejects empty, reserved, and duplicate names", () => {
      const settings: AgentSettings = {
        ...DEFAULT_AGENT_SETTINGS,
        custom: [{ name: "my-agent", command: "my-agent-cli", args: "" }],
      };

      expect(validateCustomAgent({ name: " ", command: "x" }, settings)).toBe("empty-name");
      expect(validateCustomAgent({ name: "x", command: " " }, settings)).toBe("empty-command");
      expect(validateCustomAgent({ name: "Claude", command: "x" }, settings)).toBe("reserved-name");
      expect(validateCustomAgent({ name: "My Agent", command: "x" }, settings)).toBe(
        "duplicate-name",
      );
      expect(validateCustomAgent({ name: "other", command: "x" }, settings)).toBeNull();
      expect(
        validateCustomAgent({ name: "My Agent", command: "x" }, settings, "my-agent"),
      ).toBeNull();
    });

    it("adds, renames, and removes custom agents while carrying state", () => {
      const added = upsertCustomAgent(DEFAULT_AGENT_SETTINGS, {
        name: "My Agent",
        command: " my-agent-cli ",
        args: " --resume ",
      });
      expect(added.custom).toEqual([
        { name: "my-agent", command: "my-agent-cli", args: "--resume" },
      ]);

      const withState: AgentSettings = {
        ...added,
        defaultAgentId: "my-agent",
        overrides: { "my-agent": { enabled: false } },
      };

      const renamed = upsertCustomAgent(
        withState,
        { name: "renamed", command: "my-agent-cli", args: "" },
        "my-agent",
      );
      expect(renamed.custom.map((a) => a.name)).toEqual(["renamed"]);
      expect(renamed.defaultAgentId).toBe("renamed");
      expect(renamed.overrides).toEqual({ renamed: { enabled: false } });

      const removed = removeCustomAgent(renamed, "renamed");
      expect(removed.custom).toEqual([]);
      expect(removed.defaultAgentId).toBeNull();
      expect(removed.overrides).toEqual({});
    });

    it("drops reserved, malformed, and duplicate custom entries when loading", () => {
      localStorage.setItem(
        AGENTS_SETTINGS_STORAGE_KEY,
        JSON.stringify({
          version: 1,
          defaultAgentId: null,
          overrides: {},
          custom: [
            { name: "claude", command: "claude" },
            { name: "My Agent", command: "my-agent-cli", args: "--resume" },
            { name: "my-agent", command: "duplicate" },
            { name: "no-command", command: "  " },
            "garbage",
          ],
        }),
      );

      expect(loadAgentSettings().custom).toEqual([
        { name: "my-agent", command: "my-agent-cli", args: "--resume" },
      ]);
    });

    it("migrates a custom agent whose name became built-in into an override instead of dropping it", () => {
      localStorage.setItem(
        AGENTS_SETTINGS_STORAGE_KEY,
        JSON.stringify({
          version: 1,
          defaultAgentId: "grok",
          overrides: { grok: { enabled: true } },
          custom: [
            { name: "Grok", command: "/opt/bin/grok", args: "--yolo" },
            { name: "omp", command: "omp" },
          ],
        }),
      );

      const loaded = loadAgentSettings();
      expect(loaded.custom).toEqual([]);
      expect(loaded.defaultAgentId).toBe("grok");
      expect(loaded.overrides.grok).toEqual({ enabled: true, command: "/opt/bin/grok", args: "--yolo" });
      expect(loaded.overrides.omp).toEqual({ command: "omp", args: "" });
    });

    it("probes every first-class agent binary by default", () => {
      for (const name of ["pi", "copilot", "grok", "kimi", "cline", "omp"]) {
        expect(AGENT_CANDIDATES).toContain(name);
      }
      expect(detectionTargets(DEFAULT_AGENT_SETTINGS)).toEqual(
        expect.arrayContaining(["pi", "copilot", "grok", "kimi", "cline", "omp"]),
      );
    });

    it("builds detection targets from resolved commands including custom agents", () => {
      const settings: AgentSettings = {
        version: 1,
        defaultAgentId: null,
        overrides: { claude: { command: "claude-code" } },
        custom: [{ name: "my-agent", command: "my-agent-cli", args: "" }],
      };

      const targets = detectionTargets(settings);

      expect(targets).toContain("claude-code");
      expect(targets).not.toContain("claude");
      expect(targets).toContain("my-agent-cli");
      expect(targets).toContain("codex");
      expect(targets).toContain("agy");
      expect(targets).not.toContain("antigravity");
    });
  });

  describe("getLaunchableAgents", () => {
    it("returns only agents that are both enabled AND available", () => {
      const resolved = [
        { name: "claude", available: true, enabled: true, command: "claude", args: "", custom: false },
        { name: "codex", available: true, enabled: false, command: "codex", args: "", custom: false },
        { name: "antigravity", available: false, enabled: true, command: "agy", args: "", custom: false },
        { name: "aider", available: true, enabled: true, command: "aider", args: "--chat", custom: false },
      ];

      const launchable = getLaunchableAgents(resolved);

      expect(launchable.map((a) => a.name)).toEqual(["claude", "aider"]);
      expect(launchable[1].args).toBe("--chat");
    });
  });
});
