import { describe, expect, it } from "vitest";

import {
  classifyTerminalTitleActivity,
  containsAgentSpinnerGlyph,
  formatTabLabelFromTitle,
  normalizeTerminalTitle,
  parseAgentTitle,
} from "./agentTitle";

describe("agentTitle parser & normalizer", () => {
  it("normalizes terminal title strings by stripping control characters and trimming", () => {
    expect(normalizeTerminalTitle("  \x1b[0m\x07hello world  ")).toBe("hello world");
    expect(normalizeTerminalTitle("\x1b]0;agy: running task\x07")).toBe("agy: running task");
    expect(normalizeTerminalTitle("")).toBe("");
  });

  describe("spinner detection and working state classification", () => {
    it("detects various braille and symbolic spinner glyphs", () => {
      expect(containsAgentSpinnerGlyph("⠋ omo: starting")).toBe(true);
      expect(containsAgentSpinnerGlyph("⠹ agy: optimizing")).toBe(true);
      expect(containsAgentSpinnerGlyph("✦ antigravity: analyzing")).toBe(true);
      expect(containsAgentSpinnerGlyph("✳ claude code: thinking")).toBe(true);
      expect(containsAgentSpinnerGlyph("⏲ aider: applying patch")).toBe(true);
      expect(containsAgentSpinnerGlyph("bash")).toBe(false);
    });

    it("classifies spinner-prefixed and working titles as working activity", () => {
      expect(classifyTerminalTitleActivity("⠋ omo: refactoring layout")).toBe("working");
      expect(classifyTerminalTitleActivity("✦ codex: reviewing diffs")).toBe("working");
      expect(classifyTerminalTitleActivity("✳ claude code - analyzing tests")).toBe("working");
      expect(classifyTerminalTitleActivity("⏲ aider: applying patch")).toBe("working");
      expect(classifyTerminalTitleActivity("omo: working")).toBe("working");
    });
  });

  describe("waiting and done state precedence", () => {
    it("gives waiting semantics precedence over spinner glyphs", () => {
      expect(classifyTerminalTitleActivity("✋ omo: permission required")).toBe("waiting");
      expect(classifyTerminalTitleActivity("✦ codex: needs input")).toBe("waiting");
      expect(classifyTerminalTitleActivity("⠹ agy: waiting for user confirmation")).toBe("waiting");
      expect(classifyTerminalTitleActivity("antigravity: action required")).toBe("waiting");
    });

    it("gives done semantics precedence over spinner glyphs and other activity", () => {
      expect(classifyTerminalTitleActivity("✳ claude code: done")).toBe("done");
      expect(classifyTerminalTitleActivity("⠋ omo: completed")).toBe("done");
      expect(classifyTerminalTitleActivity("◇ agy finished")).toBe("done");
      expect(classifyTerminalTitleActivity("* codex result")).toBe("done");
      expect(classifyTerminalTitleActivity("zsh /repo")).toBeNull();
    });
  });

  describe("agy / antigravity agent detection and state parsing", () => {
    it("detects agy shorthand with working, waiting, and done states", () => {
      const working = parseAgentTitle("⠹ agy: running task");
      expect(working).not.toBeNull();
      expect(working?.isAgent).toBe(true);
      expect(working?.agentType).toBe("antigravity");
      expect(working?.name).toBe("Antigravity");
      expect(working?.task).toBe("running task");
      expect(working?.state).toBe("working");

      const waiting = parseAgentTitle("agy: waiting for user approval");
      expect(waiting?.isAgent).toBe(true);
      expect(waiting?.agentType).toBe("antigravity");
      expect(waiting?.state).toBe("waiting");

      const done = parseAgentTitle("◇ agy: done");
      expect(done?.isAgent).toBe(true);
      expect(done?.agentType).toBe("antigravity");
      expect(done?.state).toBe("exited");
    });

    it("detects full antigravity name with working, waiting, and done states", () => {
      const working = parseAgentTitle("✦ antigravity: executing tools");
      expect(working?.isAgent).toBe(true);
      expect(working?.agentType).toBe("antigravity");
      expect(working?.name).toBe("Antigravity");
      expect(working?.state).toBe("working");

      const waiting = parseAgentTitle("antigravity: needs input");
      expect(waiting?.isAgent).toBe(true);
      expect(waiting?.state).toBe("waiting");
    });
  });

  describe("OMO agent parsing", () => {
    it("detects OMO agent with working, waiting, and done states", () => {
      const working = parseAgentTitle("⠋ omo: refactoring layout components");
      expect(working).not.toBeNull();
      expect(working?.isAgent).toBe(true);
      expect(working?.agentType).toBe("omo");
      expect(working?.name).toBe("OMO");
      expect(working?.task).toBe("refactoring layout components");
      expect(working?.state).toBe("working");

      const waiting = parseAgentTitle("omo: waiting for user confirmation");
      expect(waiting).not.toBeNull();
      expect(waiting?.isAgent).toBe(true);
      expect(waiting?.state).toBe("waiting");

      const done = parseAgentTitle("✳ omo: done");
      expect(done?.state).toBe("exited");
    });
  });

  describe("other agents & non-agent processes", () => {
    it("detects Claude Code, Codex, and other agents", () => {
      const claude = parseAgentTitle("✳ claude code - analyzing tests");
      expect(claude?.isAgent).toBe(true);
      expect(claude?.name).toBe("Claude Code");

      const codex = parseAgentTitle("⠋ codex: generating code");
      expect(codex?.isAgent).toBe(true);
      expect(codex?.name).toBe("Codex");
      expect(codex?.state).toBe("working");

      const opencode = parseAgentTitle("opencode task");
      expect(opencode?.isAgent).toBe(true);
      expect(opencode?.name).toBe("OpenCode");
    });

    it("handles non-agent processes", () => {
      const vim = parseAgentTitle("vim src/App.tsx");
      expect(vim?.isAgent).toBe(false);
      expect(vim?.name).toBe("vim src/App.tsx");
      // F9: Non-agent titles must not unconditionally report "working" state.
      // With no activity keywords or spinner signals, it receives neutral starting/idle state.
      expect(vim?.state).not.toBe("working");
      expect(vim?.state).toBe("starting");
    });

    it("does not classify mid-string agent keyword matches as agents (F2)", () => {
      const gitCommitCursor = parseAgentTitle('git commit -m "fix cursor position"');
      expect(gitCommitCursor?.isAgent).toBe(false);
      expect(gitCommitCursor?.agentType).toBe("terminal");
      expect(gitCommitCursor?.state).not.toBe("working");

      const gitLogPi = parseAgentTitle('git log --grep="pi calculation"');
      expect(gitLogPi?.isAgent).toBe(false);
      expect(gitLogPi?.agentType).toBe("terminal");

      const echoOmo = parseAgentTitle("echo omo");
      expect(echoOmo?.isAgent).toBe(false);
      expect(echoOmo?.agentType).toBe("terminal");

      const cargoClaude = parseAgentTitle('cargo run --bin claude-helper');
      expect(cargoClaude?.isAgent).toBe(false);
      expect(cargoClaude?.agentType).toBe("terminal");
    });

    it("still detects genuine leading-prefix agent titles", () => {
      const cursor = parseAgentTitle("cursor: fixing bug");
      expect(cursor?.isAgent).toBe(true);
      expect(cursor?.agentType).toBe("cursor");
      expect(cursor?.task).toBe("fixing bug");

      const pi = parseAgentTitle("pi: computing primes");
      expect(pi?.isAgent).toBe(true);
      expect(pi?.agentType).toBe("pi");
      expect(pi?.task).toBe("computing primes");
    });
  });

  describe("idle agent-name-only titles", () => {
    it("does not classify bare agent names as working", () => {
      expect(classifyTerminalTitleActivity("agy")).toBeNull();
      expect(classifyTerminalTitleActivity("antigravity")).toBeNull();
      expect(classifyTerminalTitleActivity("omo")).toBeNull();
      expect(classifyTerminalTitleActivity("claude code")).toBeNull();
    });

    it("reports a neutral state for an idle agent title", () => {
      const idle = parseAgentTitle("agy");
      expect(idle?.isAgent).toBe(true);
      expect(idle?.agentType).toBe("antigravity");
      expect(idle?.state).not.toBe("working");
    });
  });

  describe("tab label formatting", () => {
    it("formats clean tab labels from title", () => {
      expect(formatTabLabelFromTitle("⠋ omo: test suite", "main")).toBe("omo: test suite");
      expect(formatTabLabelFromTitle("⠹ agy: optimization pass", "main")).toBe("agy: optimization pass");
      expect(formatTabLabelFromTitle("✋ codex: needs input", "main")).toBe("codex: needs input");
      expect(formatTabLabelFromTitle("⏲ aider: fixing", "main")).toBe("aider: fixing");
      expect(formatTabLabelFromTitle("", "main")).toBe("main");
    });
  });
});
