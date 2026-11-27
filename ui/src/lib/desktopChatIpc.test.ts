import { beforeEach, describe, expect, it, vi } from "vitest";
import { desktopChatFileStagingTransport, desktopChatIpcTransport } from "./desktopChatIpc";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ipc);

const target = { target: { hostId: "local", ownerId: "owner", epoch: "1", backendSessionId: "session" } };
const signal = new AbortController().signal;

describe("desktop chat IPC", () => {
  beforeEach(() => ipc.invoke.mockReset());

  it("forwards the exact target and unwraps a successful mutation receipt", async () => {
    ipc.invoke.mockResolvedValue({ status: 200, body: { ok: true, data: { receipt: "accepted" } } });
    const request = { kind: "stop" as const, requestId: "request", target, registryId: "claude",
      params: { capability: "providerInterrupt" as const } };
    expect(await desktopChatIpcTransport(request, signal)).toEqual({ ok: true, data: { receipt: "accepted" } });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_desktop_reference_chat", { request });
  });

  it("preserves an unknown mutation outcome instead of treating it as success", async () => {
    ipc.invoke.mockResolvedValue({ status: 409, body: { ok: false,
      error: { code: "OPERATION_OUTCOME_UNKNOWN", message: "delivery unknown" } } });
    const result = await desktopChatIpcTransport({ kind: "submit", requestId: "request", target, registryId: "claude",
      params: { text: "hello", attachmentIds: [], origin: "chat" } }, signal);
    expect(result).toEqual({ ok: false, code: "OPERATION_OUTCOME_UNKNOWN", message: "delivery unknown" });
    expect(ipc.invoke).toHaveBeenCalledTimes(1);
  });

  it("does not dispatch an already cancelled read", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(desktopChatIpcTransport({ kind: "prompt", requestId: "request", target, registryId: "claude", params: {} },
      controller.signal)).rejects.toThrow();
    expect(ipc.invoke).not.toHaveBeenCalled();
  });

  it("holds an untyped failed mutation as an unknown outcome", async () => {
    ipc.invoke.mockResolvedValue({ status: 502, body: null });
    const result = await desktopChatIpcTransport({ kind: "submit", requestId: "request", target, registryId: "claude",
      params: { text: "hello", attachmentIds: [], origin: "chat" } }, signal);
    expect(result).toMatchObject({ ok: false, code: "OPERATION_OUTCOME_UNKNOWN" });
    expect(ipc.invoke).toHaveBeenCalledTimes(1);
  });

  describe("file staging transport", () => {
    const stagePayload = {
      name: "notes.txt",
      mediaType: "text/plain" as const,
      sizeBytes: 4,
      contentBase64: "aGVsbG8=",
    };
    const receipt = {
      receipt: { hostId: "local", attachmentId: "att-1", sha256: "0".repeat(64), sizeBytes: 4, mediaType: "text/plain" },
      displayName: "notes.txt",
      mentionText: "@notes.txt ",
    };

    it("stages on the pane's own host through the file-stage command", async () => {
      ipc.invoke.mockResolvedValue({ status: 200, body: receipt });
      const staging = desktopChatFileStagingTransport("session");
      expect(await staging.stage(target.target, stagePayload, signal)).toEqual(receipt);
      expect(ipc.invoke).toHaveBeenCalledWith("cmd_desktop_reference_chat_file_stage", {
        sessionId: "session",
        params: stagePayload,
      });
    });

    it("maps a refused stage to a ReferenceFileError carrying the wire code", async () => {
      ipc.invoke.mockResolvedValue({ status: 413, body: { error: { code: "PAYLOAD_TOO_LARGE", message: "too big" } } });
      const staging = desktopChatFileStagingTransport("session");
      await expect(staging.stage(target.target, stagePayload, signal)).rejects.toMatchObject({
        name: "ReferenceFileError",
        code: "PAYLOAD_TOO_LARGE",
        message: "too big",
      });
    });

    it("refuses an already cancelled stage before invoking", async () => {
      const controller = new AbortController();
      controller.abort();
      const staging = desktopChatFileStagingTransport("session");
      await expect(staging.stage(target.target, stagePayload, controller.signal)).rejects.toThrow();
      expect(ipc.invoke).not.toHaveBeenCalled();
    });

    it("removes and cancels through the file-remove command and reports the host's confirmation", async () => {
      ipc.invoke.mockResolvedValue({ status: 200, body: { ok: true } });
      const staging = desktopChatFileStagingTransport("session");
      expect(await staging.remove(target.target, "att-1")).toBe(true);
      expect(await staging.cancel(target.target, "att-1")).toBe(true);
      expect(ipc.invoke.mock.calls[0]?.[1]).toEqual({ sessionId: "session", attachmentId: "att-1" });
      expect(ipc.invoke.mock.calls[1]?.[1]).toEqual({ sessionId: "session", attachmentId: "att-1" });

      ipc.invoke.mockResolvedValue({ status: 404, body: { error: { code: "NOT_FOUND", message: "gone" } } });
      expect(await staging.remove(target.target, "att-1")).toBe(false);
    });

    it("stages against the target's own backend session when it differs from the pane's", async () => {
      ipc.invoke.mockResolvedValue({ status: 200, body: receipt });
      const staging = desktopChatFileStagingTransport("pane-session");
      const other = { target: { ...target.target, backendSessionId: "target-session" } };
      await staging.stage(other.target, stagePayload, signal);
      expect(ipc.invoke.mock.calls[0]?.[1]).toEqual({ sessionId: "target-session", params: stagePayload });
    });
  });
});
