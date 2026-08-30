import type { AxisId, OptionIndex, ToolId } from "../../data/types.ts";
import type { QuestionId } from "../../data/questions.ts";

export interface RawDiagnosticInput {
  readonly [key: string]: unknown;
}

export type DiagnosticAnswers = Readonly<Record<QuestionId, OptionIndex>>;

export type SixAxisAnswers = Readonly<Record<AxisId, OptionIndex>>;

export interface ValidationIssue {
  readonly path: string;
  readonly message: string;
}

export class DiagnosticValidationError extends Error {
  readonly issues: readonly ValidationIssue[];

  constructor(message: string, issues: readonly ValidationIssue[]) {
    super(message);
    this.name = "DiagnosticValidationError";
    this.issues = issues;
  }
}

export interface AxisContribution {
  readonly axisId: AxisId;
  readonly chosenOption: OptionIndex;
  readonly scoreAwarded: number;
  readonly evidence: string;
}

export interface ToolScore {
  readonly toolId: ToolId;
  readonly totalScore: number;
  readonly matchPercentage: number;
  readonly axisContributions: readonly AxisContribution[];
}

export interface ResultProfile {
  readonly slug: string;
  readonly titleKo: string;
  readonly titleEn: string;
  readonly summaryKo: string;
  readonly summaryEn: string;
}

export interface DiagnosticResult {
  readonly rankedTools: readonly ToolScore[];
  readonly winners: readonly ToolScore[];
  readonly isTie: boolean;
  readonly profile: ResultProfile;
  readonly explanation: string;
}

export interface ToolWinStats {
  readonly toolId: ToolId;
  readonly winCount: number;
  readonly share: number;
}

export interface DistributionEvaluationResult {
  readonly totalPaths: number;
  readonly tiePathsCount: number;
  readonly toolStats: Readonly<Record<string, ToolWinStats>>;
}
