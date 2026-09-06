import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const isTauri = vi.fn(() => true);
const toastInfo = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  isTauri: () => isTauri(),
}));
vi.mock("../components/ui/sonner", () => ({
  toast: { info: (...args: unknown[]) => toastInfo(...args) },
}));

function fakeStorage(initial: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
    clear: () => map.clear(),
    key: () => null,
    length: map.size,
  } as Storage;
}

async function freshModule() {
  vi.resetModules();
  return await import("./windowsStoreMigration");
}

function toastArgs(): { id: string; description: string; action: { onClick: () => void }; onDismiss: () => void } {
  expect(toastInfo).toHaveBeenCalledTimes(1);
  const [message, options] = toastInfo.mock.calls[0];
  expect(message).toContain("Microsoft Store");
  return options;
}

describe("windows store migration notice", () => {
  beforeEach(() => {
    invoke.mockReset();
    isTauri.mockReturnValue(true);
    toastInfo.mockReset();
  });

  it("shows the migration notice on installer-channel Windows installs", async () => {
    invoke.mockResolvedValue("installer");
    const migration = await freshModule();
    const storage = fakeStorage();

    await migration.maybeShowWindowsStoreMigrationNotice(storage);

    const options = toastArgs();
    expect(options.id).toBe("windows-store-migration");
    expect(options.description).toContain("reinstall Ferryx from the Microsoft Store");
  });

  it("stays silent on Store installs and other platforms", async () => {
    for (const channel of ["store", "native", ""]) {
      invoke.mockResolvedValue(channel);
      const migration = await freshModule();

      await migration.maybeShowWindowsStoreMigrationNotice(fakeStorage());

      expect(toastInfo).not.toHaveBeenCalled();
    }
  });

  it("stays silent once dismissed", async () => {
    invoke.mockResolvedValue("installer");
    const migration = await freshModule();
    const storage = fakeStorage({ "ferryx.windowsStoreMigration.dismissed": "1" });

    await migration.maybeShowWindowsStoreMigrationNotice(storage);

    expect(toastInfo).not.toHaveBeenCalled();
  });

  it("stays silent when the channel probe fails", async () => {
    invoke.mockRejectedValue(new Error("command not found"));
    const migration = await freshModule();

    await migration.maybeShowWindowsStoreMigrationNotice(fakeStorage());

    expect(toastInfo).not.toHaveBeenCalled();
  });

  it("persists the dismissal from the toast action and dismiss callback", async () => {
    invoke.mockResolvedValue("installer");
    const migration = await freshModule();
    const storage = fakeStorage();

    await migration.maybeShowWindowsStoreMigrationNotice(storage);
    const options = toastArgs();
    options.action.onClick();
    expect(loadFlag(storage)).toBe(true);

    toastInfo.mockClear();
    await migration.maybeShowWindowsStoreMigrationNotice(storage);
    expect(toastInfo).not.toHaveBeenCalled();

    function loadFlag(store: Storage): boolean {
      return store.getItem("ferryx.windowsStoreMigration.dismissed") === "1";
    }
  });
});
