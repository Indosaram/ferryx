export interface StepProgressProps {
  readonly currentStep: number;
  readonly totalSteps: number;
  readonly lang?: "ko" | "en";
  readonly categoryLabel?: string;
}

export function StepProgress({
  currentStep,
  totalSteps,
  lang = "ko",
  categoryLabel,
}: StepProgressProps) {
  const percent = Math.min(100, Math.round((currentStep / totalSteps) * 100));
  const currentFormatted = String(currentStep).padStart(2, "0");
  const totalFormatted = String(totalSteps).padStart(2, "0");

  const progressText =
    lang === "ko"
      ? `진행 상황: ${totalSteps}개 문항 중 ${currentStep}번째`
      : `Progress: step ${currentStep} of ${totalSteps}`;

  return (
    <div
      className="flex flex-col gap-2 w-full pb-4 border-b border-line"
      role="region"
      aria-label={progressText}
    >
      <div className="flex items-center justify-between text-xs font-mono">
        <div className="flex items-center gap-2">
          <span className="text-ink font-semibold tracking-tight">
            {currentFormatted}
          </span>
          <span className="text-ink-faint">/</span>
          <span className="text-ink-faint">{totalFormatted}</span>
          {categoryLabel !== undefined && categoryLabel.length > 0 && (
            <span className="ml-2 px-2 py-0.5 rounded bg-surface border border-line text-ink-soft text-[11px]">
              {categoryLabel}
            </span>
          )}
        </div>
        <span className="text-ink-faint">{percent}%</span>
      </div>

      <div className="relative w-full h-1.5 bg-line rounded-full overflow-hidden">
        <div
          className="h-full bg-ink transition-all duration-200 ease-out"
          style={{ width: `${percent}%` }}
        />
        <progress
          className="sr-only"
          value={currentStep}
          max={totalSteps}
          aria-label={progressText}
        >
          {percent}%
        </progress>
      </div>

      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {progressText}
      </div>
    </div>
  );
}
