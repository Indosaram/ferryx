// @vitest-environment jsdom
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { act, cleanup, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { desktopChatTransportFrom, type DesktopChatHandlers } from "./desktopChat";
import {
  DESKTOP_CHAT_FILE_CAPABILITY,
  useDesktopChat,
  type DesktopChatFileStaging,
} from "./desktopChatLane";
import { referenceSha256Hex } from "../remote/chat/referenceFiles";
import type { ReferenceSubmitPayload } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "./types";

describe("desktopChatLane disk-backed attachment integration", () => {
  it("stages one session file with its on-disk size and digest, then submits its path mention", async () => {
    const dir = await mkdtemp(join(tmpdir(), "desktop-chat-attachment-"));
    try {
      const name = "session-notes.txt";
      const sourcePath = join(dir, name);
      await writeFile(sourcePath, "Session attachment fixture\nUTF-8 bytes: \u00e9\n", "utf8");
      const expectedBytes = await readFile(sourcePath);
      const expectedStat = await stat(sourcePath);
      // Independent Node digest: do not use the staging helper as the assertion oracle.
      const expectedSha256 = createHash("sha256").update(expectedBytes).digest("hex");
      const session: TerminalSession = {
        id: dir, cwd: dir, workspaceId: "attachment-integration", worktree: null,
        backendSessionId: dir, lifecycle: "running", ownerId: "owner",
        daemonEpoch: "1", agentType: "claude",
      };
      let captured: ReferenceSubmitPayload | undefined;
      const submit = vi.fn<DesktopChatHandlers["submit"]>(async (request) => {
        captured = request.params;
        return { ok: true, data: null };
      });
      const transport = desktopChatTransportFrom({
        history: async () => ({ ok: true, data: {
          source: "claude-transcript", availability: "native", turns: [], hasMore: false, generation: "g1",
        } }),
        prompt: async () => ({ ok: true, data: { prompt: null, screenRevision: null } }),
        submit,
        stop: async () => ({ ok: true, data: null }),
        answer: async () => ({ ok: true, data: null }),
        resolvePrompt: async () => ({ ok: true, data: null }),
      });
      const stage = vi.fn<DesktopChatFileStaging["transport"]["stage"]>(async (target, payload) => {
        // The target/session stub reads the real file, rather than echoing upload metadata.
        const diskBytes = await readFile(join(session.cwd, payload.name));
        expect(Buffer.from(payload.contentBase64, "base64")).toEqual(diskBytes);
        return {
          receipt: {
            hostId: target.hostId,
            attachmentId: "att-session-notes",
            sha256: await referenceSha256Hex(diskBytes),
            sizeBytes: diskBytes.byteLength,
            mediaType: payload.mediaType,
          },
          displayName: payload.name,
          mentionText: `@${payload.name} `,
        };
      });
      const fileStaging: DesktopChatFileStaging = {
        capabilities: [DESKTOP_CHAT_FILE_CAPABILITY],
        transport: { stage, cancel: vi.fn(async () => true), remove: vi.fn(async () => true) },
      };
      const file = new File([Uint8Array.from(expectedBytes)], name, { type: "text/plain" });
      // jsdom File lacks arrayBuffer in some versions; keep its source disk-backed too.
      Object.defineProperty(file, "arrayBuffer", {
        value: async () => Uint8Array.from(await readFile(sourcePath)).buffer,
      });
      const { result } = renderHook(() => useDesktopChat({
        session, identity: { hostId: "local" }, transport, visible: false, pollMs: 0, fileStaging,
      }));
      act(() => result.current.setDraft("Read this file"));
      await act(async () => { await result.current.attachFiles([file], result.current.draft.length); });
      expect(result.current.attachError).toBeNull();
      expect(stage).toHaveBeenCalledTimes(1);
      expect(result.current.stagedFiles).toHaveLength(1);
      const staged = result.current.stagedFiles[0];
      expect(expectedStat.size).toBe(expectedBytes.byteLength);
      expect(staged.receipt).toEqual({
        hostId: "local", attachmentId: "att-session-notes", mediaType: "text/plain",
        sizeBytes: expectedStat.size, sha256: expectedSha256,
      });
      expect(staged.mentionText).toBe(`@${name} `);
      const text = result.current.draft.trim();
      await act(async () => {
        expect(result.current.send(result.current.draft)).toBe(true);
        await vi.waitFor(() => {
          expect(captured).toBeDefined();
        }, { timeout: 1000, interval: 10 });
      });
      expect(submit).toHaveBeenCalledTimes(1);
      expect(captured?.text).toContain(staged.mentionText.trimEnd());
      // Receipts stay on the host; the submit lane carries their editable mention, not ids.
      expect(captured).toEqual({ text, attachmentIds: [], origin: "chat" });
      expect(result.current.stagedFiles).toHaveLength(0);
    } finally {
      try {
        cleanup();
        localStorage.clear();
      } finally {
        await rm(dir, { recursive: true, force: true });
      }
    }
  });
});
