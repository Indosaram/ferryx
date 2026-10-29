import { describe, expect, it } from "vitest";

import {
  cleanAgentTask,
  disambiguateTabTexts,
  isShellPromptTitle,
  isTransientAgentStatusTask,
  resolveTerminalTabText,
  stripLegacyOrdinal,
} from "./tabTitle";

describe("tabTitle module", () => {
  describe("cleanAgentTask", () => {
    it("extracts task from OMO title dropping agent name and trailing project", () => {
      expect(cleanAgentTask("OmO - fix login bug - ferryx", "ferryx")).toBe("fix login bug");
      expect(cleanAgentTask("OmO - fix login bug - ferryx")).toBe("fix login bug");
    });

    it("falls back to agent display name when task only contains the project name", () => {
      expect(cleanAgentTask("OmO - ferryx", "ferryx")).toBe("OMO");
    });

    it("extracts task from claude code activity title", () => {
      expect(cleanAgentTask("✳ Refactor parser", undefined, "claude")).toBe("Refactor parser");
      expect(cleanAgentTask("✳ Refactor parser")).toBe("Refactor parser");
    });

    it("extracts task from pi extension title treating π as agent segment", () => {
      expect(cleanAgentTask("⠋ π - fix login bug - ferryx")).toBe("fix login bug");
      expect(cleanAgentTask("⠋ π - fix login bug - ferryx", "ferryx")).toBe("fix login bug");
      expect(cleanAgentTask("π - ferryx", "ferryx")).toBe("Pi");
    });

    it("returns null for non-agent titles", () => {
      expect(cleanAgentTask("vim README.md")).toBeNull();
      expect(cleanAgentTask("zsh")).toBeNull();
      expect(cleanAgentTask("")).toBeNull();
    });
  });

  describe("isShellPromptTitle", () => {
    it("detects bare shell names", () => {
      expect(isShellPromptTitle("zsh")).toBe(true);
      expect(isShellPromptTitle("bash")).toBe(true);
      expect(isShellPromptTitle("fish")).toBe(true);
      expect(isShellPromptTitle("sh")).toBe(true);
      expect(isShellPromptTitle("pwsh")).toBe(true);
      expect(isShellPromptTitle("powershell")).toBe(true);
      expect(isShellPromptTitle("cmd")).toBe(true);
      expect(isShellPromptTitle("nu")).toBe(true);
      expect(isShellPromptTitle("-zsh")).toBe(true);
      expect(isShellPromptTitle("login")).toBe(true);
    });

    it("detects user@host and user@host:path prompt titles", () => {
      expect(isShellPromptTitle("indo@mac:~/code/ferryx")).toBe(true);
      expect(isShellPromptTitle("user@host")).toBe(true);
      expect(isShellPromptTitle("user@host:/var/log")).toBe(true);
    });

    it("detects path-only titles", () => {
      expect(isShellPromptTitle("~/code/ferryx")).toBe(true);
      expect(isShellPromptTitle("/Volumes/Data/project/ferryx")).toBe(true);
      expect(isShellPromptTitle("C:\\Users\\example\\project")).toBe(true);
      expect(isShellPromptTitle("~")).toBe(true);
    });

    it("detects worktree directory name only", () => {
      expect(isShellPromptTitle("ferryx", "ferryx")).toBe(true);
      expect(isShellPromptTitle("ferryx", "/path/to/ferryx")).toBe(true);
    });

    it("treats empty or whitespace titles as shell/prompt", () => {
      expect(isShellPromptTitle("")).toBe(true);
      expect(isShellPromptTitle("   ")).toBe(true);
    });

    it("does not treat running program titles as shell/prompt", () => {
      expect(isShellPromptTitle("vim README.md")).toBe(false);
      expect(isShellPromptTitle("cargo test")).toBe(false);
      expect(isShellPromptTitle("node server.js")).toBe(false);
      expect(isShellPromptTitle("htop")).toBe(false);
    });
  });

  describe("isTransientAgentStatusTask", () => {
    it("detects PreToolUse and PostToolUse prefixes", () => {
      expect(isTransientAgentStatusTask("PostToolUse: (OmO) Checking LSP Diagnostics")).toBe(true);
      expect(isTransientAgentStatusTask("PreToolUse: reading file")).toBe(true);
      expect(isTransientAgentStatusTask("PostToolUse: running nested-agents-md")).toBe(true);
    });

    it("detects SessionStart and Stop prefixes", () => {
      expect(isTransientAgentStatusTask("SessionStart: init")).toBe(true);
      expect(isTransientAgentStatusTask("Stop: finished")).toBe(true);
    });

    it("detects Running followed by a single tool word", () => {
      expect(isTransientAgentStatusTask("Running eval")).toBe(true);
      expect(isTransientAgentStatusTask("Running bash")).toBe(true);
      expect(isTransientAgentStatusTask("Running tool_search")).toBe(true);
    });

    it("returns false for regular tasks and multi-word running tasks", () => {
      expect(isTransientAgentStatusTask("fix login bug")).toBe(false);
      expect(isTransientAgentStatusTask("Refactor parser")).toBe(false);
      expect(isTransientAgentStatusTask("Running unit tests for auth")).toBe(false);
      expect(isTransientAgentStatusTask("")).toBe(false);
    });
  });

  describe("stripLegacyOrdinal", () => {
    it("strips legacy ordinal parentheses from branch labels", () => {
      expect(stripLegacyOrdinal("main (3)")).toBe("main");
      expect(stripLegacyOrdinal("fix/x (2)")).toBe("fix/x");
      expect(stripLegacyOrdinal("main")).toBe("main");
      expect(stripLegacyOrdinal("feat/test (10)")).toBe("feat/test");
    });

    it("preserves non-numeric parentheses and empty strings", () => {
      expect(stripLegacyOrdinal("main (test)")).toBe("main (test)");
      expect(stripLegacyOrdinal("")).toBe("");
    });
  });

  describe("disambiguateTabTexts", () => {
    it("keeps first occurrence bare and appends (2), (3) to duplicate occurrences", () => {
      expect(disambiguateTabTexts(["main", "main", "build"])).toEqual(["main", "main (2)", "build"]);
      expect(disambiguateTabTexts(["main", "main", "main"])).toEqual(["main", "main (2)", "main (3)"]);
      expect(disambiguateTabTexts(["main", "build", "main", "build"])).toEqual([
        "main",
        "build",
        "main (2)",
        "build (2)",
      ]);
    });
  });

  describe("resolveTerminalTabText", () => {
    it("shows agent title when agent is active", () => {
      const omoTask = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "OmO - fix login bug - ferryx",
          isAgent: true,
        },
        projectName: "ferryx",
      });
      expect(omoTask.text).toBe("fix login bug");
      expect(omoTask.source).toBe("agent");
      expect(omoTask.tooltip).toBe("OmO - fix login bug - ferryx (main)");

      const omoOnlyProject = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "OmO - ferryx",
          isAgent: true,
        },
        projectName: "ferryx",
      });
      expect(omoOnlyProject.text).toBe("OMO");
      expect(omoOnlyProject.source).toBe("agent");

      const claude = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "✳ Refactor parser",
          isAgent: true,
          agentType: "claude",
        },
      });
      expect(claude.text).toBe("Refactor parser");
      expect(claude.source).toBe("agent");

      const piTask = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "⠋ π - fix login bug - ferryx",
        },
        projectName: "ferryx",
      });
      expect(piTask.text).toBe("fix login bug");
      expect(piTask.source).toBe("agent");
      expect(piTask.tooltip).toBe("⠋ π - fix login bug - ferryx (main)");
    });

    it("handles transient agent status tasks with previousAgentText retention", () => {
      const retainedOmo = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "OmO - PostToolUse: (OmO) Checking LSP Diagnostics - ferryx",
          isAgent: true,
        },
        projectName: "ferryx",
        previousAgentText: "fix login bug",
      });
      expect(retainedOmo.text).toBe("fix login bug");
      expect(retainedOmo.source).toBe("agent");

      const fallbackOmo = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "OmO - PostToolUse: (OmO) Checking LSP Diagnostics - ferryx",
          isAgent: true,
        },
        projectName: "ferryx",
      });
      expect(fallbackOmo.text).toBe("OMO");
      expect(fallbackOmo.source).toBe("agent");

      const retainedPi = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "⠋ π - Running eval - ferryx",
        },
        projectName: "ferryx",
        previousAgentText: "refactor parser",
      });
      expect(retainedPi.text).toBe("refactor parser");
      expect(retainedPi.source).toBe("agent");

      const fallbackPi = resolveTerminalTabText({
        baseLabel: "main",
        activity: {
          title: "⠋ π - Running eval - ferryx",
        },
        projectName: "ferryx",
      });
      expect(fallbackPi.text).toBe("Pi");
      expect(fallbackPi.source).toBe("agent");
    });

    it("falls back to base label when title is shell/prompt", () => {
      const zsh = resolveTerminalTabText({
        baseLabel: "main",
        activity: { title: "zsh" },
      });
      expect(zsh.text).toBe("main");
      expect(zsh.source).toBe("base");

      const userHost = resolveTerminalTabText({
        baseLabel: "main",
        activity: { title: "indo@mac:~/code/ferryx" },
      });
      expect(userHost.text).toBe("main");
      expect(userHost.source).toBe("base");

      const pathOnly = resolveTerminalTabText({
        baseLabel: "main",
        activity: { title: "~/code/ferryx" },
      });
      expect(pathOnly.text).toBe("main");
      expect(pathOnly.source).toBe("base");
    });

    it("falls back to Terminal when base label is blank or only ordinal", () => {
      const blankBase = resolveTerminalTabText({
        baseLabel: "",
      });
      expect(blankBase.text).toBe("Terminal");
      expect(blankBase.source).toBe("base");

      const ordinalOnly = resolveTerminalTabText({
        baseLabel: " (3)",
      });
      expect(ordinalOnly.text).toBe("Terminal");
      expect(ordinalOnly.source).toBe("base");
    });

    it("shows program title for non-agent non-shell titles", () => {
      const vim = resolveTerminalTabText({
        baseLabel: "main",
        activity: { title: "vim README.md" },
      });
      expect(vim.text).toBe("vim README.md");
      expect(vim.source).toBe("title");
      expect(vim.tooltip).toBe("main");
    });

    it("gives customLabel priority over agent titles", () => {
      const customWins = resolveTerminalTabText({
        customLabel: "Renamed Tab",
        baseLabel: "main",
        activity: {
          title: "OmO - fix login bug - ferryx",
          isAgent: true,
        },
        projectName: "ferryx",
      });
      expect(customWins.text).toBe("Renamed Tab");
      expect(customWins.source).toBe("custom");
      expect(customWins.tooltip).toBe("OmO - fix login bug - ferryx (main)");
    });

    it("strips legacy ordinal from fallback base label but preserves it on customLabel", () => {
      const legacyBase = resolveTerminalTabText({
        baseLabel: "main (3)",
      });
      expect(legacyBase.text).toBe("main");
      expect(legacyBase.source).toBe("base");

      const customWithOrdinal = resolveTerminalTabText({
        customLabel: "my tab (3)",
        baseLabel: "main",
      });
      expect(customWithOrdinal.text).toBe("my tab (3)");
      expect(customWithOrdinal.source).toBe("custom");
    });
  });
});
