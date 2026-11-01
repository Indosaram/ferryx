import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CornerDownLeft,
  Lock,
  SlidersHorizontal,
} from "lucide-react";
import React, { useEffect, useRef, useState } from "react";

export type DockModifierState = "off" | "oneshot" | "locked";

export type DockModifierName = "ctrl" | "alt" | "shift";

export type DockModifiers = Readonly<Record<DockModifierName, DockModifierState>>;

export const NO_DOCK_MODIFIERS: DockModifiers = { ctrl: "off", alt: "off", shift: "off" };

/** A second tap on a one-shot modifier within this window locks it. */
export const MODIFIER_LOCK_WINDOW_MS = 350;

/** Advance one modifier through off -> one-shot -> (double tap) locked -> off. */
export function nextModifierState(
  current: DockModifierState,
  sinceLastTapMs: number,
): DockModifierState {
  if (current === "off") return "oneshot";
  if (current === "oneshot" && sinceLastTapMs <= MODIFIER_LOCK_WINDOW_MS) return "locked";
  return "off";
}

/** Drop one-shot modifiers after a key consumed them; locked ones stay. */
export function releaseOneShotModifiers(modifiers: DockModifiers): DockModifiers {
  return {
    ctrl: modifiers.ctrl === "oneshot" ? "off" : modifiers.ctrl,
    alt: modifiers.alt === "oneshot" ? "off" : modifiers.alt,
    shift: modifiers.shift === "oneshot" ? "off" : modifiers.shift,
  };
}

export function hasActiveModifier(modifiers: DockModifiers): boolean {
  return modifiers.ctrl !== "off" || modifiers.alt !== "off" || modifiers.shift !== "off";
}

/** `ctrl-alt-shift-` style prefix for a dock action, in a fixed order. */
export function modifierPrefix(modifiers: DockModifiers): string {
  return `${modifiers.ctrl !== "off" ? "ctrl-" : ""}${modifiers.alt !== "off" ? "alt-" : ""}${
    modifiers.shift !== "off" ? "shift-" : ""
  }`;
}

export type TerminalInputMode = "direct" | "line";

/** Shrink of the visual viewport below the layout viewport that counts as an open soft keyboard. */
const KEYBOARD_MIN_SHRINK_PX = 120;

/** True while a soft keyboard covers part of the layout viewport. */
export function useSoftKeyboardOpen(): boolean {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const update = () => setOpen(window.innerHeight - viewport.height > KEYBOARD_MIN_SHRINK_PX);
    update();
    viewport.addEventListener("resize", update);
    return () => viewport.removeEventListener("resize", update);
  }, []);
  return open;
}

type MobileKeyDockProps = {
  onSendKey: (key: string) => void;
  /** Controlled modifier state; the dock keeps its own when omitted. */
  modifiers?: DockModifiers;
  onModifiersChange?: (modifiers: DockModifiers) => void;
  /** Shows the Line input toggle when both are provided. */
  inputMode?: TerminalInputMode;
  onInputModeChange?: (mode: TerminalInputMode) => void;
  /** One row only (navigation keys move behind More), for when a text field already takes space. */
  compact?: boolean;
  /** The soft keyboard already sits over the home indicator, so the bottom safe-area inset is dropped. */
  keyboardOpen?: boolean;
};

/** Keys that already carry their chord and ignore latched modifiers. */
const FIXED_CHORD_ACTIONS = new Set(["ctrl-c", "ctrl-d", "ctrl-z"]);

const ARROW_ICON = "size-4";

