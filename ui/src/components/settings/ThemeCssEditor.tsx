import { useId, useState, type KeyboardEvent } from "react";
import { AlertTriangle, BookOpen } from "lucide-react";

import { THEME_CSS_HOOKS, themeCssLoadsRemoteFiles } from "../../lib/themeCss";
import { THEME_CSS_MAX_LENGTH } from "../../lib/themePalettes";
import { Button } from "../ui/button";

export type ThemeCssEditorProps = {
  value: string;
  onChange: (value: string) => void;
};

export function ThemeCssEditor({ value, onChange }: ThemeCssEditorProps) {
  const hintId = useId();
  const [position, setPosition] = useState({ line: 1, column: 1 });
  const [tabIndents, setTabIndents] = useState(true);
  const [showHooks, setShowHooks] = useState(false);

  const hasRemoteFetch = value.length > 0 && themeCssLoadsRemoteFiles(value);
  const overLimit = value.length > THEME_CSS_MAX_LENGTH;

  function updatePosition(editor: HTMLTextAreaElement) {
    const preceding = editor.value.slice(0, editor.selectionStart).split("\n");
    setPosition({ line: preceding.length, column: (preceding.at(-1)?.length ?? 0) + 1 });
  }

  function replaceSelection(editor: HTMLTextAreaElement, replacement: string) {
    editor.setRangeText(replacement, editor.selectionStart, editor.selectionEnd, "end");
    onChange(editor.value);
    updatePosition(editor);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const editor = event.currentTarget;
    if (event.key === "Escape" && (tabIndents || event.repeat)) {
      event.preventDefault();
      event.stopPropagation();
      setTabIndents(false);
      return;
    }
    if (event.key === "Tab" && tabIndents) {
      event.preventDefault();
      const start = editor.selectionStart;
      const end = editor.selectionEnd;
      const lineStart = start === 0 ? 0 : editor.value.lastIndexOf("\n", start - 1) + 1;
      if (start === end && !event.shiftKey) {
        replaceSelection(editor, "  ");
      } else if (start === end) {
        const indent = editor.value.slice(lineStart).match(/^(?: {1,2}|\t)/)?.[0] ?? "";
        if (!indent) return;
        editor.setSelectionRange(lineStart, lineStart + indent.length);
        replaceSelection(editor, "");
        editor.setSelectionRange(
          Math.max(lineStart, start - indent.length),
          Math.max(lineStart, end - indent.length),
        );
      } else {
        const blockEnd = end > start && editor.value[end - 1] === "\n" ? end - 1 : end;
        const selected = editor.value.slice(lineStart, blockEnd);
        const replacement = selected
          .split("\n")
          .map((line) => (event.shiftKey ? line.replace(/^(?: {1,2}|\t)/, "") : `  ${line}`))
          .join("\n");
        editor.setSelectionRange(lineStart, blockEnd);
        replaceSelection(editor, replacement);
        editor.setSelectionRange(lineStart, lineStart + replacement.length);
      }
      updatePosition(editor);
    } else if (event.key === "Enter" && editor.selectionStart === editor.selectionEnd) {
      const precedingLine = editor.value.slice(0, editor.selectionStart).split("\n").at(-1) ?? "";
      const indent = precedingLine.match(/^[\t ]*/)?.[0] ?? "";
      event.preventDefault();
      replaceSelection(editor, `\n${indent}${precedingLine.trimEnd().endsWith("{") ? "  " : ""}`);
    }
  }

  return (
    <div className="space-y-2">
      <div className="overflow-hidden rounded-lg border border-input bg-background text-foreground focus-within:border-ring focus-within:ring-1 focus-within:ring-ring">
        <textarea
          aria-label="Theme custom CSS"
          aria-describedby={hintId}
          autoCapitalize="off"
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          wrap="off"
          placeholder={'.dark [data-slot="sidebar"] {\n  backdrop-filter: blur(12px);\n}'}
          className="block h-44 w-full resize-y bg-transparent p-3 font-mono text-[12px] leading-5 outline-none"
          value={value}
          onFocus={() => setTabIndents(true)}
          onChange={(event) => {
            onChange(event.currentTarget.value);
            updatePosition(event.currentTarget);
          }}
          onSelect={(event) => updatePosition(event.currentTarget)}
          onKeyDown={handleKeyDown}
        />
        <div className="flex items-center justify-between border-t border-border bg-panel px-3 py-1 font-mono text-[11px] text-muted-foreground">
          <span>CSS · {value.length.toLocaleString()} / {THEME_CSS_MAX_LENGTH.toLocaleString()}</span>
          <span>Ln {position.line}, Col {position.column}</span>
        </div>
      </div>

      {hasRemoteFetch ? (
        <div role="alert" className="flex items-start gap-2 rounded-md border border-status-warning-border bg-status-warning-container px-2.5 py-2 text-[11px] text-status-warning-on">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>
            Remote fetches (@import, image-set(), and non-data:/#id url()) are stripped at runtime.
            Use data: URLs or var(--theme-background-image) for images.
          </span>
        </div>
      ) : null}

      {overLimit ? (
        <p role="alert" className="text-[11px] text-destructive">
          Theme CSS exceeds {THEME_CSS_MAX_LENGTH.toLocaleString()} characters.
        </p>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <p id={hintId} className="text-[11px] text-muted-foreground">
          {tabIndents
            ? "Tab indents. Shift+Tab unindents. Press Escape, then Tab to leave."
            : "Tab moves to the next control."}
        </p>
        <div className="flex items-center gap-1.5">
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => setShowHooks((prev) => !prev)}
            className="h-7 gap-1 px-2 text-[11px]"
          >
            <BookOpen className="size-3" aria-hidden="true" />
            {showHooks ? "Hide selectors" : "Stable selectors"}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={!value}
            onClick={() => {
              onChange("");
              setPosition({ line: 1, column: 1 });
            }}
            className="h-7 px-2 text-[11px]"
          >
            Clear CSS
          </Button>
        </div>
      </div>

      {showHooks ? (
        <div className="max-h-40 overflow-y-auto rounded-md border border-border bg-panel p-2.5 text-[11px]">
          <p className="mb-1.5 font-medium text-foreground">Stable CSS selectors (safe across releases):</p>
          <ul className="space-y-1 font-mono text-muted-foreground">
            {THEME_CSS_HOOKS.map((hook) => (
              <li key={hook.selector} className="flex items-baseline justify-between gap-2">
                <code className="text-foreground">{hook.selector}</code>
                <span className="font-sans text-[11px]">{hook.styles}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
