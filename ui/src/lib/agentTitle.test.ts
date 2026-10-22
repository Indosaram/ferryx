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

    it("does not classify mid-sentence done or waiting keywords as done/waiting", () => {
      expect(classifyTerminalTitleActivity("\u280b omo: fixing the done-state handler")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b omo: checking permission handling")).toBe("working");
      expect(classifyTerminalTitleActivity("\u2733 claude code: done")).toBe("done");
      expect(classifyTerminalTitleActivity("\u2726 codex: needs input")).toBe("waiting");
    });

    it("still classifies status words that carry trailing detail", () => {
      expect(classifyTerminalTitleActivity("codex: done (3 files changed)")).toBe("done");
      expect(classifyTerminalTitleActivity("omo: completed in 4m12s")).toBe("done");
      expect(classifyTerminalTitleActivity("claude: finished 12 tasks")).toBe("done");
      expect(classifyTerminalTitleActivity("omo: permission required")).toBe("waiting");
      expect(classifyTerminalTitleActivity("claude: permission needed to edit file")).toBe("waiting");
      expect(classifyTerminalTitleActivity("omo: waiting for approval: user must confirm")).toBe("waiting");
    });

    it("lets a live spinner outrank a status word that merely opens the work description", () => {
      expect(classifyTerminalTitleActivity("\u280b omo: done tasks handler")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b omo: idle connection cleanup")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b codex: completed items migration")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b omo: waiting room refactor")).toBe("working");
    });

    it("still reports a real completion that ends the title even with a spinner present", () => {
      expect(classifyTerminalTitleActivity("\u2733 claude code: done")).toBe("done");
      expect(classifyTerminalTitleActivity("\u280b codex: run finished")).toBe("done");
      expect(classifyTerminalTitleActivity("\u280b omo: needs input")).toBe("waiting");
    });

    it("reads the status segment through an unrecognized leading decoration glyph", () => {
      expect(classifyTerminalTitleActivity("\u274b omo: permission required")).toBe("waiting");
      expect(classifyTerminalTitleActivity("\u25b8 codex: done (3 files changed)")).toBe("done");
    });

    it("keeps a status word embedded in work description out of done/waiting", () => {
      expect(classifyTerminalTitleActivity("\u280b omo: refactoring the completed-badge logic")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b omo: done-state handler under repair")).toBe("working");
      expect(classifyTerminalTitleActivity("\u280b claude: writing docs for the approval flow")).toBe("working");
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

  describe("GJC (gajae-code) agent parsing", () => {
    it("detects gjc titles with agent branding and task extraction", () => {
      const working = parseAgentTitle("⠋ gjc: refactoring the parser");
      expect(working?.isAgent).toBe(true);
      expect(working?.agentType).toBe("gjc");
      expect(working?.name).toBe("Gajae Code");
      expect(working?.task).toBe("refactoring the parser");
      expect(working?.state).toBe("working");

      const bare = parseAgentTitle("gjc");
      expect(bare?.isAgent).toBe(true);
      expect(bare?.agentType).toBe("gjc");
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

    // Measured tmux #{pane_title} values from omp 18.12.6 (.omo-parity/facts/remote-omp.md section 4).
    it("detects omp's 'π' titles and reads idle, working, and waiting from the marker", () => {
      const idle = parseAgentTitle("π > work");
      expect(idle?.isAgent).toBe(true);
      expect(idle?.agentType).toBe("omp");
      expect(idle?.name).toBe("OMP");
      expect(idle?.task).toBe("work");
      expect(idle?.state).toBe("starting");
      expect(classifyTerminalTitleActivity("π > Parity probe")).toBeNull();
      expect(parseAgentTitle("\x1b]0;π > Parity probe\x07")?.task).toBe("Parity probe");

      for (const title of ["π ⠼ work", "π ⠙ work"]) {
        const working = parseAgentTitle(title);
        expect(working?.agentType).toBe("omp");
        expect(working?.task).toBe("work");
        expect(working?.state).toBe("working");
      }

      for (const title of ["π ! work", "π ! Parity probe"]) {
        const waiting = parseAgentTitle(title);
        expect(waiting?.agentType).toBe("omp");
        expect(waiting?.state).toBe("waiting");
      }
    });

    // Measured OSC titles from .omo-parity/facts (F2 gap rows); each case fails without its matcher.
    it("detects opencode's titled-session form 'OC | <session title>'", () => {
      // facts/local.md:172 and facts/local-1.md:237
      for (const [title, task] of [
        ["OC | Create probe.txt with touch", "Create probe.txt with touch"],
        ["OC | Directory listing with ls", "Directory listing with ls"],
      ]) {
        const parsed = parseAgentTitle(title);
        expect(parsed?.isAgent).toBe(true);
        expect(parsed?.agentType).toBe("opencode");
        expect(parsed?.task).toBe(task);
        expect(parsed?.state).toBe("starting");
      }
      // facts/local.md:172 (idle, before the session has a title)
      expect(parseAgentTitle("OpenCode")?.agentType).toBe("opencode");
    });

    it("detects pi (senpi) 'senpi - ' titles while omo keeps its own titles", () => {
      // facts/local.md:184
      const idle = parseAgentTitle("senpi - repo-pi");
      expect(idle?.isAgent).toBe(true);
      expect(idle?.agentType).toBe("pi");
      expect(idle?.task).toBe("repo-pi");
      expect(idle?.state).toBe("starting");
      // facts/local-1.md:326-330
      expect(parseAgentTitle("senpi - Running eval")?.state).toBe("working");
      expect(parseAgentTitle("senpi - Create created.txt with touch - parity-pi.tqp0")?.agentType).toBe("pi");
      // facts/local-1.md:336: the omo run titles itself 'omo …'
      expect(parseAgentTitle("omo: refactoring layout components")?.agentType).toBe("omo");
    });

    it("detects grok's trailing ' - grok' titles with spinner and Action Required states", () => {
      // facts/remote-grok.md:100
      const idle = parseAgentTitle("PARITY_TOOL remove a file - grok");
      expect(idle?.isAgent).toBe(true);
      expect(idle?.agentType).toBe("grok");
      expect(idle?.state).toBe("starting");
      // facts/remote-grok.md:101-102
      for (const title of ["⠴ - Responding - grok", "⠹ - Responding - PARITY_TOOL remove a file - grok"]) {
        const working = parseAgentTitle(title);
        expect(working?.agentType).toBe("grok");
        expect(working?.state).toBe("working");
      }
      // facts/remote-grok.md:103
      const waiting = parseAgentTitle("⚠ Action Required - ⠴ - probe… - PARITY_TOOL remove a file - grok");
      expect(waiting?.agentType).toBe("grok");
      expect(waiting?.state).toBe("waiting");
    });

    it("detects droid's '⛬ Droid' title", () => {
      // facts/remote-droid.md:48
      const droid = parseAgentTitle("\x1b]0;⛬ Droid\x07");
      expect(droid?.isAgent).toBe(true);
      expect(droid?.agentType).toBe("droid");
      expect(droid?.name).toBe("Droid");
      expect(droid?.state).toBe("starting");
    });

    it("detects cline's 'Cline' title", () => {
      // facts/local.md:108
      const cline = parseAgentTitle("Cline");
      expect(cline?.isAgent).toBe(true);
      expect(cline?.agentType).toBe("cline");
      expect(cline?.name).toBe("Cline");
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
