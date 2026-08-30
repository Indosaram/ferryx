export type AxisId = "A1" | "A2" | "A3" | "A4" | "A5" | "A6";

export type OptionIndex = 0 | 1 | 2;

export type ToolId =
  | "herdr"
  | "conductor"
  | "tmux"
  | "orca"
  | "superset"
  | "warp"
  | "vibe-kanban"
  | "ferryx"
  | "cmux"
  | "claude-squad"
  | "crystal"
  | "zellij";

export interface ScoredAxisVector {
  readonly optionScores: readonly [number, number, number];
  readonly evidence: string;
}

export interface ToolProfile {
  readonly id: ToolId;
  readonly name: string;
  readonly repo?: string;
  readonly url: string;
  readonly license: string;
  readonly stars: number | null;
  readonly substrate: string;
  readonly tagline: string;
  readonly vectors: Readonly<Record<AxisId, ScoredAxisVector>>;
  readonly weaknesses: readonly string[];
  readonly experimentalLabel?: boolean;
}

export interface AxisDefinition {
  readonly id: AxisId;
  readonly nameKo: string;
  readonly nameEn: string;
  readonly optionsKo: readonly [string, string, string];
  readonly optionsEn: readonly [string, string, string];
}
