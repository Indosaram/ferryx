import { describe, expect, it } from "vitest";

import { scanTerminalOscTitles } from "./terminalEvents";

describe("scanTerminalOscTitles", () => {
  it("extracts OSC 0/1/2 titles terminated by BEL or ST", () => {
    expect(scanTerminalOscTitles("before\x1b]0;⠋ omo: working\x07after")).toEqual({
      titles: ["⠋ omo: working"],
      carry: "",
    });

    expect(scanTerminalOscTitles("\x1b]2;✋ codex: needs input\x1b\\")).toEqual({
      titles: ["✋ codex: needs input"],
      carry: "",
    });

    expect(scanTerminalOscTitles("\x1b]1;◇ claude done\x9c")).toEqual({
      titles: ["◇ claude done"],
      carry: "",
    });
  });

  it("carries OSC titles across arbitrary PTY output chunk boundaries", () => {
    const first = scanTerminalOscTitles("output\x1b]0;⠋ omo: buil");
    expect(first.titles).toEqual([]);
    expect(first.carry).toBe("\x1b]0;⠋ omo: buil");

    const second = scanTerminalOscTitles("ding indicators\x07rest", first.carry);
    expect(second).toEqual({
      titles: ["⠋ omo: building indicators"],
      carry: "",
    });
  });

  it("preserves a split OSC prefix and extracts multiple title updates in order", () => {
    const first = scanTerminalOscTitles("noise\x1b]2");
    expect(first).toEqual({ titles: [], carry: "\x1b]2" });

    const second = scanTerminalOscTitles(";✦ codex working\x07mid\x1b]0;✳ codex done\x07", first.carry);
    expect(second).toEqual({
      titles: ["✦ codex working", "✳ codex done"],
      carry: "",
    });
  });

  it("ignores non-title escape sequences and ordinary output", () => {
    expect(scanTerminalOscTitles("\x1b[31mred\x1b[0m zsh /repo")).toEqual({ titles: [], carry: "" });
  });
});