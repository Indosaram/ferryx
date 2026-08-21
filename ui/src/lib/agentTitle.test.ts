import { describe, expect, it } from "vitest";

import { formatTabLabelFromTitle, normalizeTerminalTitle, parseAgentTitle } from "./agentTitle";

describe("agentTitle parser & normalizer", () => {
  it("normalizes terminal title strings by stripping control characters and trimming", () => {
    expect(normalizeTerminalTitle("  \x1b[0m\x07hello world  ")).toBe("hello world");
    expect(normalizeTerminalTitle("")).toBe("");
  });

  it("detects OMO agent as first-class default agent with working/waiting states", () => {
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
    expect(waiting?.agentType).toBe("omo");
    expect(waiting?.name).toBe("OMO");
    expect(waiting?.task).toBe("waiting for user confirmation");
    expect(waiting?.state).toBe("waiting");

    const spinnerOmo = parseAgentTitle("⠹ omo - reviewing diffs");
    expect(spinnerOmo).not.toBeNull();
    expect(spinnerOmo?.isAgent).toBe(true);
    expect(spinnerOmo?.agentType).toBe("omo");
    expect(spinnerOmo?.name).toBe("OMO");
    expect(spinnerOmo?.task).toBe("reviewing diffs");
    expect(spinnerOmo?.state).toBe("working");
  });

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
  });

  it("formats clean tab labels from title", () => {
    expect(formatTabLabelFromTitle("⠋ omo: test suite", "main")).toBe("omo: test suite");
    expect(formatTabLabelFromTitle("", "main")).toBe("main");
  });
});
