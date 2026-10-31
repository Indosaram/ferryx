import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CornerDownLeft,
  SlidersHorizontal,
} from "lucide-react";
import React, { useRef, useState } from "react";

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

type MobileKeyDockProps = {
  onSendKey: (key: string) => void;
  /** Controlled modifier state; the dock keeps its own when omitted. */
  modifiers?: DockModifiers;
  onModifiersChange?: (modifiers: DockModifiers) => void;
  /** Shows the Direct/Line input toggle when both are provided. */
  inputMode?: TerminalInputMode;
  onInputModeChange?: (mode: TerminalInputMode) => void;
};

/** Keys that already carry their chord and ignore latched modifiers. */
const FIXED_CHORD_ACTIONS = new Set(["ctrl-c", "ctrl-d", "ctrl-z"]);

export const MobileKeyDock: React.FC<MobileKeyDockProps> = ({
  onSendKey,
  modifiers: controlledModifiers,
  onModifiersChange,
  inputMode,
  onInputModeChange,
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
    if ("vibrate" in navigator) {
      try {
        navigator.vibrate(15);
      } catch {
        // Haptics are optional; a refusing platform simply stays silent.
      }
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

  return (
    <div className="w-full bg-card border-t border-border select-none touch-manipulation z-30 pb-[env(safe-area-inset-bottom)]">
      {/* Expanded utility row */}
      {isExpanded && (
        <div className="flex gap-1.5 px-2 pt-2 pb-1 overflow-x-auto no-scrollbar text-xs font-mono">
          <KeyButton label="~" onClick={() => triggerKey("~")} />
          <KeyButton label="`" onClick={() => triggerKey("`")} />
          <KeyButton label="|" onClick={() => triggerKey("|")} />
          <KeyButton label="/" onClick={() => triggerKey("/")} />
          <KeyButton label="\" onClick={() => triggerKey("\\")} />
          <KeyButton label="-" onClick={() => triggerKey("-")} />
          <KeyButton label="_" onClick={() => triggerKey("_")} />
          <KeyButton label="=" onClick={() => triggerKey("=")} />
          <KeyButton label="PgUp" onClick={() => triggerKey("pageup")} />
          <KeyButton label="PgDn" onClick={() => triggerKey("pagedown")} />
          <KeyButton label="Home" onClick={() => triggerKey("home")} />
          <KeyButton label="End" onClick={() => triggerKey("end")} />
          <KeyButton label="Ctrl-Z" onClick={() => triggerKey("ctrl-z")} />
          <KeyButton label="Ctrl-D" onClick={() => triggerKey("ctrl-d")} />
        </div>
      )}

      {/* Primary essential row */}
      <div className="flex items-center justify-between gap-1 px-2 py-1.5 overflow-x-auto no-scrollbar">
        <div className="flex items-center gap-1.5">
          <ModifierButton label="Ctrl" state={modifiers.ctrl} onClick={() => toggleModifier("ctrl")} />
          <ModifierButton label="Alt" state={modifiers.alt} onClick={() => toggleModifier("alt")} />
          <KeyButton label="Esc" onClick={() => triggerKey("esc")} />
          <KeyButton label="Tab" onClick={() => triggerKey("tab")} />
          <KeyButton
            label="Ctrl-C"
            highlight
            onClick={() => triggerKey("ctrl-c")}
            className="font-bold text-destructive-foreground border-destructive/50 bg-destructive/20"
          />
        </div>

        <div className="flex items-center gap-1">
          <KeyButton icon={<ArrowLeft size={14} />} ariaLabel="Left" onClick={() => triggerKey("left")} />
          <KeyButton icon={<ArrowUp size={14} />} ariaLabel="Up" onClick={() => triggerKey("up")} />
          <KeyButton icon={<ArrowDown size={14} />} ariaLabel="Down" onClick={() => triggerKey("down")} />
          <KeyButton icon={<ArrowRight size={14} />} ariaLabel="Right" onClick={() => triggerKey("right")} />
          <ModifierButton label="Shift" state={modifiers.shift} onClick={() => toggleModifier("shift")} />
          <KeyButton
            icon={<CornerDownLeft size={14} />}
            ariaLabel="Enter"
            title="Enter"
            onClick={() => triggerKey("enter")}
          />
          <KeyButton
            icon={<SlidersHorizontal size={13} />}
            active={isExpanded}
            onClick={() => setIsExpanded(!isExpanded)}
            title="More keys"
            ariaLabel="More keys"
          />
          {inputMode !== undefined && onInputModeChange ? (
            <KeyButton
              label={inputMode === "line" ? "Line" : "Direct"}
              active={inputMode === "line"}
              pressed={inputMode === "line"}
              ariaLabel="Line input mode"
              title={inputMode === "line" ? "Line input: edit a whole line, then send it" : "Direct input: every key goes straight to the terminal"}
              testId="remote-terminal-input-mode-toggle"
              onClick={() => onInputModeChange(inputMode === "line" ? "direct" : "line")}
            />
          ) : null}
        </div>
      </div>
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
      pressed={state === "locked" ? true : state === "oneshot" ? "mixed" : false}
      dataState={state}
      onClick={onClick}
      title={state === "locked" ? `${label} locked` : undefined}
      className={`font-semibold ${state === "locked" ? "underline decoration-2 underline-offset-2 ring-2 ring-primary/60 ring-offset-1 ring-offset-card" : ""}`}
    />
  );
}

function KeyButton({
  label,
  icon,
  active,
  highlight,
  onClick,
  className = "",
  title,
  ariaLabel,
  pressed,
  dataState,
  testId,
}: {
  label?: string;
  icon?: React.ReactNode;
  active?: boolean;
  highlight?: boolean;
  onClick: () => void;
  className?: string;
  title?: string;
  ariaLabel?: string;
  pressed?: boolean | "mixed";
  dataState?: string;
  testId?: string;
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
      className={`min-w-[44px] min-h-11 px-2 flex items-center justify-center rounded border text-xs font-mono font-medium transition-all active:scale-95 ${
        active
          ? "bg-primary border-primary text-primary-foreground shadow-sm"
          : highlight
          ? "border-border text-foreground bg-secondary hover:bg-secondary/80"
          : "bg-secondary/60 border-border text-foreground hover:bg-secondary hover:text-foreground"
      } ${className}`}
    >
      {icon || label}
    </button>
  );
}
