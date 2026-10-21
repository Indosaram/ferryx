import { useRef, type KeyboardEvent, type ReactNode } from "react";

import { cn } from "../../lib/cn";

export type SegmentedControlOption<T extends string> = {
  value: T;
  label: ReactNode;
  /** Accessible name; defaults to the label when it is a string. */
  ariaLabel?: string;
  /** Optional count rendered beside the label in the warning tone. */
  count?: ReactNode;
  countTestId?: string;
  "data-shortcut"?: string;
};

type SegmentedControlProps<T extends string> = {
  label: string;
  value: T;
  options: readonly SegmentedControlOption<T>[];
  onChange: (value: T) => void;
  className?: string;
};

/**
 * ui/DESIGN.md section 2: a `role="tablist"` of equal-width tabs with `aria-selected` and
 * arrow-key roving. Selection is a fill, never a border.
 */
export function SegmentedControl<T extends string>({ label, value, options, onChange, className }: SegmentedControlProps<T>) {
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number | null = null;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") next = (index + 1) % options.length;
    else if (event.key === "ArrowLeft" || event.key === "ArrowUp") next = (index - 1 + options.length) % options.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = options.length - 1;
    if (next === null) return;
    event.preventDefault();
    tabRefs.current[next]?.focus();
    onChange(options[next].value);
  };

  return (
    <div role="tablist" aria-label={label} className={cn("flex w-full rounded-md bg-card p-0.5", className)}>
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => { tabRefs.current[index] = element; }}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-label={option.ariaLabel}
            tabIndex={selected ? 0 : -1}
            data-shortcut={option["data-shortcut"]}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => handleKeyDown(event, index)}
            className={cn(
              "flex h-6 min-w-0 flex-1 items-center justify-center gap-1 rounded-sm px-2 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
              selected ? "bg-worktree-sidebar-accent text-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            <span className="truncate">{option.label}</span>
            {option.count !== undefined && option.count !== null ? (
              <span data-testid={option.countTestId} className="shrink-0 text-status-warning">
                {option.count}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
