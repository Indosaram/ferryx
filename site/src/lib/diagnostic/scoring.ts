import type { OptionIndex, ToolId, ToolProfile } from "../../data/types.ts";
import { AXES, TOOLS } from "../../data/tools.ts";
import { resolveResultProfile } from "./profiles.ts";
import type {
  AxisContribution,
  DiagnosticAnswers,
  DiagnosticResult,
  DistributionEvaluationResult,
  SixAxisAnswers,
  ToolScore,
  ToolWinStats,
  ValidationIssue,
} from "./types.ts";
import { DiagnosticValidationError } from "./types.ts";

export { DiagnosticValidationError };

function isOptionIndex(value: unknown): value is OptionIndex {
  return value === 0 || value === 1 || value === 2;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function parseDiagnosticAnswers(raw: unknown): DiagnosticAnswers {
  if (!isRecord(raw)) {
    throw new DiagnosticValidationError("Answers input must be a non-null object", [
      { path: "root", message: `Expected object, received ${typeof raw}` },
    ]);
  }

  const issues: ValidationIssue[] = [];

  const rawQ1 = raw["q1_parallelism"];
  const rawQ2 = raw["q2_persistence"];
  const rawQ3 = raw["q3_isolation"];
  const rawQ4 = raw["q4_supervision"];
  const rawQ5 = raw["q5_platform"];
  const rawQ6 = raw["q6_interface"];
  const rawQ7 = raw["q7_maturity"];

  const checkField = (id: string, val: unknown): OptionIndex => {
    if (val === undefined) {
      issues.push({ path: id, message: `Missing required question answer for ${id}` });
      return 0;
    }
    if (!isOptionIndex(val)) {
      issues.push({
        path: id,
        message: `Invalid option value for ${id}. Expected 0, 1, or 2, received ${String(val)}`,
      });
      return 0;
    }
    return val;
  };

  const q1 = checkField("q1_parallelism", rawQ1);
  const q2 = checkField("q2_persistence", rawQ2);
  const q3 = checkField("q3_isolation", rawQ3);
  const q4 = checkField("q4_supervision", rawQ4);
  const q5 = checkField("q5_platform", rawQ5);
  const q6 = checkField("q6_interface", rawQ6);
  const q7 = checkField("q7_maturity", rawQ7);

  if (issues.length > 0) {
    throw new DiagnosticValidationError("Validation failed for diagnostic answers", issues);
  }

  return {
    q1_parallelism: q1,
    q2_persistence: q2,
    q3_isolation: q3,
    q4_supervision: q4,
    q5_platform: q5,
    q6_interface: q6,
    q7_maturity: q7,
  };
}

export function mapAnswersToSixAxes(answers: DiagnosticAnswers): SixAxisAnswers {
  return {
    A1: answers.q6_interface,
    A2: answers.q2_persistence,
    A3: answers.q3_isolation,
    A4: answers.q4_supervision,
    A5: answers.q5_platform,
    A6: answers.q7_maturity,
  };
}

export function calculateDiagnosticScore(
  answers: DiagnosticAnswers,
  tools: Readonly<Record<string, ToolProfile>> = TOOLS
): DiagnosticResult {
  const sixAxes = mapAnswersToSixAxes(answers);
  const toolList = Object.values(tools);

  const scoredTools: ToolScore[] = [];

  for (const tool of toolList) {
    let totalScore = 0;
    const contributions: AxisContribution[] = [];

    for (const axisId of AXES) {
      const chosenOption = sixAxes[axisId];
      const vector = tool.vectors[axisId];
      const scoreAwarded = vector.optionScores[chosenOption];
      totalScore += scoreAwarded;

      contributions.push({
        axisId,
        chosenOption,
        scoreAwarded,
        evidence: vector.evidence,
      });
    }

    const matchPercentage = Math.round((totalScore / 60) * 100);

    scoredTools.push({
      toolId: tool.id,
      totalScore,
      matchPercentage,
      axisContributions: contributions,
    });
  }

  scoredTools.sort((a, b) => {
    if (b.totalScore !== a.totalScore) {
      return b.totalScore - a.totalScore;
    }
    return a.toolId.localeCompare(b.toolId);
  });

  const firstTool = scoredTools[0];
  const highestScore = firstTool !== undefined ? firstTool.totalScore : 0;
  const winners = scoredTools.filter((t) => t.totalScore === highestScore);
  const isTie = winners.length > 1;

  const firstWinner = winners[0];
  const primaryWinnerId = firstWinner !== undefined ? firstWinner.toolId : "tmux";
  const profile = resolveResultProfile(answers, primaryWinnerId);

  const explanation = buildExplanation(winners, sixAxes, isTie);

  return {
    rankedTools: scoredTools,
    winners,
    isTie,
    profile,
    explanation,
  };
}

function buildExplanation(
  winners: readonly ToolScore[],
  _axes: SixAxisAnswers,
  isTie: boolean
): string {
  if (winners.length === 0) {
    return "No matching tools found.";
  }

  const topScore = winners[0]?.totalScore ?? 0;
  if (isTie) {
    const winnerNames = winners.map((w) => w.toolId).join(", ");
    return `Top match resulted in a co-winner tie between ${winnerNames} (score: ${topScore}/60).`;
  }

  const winner = winners[0];
  const primaryId = winner?.toolId ?? "tool";
  return `${primaryId} emerged as your top match with a score of ${topScore}/60 based on your workflow priorities.`;
}

export function evaluateAllCombinations(
  tools: Readonly<Record<string, ToolProfile>> = TOOLS
): DistributionEvaluationResult {
  const toolList = Object.values(tools);
  const winCounts: Record<string, number> = {};
  for (const t of toolList) {
    winCounts[t.id] = 0;
  }

  let totalPaths = 0;
  let tiePathsCount = 0;

  const optionIndices: readonly OptionIndex[] = [0, 1, 2];

  for (const a1 of optionIndices) {
    for (const a2 of optionIndices) {
      for (const a3 of optionIndices) {
        for (const a4 of optionIndices) {
          for (const a5 of optionIndices) {
            for (const a6 of optionIndices) {
              totalPaths++;

              let maxScore = -1;
              let topToolIds: ToolId[] = [];

              for (const tool of toolList) {
                const score =
                  tool.vectors.A1.optionScores[a1] +
                  tool.vectors.A2.optionScores[a2] +
                  tool.vectors.A3.optionScores[a3] +
                  tool.vectors.A4.optionScores[a4] +
                  tool.vectors.A5.optionScores[a5] +
                  tool.vectors.A6.optionScores[a6];

                if (score > maxScore) {
                  maxScore = score;
                  topToolIds = [tool.id];
                } else if (score === maxScore) {
                  topToolIds.push(tool.id);
                }
              }

              if (topToolIds.length > 1) {
                tiePathsCount++;
              }

              const splitWin = 1 / topToolIds.length;
              for (const id of topToolIds) {
                const prev = winCounts[id] ?? 0;
                winCounts[id] = prev + splitWin;
              }
            }
          }
        }
      }
    }
  }

  const toolStats: Record<string, ToolWinStats> = {};
  for (const t of toolList) {
    const wins = winCounts[t.id] ?? 0;
    toolStats[t.id] = {
      toolId: t.id,
      winCount: wins,
      share: totalPaths > 0 ? wins / totalPaths : 0,
    };
  }

  return {
    totalPaths,
    tiePathsCount,
    toolStats,
  };
}
