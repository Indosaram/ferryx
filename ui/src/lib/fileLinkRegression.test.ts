import { describe, expect, it, vi } from "vitest";
import { resolveTokenAtCol, openTerminalToken } from "./linkRouting";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => true }));

describe("cross-platform file links", () => {
  it.each([
    ["src/한글.ts:12:3", "src/한글.ts"],
    ["C:\\작업\\파일.ts:12:3", "C:\\작업\\파일.ts"],
    ["\\\\server\\share\\file.ts:12:3", "\\\\server\\share\\file.ts"],
  ])("recognizes %s", (text, path) => {
    expect(resolveTokenAtCol(text, 0)).toMatchObject({ type: "file", path, line: 12, col: 3 });
  });

  it("does not activate whitespace after a token", () => {
    expect(resolveTokenAtCol("src/a.ts next", 8)).toBeNull();
  });

  it("propagates open errors to the visible click boundary", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: "INVALID_PATH", message: "Missing file" });
    await expect(openTerminalToken({ type: "file", path: "a.ts", raw: "a.ts" }))
      .rejects.toMatchObject({ code: "INVALID_PATH" });
  });
});
