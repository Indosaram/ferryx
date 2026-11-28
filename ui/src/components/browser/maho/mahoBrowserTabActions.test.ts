import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBrowserStore } from "../../../state/browserStore";
import { activateMahoTab, suspendMahoTab } from "./mahoBrowserTabActions";

const ipc = vi.hoisted(() => ({ close: vi.fn(), create: vi.fn() }));
vi.mock("../../../lib/browserTauri", () => ({ closeBrowser: ipc.close, createBrowser: ipc.create }));

beforeEach(() => { ipc.close.mockReset(); ipc.create.mockReset(); });

describe("pinned native-session lifecycle", () => {
  it("waits for close before recreation, preserves identity, and activates only after creation", async () => {
    const store = createBrowserStore({ storage: null });
    store.upsertTab({ browserId: "pin", section: "pinned", url: "https://example.test", title: "Saved", profileId: "default" });
    let finishClose!: () => void;
    ipc.close.mockImplementation(() => new Promise<void>((resolve) => { finishClose = resolve; }));
    let finishCreate!: () => void;
    const createStarted = new Promise<void>((resolve) => {
      ipc.create.mockImplementation(() => { resolve(); return new Promise<void>((done) => { finishCreate = done; }); });
    });
    const onActivate = vi.fn();
    const unloading = suspendMahoTab(store, "pin");
    expect(store.getState().tabs.pin.suspended).toBe(true);
    const activating = activateMahoTab(store, "pin", onActivate);
    expect(ipc.create).not.toHaveBeenCalled();
    finishClose();
    await unloading;
    await createStarted;
    expect(ipc.create).toHaveBeenCalledWith({ browserId: "pin", url: "https://example.test", profile: "default", visible: false });
    expect(onActivate).not.toHaveBeenCalled();
    finishCreate();
    await activating;
    expect(store.getState().tabs.pin.suspended).toBe(false);
    expect(onActivate).toHaveBeenCalledWith("pin");
    expect(store.getState().archive).toHaveLength(0);
  });

  it("retains the suspended tab when creation fails and rolls back a failed close", async () => {
    const store = createBrowserStore({ storage: null });
    store.upsertTab({ browserId: "pin", section: "pinned", url: "https://example.test" });
    ipc.close.mockRejectedValueOnce(new Error("close failed"));
    await expect(suspendMahoTab(store, "pin")).rejects.toThrow("close failed");
    expect(store.getState().tabs.pin.suspended).toBe(false);
    store.setSuspended("pin", true);
    ipc.create.mockRejectedValueOnce(new Error("create failed"));
    await expect(activateMahoTab(store, "pin")).rejects.toThrow("create failed");
    expect(store.getState().tabs.pin.suspended).toBe(true);
  });
});
