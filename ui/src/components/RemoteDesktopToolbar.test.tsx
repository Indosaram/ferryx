import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";

import {
  RD_AUDIO_MUTED_STORAGE_KEY,
  RD_AUDIO_VOLUME_STORAGE_KEY,
  RD_PLAYOUT_MODE_STORAGE_KEY,
  RD_SCALE_MODE_STORAGE_KEY,
  RD_STATS_OVERLAY_STORAGE_KEY,
} from "../lib/storageKeys";
import type { RdPlayoutMode, RdScaleMode } from "../lib/remoteDesktop";
import {
  RD_DEFAULT_PREFERENCES,
  RemoteDesktopToolbar,
  readRdPreferences,
  writeRdPreference,
  type RemoteDesktopToolbarProps,
} from "./RemoteDesktopToolbar";

type Handlers = {
  onPlayoutModeChange: Mock<(mode: RdPlayoutMode) => Promise<void> | void>;
  onScaleModeChange: Mock<(mode: RdScaleMode) => Promise<void> | void>;
  onAudioChange: Mock<(muted: boolean, volume: number) => Promise<void> | void>;
  onToggleStats: Mock<() => void>;
  onTogglePointerLock: Mock<() => void>;
};

function setup(overrides: Partial<RemoteDesktopToolbarProps> = {}) {
  const handlers: Handlers = {
    onPlayoutModeChange: vi.fn<(mode: RdPlayoutMode) => Promise<void> | void>(),
    onScaleModeChange: vi.fn<(mode: RdScaleMode) => Promise<void> | void>(),
    onAudioChange: vi.fn<(muted: boolean, volume: number) => Promise<void> | void>(),
    onToggleStats: vi.fn<() => void>(),
    onTogglePointerLock: vi.fn<() => void>(),
  };
  const props: RemoteDesktopToolbarProps = {
    rdSessionId: "rd-1",
    connected: true,
    playoutMode: "immediate",
    scaleMode: "fit",
    audioMuted: false,
    audioVolume: 1,
    statsOpen: false,
    pointerLocked: false,
    onPlayoutModeChange: handlers.onPlayoutModeChange,
    onScaleModeChange: handlers.onScaleModeChange,
    onAudioChange: handlers.onAudioChange,
    onToggleStats: handlers.onToggleStats,
    onTogglePointerLock: handlers.onTogglePointerLock,
    ...overrides,
  };
  const view = render(<RemoteDesktopToolbar {...props} />);
  return { ...handlers, props, ...view };
}

/**
 * Flushes the microtask queue so every promise an optimistic commit starts has settled. Three
 * hops cover `await onAudioChange()` -> write -> `await commit()` inside `runChange` -> catch.
 * No wall-clock delay is involved: the assertions only need the pending chains to drain.
 */
async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

/** Clicks and lets every promise the optimistic commit starts settle. */
async function clickAndSettle(element: HTMLElement) {
  fireEvent.click(element);
  await settle();
}

/** Presses a key on the volume thumb and lets the commit settle. */
async function keyAndSettle(element: HTMLElement, key: string) {
  fireEvent.keyDown(element, { key });
  await settle();
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  cleanup();
});

