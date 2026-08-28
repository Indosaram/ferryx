import { RotateCcw, TerminalSquare } from "lucide-react";

import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Switch } from "../ui/switch";
import { SettingsHeading } from "./primitives";
import type { TerminalSectionProps } from "./types";

export function TerminalSection({
  fontFamily,
  fontSize,
  scrollback,
  macosOptionAsAlt,
  source,
  sourcePath,
  onFontFamily,
  onFontSize,
  onScrollback,
  onOptionAsAlt,
  onUseImported,
}: TerminalSectionProps) {
  return (
    <section aria-labelledby="settings-terminal-heading">
      <SettingsHeading
        icon={<TerminalSquare />}
        title="Terminal"
        description="Ghostty preferences are imported by the native runtime. Explicit values set here take precedence locally."
      />
      <h2 id="settings-terminal-heading" className="sr-only">
        Terminal
      </h2>
      <div className="mb-5 flex items-start justify-between gap-5 border-y border-border py-3">
        <div className="min-w-0">
          <div className="text-[12px] font-semibold">Effective preferences</div>
          <div className="mt-1 text-[11px] text-muted-foreground">{source}</div>
          {sourcePath ? (
            <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground/65">
              {sourcePath}
            </div>
          ) : null}
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={onUseImported}
          className="no-drag h-7 shrink-0 gap-1.5 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <RotateCcw className="size-3" />
          Use imported
        </Button>
      </div>

      <div className="space-y-5">
        <div>
          <Label
            htmlFor="terminal-font-family"
            className="mb-1.5 block text-[11px] font-medium"
          >
            Font family
          </Label>
          <Input
            id="terminal-font-family"
            value={fontFamily}
            onChange={(event) => onFontFamily(event.target.value)}
            className="h-8 text-xs"
          />
          <p className="mt-1 text-[10px] text-muted-foreground">
            Leave the local override reset to follow Ghostty.
          </p>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <div>
            <Label
              htmlFor="terminal-font-size"
              className="mb-1.5 block text-[11px] font-medium"
            >
              Font size
            </Label>
            <Input
              id="terminal-font-size"
              type="number"
              min={10}
              max={24}
              value={fontSize}
              onChange={(event) => onFontSize(Number(event.target.value))}
              className="h-8 text-xs"
            />
          </div>
          <div>
            <Label
              htmlFor="terminal-scrollback"
              className="mb-1.5 block text-[11px] font-medium"
            >
              Scrollback
            </Label>
            <Input
              id="terminal-scrollback"
              type="number"
              min={1000}
              max={100000}
              step={1000}
              value={scrollback}
              onChange={(event) => onScrollback(Number(event.target.value))}
              className="h-8 text-xs"
            />
          </div>
        </div>
        <div className="flex items-center justify-between gap-4 border-y border-border py-3 text-[11px]">
          <div>
            <Label
              htmlFor="terminal-macos-option-as-alt"
              className="font-medium text-foreground cursor-pointer"
            >
              macOS Option as Alt
            </Label>
            <div className="mt-0.5 text-[10px] text-muted-foreground">
              Maps the Option key to terminal Meta/Alt behavior.
            </div>
          </div>
          <Switch
            id="terminal-macos-option-as-alt"
            checked={macosOptionAsAlt}
            onCheckedChange={onOptionAsAlt}
          />
        </div>
      </div>
    </section>
  );
}
