import { useState, useEffect, useCallback } from "react";
import { QUESTIONS } from "../../data/questions.ts";
import type { OptionIndex } from "../../data/types.ts";
import type { DiagnosticAnswers } from "../../lib/diagnostic/types.ts";
import { calculateDiagnosticScore, parseDiagnosticAnswers } from "../../lib/diagnostic/scoring.ts";
import { sendDiagnosticEvent } from "../../lib/diagnostic/analytics.ts";
import { StepProgress } from "./StepProgress.tsx";
import { QuestionCard } from "./QuestionCard.tsx";
import { EvidenceRail } from "./EvidenceRail.tsx";

export interface DiagnosticAppProps {
  readonly lang?: "ko" | "en";
  readonly basePath?: string;
}

const STORAGE_KEY = "ferryx_diagnostic_answers_v1";

export function DiagnosticApp({ lang = "ko", basePath = "/" }: DiagnosticAppProps) {
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [answers, setAnswers] = useState<Partial<DiagnosticAnswers>>({});
  const [validationError, setValidationError] = useState(false);

  useEffect(() => {
    sendDiagnosticEvent({ name: "diagnostic_start", lang });
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved) {
        setAnswers(parseDiagnosticAnswers(JSON.parse(saved)));
      }
    } catch {
      // Ignore storage read errors
    }
  }, [lang]);

  const currentQuestion = QUESTIONS[currentStepIndex];
  const currentAnswer = currentQuestion ? answers[currentQuestion.id] : undefined;

  const handleSelectOption = useCallback((option: OptionIndex) => {
    if (!currentQuestion) return;
    setValidationError(false);
    setAnswers((prev) => {
      const next = { ...prev, [currentQuestion.id]: option };
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      } catch {
        // Ignore storage write errors
      }
      return next;
    });
    sendDiagnosticEvent({
      name: "question_answered",
      questionIndex: currentStepIndex,
      lang,
    });
  }, [currentQuestion, currentStepIndex, lang]);

  const handleNext = useCallback(() => {
    if (currentAnswer === undefined) {
      setValidationError(true);
      return;
    }
    if (currentStepIndex < QUESTIONS.length - 1) {
      setCurrentStepIndex((prev) => prev + 1);
    } else {
      const calculated = calculateDiagnosticScore(
        parseDiagnosticAnswers(answers)
      );
      sendDiagnosticEvent({
        name: "diagnostic_complete",
        resultType: calculated.profile.slug,
        primaryWinnerId: calculated.winners[0]?.toolId ?? "tmux",
        isTie: calculated.isTie,
        lang,
      });
      const languagePrefix = lang === "ko" ? "ko/" : "";
      window.location.assign(
        `${basePath}${languagePrefix}diagnostic/r/${calculated.profile.slug}/`
      );
    }
  }, [answers, basePath, currentAnswer, currentStepIndex, lang]);

  const handlePrev = useCallback(() => {
    if (currentStepIndex > 0) {
      setValidationError(false);
      setCurrentStepIndex((prev) => prev - 1);
    }
  }, [currentStepIndex]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        return;
      }
      if (e.key === "1") handleSelectOption(0);
      else if (e.key === "2") handleSelectOption(1);
      else if (e.key === "3") handleSelectOption(2);
      else if (e.key === "Enter") handleNext();
      else if (e.key === "ArrowLeft") handlePrev();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleSelectOption, handleNext, handlePrev]);

  if (!currentQuestion) return null;

  return (
    <div className="w-full max-w-5xl mx-auto py-2">
      <StepProgress
        currentStep={currentStepIndex + 1}
        totalSteps={QUESTIONS.length}
        lang={lang}
      />

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_22rem] gap-8 items-start mt-6">
        <main className="w-full">
          <QuestionCard
            question={currentQuestion}
            questionIndex={currentStepIndex}
            totalQuestions={QUESTIONS.length}
            selectedOption={currentAnswer}
            onSelectOption={handleSelectOption}
            onNext={handleNext}
            onPrev={handlePrev}
            lang={lang}
            validationError={validationError}
          />
        </main>

        <div className="w-full lg:sticky lg:top-24">
          <EvidenceRail
            question={currentQuestion}
            selectedOption={currentAnswer}
            lang={lang}
          />
        </div>
      </div>
    </div>
  );
}
