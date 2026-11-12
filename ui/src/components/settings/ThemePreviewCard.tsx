import { useLayoutEffect, useRef, type KeyboardEvent } from "react";

import type { AppearanceAccent, AppearanceTheme } from "../../lib/appearanceSettings";
import { cn } from "../../lib/cn";

export const THEME_OPTIONS: ReadonlyArray<{ value: AppearanceTheme; label: string }> = [
  { value: "charcoal", label: "Charcoal" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
  { value: "system", label: "System" },
];

export const ACCENT_OPTIONS: ReadonlyArray<{ value: AppearanceAccent; label: string }> = [
  { value: "default", label: "Slate" },
  { value: "blue", label: "Ocean Blue" },
  { value: "emerald", label: "Emerald" },
  { value: "purple", label: "Violet" },
  { value: "amber", label: "Amber" },
  { value: "rose", label: "Rose" },
];

/** Arrow/Home/End roving for a radiogroup that selects as the focus moves. */
function rove<T>(event: KeyboardEvent<HTMLElement>, index: number, options: readonly T[], pick: (value: T) => void) {
  let next: number | null = null;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") next = (index + 1) % options.length;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = (index - 1 + options.length) % options.length;
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = options.length - 1;
  if (next === null) return;
  event.preventDefault();
  pick(options[next]);
}

/** Palette comes from the [data-theme-card] ancestor; the art only draws the layout. */
function ThemePreviewArt() {
  return (
    <span className="theme-preview-art" aria-hidden="true">
      <span className="theme-preview-side" />
      <span className="theme-preview-pane">
        <i className="theme-preview-bar" />
        <i className="theme-preview-bar" />
        <b className="theme-preview-dot" />
      </span>
    </span>
  );
}

/**
 * Theme presets as a comparison strip: each card paints its own palette no matter which theme is
 * active, so the choice is visual instead of four words in a dropdown. Radiogroup semantics with
 * roving focus; the strip scrolls to the selected card because a narrow pane cannot show them all.
 */
export function ThemePreviewStrip({
  value,
  onChange,
}: {
  value: AppearanceTheme;
  onChange: (value: AppearanceTheme) => void;
}) {
  const cardRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useLayoutEffect(() => {
    const index = THEME_OPTIONS.findIndex((option) => option.value === value);
    cardRefs.current[index]?.scrollIntoView?.({ block: "nearest", inline: "center" });
  }, [value]);

  return (
    <div role="radiogroup" aria-label="Theme mode" className="scrollbar-none flex gap-1.5 overflow-x-auto p-0.5">
      {THEME_OPTIONS.map((option, index) => {
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              cardRefs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={option.label}
            data-theme-card={option.value}
            tabIndex={checked ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) =>
              rove(event, index, THEME_OPTIONS, (next) => {
                onChange(next.value);
                cardRefs.current[THEME_OPTIONS.indexOf(next)]?.focus();
              })
            }
            className="theme-preview-card flex w-[104px] shrink-0 flex-col gap-1.5 rounded-lg p-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <ThemePreviewArt />
            <span
              className={cn(
                "truncate px-0.5 text-[11px] leading-4",
                checked ? "font-semibold text-foreground" : "text-muted-foreground",
              )}
            >
              {option.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** The accent choice as the colour itself: six dots, pressed state ringed, name in the tooltip. */
export function AccentSwatchRow({
  value,
  onChange,
}: {
  value: AppearanceAccent;
  onChange: (value: AppearanceAccent) => void;
}) {
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);

  return (
    <div role="radiogroup" aria-label="Accent color" className="flex flex-wrap items-center gap-2">
      {ACCENT_OPTIONS.map((option, index) => {
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              buttonRefs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={option.label}
            title={option.label}
            data-accent-color={option.value}
            tabIndex={checked ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) =>
              rove(event, index, ACCENT_OPTIONS, (next) => {
                onChange(next.value);
                buttonRefs.current[ACCENT_OPTIONS.indexOf(next)]?.focus();
              })
            }
            className="accent-swatch flex size-6 items-center justify-center rounded-full focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <span className="accent-swatch-dot" aria-hidden="true" />
          </button>
        );
      })}
      <span className="ml-1 text-[11px] text-muted-foreground">
        {ACCENT_OPTIONS.find((option) => option.value === value)?.label}
      </span>
    </div>
  );
}