export const MobileKeyDock: React.FC<MobileKeyDockProps> = ({
  onSendKey,
  modifiers: controlledModifiers,
  onModifiersChange,
  inputMode,
  onInputModeChange,
  compact = false,
  keyboardOpen = false,
}) => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [ownModifiers, setOwnModifiers] = useState<DockModifiers>(NO_DOCK_MODIFIERS);
  const lastTapRef = useRef<Record<DockModifierName, number>>({ ctrl: 0, alt: 0, shift: 0 });
  const modifiers = controlledModifiers ?? ownModifiers;

  const updateModifiers = (next: DockModifiers) => {
    if (controlledModifiers === undefined) setOwnModifiers(next);
    onModifiersChange?.(next);
  };

  const vibrate = () => {
    if (!("vibrate" in navigator)) return;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    try {
      navigator.vibrate(15);
    } catch {
      // Haptics are optional; a refusing platform simply stays silent.
    }
  };

  const toggleModifier = (name: DockModifierName) => {
    vibrate();
    const now = Date.now();
    const sinceLastTap = now - lastTapRef.current[name];
    lastTapRef.current[name] = now;
    updateModifiers({ ...modifiers, [name]: nextModifierState(modifiers[name], sinceLastTap) });
  };

  const triggerKey = (action: string) => {
    vibrate();
    if (FIXED_CHORD_ACTIONS.has(action)) {
      onSendKey(action);
      return;
    }
    const prefix = modifierPrefix(modifiers);
    if (prefix.length > 0) updateModifiers(releaseOneShotModifiers(modifiers));
    onSendKey(`${prefix}${action}`);
  };

  const showNavigationRow = !compact || isExpanded;

  return (
    <div
      data-testid="mobile-key-dock"
      className={`z-30 w-full select-none touch-manipulation border-t border-chat-border bg-chat-surface ${keyboardOpen ? "" : "pb-safe"}`}
    >
      {isExpanded && (
        <div data-testid="mobile-key-dock-more" className="grid grid-cols-7 gap-1 px-2 pt-2">
          <KeyButton label="~" onClick={() => triggerKey("~")} />
          <KeyButton label="`" onClick={() => triggerKey("`")} />
          <KeyButton label="|" onClick={() => triggerKey("|")} />
          <KeyButton label="/" onClick={() => triggerKey("/")} />
          <KeyButton label="\" onClick={() => triggerKey("\\")} />
          <KeyButton label="-" onClick={() => triggerKey("-")} />
          <KeyButton label="_" onClick={() => triggerKey("_")} />
          <KeyButton label="=" onClick={() => triggerKey("=")} />
          <KeyButton label="PgUp" ariaLabel="Page up" onClick={() => triggerKey("pageup")} />
          <KeyButton label="PgDn" ariaLabel="Page down" onClick={() => triggerKey("pagedown")} />
          <KeyButton label="Home" onClick={() => triggerKey("home")} />
          <KeyButton label="End" onClick={() => triggerKey("end")} />
          <KeyButton label="Ctrl-Z" onClick={() => triggerKey("ctrl-z")} />
          <KeyButton label="Ctrl-D" onClick={() => triggerKey("ctrl-d")} />
        </div>
      )}

      <div className="grid grid-cols-7 gap-1 px-2 pt-1.5">
        <KeyButton label="Esc" onClick={() => triggerKey("esc")} />
        <KeyButton label="Tab" onClick={() => triggerKey("tab")} />
        <ModifierButton label="Ctrl" state={modifiers.ctrl} onClick={() => toggleModifier("ctrl")} />
        <ModifierButton label="Alt" state={modifiers.alt} onClick={() => toggleModifier("alt")} />
        <ModifierButton label="Shift" state={modifiers.shift} onClick={() => toggleModifier("shift")} />
        <KeyButton label="Ctrl-C" onClick={() => triggerKey("ctrl-c")} className="font-bold text-chat-danger" />
        <KeyButton
          icon={<SlidersHorizontal className="size-4" aria-hidden="true" />}
          active={isExpanded}
          pressed={isExpanded}
          onClick={() => setIsExpanded(!isExpanded)}
          title="More keys"
          ariaLabel="More keys"
        />
      </div>

      {showNavigationRow ? (
        <div data-testid="mobile-key-dock-navigation" className="grid grid-cols-7 gap-1 px-2 pt-1">
          <KeyButton icon={<ArrowLeft className={ARROW_ICON} aria-hidden="true" />} ariaLabel="Left" onClick={() => triggerKey("left")} />
          <KeyButton icon={<ArrowUp className={ARROW_ICON} aria-hidden="true" />} ariaLabel="Up" onClick={() => triggerKey("up")} />
          <KeyButton icon={<ArrowDown className={ARROW_ICON} aria-hidden="true" />} ariaLabel="Down" onClick={() => triggerKey("down")} />
          <KeyButton icon={<ArrowRight className={ARROW_ICON} aria-hidden="true" />} ariaLabel="Right" onClick={() => triggerKey("right")} />
          <KeyButton
            icon={<CornerDownLeft className={ARROW_ICON} aria-hidden="true" />}
            ariaLabel="Enter"
            title="Enter"
            onClick={() => triggerKey("enter")}
            className="col-span-2"
          />
          {inputMode !== undefined && onInputModeChange ? (
            <KeyButton
              label="Line"
              active={inputMode === "line"}
              pressed={inputMode === "line"}
              title={inputMode === "line" ? "Line input: edit a whole line, then send it" : "Direct input: every key goes straight to the terminal"}
              testId="remote-terminal-input-mode-toggle"
              onClick={() => onInputModeChange(inputMode === "line" ? "direct" : "line")}
            />
          ) : null}
        </div>
      ) : null}
      <div className="h-1.5" aria-hidden="true" />
    </div>
  );
};

function ModifierButton({
  label,
  state,
  onClick,
}: {
  label: string;
  state: DockModifierState;
  onClick: () => void;
}) {
  return (
    <KeyButton
      label={label}
      active={state !== "off"}
      pressed={state !== "off"}
      dataState={state}
      onClick={onClick}
      title={state === "locked" ? `${label} locked` : state === "oneshot" ? `${label} for the next key` : undefined}
      className="font-semibold"
      indicator={
        state === "locked" ? (
          <Lock data-testid="dock-modifier-locked" className="size-3" aria-hidden="true" />
        ) : state === "oneshot" ? (
          <span data-testid="dock-modifier-oneshot" className="size-1.5 rounded-full bg-current" aria-hidden="true" />
        ) : null
      }
    />
  );
}

function KeyButton({
  label,
  icon,
  active,
  onClick,
  className = "",
  title,
  ariaLabel,
  pressed,
  dataState,
  testId,
  indicator,
}: {
  label?: string;
  icon?: React.ReactNode;
  active?: boolean;
  onClick: () => void;
  className?: string;
  title?: string;
  ariaLabel?: string;
  pressed?: boolean;
  dataState?: string;
  testId?: string;
  indicator?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      onPointerDown={(e) => {
        e.preventDefault();
      }}
      title={title}
      aria-label={ariaLabel}
      aria-pressed={pressed}
      data-state={dataState}
      data-testid={testId}
      className={`flex min-h-11 min-w-0 items-center justify-center gap-1 rounded border px-1 font-mono text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
        active
          ? "border-chat-primary bg-chat-primary text-chat-primary-foreground"
          : "border-chat-border bg-chat-surface-raised text-chat-foreground hover:bg-chat-surface-hover"
      } ${className}`}
    >
      {icon || label}
      {indicator}
    </button>
  );
}
