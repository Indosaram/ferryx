import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resolveFilePreviewPath } from "./filePreviewCommands";
import type { FilePreviewSource } from "./filePreviewTypes";
import {
  filePathToFileUrl,
  openTerminalToken,
  registerBuiltInBrowserLinkOpener,
  registerWorktreePathOpener,
} from "./linkRouting";
import { saveFileOpenTargets } from "./fileOpenTargets";
import { revealPath } from "./tauri";

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: vi.fn(async () => true),
}));

vi.mock("./filePreviewCommands", () => ({
  resolveFilePreviewPath: vi.fn(),
}));

vi.mock("./tauri", () => ({
  revealPath: vi.fn(async () => undefined),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn() },
}));

const source: FilePreviewSource = {
  leafId: "l",
  sessionId: "s",
  backendSessionId: "b",
  workspaceId: null,
};

describe("openTerminalToken file routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("routes HTML file to registered built-in browser opener and suppresses preview event", async () => {
    const opener = vi.fn();
    const unregister = registerBuiltInBrowserLinkOpener(opener);
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/a b/x.html",
        exists: true,
        isDirectory: false,
      });

      const res = await openTerminalToken(
        { type: "file", path: "/repo/a b/x.html", raw: "/repo/a b/x.html" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/a b/x.html", "b", null);
      expect(opener).toHaveBeenCalledTimes(1);
      expect(opener).toHaveBeenCalledWith("file:///repo/a%20b/x.html");
      expect(events).toHaveLength(0);
    } finally {
      unregister();
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("never hands a file fetched from an SSH or paired host to the built-in browser or the folder openers (R4-UI-1)", async () => {
    const opener = vi.fn();
    const worktreeOpener = vi.fn(() => true);
    const unregister = registerBuiltInBrowserLinkOpener(opener);
    const unregisterWorktree = registerWorktreePathOpener(worktreeOpener);
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      for (const workspaceId of ["ssh:host-a", "daemon:host-b"]) {
        events.length = 0;
        const remoteSource: FilePreviewSource = { ...source, workspaceId };
        const cached = "/cache/remote-files/abc/report.html";
        vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
          resolvedPath: cached,
          exists: true,
          isDirectory: false,
        });
        const res = await openTerminalToken(
          { type: "file", path: cached, raw: cached },
          { source: remoteSource },
        );
        expect(res).toBe(true);
        expect(opener).not.toHaveBeenCalled();
        expect(events).toHaveLength(1);
        expect(events[0]!.detail.request.path).toBe(cached);

        vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
          resolvedPath: "/cache/remote-files/abc",
          exists: true,
          isDirectory: true,
        });
        await openTerminalToken(
          { type: "file", path: "/cache/remote-files/abc", raw: "/cache/remote-files/abc" },
          { source: remoteSource },
        );
        expect(worktreeOpener).not.toHaveBeenCalled();
        expect(revealPath).not.toHaveBeenCalled();
      }
    } finally {
      unregister();
      unregisterWorktree();
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("reveals directory via revealPath and suppresses preview event", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/src",
        exists: true,
        isDirectory: true,
      });

      const res = await openTerminalToken(
        { type: "file", path: "/repo/src", raw: "/repo/src" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/src", "b", null);
      expect(revealPath).toHaveBeenCalledWith("/repo/src");
      expect(events).toHaveLength(0);
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("shows error toast when file is missing and suppresses preview event", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/missing.ts",
        exists: false,
        isDirectory: false,
      });

      const res = await openTerminalToken(
        { type: "file", path: "/repo/missing.ts", raw: "/repo/missing.ts" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/missing.ts", "b", null);
      expect(toast.error).toHaveBeenCalledWith("File not found: /repo/missing.ts");
      expect(events).toHaveLength(0);
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("dispatches open-file-preview event with detail.request.path equal to the resolved absolute path for regular file", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/src/main.ts",
        exists: true,
        isDirectory: false,
      });

      const res = await openTerminalToken(
        { type: "file", path: "src/main.ts", raw: "src/main.ts" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("src/main.ts", "b", null);
      expect(events).toHaveLength(1);
      expect(events[0]?.detail).toEqual({
        source,
        request: {
          path: "/repo/src/main.ts",
          backendSessionId: "b",
          line: null,
          col: null,
        },
      });
      expect(events[0]?.detail.request.path).toBe("/repo/src/main.ts");
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("keys preview requests by resolved absolute path so cwd changes and spellings do not collide", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath)
        .mockResolvedValueOnce({ resolvedPath: "/a/out.png", exists: true, isDirectory: false })
        .mockResolvedValueOnce({ resolvedPath: "/b/out.png", exists: true, isDirectory: false })
        .mockResolvedValueOnce({ resolvedPath: "/b/out.png", exists: true, isDirectory: false });

      await openTerminalToken({ type: "file", path: "out.png", line: 3, col: 7, raw: "out.png:3:7" }, { source });
      await openTerminalToken({ type: "file", path: "out.png", raw: "out.png" }, { source });
      await openTerminalToken({ type: "file", path: "./out.png", raw: "./out.png" }, { source });

      expect(events.map((event) => event.detail.request)).toEqual([
        { path: "/a/out.png", backendSessionId: "b", line: 3, col: 7 },
        { path: "/b/out.png", backendSessionId: "b", line: null, col: null },
        { path: "/b/out.png", backendSessionId: "b", line: null, col: null },
      ]);
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("dispatches open-file-preview event even when resolve rejects", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockRejectedValueOnce(new Error("Resolution failure"));

      const res = await openTerminalToken(
        { type: "file", path: "src/fallback.ts", raw: "src/fallback.ts" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("src/fallback.ts", "b", null);
      expect(events).toHaveLength(1);
      expect(events[0]?.detail).toEqual({
        source,
        request: {
          path: "src/fallback.ts",
          backendSessionId: "b",
          line: null,
          col: null,
        },
      });
      expect(events[0]?.detail.request.path).toBe("src/fallback.ts");
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("does not call resolveFilePreviewPath and invokes cmd_open_file_path when shiftKey is true", async () => {
    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      const res = await openTerminalToken(
        { type: "file", path: "src/main.ts", raw: "src/main.ts" },
        { source, shiftKey: true },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).not.toHaveBeenCalled();
      expect(invoke).toHaveBeenCalledWith("cmd_open_file_path", {
        path: "src/main.ts",
        cwd: undefined,
        sessionId: "b",
        workspaceId: null,
        editor: undefined,
        line: undefined,
        col: undefined,
      });
      expect(events).toHaveLength(0);
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
    }
  });

  it("does not dispatch ferryx:open-file-preview and takes external path when image target is external", async () => {
    saveFileOpenTargets({
      markdown: "in-app",
      image: "external",
      pdf: "in-app",
      media: "in-app",
      text: "in-app",
    });

    const events: CustomEvent[] = [];
    const onOpenFilePreview = (event: Event) => events.push(event as CustomEvent);
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/shot.png",
        exists: true,
        isDirectory: false,
      });

      const res = await openTerminalToken(
        { type: "file", path: "shot.png", raw: "shot.png" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("shot.png", "b", null);
      expect(events).toHaveLength(0);
      expect(invoke).toHaveBeenCalledWith("cmd_open_file_path", {
        path: "shot.png",
        cwd: undefined,
        sessionId: "b",
        workspaceId: null,
        editor: undefined,
        line: undefined,
        col: undefined,
      });
    } finally {
      window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
      localStorage.clear();
    }
  });

  it("passes the source workspaceId to the resolve and external-open invokes (RS-F1)", async () => {
    const sshSource: FilePreviewSource = { ...source, workspaceId: "ssh:0123abcd" };
    vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
      resolvedPath: "/srv/app/main.rs",
      exists: true,
      isDirectory: false,
    });

    await openTerminalToken(
      { type: "file", path: "src/main.rs", raw: "src/main.rs" },
      { source: sshSource, preview: true },
    );
    expect(resolveFilePreviewPath).toHaveBeenCalledWith("src/main.rs", "b", "ssh:0123abcd");

    await openTerminalToken(
      { type: "file", path: "src/main.rs", raw: "src/main.rs" },
      { source: sshSource, shiftKey: true },
    );
    expect(invoke).toHaveBeenCalledWith(
      "cmd_open_file_path",
      expect.objectContaining({ sessionId: "b", workspaceId: "ssh:0123abcd" }),
    );
  });
});

describe("filePathToFileUrl", () => {
  it("converts POSIX file path with spaces to file URL", () => {
    expect(filePathToFileUrl("/a b/x.html")).toBe("file:///a%20b/x.html");
  });

  it("converts Windows file path with backslashes and spaces to file URL", () => {
    expect(filePathToFileUrl("C:\\a b\\x.html")).toBe("file:///C:/a%20b/x.html");
  });
});

describe("openTerminalToken worktree root routing", () => {
  it("calls registered opener with resolvedPath and does not call revealPath when opener returns true", async () => {
    const opener = vi.fn().mockReturnValue(true);
    const unregister = registerWorktreePathOpener(opener);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/worktree-a",
        exists: true,
        isDirectory: true,
      });

      const res = await openTerminalToken(
        { type: "file", path: "/repo/worktree-a", raw: "/repo/worktree-a" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/worktree-a", "b", null);
      expect(opener).toHaveBeenCalledTimes(1);
      expect(opener).toHaveBeenCalledWith("/repo/worktree-a");
      expect(revealPath).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("calls revealPath when registered opener returns false", async () => {
    const opener = vi.fn().mockReturnValue(false);
    const unregister = registerWorktreePathOpener(opener);

    try {
      vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
        resolvedPath: "/repo/worktree-b",
        exists: true,
        isDirectory: true,
      });

      const res = await openTerminalToken(
        { type: "file", path: "/repo/worktree-b", raw: "/repo/worktree-b" },
        { source },
      );

      expect(res).toBe(true);
      expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/worktree-b", "b", null);
      expect(opener).toHaveBeenCalledTimes(1);
      expect(opener).toHaveBeenCalledWith("/repo/worktree-b");
      expect(revealPath).toHaveBeenCalledTimes(1);
      expect(revealPath).toHaveBeenCalledWith("/repo/worktree-b");
    } finally {
      unregister();
    }
  });

  it("calls revealPath when no opener is registered", async () => {
    vi.mocked(resolveFilePreviewPath).mockResolvedValueOnce({
      resolvedPath: "/repo/worktree-c",
      exists: true,
      isDirectory: true,
    });

    const res = await openTerminalToken(
      { type: "file", path: "/repo/worktree-c", raw: "/repo/worktree-c" },
      { source },
    );

    expect(res).toBe(true);
    expect(resolveFilePreviewPath).toHaveBeenCalledWith("/repo/worktree-c", "b", null);
    expect(revealPath).toHaveBeenCalledTimes(1);
    expect(revealPath).toHaveBeenCalledWith("/repo/worktree-c");
  });
});
