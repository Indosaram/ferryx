import { beforeEach, describe, expect, it, vi } from "vitest";
import { desktopChatIpcTransport } from "./desktopChatIpc";

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
});
