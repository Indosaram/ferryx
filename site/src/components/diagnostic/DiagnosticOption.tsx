import { Check } from "lucide-react";
import type { OptionIndex } from "../../data/types.ts";

export interface DiagnosticOptionProps {
  readonly name: string;
  readonly optionIndex: OptionIndex;
  readonly keyNumber: number;
  readonly label: string;
  readonly description: string;
  readonly isSelected: boolean;
  readonly onSelect: (index: OptionIndex) => void;
  readonly disabled?: boolean;
}

export function DiagnosticOption({
  name,
  optionIndex,
  keyNumber,
  label,
  description,
  isSelected,
  onSelect,
  disabled = false,
}: DiagnosticOptionProps) {
  const inputId = `${name}-opt-${optionIndex}`;

  return (
    <label
      htmlFor={inputId}
      className={`group relative flex items-start gap-4 p-4 sm:p-5 rounded-xl border transition-colors duration-150 cursor-pointer min-h-[56px] select-none ${
        isSelected
          ? "border-ink bg-page-raised shadow-card"
          : "border-line bg-surface hover:border-line-strong hover:bg-page-raised/60"
      } ${disabled ? "opacity-50 cursor-not-allowed pointer-events-none" : ""}`}
    >
      <input
        type="radio"
        id={inputId}
        name={name}
        value={optionIndex}
        checked={isSelected}
        onChange={() => onSelect(optionIndex)}
        disabled={disabled}
        className="sr-only"
      />

      <div className="flex items-center gap-3 shrink-0 pt-0.5">
        <span
          className={`flex items-center justify-center w-6 h-6 rounded-md font-mono text-xs font-medium border transition-colors ${
            isSelected
              ? "bg-ink text-page border-ink"
              : "bg-page text-ink-soft border-line group-hover:border-line-strong"
          }`}
          aria-hidden="true"
        >
          {keyNumber}
        </span>

        <span
          className={`flex items-center justify-center w-5 h-5 rounded-full border transition-all ${
            isSelected
              ? "bg-ink border-ink text-page scale-100"
              : "border-line-strong bg-surface group-hover:border-ink-soft"
          }`}
          aria-hidden="true"
        >
          {isSelected && <Check className="w-3 h-3 stroke-[3]" />}
        </span>
      </div>

      <div className="flex-1 flex flex-col gap-1 min-w-0">
        <span
          className={`text-[15px] sm:text-base font-semibold leading-snug tracking-tight transition-colors ${
            isSelected ? "text-ink" : "text-ink/90 group-hover:text-ink"
          }`}
        >
          {label}
        </span>
        <span className="text-[13px] sm:text-sm text-ink-soft leading-relaxed">
          {description}
        </span>
      </div>
    </label>
  );
}
