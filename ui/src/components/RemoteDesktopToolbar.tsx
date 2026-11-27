import { useEffect, useRef, useState } from "react";
import * as SliderPrimitive from "@radix-ui/react-slider";
import { Activity, MousePointer2, Volume2, VolumeX } from "lucide-react";

import { cn } from "../lib/cn";
import type { RdPlayoutMode, RdScaleMode } from "../lib/remoteDesktop";
import {
  RD_AUDIO_MUTED_STORAGE_KEY,
  RD_AUDIO_VOLUME_STORAGE_KEY,
  RD_PLAYOUT_MODE_STORAGE_KEY,
  RD_SCALE_MODE_STORAGE_KEY,
  RD_STATS_OVERLAY_STORAGE_KEY,
} from "../lib/storageKeys";
import { Button } from "./ui/button";
import { SegmentedControl, type SegmentedControlOption } from "./ui/SegmentedControl";

/** Everything the viewer remembers between sessions, under the canonical `ferryx.rd.*` keys. */
export type RdPreferences = {
  playoutMode: RdPlayoutMode;
  scaleMode: RdScaleMode;
  audioMuted: boolean;
  audioVolume: number;
  statsOpen: boolean;
};

/**
 * First-run values, and the fallback for anything unreadable. `immediate` + `fit` + unmuted at full
 * volume is the lowest-latency, least surprising viewer: a stale or hand-edited storage entry must
 * never leave the stream in a preset the user cannot see they picked.
 */
export const RD_DEFAULT_PREFERENCES: RdPreferences = {
  playoutMode: "immediate",
  scaleMode: "fit",
  audioMuted: false,
  audioVolume: 1,
  statsOpen: false,
};

const PLAYOUT_MODES: readonly RdPlayoutMode[] = ["immediate", "latest_wins", "adaptive"];
const SCALE_MODES: readonly RdScaleMode[] = ["fit", "native100", "scale75", "scale50"];

const PREFERENCE_KEYS: Record<keyof RdPreferences, string> = {
  playoutMode: RD_PLAYOUT_MODE_STORAGE_KEY,
  scaleMode: RD_SCALE_MODE_STORAGE_KEY,
  audioMuted: RD_AUDIO_MUTED_STORAGE_KEY,
  audioVolume: RD_AUDIO_VOLUME_STORAGE_KEY,
  statsOpen: RD_STATS_OVERLAY_STORAGE_KEY,
};

/**
 * `window.localStorage` itself throws on property access when the browser denies storage (Safari
 * private mode), so even reaching for it needs a guard: a denied area must cost the persistence,
 * never the toolbar.
 */
function preferenceStorage(): Storage | null {
  try {
    if (typeof window === "undefined") return null;
    return window.localStorage ?? null;
  } catch {
    return null;
  }
}

