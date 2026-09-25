import { useEffect, useState } from "react";
import { RotateCcw, TerminalSquare } from "lucide-react";

import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Switch } from "../ui/switch";
import { SettingsHeading } from "./primitives";
import type { TerminalSectionProps } from "./types";
import { isMacShortcutPlatform } from "../../lib/shortcuts";
import { loadFileLinkEditor, parseFileLinkEditor, saveFileLinkEditor } from "../../lib/fileLinkSettings";
import { getSystemPermissionsStatus } from "../../lib/tauri";

type ShellPreset = { value: string; label: string };

/**
 * Presets are keyed to the HOST platform reported by the backend, never to the
 * browser OS: the remote web client can drive a host of a different platform.
 */
const WINDOWS_SHELL_PRESETS: ShellPreset[] = [
  { value: "pwsh", label: "PowerShell" },
  { value: "powershell", label: "Windows PowerShell" },
  { value: "cmd", label: "Command Prompt" },
  { value: "wsl", label: "WSL" },
];

const POSIX_SHELL_PRESETS: ShellPreset[] = [
  { value: "zsh", label: "zsh" },
  { value: "bash", label: "bash" },
  { value: "fish", label: "fish" },
];

export function TerminalSection({
  fontFamily,
  fontSize,
  macosOptionAsAlt,
  shell = null,
  scrollback = 10_000,
  source,
  sourcePath,
  onFontFamily,
  onFontSize,
  onOptionAsAlt,
  onShell,
  onScrollback,
  onUseImported,
}: TerminalSectionProps) {
  const [familyDraft, setFamilyDraft] = useState(fontFamily);
  const [fileEditor, setFileEditor] = useState(loadFileLinkEditor);
  const [sizeDraft, setSizeDraft] = useState(String(fontSize));
  const [scrollbackDraft, setScrollbackDraft] = useState(String(scrollback));

  const [hostPlatform, setHostPlatform] = useState<string | null>(null);
  // While the host platform is unknown no preset is offered: guessing either side
  // would list shells that cannot spawn on the host.
  const shellPresets: ShellPreset[] =
    hostPlatform === "windows"
      ? WINDOWS_SHELL_PRESETS
      : hostPlatform === "macos" || hostPlatform === "linux"
        ? POSIX_SHELL_PRESETS
        : [];
  const knownShells = shellPresets.map((preset) => preset.value);
  const isKnownShell = shell !== null && knownShells.includes(shell);
  const isCustomShell = shell !== null && shell !== "" && !isKnownShell;

  const [selectedShell, setSelectedShell] = useState<string>(() => {
    if (shell === null || shell === "") return "";
    if (isKnownShell) return shell;
    return "custom";
  });
  const [customDraft, setCustomDraft] = useState<string>(() => (isCustomShell ? shell : ""));

  useEffect(() => {
    setFamilyDraft(fontFamily);
  }, [fontFamily]);

  useEffect(() => {
    setSizeDraft(String(fontSize));
  }, [fontSize]);

  useEffect(() => {
    setScrollbackDraft(String(scrollback));
  }, [scrollback]);

  useEffect(() => {
    let cancelled = false;
    void getSystemPermissionsStatus()
      .then((status) => {
        if (!cancelled) setHostPlatform(status?.platform ?? null);
      })
      .catch(() => {
        if (!cancelled) setHostPlatform(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (shell === null || shell === "") {
      setSelectedShell("");
    } else if (knownShells.includes(shell)) {
      setSelectedShell(shell);
    } else {
      setSelectedShell("custom");
      setCustomDraft(shell);
    }
  }, [shell, hostPlatform]);

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

  const handleScrollbackChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setScrollbackDraft(event.target.value);
  };

  const commitScrollback = () => {
    const parsed = Number(scrollbackDraft);
    if (scrollbackDraft.trim() === "" || !Number.isFinite(parsed)) {
      setScrollbackDraft(String(scrollback));
      return;
    }
    const clamped = Math.min(100_000, Math.max(1_000, Math.round(parsed)));
    setScrollbackDraft(String(clamped));
    onScrollback?.(clamped);
  };

  const handleShellChange = (event: React.ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value;
    setSelectedShell(value);
    if (value === "custom") {
      if (customDraft.trim()) {
        onShell?.(customDraft.trim());
      }
    } else {
      onShell?.(value === "" ? null : value);
    }
  };

  const commitCustom = () => {
    const trimmed = customDraft.trim();
    if (trimmed) {
      onShell?.(trimmed);
    } else {
      onShell?.(null);
    }
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
          <Label htmlFor="terminal-file-editor" className="mb-1.5 block text-[11px] font-medium">
            File link editor
          </Label>
          <select
            id="terminal-file-editor"
            value={fileEditor}
            onChange={(event) => {
              const editor = parseFileLinkEditor(event.target.value);
              saveFileLinkEditor(editor);
              setFileEditor(editor);
            }}
            className="h-8 w-full rounded-md border border-input bg-background px-3 text-[11px] focus:outline-none focus:ring-1 focus:ring-ring"
          >
            <option value="system">System default</option>
            <option value="vscode">Visual Studio Code</option>
            <option value="cursor">Cursor</option>
            <option value="zed">Zed</option>
          </select>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {isMacShortcutPlatform() ? "Cmd" : "Ctrl"}+click a local file path to open it. Choose an editor to jump to its line and column.
          </p>
        </div>
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
        <div>
          <Label
            htmlFor="terminal-scrollback"
            className="mb-1.5 block text-[11px] font-medium"
          >
            Scrollback lines
          </Label>
          <Input
            id="terminal-scrollback"
            type="number"
            min={1000}
            max={100000}
            step={1000}
            value={scrollbackDraft}
            onChange={handleScrollbackChange}
            onBlur={commitScrollback}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitScrollback();
            }}
            className="h-8 text-[11px]"
          />
          <p className="mt-1 text-[11px] text-muted-foreground">
            Maximum scrollback lines retained in memory (1,000 – 100,000).
          </p>
        </div>
        <div>
          <Label
            htmlFor="terminal-default-shell"
            className="mb-1.5 block text-[11px] font-medium"
          >
            Default shell
          </Label>
          <select
            id="terminal-default-shell"
            value={selectedShell}
            onChange={handleShellChange}
            className="h-8 w-full rounded-md border border-input bg-transparent px-3 py-1 text-[11px] shadow-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
          >
            <option value="" className="bg-popover text-popover-foreground">Platform default</option>
            {shellPresets.map((preset) => (
              <option key={preset.value} value={preset.value} className="bg-popover text-popover-foreground">
                {preset.label}
              </option>
            ))}
            <option value="custom" className="bg-popover text-popover-foreground">Custom...</option>
          </select>
          {selectedShell === "custom" ? (
            <div className="mt-2">
              <Input
                id="terminal-custom-shell"
                placeholder="/path/to/shell"
                aria-label="Custom shell path"
                value={customDraft}
                onChange={(event) => setCustomDraft(event.target.value)}
                onBlur={commitCustom}
                onKeyDown={(event) => {
                  if (event.key === "Enter") commitCustom();
                }}
                className="h-8 text-[11px]"
              />
            </div>
          ) : null}
          <p className="mt-1 text-[11px] text-muted-foreground">
            Choose the default shell spawned in new tabs and panes.
          </p>
        </div>
        {isMacShortcutPlatform() && <div className="flex items-center justify-between gap-4 border-y border-border py-3 text-[11px]">
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
        </div>}
      </div>
    </section>
  );
}
