import { GitFork } from "lucide-react";
import type { AgentContinueMode } from "../lib/tauri";
import { Button } from "./ui/button";
import { IconButton } from "./ui/IconButton";

export function continueInNewPaneLabel(mode: AgentContinueMode, modes: readonly AgentContinueMode[]): string {
  return modes.length === 1 ? "Continue in new pane" : `Continue in new pane (${mode})`;
}

export function ContinueInNewPaneButtons({
  modes,
  onContinue,
  held = false,
  compact = false,
}: {
  modes?: readonly AgentContinueMode[];
  onContinue?: (mode: AgentContinueMode) => void;
  held?: boolean;
  compact?: boolean;
}) {
  if (!onContinue || !modes || modes.length === 0) return null;
  return (
    <>
      {modes.map((mode) => {
        const label = continueInNewPaneLabel(mode, modes);
        if (compact) {
          return (
            <IconButton
              key={mode}
              label={label}
              data-testid={`continue-in-new-pane-${mode}`}
              data-mode={mode}
              disabled={held}
              size="sm"
              className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation();
                if (!held) onContinue(mode);
              }}
            >
              <GitFork className="size-3" />
            </IconButton>
          );
        }
        return (
          <Button
            key={mode}
            type="button"
            size="sm"
            variant="secondary"
            disabled={held}
            data-testid={`continue-in-new-pane-${mode}`}
            data-mode={mode}
            aria-label={label}
            onClick={() => {
              if (!held) onContinue(mode);
            }}
            className="w-full max-w-[220px]"
          >
            <span>{label}</span>
          </Button>
        );
      })}
    </>
  );
}
