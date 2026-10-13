import { describe, expect, it, vi } from "vitest";
import {
  isSessionMouseTrackingEnabled,
  openRemoteFileToken,
  readLinkLine,
  setSessionMouseTracking,
  tokenFromHyperlink,
  TERMINAL_FILE_LINK_ACTION_EVENT,
} from "./terminalLinkTarget";
import type { TerminalFileToken, TerminalInvokeFn } from "./terminalLinkTarget";

describe("terminalLinkTarget", () => {
  describe("tokenFromHyperlink", () => {
    it("parses http urls", () => {
      expect(tokenFromHyperlink("http://example.com/guide")).toEqual({
        type: "url",
        target: "http://example.com/guide",
      });
    });

    it("parses https urls", () => {
      expect(tokenFromHyperlink("https://ferryx.dev/docs?q=1#heading")).toEqual({
        type: "url",
        target: "https://ferryx.dev/docs?q=1#heading",
      });
    });

    it("parses file urls with empty host", () => {
      expect(tokenFromHyperlink("file:///usr/local/bin/ferryx")).toEqual({
        type: "file",
        path: "/usr/local/bin/ferryx",
        raw: "file:///usr/local/bin/ferryx",
      });
    });

    it("parses file urls with localhost", () => {
      expect(tokenFromHyperlink("file://localhost/home/user/code.ts")).toEqual({
        type: "file",
        path: "/home/user/code.ts",
        raw: "file://localhost/home/user/code.ts",
      });
    });

    it("returns null for file urls with foreign host", () => {
      expect(tokenFromHyperlink("file://remote-server/share/file.txt")).toBeNull();
      expect(tokenFromHyperlink("file://192.168.1.5/share/file.txt")).toBeNull();
    });

    it("returns null for non-http and non-file schemes", () => {
      expect(tokenFromHyperlink("mailto:dev@ferryx.dev")).toBeNull();
      expect(tokenFromHyperlink("ssh://user@host/path")).toBeNull();
      expect(tokenFromHyperlink("javascript:alert(1)")).toBeNull();
    });

    it("strips leading slash on Windows drive paths", () => {
      expect(tokenFromHyperlink("file:///C:/Users/name/repo/file.txt")).toEqual({
        type: "file",
        path: "C:/Users/name/repo/file.txt",
        raw: "file:///C:/Users/name/repo/file.txt",
      });
      expect(tokenFromHyperlink("file://localhost/D:/projects/ferryx/main.rs")).toEqual({
        type: "file",
        path: "D:/projects/ferryx/main.rs",
        raw: "file://localhost/D:/projects/ferryx/main.rs",
      });
    });

    it("decodes percent-encoded path segments", () => {
      expect(tokenFromHyperlink("file:///home/user/my%20folder/my%20file.txt")).toEqual({
        type: "file",
        path: "/home/user/my folder/my file.txt",
        raw: "file:///home/user/my%20folder/my%20file.txt",
      });
    });

    it("returns null on malformed uri strings", () => {
      expect(tokenFromHyperlink("not a url")).toBeNull();
      expect(tokenFromHyperlink("")).toBeNull();
      expect(tokenFromHyperlink("file://%")).toBeNull();
    });
  });

  describe("readLinkLine", () => {
    it("stitches rows when row0 fills cols and continues into row1", async () => {
      const rows = [
        { text: "https://exa", col: 0, row: 0 },
        { text: "mple.com/pa", col: 3, row: 1 },
        { text: "th-end", col: 0, row: 2 },
      ];
      const fakeInvoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        return rows[r];
      });

      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 3, 1, 3, 11);
      expect(result).not.toBeNull();
      expect(result?.text).toBe("https://example.com/path-end");
      expect(result?.col).toBe(3 + 11);
      expect(fakeInvoke).toHaveBeenCalledTimes(3);
    });

    it("ignores neighbour row failure and still stitches available rows", async () => {
      const fakeInvoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        if (r === 0) {
          throw new Error("Pty read error on row 0");
        }
        if (r === 1) {
          return { text: "current-line", col: 2, row: 1 };
        }
        if (r === 2) {
          return { text: "cont", col: 0, row: 2 };
        }
        throw new Error("Out of range");
      });

      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 2, 1, 3, 12);
      expect(result).not.toBeNull();
      expect(result?.text).toBe("current-linecont");
      expect(result?.col).toBe(2);
    });

    it("returns null when primary row query fails", async () => {
      const fakeInvoke = vi.fn(async () => {
        throw new Error("Primary row query failed");
      });

      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 0, 0, 10, 80);
      expect(result).toBeNull();
    });

    it("skips row - 1 when row is 0 and skips row + 1 when row is rows - 1", async () => {
      const queriedRows: number[] = [];
      const fakeInvoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        queriedRows.push(r);
        return { text: "single", col: 0, row: r };
      });

      await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 0, 0, 5, 80);
      expect(queriedRows).toEqual([0, 1]);

      queriedRows.length = 0;
      await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 0, 4, 5, 80);
      expect(queriedRows).toEqual([4, 3]);
    });

    it("returns current unstitched line when cols is not provided", async () => {
      const fakeInvoke = vi.fn(async () => ({ text: "no cols specified", col: 5, row: 2 }));
      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 5, 2);
      expect(result).toEqual({ text: "no cols specified", col: 5 });
      expect(fakeInvoke).toHaveBeenCalledTimes(1);
    });
  });

  describe("openRemoteFileToken", () => {
    it("fetches remote file and passes localPath to openLocal and forwards cwd", async () => {
      const token: TerminalFileToken = {
        type: "file",
        path: "/remote/path/to/src/index.ts",
        raw: "/remote/path/to/src/index.ts",
        line: 42,
        col: 10,
      };

      const fakeInvoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
        expect(cmd).toBe("cmd_remote_file_fetch");
        expect(args).toEqual({
          workspaceId: "ws-ssh-1",
          path: "/remote/path/to/src/index.ts",
          cwd: "/remote/path/to",
        });
        return {
          localPath: "/tmp/cache/index.ts",
          remotePath: "/remote/path/to/src/index.ts",
          byteLength: 1024,
        };
      });

      let localTokenPassed: TerminalFileToken | null = null;
      const openLocal = vi.fn(async (localToken: TerminalFileToken) => {
        localTokenPassed = localToken;
        return true;
      });

      const success = await openRemoteFileToken(token, {
        workspaceId: "ws-ssh-1",
        cwd: "/remote/path/to",
        shiftKey: true,
        openLocal,
        invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
      });

      expect(success).toBe(true);
      expect(openLocal).toHaveBeenCalledTimes(1);
      expect(localTokenPassed).toEqual({
        type: "file",
        path: "/tmp/cache/index.ts",
        raw: "/remote/path/to/src/index.ts",
        line: 42,
        col: 10,
      });
    });

    it("passes cwd as null when not specified", async () => {
      const token: TerminalFileToken = {
        type: "file",
        path: "/remote/file.txt",
        raw: "/remote/file.txt",
      };

      const fakeInvoke = vi.fn(async () => ({
        localPath: "/local/cache/file.txt",
        remotePath: "/remote/file.txt",
        byteLength: 50,
      }));

      const openLocal = vi.fn(async () => true);

      await openRemoteFileToken(token, {
        workspaceId: "ws-ssh-2",
        shiftKey: false,
        openLocal,
        invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
      });

      expect(fakeInvoke).toHaveBeenCalledWith("cmd_remote_file_fetch", {
        workspaceId: "ws-ssh-2",
        path: "/remote/file.txt",
        cwd: null,
      });
    });

    it("propagates fetch error when remote fetch rejects", async () => {
      const token: TerminalFileToken = {
        type: "file",
        path: "/remote/missing.txt",
        raw: "/remote/missing.txt",
      };

      const fakeInvoke = vi.fn(async () => {
        throw new Error("File not found on remote");
      });
      const openLocal = vi.fn(async () => true);

      await expect(
        openRemoteFileToken(token, {
          workspaceId: "ws-ssh-1",
          shiftKey: false,
          openLocal,
          invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
        }),
      ).rejects.toThrow("File not found on remote");

      expect(openLocal).not.toHaveBeenCalled();
    });
  });

  describe("mouse tracking helpers and event constants", () => {
    it("tracks session mouse tracking state", () => {
      expect(isSessionMouseTrackingEnabled("session-none")).toBe(false);
      expect(isSessionMouseTrackingEnabled(null)).toBe(false);

      setSessionMouseTracking("session-active", true);
      expect(isSessionMouseTrackingEnabled("session-active")).toBe(true);

      setSessionMouseTracking("session-active", false);
      expect(isSessionMouseTrackingEnabled("session-active")).toBe(false);
    });

    it("exports TERMINAL_FILE_LINK_ACTION_EVENT constant", () => {
      expect(TERMINAL_FILE_LINK_ACTION_EVENT).toBe("ferryx:terminal-file-link-actions");
    });
  });
});
