import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  SlidersHorizontal,
} from "lucide-react";
import React, { useState } from "react";

type MobileKeyDockProps = {
  onSendKey: (key: string) => void;
};

export const MobileKeyDock: React.FC<MobileKeyDockProps> = ({ onSendKey }) => {
  const [isExpanded, setIsExpanded] = useState(false);
  const [ctrlLatched, setCtrlLatched] = useState(false);
  const [altLatched, setAltLatched] = useState(false);

  const triggerKey = (action: string) => {
    // Haptic vibration if supported
    if ("vibrate" in navigator) {
      try {
        navigator.vibrate(15);
      } catch {
        // ignore
      }
    }

    if (action === "ctrl") {
      setCtrlLatched(!ctrlLatched);
      return;
    }
    if (action === "alt") {
      setAltLatched(!altLatched);
      return;
    }

    let finalAction = action;
    if (ctrlLatched) {
      finalAction = `ctrl-${action}`;
    } else if (altLatched) {
      finalAction = `alt-${action}`;
    }

    setCtrlLatched(false);
    setAltLatched(false);

    onSendKey(finalAction);
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
          <KeyButton
            label="Ctrl"
            active={ctrlLatched}
            onClick={() => triggerKey("ctrl")}
            className="font-semibold"
          />
          <KeyButton
            label="Alt"
            active={altLatched}
            onClick={() => triggerKey("alt")}
            className="font-semibold"
          />
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
          <KeyButton icon={<ArrowLeft size={14} />} onClick={() => triggerKey("left")} />
          <KeyButton icon={<ArrowUp size={14} />} onClick={() => triggerKey("up")} />
          <KeyButton icon={<ArrowDown size={14} />} onClick={() => triggerKey("down")} />
          <KeyButton icon={<ArrowRight size={14} />} onClick={() => triggerKey("right")} />
          <KeyButton
            icon={<SlidersHorizontal size={13} />}
            active={isExpanded}
            onClick={() => setIsExpanded(!isExpanded)}
            title="More keys"
          />
        </div>
      </div>
    </div>
  );
};

function KeyButton({
  label,
  icon,
  active,
  highlight,
  onClick,
  className = "",
  title,
}: {
  label?: string;
  icon?: React.ReactNode;
  active?: boolean;
  highlight?: boolean;
  onClick: () => void;
  className?: string;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
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
