import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invoke(command, args),
}));

Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, writable: true });

const { playNotificationSound } = await import("./tauri");

describe("cmd_notification_play_sound wire contract", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue({ played: true });
  });

  it("sends the path argument the Rust command requires", async () => {
    await playNotificationSound({
      soundId: "custom",
      customSoundPath: "/custom/bell.mp3",
      volume: 0.8,
      force: true,
    });

    const [command, args] = invoke.mock.calls[0];
    expect(command).toBe("cmd_notification_play_sound");
    expect(args).toHaveProperty("path", "/custom/bell.mp3");
  });

  it("does not invoke the player when no custom sound file is configured", async () => {
    const result = await playNotificationSound({
      soundId: "system",
      customSoundPath: null,
      volume: 0.8,
    });

    expect(invoke).not.toHaveBeenCalled();
    expect(result).toMatchObject({ played: false });
  });

  it("sends volume on the 0-100 scale the Rust command expects", async () => {
    await playNotificationSound({
      soundId: "custom",
      customSoundPath: "/custom/bell.mp3",
      volume: 0.8,
    });

    const [, args] = invoke.mock.calls[0];
    expect(args).toMatchObject({ volume: 80 });
  });
});
