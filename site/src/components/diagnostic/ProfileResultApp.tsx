import { useState, useEffect } from "react";
import type { ResultProfile, DiagnosticAnswers, DiagnosticResult } from "../../lib/diagnostic/types.ts";
import { calculateDiagnosticScore, parseDiagnosticAnswers } from "../../lib/diagnostic/scoring.ts";
import { ResultView } from "./ResultView.tsx";
import { sendDiagnosticEvent } from "../../lib/diagnostic/analytics.ts";

export interface ProfileResultAppProps {
  readonly profile: ResultProfile;
  readonly lang?: "ko" | "en";
  readonly basePath?: string;
}

const STORAGE_KEY = "ferryx_diagnostic_answers_v1";

export function ProfileResultApp({
  profile,
  lang = "ko",
  basePath = "/",
}: ProfileResultAppProps) {
  const [result, setResult] = useState<DiagnosticResult | null>(null);

  useEffect(() => {
    sendDiagnosticEvent({
      name: "result_view",
      resultType: profile.slug,
      lang,
    });

    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved) {
        const calculated = calculateDiagnosticScore(
          parseDiagnosticAnswers(JSON.parse(saved))
        );
        setResult(calculated);
        return;
      }
    } catch {
      // Ignore
    }

    // Default static fallback result for direct hits
    const fallbackAnswers: DiagnosticAnswers = {
      q1_parallelism: 1,
      q2_persistence: profile.slug === "headless-persistence" ? 2 : 1,
      q3_isolation: profile.slug === "worktree-isolationist" ? 1 : (profile.slug === "cloud-orchestrator" ? 2 : 0),
      q4_supervision: profile.slug === "kanban-coordinator" ? 2 : 0,
      q5_platform: 0,
      q6_interface: profile.slug === "terminal-purist" ? 0 : (profile.slug === "web-diff-pilot" ? 2 : 1),
      q7_maturity: profile.slug === "experimental-pioneer" ? 0 : 1,
    };
    const fallbackCalculated = calculateDiagnosticScore(fallbackAnswers);
    setResult(fallbackCalculated);
  }, [profile, lang]);

  if (!result) {
    return (
      <div className="w-full max-w-3xl mx-auto py-12 text-center text-ink-soft">
        Loading diagnostic profile...
      </div>
    );
  }

  return (
    <ResultView
      result={result}
      lang={lang}
      basePath={basePath}
    />
  );
}
