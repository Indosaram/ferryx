import { ArrowLeft, ArrowRight } from "lucide-react";
import type { QuestionDefinition } from "../../data/questions.ts";
import type { OptionIndex } from "../../data/types.ts";
import { DiagnosticOption } from "./DiagnosticOption.tsx";
import { Button } from "../ui/Button.tsx";

export interface QuestionCardProps {
  readonly question: QuestionDefinition;
  readonly questionIndex: number;
  readonly totalQuestions: number;
  readonly selectedOption: OptionIndex | undefined;
  readonly onSelectOption: (option: OptionIndex) => void;
  readonly onNext: () => void;
  readonly onPrev: () => void;
  readonly lang?: "ko" | "en";
  readonly validationError?: boolean;
}

export function QuestionCard({
  question,
  questionIndex,
  totalQuestions,
  selectedOption,
  onSelectOption,
  onNext,
  onPrev,
  lang = "ko",
  validationError = false,
}: QuestionCardProps) {
  const isKo = lang === "ko";
  const prompt = isKo ? question.promptKo : question.promptEn;
  const isFirst = questionIndex === 0;
  const isLast = questionIndex === totalQuestions - 1;

  return (
    <div className="flex flex-col gap-6 w-full">
      <fieldset className="flex flex-col gap-5 border-none p-0 m-0">
        <legend className="text-xl sm:text-2xl font-bold tracking-tight text-ink mb-2">
          {prompt}
        </legend>

        {validationError && (
          <div
            role="alert"
            className="p-3 rounded-lg border border-red-500/40 bg-red-500/10 text-xs text-red-600 dark:text-red-400 font-medium"
          >
            {isKo
              ? "다음으로 넘어가려면 항목을 선택해 주세요."
              : "Please select an option to continue."}
          </div>
        )}

        <div className="flex flex-col gap-3">
          {question.options.map((opt, idx) => (
            <DiagnosticOption
              key={opt.optionIndex}
              name={question.id}
              optionIndex={opt.optionIndex}
              keyNumber={idx + 1}
              label={isKo ? opt.labelKo : opt.labelEn}
              description={isKo ? opt.descriptionKo : opt.descriptionEn}
              isSelected={selectedOption === opt.optionIndex}
              onSelect={onSelectOption}
            />
          ))}
        </div>
      </fieldset>

      <div className="flex items-center justify-between pt-4 border-t border-line mt-2">
        <Button
          type="button"
          variant="outline"
          onClick={onPrev}
          disabled={isFirst}
          className="gap-2 text-xs font-medium"
        >
          <ArrowLeft className="w-3.5 h-3.5" />
          <span>{isKo ? "이전 문항" : "Previous"}</span>
        </Button>

        <div className="flex items-center gap-3">
          <span className="hidden sm:inline text-xs font-mono text-ink-faint">
            {isKo ? "1~3 선택 · Enter 다음" : "Keys 1-3 · Enter Next"}
          </span>
          <Button
            type="button"
          variant="default"
            onClick={onNext}
            className="gap-2 text-xs font-medium"
          >
            <span>
              {isLast
                ? isKo
                  ? "결과 확인하기"
                  : "View Results"
                : isKo
                ? "다음 문항"
                : "Next"}
            </span>
            <ArrowRight className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>
    </div>
  );
}