describe("RemoteDesktopToolbar chrome", () => {
  it("renders as a non-yielding top control strip", () => {
    setup();

    const toolbar = screen.getByTestId("remote-desktop-toolbar");
    expect(toolbar).toHaveAttribute("data-native-terminal-yield", "off");
    expect(toolbar).toHaveAttribute("role", "toolbar");
    expect(toolbar).toHaveAccessibleName("Remote desktop controls");
    expect(screen.getByRole("tablist", { name: "Playout preset" })).toBeInTheDocument();
    expect(screen.getByRole("tablist", { name: "Resolution scale" })).toBeInTheDocument();
  });

  it("carries no gamepad control", () => {
    setup();

    expect(screen.queryByLabelText(/gamepad/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /gamepad/i })).toBeNull();
  });

  it("shows a compact refresh badge only when a refresh rate is known", () => {
    const view = setup({ refreshHz: 75 });
    expect(screen.getByTestId("remote-desktop-refresh-badge")).toHaveTextContent("75 Hz");

    view.rerender(<RemoteDesktopToolbar {...view.props} refreshHz={null} />);
    expect(screen.queryByTestId("remote-desktop-refresh-badge")).toBeNull();
  });

  it("keeps a fractional refresh rate readable", () => {
    setup({ refreshHz: 74.5 });

    expect(screen.getByTestId("remote-desktop-refresh-badge")).toHaveTextContent("74.5 Hz");
  });

  it("disables the controls until a session is connected", () => {
    setup({ connected: false, rdSessionId: null });

    expect(screen.getByRole("tab", { name: "Competitive" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Mute audio" })).toBeDisabled();
    expect(screen.getByRole("slider")).toHaveAttribute("data-disabled");
  });
});

describe("RemoteDesktopToolbar control callbacks", () => {
  it("maps each playout preset to its typed mode and persists it", async () => {
    const { onPlayoutModeChange } = setup();

    await clickAndSettle(screen.getByRole("tab", { name: "Competitive" }));
    expect(onPlayoutModeChange).toHaveBeenCalledWith("latest_wins");
    expect(localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBe("latest_wins");

    await clickAndSettle(screen.getByRole("tab", { name: "Smooth" }));
    expect(onPlayoutModeChange).toHaveBeenLastCalledWith("adaptive");
    expect(localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBe("adaptive");

    await clickAndSettle(screen.getByRole("tab", { name: "Game" }));
    expect(onPlayoutModeChange).toHaveBeenLastCalledWith("immediate");
    expect(localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBe("immediate");
  });

  it("maps each resolution scale to its typed mode and persists it", async () => {
    const { onScaleModeChange } = setup();

    await clickAndSettle(screen.getByRole("tab", { name: "100%" }));
    expect(onScaleModeChange).toHaveBeenCalledWith("native100");

    await clickAndSettle(screen.getByRole("tab", { name: "75%" }));
    expect(onScaleModeChange).toHaveBeenLastCalledWith("scale75");
    expect(localStorage.getItem(RD_SCALE_MODE_STORAGE_KEY)).toBe("scale75");

    await clickAndSettle(screen.getByRole("tab", { name: "50%" }));
    expect(onScaleModeChange).toHaveBeenLastCalledWith("scale50");

    await clickAndSettle(screen.getByRole("tab", { name: "Fit" }));
    expect(onScaleModeChange).toHaveBeenLastCalledWith("fit");
    expect(localStorage.getItem(RD_SCALE_MODE_STORAGE_KEY)).toBe("fit");
  });

  it("does not re-issue the active preset", async () => {
    const { onPlayoutModeChange } = setup({ playoutMode: "adaptive" });

    await clickAndSettle(screen.getByRole("tab", { name: "Smooth" }));

    expect(onPlayoutModeChange).not.toHaveBeenCalled();
  });

  it("mutes and unmutes audio with the current 0..1 volume", async () => {
    const view = setup({ audioVolume: 0.5 });

    await clickAndSettle(screen.getByRole("button", { name: "Mute audio" }));
    expect(view.onAudioChange).toHaveBeenCalledWith(true, 0.5);
    expect(localStorage.getItem(RD_AUDIO_MUTED_STORAGE_KEY)).toBe("true");
    expect(localStorage.getItem(RD_AUDIO_VOLUME_STORAGE_KEY)).toBe("0.5");

    view.rerender(<RemoteDesktopToolbar {...view.props} audioMuted audioVolume={0.5} />);
    await clickAndSettle(screen.getByRole("button", { name: "Unmute audio" }));
    expect(view.onAudioChange).toHaveBeenLastCalledWith(false, 0.5);
    expect(localStorage.getItem(RD_AUDIO_MUTED_STORAGE_KEY)).toBe("false");
  });

  it("converts the 0..100 volume slider into a 0..1 volume", async () => {
    const { onAudioChange } = setup({ audioVolume: 0.5 });
    const slider = screen.getByRole("slider");

    expect(slider).toHaveAttribute("aria-valuemin", "0");
    expect(slider).toHaveAttribute("aria-valuemax", "100");
    expect(slider).toHaveAttribute("aria-valuenow", "50");

    await keyAndSettle(slider, "End");
    expect(onAudioChange).toHaveBeenCalledWith(false, 1);
    expect(localStorage.getItem(RD_AUDIO_VOLUME_STORAGE_KEY)).toBe("1");

    await keyAndSettle(screen.getByRole("slider"), "Home");
    expect(onAudioChange).toHaveBeenLastCalledWith(false, 0);
    expect(localStorage.getItem(RD_AUDIO_VOLUME_STORAGE_KEY)).toBe("0");

    // One keyboard step is one percent, and one commit: no fractional drift in the IPC payload.
    await keyAndSettle(screen.getByRole("slider"), "ArrowRight");
    expect(onAudioChange).toHaveBeenLastCalledWith(false, 0.01);
  });

  it("toggles the stats overlay and persists the choice", async () => {
    const { onToggleStats } = setup();

    await clickAndSettle(screen.getByRole("button", { name: "Toggle stats overlay" }));

    expect(onToggleStats).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(RD_STATS_OVERLAY_STORAGE_KEY)).toBe("true");
  });

  it("reflects an open stats overlay with aria-pressed", () => {
    setup({ statsOpen: true });

    expect(screen.getByRole("button", { name: "Toggle stats overlay" })).toHaveAttribute("aria-pressed", "true");
  });

  it("toggles pointer lock and reflects its state with aria-pressed", async () => {
    const { onTogglePointerLock } = setup({ pointerLocked: true });
    const lock = screen.getByRole("button", { name: "Toggle pointer lock" });

    expect(lock).toHaveAttribute("aria-pressed", "true");
    await clickAndSettle(lock);

    expect(onTogglePointerLock).toHaveBeenCalledTimes(1);
  });

  it("blurs a clicked control so the streamed desktop keeps the keyboard", async () => {
    setup();
    const tab = screen.getByRole("tab", { name: "Competitive" });
    tab.focus();
    expect(tab).toHaveFocus();

    await clickAndSettle(tab);
    expect(tab).not.toHaveFocus();

    const stats = screen.getByRole("button", { name: "Toggle stats overlay" });
    stats.focus();
    expect(stats).toHaveFocus();

    await clickAndSettle(stats);
    expect(stats).not.toHaveFocus();
  });
});

describe("RemoteDesktopToolbar persistence", () => {
  it("round-trips every preference through the canonical ferryx.rd keys", () => {
    writeRdPreference("playoutMode", "latest_wins");
    writeRdPreference("scaleMode", "scale50");
    writeRdPreference("audioMuted", true);
    writeRdPreference("audioVolume", 0.25);
    writeRdPreference("statsOpen", true);

    expect(localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBe("latest_wins");
    expect(localStorage.getItem(RD_SCALE_MODE_STORAGE_KEY)).toBe("scale50");
    expect(localStorage.getItem(RD_AUDIO_MUTED_STORAGE_KEY)).toBe("true");
    expect(localStorage.getItem(RD_AUDIO_VOLUME_STORAGE_KEY)).toBe("0.25");
    expect(localStorage.getItem(RD_STATS_OVERLAY_STORAGE_KEY)).toBe("true");

    expect(readRdPreferences()).toEqual({
      playoutMode: "latest_wins",
      scaleMode: "scale50",
      audioMuted: true,
      audioVolume: 0.25,
      statsOpen: true,
    });
  });

  it("restores stored preferences on a fresh mount", async () => {
    writeRdPreference("playoutMode", "adaptive");
    writeRdPreference("scaleMode", "scale75");
    writeRdPreference("audioMuted", true);
    writeRdPreference("audioVolume", 0.25);

    setup(readRdPreferences());
    expect(screen.getByRole("tab", { name: "Smooth" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "75%" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Unmute audio" })).toBeInTheDocument();
    expect(screen.getByTestId("remote-desktop-volume-readout")).toHaveTextContent("25%");

    // A change made here is what the next mount reads back.
    await clickAndSettle(screen.getByRole("tab", { name: "50%" }));
    cleanup();

    setup(readRdPreferences());
    expect(screen.getByRole("tab", { name: "50%" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Smooth" })).toHaveAttribute("aria-selected", "true");
  });

  it("falls back to immediate, fit, unmuted, full volume for unusable values", () => {
    localStorage.setItem(RD_PLAYOUT_MODE_STORAGE_KEY, "turbo");
    localStorage.setItem(RD_SCALE_MODE_STORAGE_KEY, "retina2x");
    localStorage.setItem(RD_AUDIO_MUTED_STORAGE_KEY, "maybe");
    localStorage.setItem(RD_AUDIO_VOLUME_STORAGE_KEY, "loud");
    localStorage.setItem(RD_STATS_OVERLAY_STORAGE_KEY, "sometimes");

    expect(readRdPreferences()).toEqual(RD_DEFAULT_PREFERENCES);
    expect(RD_DEFAULT_PREFERENCES).toEqual({
      playoutMode: "immediate",
      scaleMode: "fit",
      audioMuted: false,
      audioVolume: 1,
      statsOpen: false,
    });

    setup(readRdPreferences());
    expect(screen.getByRole("tab", { name: "Game" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Fit" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: "Mute audio" })).toBeInTheDocument();
    expect(screen.getByTestId("remote-desktop-volume-readout")).toHaveTextContent("100%");
  });

  it("clamps a stored volume that is out of range", () => {
    localStorage.setItem(RD_AUDIO_VOLUME_STORAGE_KEY, "4");
    expect(readRdPreferences().audioVolume).toBe(1);

    localStorage.setItem(RD_AUDIO_VOLUME_STORAGE_KEY, "-2");
    expect(readRdPreferences().audioVolume).toBe(0);
  });

  it("survives a localStorage area that throws", () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage denied");
    });
    expect(readRdPreferences()).toEqual(RD_DEFAULT_PREFERENCES);

    getItem.mockRestore();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    expect(() => writeRdPreference("playoutMode", "adaptive")).not.toThrow();
  });
});

describe("RemoteDesktopToolbar rejected changes", () => {
  it("rolls the playout preset back and reports it without throwing", async () => {
    const { onPlayoutModeChange } = setup();
    onPlayoutModeChange.mockRejectedValueOnce(new Error("RD_NOT_FOUND"));

    await clickAndSettle(screen.getByRole("tab", { name: "Competitive" }));

    expect(onPlayoutModeChange).toHaveBeenCalledWith("latest_wins");
    expect(screen.getByRole("tab", { name: "Game" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Competitive" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByRole("status")).toHaveTextContent("Playout preset could not be applied");
    // A rejected change must not be remembered as if it had been applied.
    expect(localStorage.getItem(RD_PLAYOUT_MODE_STORAGE_KEY)).toBeNull();
    expect(screen.getByTestId("remote-desktop-toolbar")).toBeInTheDocument();
  });

  it("rolls the resolution scale back and reports it", async () => {
    const { onScaleModeChange } = setup();
    onScaleModeChange.mockRejectedValueOnce(new Error("RD_BUSY"));

    await clickAndSettle(screen.getByRole("tab", { name: "50%" }));

    expect(screen.getByRole("tab", { name: "Fit" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "50%" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByRole("status")).toHaveTextContent("Resolution scale could not be applied");
    expect(localStorage.getItem(RD_SCALE_MODE_STORAGE_KEY)).toBeNull();
  });

  it("rolls the audio setting back and reports it", async () => {
    const { onAudioChange } = setup({ audioVolume: 0.4 });
    onAudioChange.mockRejectedValueOnce(new Error("RD_AUDIO_UNAVAILABLE"));

    await clickAndSettle(screen.getByRole("button", { name: "Mute audio" }));

    expect(onAudioChange).toHaveBeenCalledWith(true, 0.4);
    expect(screen.getByRole("button", { name: "Mute audio" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("status")).toHaveTextContent("Audio setting could not be applied");
    expect(localStorage.getItem(RD_AUDIO_MUTED_STORAGE_KEY)).toBeNull();
  });

  it("clears the warning once a later change succeeds", async () => {
    const { onPlayoutModeChange } = setup();
    onPlayoutModeChange.mockRejectedValueOnce(new Error("RD_BUSY"));

    await clickAndSettle(screen.getByRole("tab", { name: "Competitive" }));
    expect(screen.getByRole("status")).toBeInTheDocument();

    await clickAndSettle(screen.getByRole("tab", { name: "Smooth" }));
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByRole("tab", { name: "Smooth" })).toHaveAttribute("aria-selected", "true");
  });
});
