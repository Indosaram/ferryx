import { Moon, Sun } from "lucide-react";

import {
  buildThemePreviewGradient,
  previewColorsOf,
  type ThemeAppearance,
  type ThemeColors,
} from "../../lib/themePalettes";

export type ThemePreviewCirclesProps = {
  label: string;
  lightColors?: ThemeColors | Record<string, string>;
  darkColors?: ThemeColors | Record<string, string>;
  selectedLight?: boolean;
  selectedDark?: boolean;
  onSelectAppearance?: (appearance: ThemeAppearance) => void;
  size?: "sm" | "md";
};

function PreviewCircle({
  appearance,
  colors,
  selected,
  label,
  size,
  onClick,
}: {
  appearance: ThemeAppearance;
  colors: ThemeColors | Record<string, string>;
  selected?: boolean;
  label: string;
  size: "sm" | "md";
  onClick?: () => void;
}) {
  const preview = previewColorsOf(colors, appearance);
  const background = buildThemePreviewGradient(preview, appearance);
  const dim = size === "sm" ? "size-7" : "size-10";
  const ring = selected
    ? "ring-2 ring-primary ring-offset-2 ring-offset-panel"
    : "ring-1 ring-border hover:ring-foreground/40";

  if (!onClick) {
    return (
      <span
        aria-hidden="true"
        className={`relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full ${dim} ${ring}`}
        style={{ background }}
      />
    );
  }

  return (
    <button
      type="button"
      aria-label={`Use ${label} (${appearance})`}
      aria-pressed={Boolean(selected)}
      title={`Use ${label} for ${appearance} mode`}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
      className={`group relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full transition-shadow focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${dim} ${ring}`}
      style={{ background }}
    >
      <span className="sr-only">{appearance}</span>
      <span className="inline-flex size-4 items-center justify-center rounded-full bg-background/70 text-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
        {appearance === "light" ? (
          <Sun className="size-2.5" aria-hidden="true" />
        ) : (
          <Moon className="size-2.5" aria-hidden="true" />
        )}
      </span>
    </button>
  );
}

export function ThemePreviewCircles({
  label,
  lightColors,
  darkColors,
  selectedLight,
  selectedDark,
  onSelectAppearance,
  size = "md",
}: ThemePreviewCirclesProps) {
  return (
    <div className="flex items-center gap-1.5">
      {lightColors ? (
        <PreviewCircle
          appearance="light"
          colors={lightColors}
          selected={selectedLight}
          label={label}
          size={size}
          onClick={onSelectAppearance ? () => onSelectAppearance("light") : undefined}
        />
      ) : null}
      {darkColors ? (
        <PreviewCircle
          appearance="dark"
          colors={darkColors}
          selected={selectedDark}
          label={label}
          size={size}
          onClick={onSelectAppearance ? () => onSelectAppearance("dark") : undefined}
        />
      ) : null}
    </div>
  );
}
