import { useMemo, useRef, useState } from "react";
import { FileUp, Upload } from "lucide-react";

import type { ThemeDocumentV1, ThemeValidationError } from "../../lib/themeDocument";
import { resolveDocumentRoles } from "../../lib/themePalettes";
import { parseAnyThemeImport } from "../../lib/vscodeThemeImport";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
import { ThemePreviewCircles } from "./ThemePreviewCircles";

export type ThemeImportDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existingIds: ReadonlySet<string>;
  onImport: (doc: ThemeDocumentV1) => void;
};

export function ThemeImportDialog({
  open,
  onOpenChange,
  existingIds,
  onImport,
}: ThemeImportDialogProps) {
  const [text, setText] = useState("");
  const [fileName, setFileName] = useState<string | undefined>(undefined);
  const [dragActive, setDragActive] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const parsed = useMemo(() => {
    if (!text.trim()) return null;
    const baseName = fileName?.replace(/\.(?:ferryx-theme\.json|jsonc|json)$/i, "");
    return parseAnyThemeImport(text, baseName);
  }, [fileName, text]);

  const conflict = parsed?.ok && existingIds.has(parsed.doc.id);

  async function readFile(file: File) {
    setFileName(file.name);
    setText(await file.text());
  }

  function handleConfirm(mode: "replace" | "keep-both") {
    if (!parsed || !parsed.ok) return;
    let doc = parsed.doc;
    if (mode === "keep-both" && existingIds.has(doc.id)) {
      let suffix = 2;
      let nextId = `${doc.id.slice(0, 58)}-${suffix}`;
      while (existingIds.has(nextId)) {
        suffix += 1;
        nextId = `${doc.id.slice(0, 58)}-${suffix}`;
      }
      doc = { ...doc, id: nextId, name: `${doc.name} (${suffix})`.slice(0, 64) };
    }
    onImport(doc);
    setText("");
    setFileName(undefined);
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <div className="space-y-1.5">
          <DialogTitle>Import T3 Code or VS Code theme</DialogTitle>
          <DialogDescription>
            Drop or paste a T3 Code theme JSON, shorthand palette, VS Code theme (.json / .jsonc), or Ferryx theme file.
          </DialogDescription>
        </div>

        <div
          onDragOver={(event) => {
            event.preventDefault();
            setDragActive(true);
          }}
          onDragLeave={() => setDragActive(false)}
          onDrop={(event) => {
            event.preventDefault();
            setDragActive(false);
            const file = event.dataTransfer.files?.[0];
            if (file) void readFile(file);
          }}
          className={`flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed p-4 text-center transition-colors ${
            dragActive ? "border-primary bg-accent/30" : "border-border bg-background/60"
          }`}
        >
          <Upload className="size-4 text-muted-foreground" aria-hidden="true" />
          <p className="text-[12px] text-muted-foreground">
            {fileName ? `Loaded ${fileName}` : "Drop a .json or .jsonc theme file here"}
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => fileInputRef.current?.click()}
            className="h-7 gap-1.5 px-2.5 text-[11px]"
          >
            <FileUp className="size-3" aria-hidden="true" />
            Choose file
          </Button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,.jsonc,.ferryx-theme.json,application/json"
            aria-label="Choose theme JSON or VS Code theme file"
            className="sr-only"
            onChange={(event) => {
              const file = event.target.files?.[0];
              event.target.value = "";
              if (file) void readFile(file);
            }}
          />
        </div>

        <div className="space-y-1">
          <label htmlFor="theme-import-paste" className="block text-[11px] font-medium text-muted-foreground">
            Or paste JSON / JSONC
          </label>
          <textarea
            id="theme-import-paste"
            aria-label="Paste theme JSON or VS Code JSONC"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder={'{\n  "name": "Nightfall",\n  "appearance": "dark",\n  "canvas": "#1a1b26",\n  "accent": "#7aa2f7"\n}'}
            spellCheck={false}
            className="h-36 w-full rounded-md border border-input bg-background p-2.5 font-mono text-[11px] text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
        </div>

        {parsed && parsed.ok ? (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-panel p-3">
            <div className="flex items-center gap-3">
              <ThemePreviewCircles
                label={parsed.doc.name}
                lightColors={
                  parsed.doc.base === "light" || parsed.doc.variants?.light
                    ? resolveDocumentRoles(parsed.doc, "light")
                    : undefined
                }
                darkColors={
                  parsed.doc.base === "dark" || parsed.doc.variants?.dark
                    ? resolveDocumentRoles(parsed.doc, "dark")
                    : undefined
                }
              />
              <div>
                <p className="text-[12px] font-semibold text-foreground">{parsed.doc.name}</p>
                <p className="text-[11px] text-muted-foreground">
                  Format: {parsed.format.toUpperCase()} · Base: {parsed.doc.base}
                  {parsed.doc.css ? " · Includes CSS" : ""}
                </p>
              </div>
            </div>
            <div className="flex items-center gap-1.5">
              {conflict ? (
                <>
                  <Button type="button" size="sm" variant="outline" onClick={() => handleConfirm("keep-both")} className="h-7 px-2.5 text-[11px]">
                    Keep both
                  </Button>
                  <Button type="button" size="sm" onClick={() => handleConfirm("replace")} className="h-7 px-2.5 text-[11px]">
                    Replace existing
                  </Button>
                </>
              ) : (
                <Button type="button" size="sm" onClick={() => handleConfirm("replace")} className="h-7 px-2.5 text-[11px]">
                  Import & activate
                </Button>
              )}
            </div>
          </div>
        ) : null}

        {parsed && !parsed.ok ? (
          <ul role="alert" className="space-y-1 rounded-md border border-destructive/40 bg-destructive/10 p-2.5 text-[11px] text-destructive">
            {parsed.errors.map((err: ThemeValidationError, idx: number) => (
              <li key={`${err.path}-${idx}`}>
                {err.path}: {err.message}
              </li>
            ))}
          </ul>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
