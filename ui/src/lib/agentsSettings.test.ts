import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  AGENT_CANDIDATES,
  AGENTS_SETTINGS_CHANGED_EVENT,
  AGENTS_SETTINGS_STORAGE_KEY,
  DEFAULT_AGENT_SETTINGS,
  getLaunchableAgents,
  loadAgentSettings,
  mergeDetections,
  saveAgentSettings,
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
        { name: "gemini", available: false },
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
      });

      const gemini = resolved.find((r) => r.name === "gemini")!;
      expect(gemini).toEqual({
        name: "gemini",
        available: false,
        enabled: false,
        command: "gemini",
        args: "",
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
      };

      const detections = [
        { name: "claude", available: true },
        { name: "opencode", available: false },
      ];

      const resolved = mergeDetections(settings, detections);

      const claude = resolved.find((r) => r.name === "claude")!;
      expect(claude).toEqual({
        name: "claude",
        available: true,
        enabled: false, // overridden to false despite available
        command: "/usr/local/bin/claude",
        args: "--model sonnet",
      });

      const opencode = resolved.find((r) => r.name === "opencode")!;
      expect(opencode).toEqual({
        name: "opencode",
        available: false,
        enabled: true, // overridden to true despite not available
        command: "opencode-custom",
        args: "",
      });
    });
  });

  describe("getLaunchableAgents", () => {
    it("returns only agents that are both enabled AND available", () => {
      const resolved = [
        { name: "claude", available: true, enabled: true, command: "claude", args: "" },
        { name: "codex", available: true, enabled: false, command: "codex", args: "" },
        { name: "gemini", available: false, enabled: true, command: "gemini", args: "" },
        { name: "aider", available: true, enabled: true, command: "aider", args: "--chat" },
      ];

      const launchable = getLaunchableAgents(resolved);

      expect(launchable.map((a) => a.name)).toEqual(["claude", "aider"]);
      expect(launchable[1].args).toBe("--chat");
    });
  });
});