function readStoredPreference(key: string): string | null {
  const storage = preferenceStorage();
  if (!storage) return null;
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

function isPlayoutMode(value: string | null): value is RdPlayoutMode {
  return value !== null && (PLAYOUT_MODES as readonly string[]).includes(value);
}

function isScaleMode(value: string | null): value is RdScaleMode {
  return value !== null && (SCALE_MODES as readonly string[]).includes(value);
}

function parseStoredBoolean(value: string | null): boolean | null {
  if (value === "true" || value === "1") return true;
  if (value === "false" || value === "0") return false;
  return null;
}

function clampVolume(value: number): number {
  if (!Number.isFinite(value)) return RD_DEFAULT_PREFERENCES.audioVolume;
  return Math.min(1, Math.max(0, value));
}

function parseStoredVolume(value: string | null): number {
  if (value === null) return RD_DEFAULT_PREFERENCES.audioVolume;
  const trimmed = value.trim();
  // `Number("")` is 0 and `Number("0.5abc")` is NaN: only a finite token counts as a volume.
  if (trimmed.length === 0) return RD_DEFAULT_PREFERENCES.audioVolume;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return RD_DEFAULT_PREFERENCES.audioVolume;
  return clampVolume(parsed);
}

/** Reads every viewer preference, substituting the default for anything missing or malformed. */
export function readRdPreferences(): RdPreferences {
  const storedPlayout = readStoredPreference(PREFERENCE_KEYS.playoutMode);
  const storedScale = readStoredPreference(PREFERENCE_KEYS.scaleMode);
  const storedMuted = readStoredPreference(PREFERENCE_KEYS.audioMuted);
  const storedVolume = readStoredPreference(PREFERENCE_KEYS.audioVolume);
  const storedStats = readStoredPreference(PREFERENCE_KEYS.statsOpen);

  return {
    playoutMode: isPlayoutMode(storedPlayout) ? storedPlayout : RD_DEFAULT_PREFERENCES.playoutMode,
    scaleMode: isScaleMode(storedScale) ? storedScale : RD_DEFAULT_PREFERENCES.scaleMode,
    audioMuted: parseStoredBoolean(storedMuted) ?? RD_DEFAULT_PREFERENCES.audioMuted,
    audioVolume: parseStoredVolume(storedVolume),
    statsOpen: parseStoredBoolean(storedStats) ?? RD_DEFAULT_PREFERENCES.statsOpen,
  };
}

/** Writes one preference under its canonical key; a full or denied storage area is silently kept. */
export function writeRdPreference<K extends keyof RdPreferences>(key: K, value: RdPreferences[K]): void {
  const storage = preferenceStorage();
  if (!storage) return;
  try {
    storage.setItem(PREFERENCE_KEYS[key], String(value));
  } catch {
    // Quota or disabled storage: the choice still applies for this session.
  }
}

function formatRefreshHz(refreshHz: number): string {
  return `${Number.isInteger(refreshHz) ? refreshHz.toFixed(0) : refreshHz.toFixed(1)} Hz`;
}

/**
 * Hands the keyboard back to the streamed desktop. A focused toolbar control would swallow the next
 * keystroke, so every control that is clicked (never typed into) drops focus afterwards.
 */
function blurFocusedControl(): void {
  const active = typeof document === "undefined" ? null : document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

const PLAYOUT_OPTIONS: readonly SegmentedControlOption<RdPlayoutMode>[] = [
  { value: "immediate", label: "Game" },
  { value: "latest_wins", label: "Competitive" },
  { value: "adaptive", label: "Smooth" },
];

const SCALE_OPTIONS: readonly SegmentedControlOption<RdScaleMode>[] = [
  { value: "fit", label: "Fit" },
  { value: "native100", label: "100%" },
  { value: "scale75", label: "75%" },
  { value: "scale50", label: "50%" },
];

export type RemoteDesktopToolbarProps = {
  rdSessionId: string | null;
  connected: boolean;
  playoutMode: RdPlayoutMode;
  scaleMode: RdScaleMode;
  audioMuted: boolean;
  /** 0..1, matching `rdSetAudio`. */
  audioVolume: number;
  statsOpen: boolean;
  pointerLocked: boolean;
  refreshHz?: number | null;
  onPlayoutModeChange: (mode: RdPlayoutMode) => Promise<void> | void;
  onScaleModeChange: (mode: RdScaleMode) => Promise<void> | void;
  onAudioChange: (muted: boolean, volume: number) => Promise<void> | void;
  onToggleStats: () => void;
  onTogglePointerLock: () => void;
};

/**
 * Compact control strip for one viewer session. It sits in flow above the streamed surface and opts
 * out of native-terminal yielding (`data-native-terminal-yield="off"`), because it is chrome the
 * user must be able to click. It carries no gamepad controls by design.
 *
 * Every change is optimistic: the control shows the requested value immediately, and a rejected
 * `rdSetOptions` / `rdSetAudio` rolls it back to the last confirmed value and says why in an inline
 * `role="status"` region, instead of throwing or disconnecting the session.
 *
 * The volume slider is built on the Radix primitive directly rather than `ui/slider`, because that
 * wrapper drops `aria-label` on the root span while Radix names only the thumb: reusing it would
 * leave the one control here that a screen reader must be able to identify unnamed.
 */
export function RemoteDesktopToolbar({
  rdSessionId,
  connected,
  playoutMode,
  scaleMode,
  audioMuted,
  audioVolume,
  statsOpen,
  pointerLocked,
  refreshHz,
  onPlayoutModeChange,
  onScaleModeChange,
  onAudioChange,
  onToggleStats,
  onTogglePointerLock,
}: RemoteDesktopToolbarProps) {
  const [pendingPlayout, setPendingPlayout] = useState<RdPlayoutMode | null>(null);
  const [pendingScale, setPendingScale] = useState<RdScaleMode | null>(null);
  const [pendingAudio, setPendingAudio] = useState<{ muted: boolean; volume: number } | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const volumeDragRef = useRef(false);

  // The props are authoritative once they change: a confirmed change lands there, and a rejected
  // one leaves them untouched, so dropping the optimistic overlay restores the previous value.
  useEffect(() => {
    setPendingPlayout(null);
  }, [playoutMode]);
  useEffect(() => {
    setPendingScale(null);
  }, [scaleMode]);
  useEffect(() => {
    setPendingAudio(null);
  }, [audioMuted, audioVolume]);

  const activePlayout = pendingPlayout ?? playoutMode;
  const activeScale = pendingScale ?? scaleMode;
  const activeAudio = pendingAudio ?? { muted: audioMuted, volume: audioVolume };
  const disabled = !connected || rdSessionId === null;
  const volumePercent = Math.round(activeAudio.volume * 100);

  const runChange = async (label: string, commit: () => Promise<void> | void, rollback: () => void) => {
    try {
      await commit();
      setWarning(null);
    } catch {
      rollback();
      setWarning(`${label} could not be applied. The previous setting is still active.`);
    }
  };

  const changePlayout = (mode: RdPlayoutMode) => {
    if (mode === activePlayout) return;
    blurFocusedControl();
    setPendingPlayout(mode);
    void runChange(
      "Playout preset",
      async () => {
        await onPlayoutModeChange(mode);
        writeRdPreference("playoutMode", mode);
      },
      () => setPendingPlayout(null),
    );
  };

  const changeScale = (mode: RdScaleMode) => {
    if (mode === activeScale) return;
    blurFocusedControl();
    setPendingScale(mode);
    void runChange(
      "Resolution scale",
      async () => {
        await onScaleModeChange(mode);
        writeRdPreference("scaleMode", mode);
      },
      () => setPendingScale(null),
    );
  };

  const commitAudio = (muted: boolean, volume: number) => {
    const nextVolume = clampVolume(volume);
    setPendingAudio({ muted, volume: nextVolume });
    void runChange(
      "Audio setting",
      async () => {
        await onAudioChange(muted, nextVolume);
        writeRdPreference("audioMuted", muted);
        writeRdPreference("audioVolume", nextVolume);
      },
      () => setPendingAudio(null),
    );
  };

  const toggleMute = () => {
    blurFocusedControl();
    commitAudio(!activeAudio.muted, activeAudio.volume);
  };

  const toggleStats = () => {
    blurFocusedControl();
    writeRdPreference("statsOpen", !statsOpen);
    onToggleStats();
  };

  const togglePointerLock = () => {
    blurFocusedControl();
    onTogglePointerLock();
  };

  return (
    <div
      data-testid="remote-desktop-toolbar"
      data-native-terminal-yield="off"
      role="toolbar"
      aria-label="Remote desktop controls"
      className="flex flex-wrap items-center gap-2 border-b border-border bg-background/90 px-2 py-1 text-[11px] text-foreground"
    >
      <SegmentedControl
        label="Playout preset"
        value={activePlayout}
        options={PLAYOUT_OPTIONS}
        onChange={changePlayout}
        disabled={disabled}
        className="w-[190px] shrink-0"
      />

      <SegmentedControl
        label="Resolution scale"
        value={activeScale}
        options={SCALE_OPTIONS}
        onChange={changeScale}
        disabled={disabled}
        className="w-[160px] shrink-0"
      />

      <div className="flex shrink-0 items-center gap-1">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-6 px-2"
          aria-label={activeAudio.muted ? "Unmute audio" : "Mute audio"}
          aria-pressed={activeAudio.muted}
          onClick={toggleMute}
          disabled={disabled}
        >
          {activeAudio.muted ? <VolumeX aria-hidden="true" /> : <Volume2 aria-hidden="true" />}
        </Button>
        <SliderPrimitive.Root
          className="relative flex h-4 w-20 shrink-0 touch-none select-none items-center"
          min={0}
          max={100}
          step={1}
          value={[volumePercent]}
          disabled={disabled}
          // A drag only previews: one `cmd_rd_set_audio` per gesture, not one per percent.
          onPointerDown={() => {
            volumeDragRef.current = true;
          }}
          onValueChange={(values) => {
            const percent = values[0];
            if (percent === undefined) return;
            setPendingAudio({ muted: activeAudio.muted, volume: percent / 100 });
          }}
          onValueCommit={(values) => {
            const percent = values[0];
            const wasDrag = volumeDragRef.current;
            volumeDragRef.current = false;
            if (percent === undefined) return;
            commitAudio(activeAudio.muted, percent / 100);
            // A drag leaves the thumb focused and would eat the next keystroke; arrow-key use keeps it.
            if (wasDrag) blurFocusedControl();
          }}
        >
          <SliderPrimitive.Track className="relative h-1 w-full grow overflow-hidden rounded-full bg-primary/20">
            <SliderPrimitive.Range className="absolute h-full bg-primary" />
          </SliderPrimitive.Track>
          <SliderPrimitive.Thumb
            aria-label="Audio volume"
            className="block size-3 rounded-full border border-primary/50 bg-background shadow transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
        </SliderPrimitive.Root>
        <span
          data-testid="remote-desktop-volume-readout"
          className="w-9 text-right tabular-nums text-muted-foreground"
        >{`${volumePercent}%`}</span>
      </div>

      <Button
        type="button"
        variant="ghost"
        size="sm"
        className={cn("h-6 gap-1 px-2", statsOpen && "bg-accent text-foreground")}
        aria-label="Toggle stats overlay"
        aria-pressed={statsOpen}
        onClick={toggleStats}
        disabled={disabled}
      >
        <Activity aria-hidden="true" />
        <span>Stats</span>
      </Button>

      <Button
        type="button"
        variant="ghost"
        size="sm"
        className={cn("h-6 gap-1 px-2", pointerLocked && "bg-accent text-foreground")}
        aria-label="Toggle pointer lock"
        aria-pressed={pointerLocked}
        onClick={togglePointerLock}
        disabled={disabled}
      >
        <MousePointer2 aria-hidden="true" />
        <span>Lock</span>
      </Button>

      {warning ? (
        <span role="status" className="min-w-0 truncate text-[10px] text-status-warning">
          {warning}
        </span>
      ) : null}

      {refreshHz !== null && refreshHz !== undefined && Number.isFinite(refreshHz) && refreshHz > 0 ? (
        <span
          data-testid="remote-desktop-refresh-badge"
          className="ml-auto shrink-0 rounded border border-border px-1.5 py-0.5 font-mono text-[10px] tabular-nums text-muted-foreground"
        >
          {formatRefreshHz(refreshHz)}
        </span>
      ) : null}
    </div>
  );
}
