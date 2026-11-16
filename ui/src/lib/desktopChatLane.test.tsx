import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { desktopChatTransportFrom, type DesktopChatHandlers } from "./desktopChat";
import {
  DESKTOP_CHAT_FILE_CAPABILITY,
  useDesktopChat,
  type DesktopChatFileStaging,
} from "./desktopChatLane";
import { ReferenceFileError, referenceSha256Hex } from "../remote/chat/referenceFiles";
import type { ReferenceSubmitPayload } from "../remote/chat/referenceTypes";
import type { TerminalSession } from "./types";

let sequence = 0;
function setup(capabilities: readonly string[] = [DESKTOP_CHAT_FILE_CAPABILITY]) {
  sequence += 1;
  const session: TerminalSession = {
    id: `attachment-pane-${sequence}`, cwd: "/repo", workspaceId: "ws", worktree: null,
    backendSessionId: `attachment-backend-${sequence}`, lifecycle: "running",
    ownerId: "owner", daemonEpoch: "1", agentType: "claude",
  };
  let resolveSubmit!: (payload: ReferenceSubmitPayload) => void;
  const submitted = new Promise<ReferenceSubmitPayload>((resolve) => { resolveSubmit = resolve; });
  const submit = vi.fn<DesktopChatHandlers["submit"]>(async (request) => {
    resolveSubmit(request.params);
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
  });
  const bytes = new TextEncoder().encode("notes");
  const file = new File([bytes], "notes.txt", { type: "text/plain" });
  // jsdom File does not implement arrayBuffer in every supported version.
  Object.defineProperty(file, "arrayBuffer", { value: async () => bytes.slice().buffer });
  const stage = vi.fn<DesktopChatFileStaging["transport"]["stage"]>(async (target, payload) => ({
    receipt: {
      hostId: target.hostId, attachmentId: "att-notes", sha256: await referenceSha256Hex(bytes),
      sizeBytes: payload.sizeBytes, mediaType: payload.mediaType,
    },
    displayName: payload.name,
    mentionText: "@notes.txt ",
  }));
  const fileStaging: DesktopChatFileStaging = {
    capabilities,
    transport: { stage, cancel: vi.fn(async () => true), remove: vi.fn(async () => true) },
  };
  const hook = renderHook(() => useDesktopChat({
    session, identity: { hostId: "local" }, transport, visible: false, pollMs: 0, fileStaging,
  }));
  return { ...hook, file, stage, submit, submitted, fileStaging };
}

afterEach(() => { cleanup(); localStorage.clear(); });

describe("desktopChatLane staged file delivery", () => {
  it("stages one file, inserts its editable mention at the caret, and forwards it in submit text", async () => {
    const { result, file, stage, submitted } = setup();
    act(() => result.current.setDraft("Read this"));
    await act(async () => { await result.current.attachFiles([file], 5); });
    expect(stage).toHaveBeenCalledTimes(1);
    expect(result.current.stagedFiles).toHaveLength(1);
    expect(result.current.attachError).toBeNull();
    expect(result.current.draft).toBe("Read @notes.txt this");
    // Editing the draft must not cause the lane to append a second copy of the receipt.
    act(() => result.current.setDraft(`${result.current.draft} carefully`));
    const text = result.current.draft;
    await act(async () => {
      expect(result.current.send(text)).toBe(true);
      expect(await submitted).toEqual({ text, attachmentIds: [], origin: "chat" });
    });
    expect(result.current.stagedFiles).toHaveLength(0);
  });

  it("refuses a host without the upload capability with a typed error and sends nothing", async () => {
    const { result, file, stage, submit } = setup([]);
    act(() => result.current.setDraft("Read this"));
    await act(async () => { await result.current.attachFiles([file], 9); });
    expect(result.current.attachError).toBeInstanceOf(ReferenceFileError);
    expect(result.current.attachError?.code).toBe("UNSUPPORTED");
    expect(result.current.attachError?.message).toContain(DESKTOP_CHAT_FILE_CAPABILITY);
    expect(result.current.draft).toBe("Read this");
    expect(result.current.send(result.current.draft)).toBe(false);
    expect(stage).not.toHaveBeenCalled();
    expect(submit).not.toHaveBeenCalled();
  });

  it("keeps plain text free of mentions and attachment ids", async () => {
    const { result, stage, submitted } = setup();
    await act(async () => {
      expect(result.current.send("Plain text")).toBe(true);
      expect(await submitted).toEqual({ text: "Plain text", attachmentIds: [], origin: "chat" });
    });
    expect(stage).not.toHaveBeenCalled();
  });

  it("keeps the mention and receipt after a known failed submit without deleting the file", async () => {
    const { result, file, submit, fileStaging } = setup();
    let finish!: () => void;
    const completed = new Promise<void>((resolve) => { finish = resolve; });
    submit.mockImplementation(async () => {
      finish();
      return { ok: false, code: "FORBIDDEN", message: "View only" };
    });
    await act(async () => { await result.current.attachFiles([file], 0); });
    await act(async () => {
      expect(result.current.send(result.current.draft)).toBe(true);
      await completed;
    });
    expect(result.current.draft).toContain("@notes.txt");
    expect(result.current.stagedFiles).toHaveLength(1);
    expect(fileStaging.transport.remove).not.toHaveBeenCalled();
    expect(fileStaging.transport.cancel).not.toHaveBeenCalled();
  });
});
