import { useEffect, useState } from "react";
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
  macosOptionAsAlt,
  source,
  sourcePath,
  onFontFamily,
  onFontSize,
  onOptionAsAlt,
  onUseImported,
}: TerminalSectionProps) {
  const [familyDraft, setFamilyDraft] = useState(fontFamily);
  const [sizeDraft, setSizeDraft] = useState(String(fontSize));

  useEffect(() => {
    setFamilyDraft(fontFamily);
  }, [fontFamily]);

  useEffect(() => {
    setSizeDraft(String(fontSize));
  }, [fontSize]);

  const commitFamily = () => {
    if (familyDraft.trim() === "") {
      setFamilyDraft(fontFamily);
      return;
    }
    onFontFamily(familyDraft);
  };

  const handleFontSizeChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSizeDraft(event.target.value);
  };

  const commitFontSize = () => {
    const parsed = Number(sizeDraft);
    if (sizeDraft.trim() === "" || !Number.isFinite(parsed)) {
      setSizeDraft(String(fontSize));
      return;
    }
    onFontSize(parsed);
  };



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
          <div className="text-[13px] font-semibold">Effective preferences</div>
          <div className="mt-1 text-[11px] text-muted-foreground">{source}</div>
          {sourcePath ? (
            <div className="mt-1 truncate font-mono text-[11px] text-muted-foreground/65">
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
            value={familyDraft}
            onChange={(event) => setFamilyDraft(event.target.value)}
            onBlur={commitFamily}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                commitFamily();
              }
            }}
            className="h-8 text-[11px]"
          />
          <p className="mt-1 text-[11px] text-muted-foreground">
            Leave the local override reset to follow Ghostty.
          </p>
        </div>
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
            value={sizeDraft}
            onChange={handleFontSizeChange}
            onBlur={commitFontSize}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitFontSize();
            }}
            className="h-8 text-[11px]"
          />
        </div>
        <div className="flex items-center justify-between gap-4 border-y border-border py-3 text-[11px]">
          <div>
            <Label
              htmlFor="terminal-macos-option-as-alt"
              className="font-medium text-foreground cursor-pointer"
            >
              macOS Option as Alt
            </Label>
            <div className="mt-0.5 text-[11px] text-muted-foreground">
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
