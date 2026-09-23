import { describe, expect, it } from "vitest";

import { isAbsoluteTerminalCwd } from "./terminalCwd";

describe("isAbsoluteTerminalCwd", () => {
  it("rejects probe output that is not a path", () => {
    expect(isAbsoluteTerminalCwd("cwd|rtd info error: No such file or directory")).toBe(false);
    expect(isAbsoluteTerminalCwd("relative/path")).toBe(false);
    expect(isAbsoluteTerminalCwd("")).toBe(false);
    expect(isAbsoluteTerminalCwd(null)).toBe(false);
    expect(isAbsoluteTerminalCwd(undefined)).toBe(false);
    expect(isAbsoluteTerminalCwd("/tmp/with\nnewline")).toBe(false);
  });

  it("accepts platform absolute paths", () => {
    expect(isAbsoluteTerminalCwd("/Volumes/T9-Mac/project/ferryx")).toBe(true);
    expect(isAbsoluteTerminalCwd("C:\\Users\\sook\\work")).toBe(true);
    expect(isAbsoluteTerminalCwd("\\\\host\\share")).toBe(true);
  });
});
