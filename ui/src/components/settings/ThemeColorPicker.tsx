import { memo, useEffect, useState } from "react";
import { RotateCcw } from "lucide-react";

import { parseThemeColor } from "../../lib/themeDocument";
import { themeColorToHex } from "../../lib/themePalettes";

export type ThemeColorFieldProps = {
  id: string;
  label: string;
  value: string;
  defaultValue?: string;
  highlighted?: boolean;
  onChange: (value: string) => void;
  onReset?: () => void;
};

export const ThemeColorField = memo(function ThemeColorField({
  id,
  label,
  value,
  defaultValue,
  highlighted,
  onChange,
  onReset,
}: ThemeColorFieldProps) {
  const hexValue = themeColorToHex(value) ?? "#000000";
  const [text, setText] = useState(value);
  const isDirty = defaultValue !== undefined && themeColorToHex(defaultValue) !== themeColorToHex(value);

  useEffect(() => {
    setText(value);
  }, [value]);

  const invalid = text.trim() !== "" && !parseThemeColor(text.trim());

  return (
    <div
      className={`flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5 transition-colors ${
        highlighted ? "border-primary bg-accent/40" : "border-border bg-background/60"
      }`}
    >
      <label htmlFor={id} className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground">
        {label}
      </label>
      <div className="flex shrink-0 items-center gap-1.5">
        <label className="relative inline-flex size-6 shrink-0 cursor-pointer items-center justify-center overflow-hidden rounded border border-border">
          <span className="sr-only">Pick {label} color</span>
          <span className="size-full" style={{ backgroundColor: hexValue }} />
          <input
            type="color"
            aria-label={`${label} color picker`}
            value={hexValue}
            onChange={(event) => {
              const next = event.target.value;
              setText(next);
              onChange(next);
            }}
            className="absolute inset-0 cursor-pointer opacity-0"
          />
        </label>
        <input
          id={id}
          type="text"
          aria-label={label}
          aria-invalid={invalid}
          value={text}
          spellCheck={false}
          autoCapitalize="none"
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            if (parseThemeColor(next.trim())) {
              onChange(next.trim());
            }
          }}
          onBlur={() => {
            if (!parseThemeColor(text.trim())) {
              setText(value);
            }
          }}
          className={`h-7 w-[112px] rounded border bg-background px-2 font-mono text-[11px] text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
            invalid ? "border-destructive" : "border-input"
          }`}
        />
        {onReset && isDirty ? (
          <button
            type="button"
            aria-label={`Reset ${label}`}
            title={`Reset ${label}`}
            onClick={onReset}
            className="inline-flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-surface-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <RotateCcw className="size-3" aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </div>
  );
});
