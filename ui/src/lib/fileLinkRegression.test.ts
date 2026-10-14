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

  it("sends the paired workspaceId with an external open so the backend can refuse it (RS-F1)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(true);
    await openTerminalToken(
      { type: "file", path: "/etc/hosts", raw: "/etc/hosts" },
      {
        shiftKey: true,
        source: { leafId: "l", sessionId: "s", backendSessionId: "b", workspaceId: "daemon:host-1/ws" },
      },
    );
    expect(invoke).toHaveBeenLastCalledWith(
      "cmd_open_file_path",
      expect.objectContaining({ path: "/etc/hosts", sessionId: "b", workspaceId: "daemon:host-1/ws" }),
    );
  });
});

function tokenAt(line: string, needle: string) {
  const col = line.indexOf(needle);
  if (col < 0) throw new Error(`needle ${needle} not in ${line}`);
  return resolveTokenAtCol(line, col);
}

describe("dot-directory path segments (F1-1)", () => {
  it.each([
    ["saved .omo/evidence/ulw-neon-border/breathe-qa-release2/shot-01.png", "shot-01", ".omo/evidence/ulw-neon-border/breathe-qa-release2/shot-01.png", undefined],
    ["saved .omo/evidence/ulw-neon-border/breathe-qa-release2/shot-01.png", ".omo", ".omo/evidence/ulw-neon-border/breathe-qa-release2/shot-01.png", undefined],
    ["at .github/workflows/ci.yml:12", "ci.yml", ".github/workflows/ci.yml", 12],
    ["edit .config/x/y.toml now", "y.toml", ".config/x/y.toml", undefined],
    ["(.config/x/y.toml)", "y.toml", ".config/x/y.toml", undefined],
  ])("keeps the leading dot in %s", (line, needle, path, lineNo) => {
    expect(tokenAt(line, needle)).toMatchObject({ type: "file", path, line: lineNo });
  });

  it.each([
    ["run ./x.sh", "x.sh", "./x.sh"],
    ["up ../y/z.ts:3", "z.ts", "../y/z.ts"],
    ["wait...src/a.ts", "a.ts", "src/a.ts"],
    ["see src/a.ts.", "a.ts", "src/a.ts"],
  ])("keeps existing relative and sentence boundaries for %s", (line, needle, path) => {
    expect(tokenAt(line, needle)).toMatchObject({ type: "file", path });
  });

  it.each([
    ["Loading... done", "..."],
    ["server 10.0.0.1:8080", "10.0"],
    ["version 1.2.3", "1.2"],
  ])("does not turn %s into a file token", (line, needle) => {
    expect(tokenAt(line, needle)).toBeNull();
  });

  it("still resolves URLs before paths", () => {
    expect(tokenAt("see https://example.com/.well-known/x.json", "x.json"))
      .toEqual({ type: "url", target: "https://example.com/.well-known/x.json" });
  });
});

describe("'@' inside path segments (F1-2)", () => {
  it.each([
    ["Wrote ./breathe-qa-release2/x@2x.png", "x@2x", "./breathe-qa-release2/x@2x.png"],
    ["icons/logo@3x.png:4", "logo", "icons/logo@3x.png"],
    ["wrote logo@2x.png", "logo", "logo@2x.png"],
  ])("matches %s", (line, needle, path) => {
    expect(tokenAt(line, needle)).toMatchObject({ type: "file", path });
  });

  it.each([
    ["git clone user@host:/srv/repo.git", "srv"],
    ["git clone user@host:/srv/repo.git", "user"],
    ["remote git@github.com:org/repo.git", "org"],
  ])("does not make an scp spec in %s a local file token", (line, needle) => {
    expect(tokenAt(line, needle)).toBeNull();
  });
});

describe("trailing sentence punctuation (A-5)", () => {
  it.each([
    ["Saved screenshot to out/shots/foo.png.", "foo", "out/shots/foo.png", undefined, undefined],
    ["Saved to out/shots/foo.png, then exit", "foo", "out/shots/foo.png", undefined, undefined],
    ["Check .omo/notes/a.md!", "a.md", ".omo/notes/a.md", undefined, undefined],
    ["edit package.json.", "package", "package.json", undefined, undefined],
    ["at a/b:12:3.", "a/b", "a/b", 12, 3],
    ["at a/b:12:3", "a/b", "a/b", 12, 3],
  ])("strips punctuation in %s", (line, needle, path, lineNo, colNo) => {
    expect(tokenAt(line, needle)).toMatchObject({ type: "file", path, line: lineNo, col: colNo });
  });

  it("keeps pure dot segments such as ../..", () => {
    expect(tokenAt("cd ../..", "..")).toMatchObject({ type: "file", path: "../.." });
  });
});
