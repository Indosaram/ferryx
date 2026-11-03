import { describe, expect, it, vi } from "vitest";
import {
  isLikelyRemoteFilePath,
  isSessionMouseTrackingEnabled,
  openRemoteFileToken,
  linkTokenSpans,
  readLinkLine,
  resolveLinkLine,
  remoteFileFetchErrorMessage,
  remoteFetchCwd,
  setSessionMouseTracking,
  tokenFromHyperlink,
  TERMINAL_FILE_LINK_ACTION_EVENT,
} from "./terminalLinkTarget";
import type { TerminalFileToken, TerminalInvokeFn } from "./terminalLinkTarget";

describe("terminalLinkTarget", () => {
  describe("isLikelyRemoteFilePath", () => {
    // Remote panes cannot stat the path locally, so prose tokens must not look like links.
    it("rejects short prose tokens that only happen to contain a slash", () => {
      for (const path of ["I/O", "n/a", "w/o", "and/or", "A/B", "TCP/IP", "yes/no", "24/7"]) {
        expect(isLikelyRemoteFilePath(path), path).toBe(false);
      }
    });

    it("accepts real file shapes", () => {
      for (const path of [
        "out/run.png",
        ".omo/evidence/ulw-neon-border/breathe-qa-release2/a.png",
        "/etc/hosts",
        "~/x",
        "./build",
        "../a",
        "src/lib/linkRouting.ts",
        "a/b/c",
        "src/lib",
        "bin/ls",
        "out/v2",
        "docs/API",
        "ui/src",
        "logs/2024",
        "package.json",
        "Makefile",
        "C:\\Users\\x.txt",
      ]) {
        expect(isLikelyRemoteFilePath(path), path).toBe(true);
      }
    });
  });

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

    it("accepts any authority for remote sessions and uses the absolute pathname", () => {
      expect(tokenFromHyperlink("file://remote-box/srv/app/out/run.png", { remote: true })).toEqual({
        type: "file",
        path: "/srv/app/out/run.png",
        raw: "file://remote-box/srv/app/out/run.png",
      });
      expect(tokenFromHyperlink("file://remote-box/srv/my%20dir/a.txt", { remote: true })).toMatchObject({
        type: "file",
        path: "/srv/my dir/a.txt",
      });
      expect(tokenFromHyperlink("file://remote-box/srv/a.txt", { remote: false })).toBeNull();
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
    const rowsInvoke = (rows: Array<{ text: string; wrapped?: boolean }>, clickedRow: number, clickedCol: number) =>
      vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        if (!rows[r]) throw new Error("Out of range");
        return { ...rows[r], col: r === clickedRow ? clickedCol : 0, row: r };
      });

    it("joins every row the terminal soft-wrapped, using its wrap flags", async () => {
      const rows = [
        { text: "$ echo", wrapped: false },
        { text: "https://exa", wrapped: true },
        { text: "mple.com/pa", wrapped: true },
        { text: "th-end     ", wrapped: false },
        { text: "next", wrapped: false },
      ];
      const invoke = rowsInvoke(rows, 2, 3);
      const line = await readLinkLine(invoke as unknown as TerminalInvokeFn, "session-1", 3, 2, 5, 11);
      expect(line?.text).toBe("https://example.com/path-end     ");
      expect(line?.col).toBe(11 + 3);
      expect(line?.exact).toBeUndefined();
      expect(await resolveLinkLine(line!)).toMatchObject({ token: { type: "url", target: "https://example.com/path-end" } });
    });

    it("treats a full row that ended with a newline only as a guess, never a terminal wrap", async () => {
      const rows = [
        { text: "src/aaaaaaa", wrapped: false },
        { text: "bbb.ts", wrapped: false },
      ];
      const line = await readLinkLine(rowsInvoke(rows, 1, 1) as unknown as TerminalInvokeFn, "session-1", 1, 1, 2, 11);
      expect(line?.exact?.text).toBe("bbb.ts");
      expect((await resolveLinkLine(line!, async () => false))?.token).toMatchObject({ type: "file", path: "bbb.ts" });
    });

    it("stitches full rows by shape when the backend sends no wrap flags", async () => {
      const rows = [{ text: "https://exa" }, { text: "mple.com/pa" }, { text: "th-end" }];
      const line = await readLinkLine(rowsInvoke(rows, 1, 3) as unknown as TerminalInvokeFn, "session-1", 3, 1, 3, 11);
      expect(line?.text).toBe("https://example.com/path-end");
      expect(line?.col).toBe(3 + 11);
    });

    it("joins a path a TUI wrapped with its own newline and keeps the exact line as fallback", async () => {
      const rows = [
        { text: "  /Users/indo/.aside/u/1/sessions/2026-11-02_sIAW3lPLLPzn4OMC/", wrapped: false },
        { text: "  qr-fresh.png", wrapped: false },
      ];
      const line = await readLinkLine(rowsInvoke(rows, 1, 4) as unknown as TerminalInvokeFn, "session-1", 4, 1, 2, 66);
      expect(line?.text).toBe("  /Users/indo/.aside/u/1/sessions/2026-11-02_sIAW3lPLLPzn4OMC/qr-fresh.png");
      expect(line?.exact?.text).toBe("  qr-fresh.png");

      const confirm = vi.fn(async (path: string) => path.endsWith("/qr-fresh.png"));
      const resolved = await resolveLinkLine(line!, confirm);
      expect(resolved?.token).toMatchObject({
        type: "file",
        path: "/Users/indo/.aside/u/1/sessions/2026-11-02_sIAW3lPLLPzn4OMC/qr-fresh.png",
      });
      expect(linkTokenSpans(resolved!)).toEqual([
        { row: 0, start: 2, end: 62 },
        { row: 1, start: 2, end: 14 },
      ]);
    });

    it("falls back to the exact line when the joined path does not exist", async () => {
      const rows = [
        { text: "see docs/very/long/directory/name/that/fills/the/row/", wrapped: false },
        { text: "notes.md in the repo", wrapped: false },
      ];
      const line = await readLinkLine(rowsInvoke(rows, 1, 2) as unknown as TerminalInvokeFn, "session-1", 2, 1, 2, 60);
      expect(line?.exact).toBeDefined();
      const resolved = await resolveLinkLine(line!, async () => false);
      expect(resolved?.token).toMatchObject({ type: "file", path: "notes.md" });
    });

    it("joins a URL a TUI broke mid-token across rows", async () => {
      const rows = [
        { text: "  https://github.com/Indosaram/ferryx/blob/main/src-tauri/src/native_ter", wrapped: false },
        { text: "  minal/selection.rs#L560 is the reader", wrapped: false },
      ];
      const line = await readLinkLine(rowsInvoke(rows, 1, 5) as unknown as TerminalInvokeFn, "session-1", 5, 1, 2, 80);
      expect(await resolveLinkLine(line!, async () => false)).toMatchObject({
        token: { type: "url", target: "https://github.com/Indosaram/ferryx/blob/main/src-tauri/src/native_terminal/selection.rs#L560" },
      });
    });

    it("ignores neighbour row failure and still reads the clicked row", async () => {
      const fakeInvoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        if (r === 1) return { text: "current-line", col: 2, row: 1, wrapped: false };
        throw new Error("Pty read error");
      });

      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 2, 1, 3, 12);
      expect(result?.text).toBe("current-line");
      expect(result?.col).toBe(2);
    });

    it("returns null when primary row query fails", async () => {
      const fakeInvoke = vi.fn(async () => {
        throw new Error("Primary row query failed");
      });

      const result = await readLinkLine(fakeInvoke as unknown as TerminalInvokeFn, "session-1", 0, 0, 10, 80);
      expect(result).toBeNull();
    });

    it("never reads rows outside the viewport", async () => {
      const queriedRows: number[] = [];
      const fakeInvoke = vi.fn(async (_cmd: string, args?: Record<string, unknown>) => {
        const r = args?.row as number;
        queriedRows.push(r);
        return { text: "single", col: 0, row: r, wrapped: false };
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
      expect(result).toMatchObject({ text: "no cols specified", col: 5 });
      expect(fakeInvoke).toHaveBeenCalledTimes(1);
    });
  });

  describe("openRemoteFileToken", () => {
    it("shares one in-flight fetch between repeated clicks on the same link", async () => {
      const token: TerminalFileToken = { type: "file", path: "/srv/big.bin", raw: "/srv/big.bin" };
      let finish: (value: unknown) => void = () => {};
      const fakeInvoke = vi.fn(
        () => new Promise((resolve) => { finish = resolve; }),
      );
      const openLocal = vi.fn(async () => true);
      const opts = {
        workspaceId: "ssh:dedupe",
        cwd: "/srv",
        shiftKey: false,
        openLocal,
        invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
      };
      const first = openRemoteFileToken(token, opts);
      const second = openRemoteFileToken(token, opts);
      expect(fakeInvoke).toHaveBeenCalledTimes(1);
      finish({ localPath: "/tmp/cache/big.bin", remotePath: "/srv/big.bin", byteLength: 1 });
      await expect(Promise.all([first, second])).resolves.toEqual([true, true]);
      expect(openLocal).toHaveBeenCalledTimes(2);

      const third = openRemoteFileToken(token, opts);
      expect(fakeInvoke).toHaveBeenCalledTimes(2);
      finish({ localPath: "/tmp/cache/big.bin", remotePath: "/srv/big.bin", byteLength: 1 });
      await third;
    });

    it("maps remote fetch reasons to user copy", () => {
      expect(remoteFetchCwd({ cwd: "/srv/app" })).toBe("/srv/app");
      // Never the worktree root: a relative token without a live cwd must be refused.
      expect(remoteFetchCwd({ cwd: "" })).toBeNull();
      expect(remoteFetchCwd({ cwd: null })).toBeNull();
      expect(remoteFetchCwd(null)).toBeNull();
      expect(remoteFileFetchErrorMessage({ details: { reason: "remoteFileTooLarge", maxBytes: 8 * 1024 * 1024 } }))
        .toBe("Remote files larger than 8 MiB cannot be opened.");
      expect(remoteFileFetchErrorMessage({ details: { reason: "remoteFileMissing" } })).toMatch(/does not exist/);
      expect(remoteFileFetchErrorMessage({ details: { reason: "remoteCwdUnknown" } })).toMatch(/current directory is unknown/);
      expect(remoteFileFetchErrorMessage({ message: "boom" })).toBeNull();
      expect(remoteFileFetchErrorMessage(null)).toBeNull();
    });

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
        freshRemoteSnapshot: true,
        remote: { workspaceId: "ws-ssh-1", path: "/remote/path/to/src/index.ts", sessionId: null },
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

    it("fetches a paired-host file through cmd_paired_file_fetch with the backend session id", async () => {
      const token: TerminalFileToken = {
        type: "file",
        path: "src/main.rs",
        raw: "src/main.rs:7",
        line: 7,
      };

      const fakeInvoke = vi.fn(async () => ({
        localPath: "/cache/paired-files/main.rs",
        remotePath: "/home/u/proj/src/main.rs",
        byteLength: 12,
      }));
      const openLocal = vi.fn(async (_localToken: TerminalFileToken) => true);

      const success = await openRemoteFileToken(token, {
        workspaceId: "daemon:host-1:proj",
        cwd: "/home/u/proj",
        sessionId: "daemon-session:host-1:abc",
        shiftKey: false,
        openLocal,
        invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
      });

      expect(success).toBe(true);
      expect(fakeInvoke).toHaveBeenCalledTimes(1);
      expect(fakeInvoke).toHaveBeenCalledWith("cmd_paired_file_fetch", {
        workspaceId: "daemon:host-1:proj",
        path: "src/main.rs",
        cwd: "/home/u/proj",
        sessionId: "daemon-session:host-1:abc",
      });
      expect(openLocal).toHaveBeenCalledWith({
        type: "file",
        path: "/cache/paired-files/main.rs",
        freshRemoteSnapshot: true,
        remote: { workspaceId: "daemon:host-1:proj", path: "/home/u/proj/src/main.rs", sessionId: "daemon-session:host-1:abc" },
        raw: "src/main.rs:7",
        line: 7,
      });
    });

    it("keeps ssh workspaces on cmd_remote_file_fetch without a session id", async () => {
      const token: TerminalFileToken = { type: "file", path: "a.txt", raw: "a.txt" };
      const fakeInvoke = vi.fn(async () => ({ localPath: "/cache/a.txt", remotePath: "/r/a.txt", byteLength: 1 }));

      await openRemoteFileToken(token, {
        workspaceId: "ssh:abc",
        cwd: "/r",
        sessionId: "backend-1",
        shiftKey: false,
        openLocal: vi.fn(async () => true),
        invokeFn: fakeInvoke as unknown as TerminalInvokeFn,
      });

      expect(fakeInvoke).toHaveBeenCalledWith("cmd_remote_file_fetch", {
        workspaceId: "ssh:abc",
        path: "a.txt",
        cwd: "/r",
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
