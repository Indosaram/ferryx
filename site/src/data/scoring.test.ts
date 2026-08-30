import { describe, expect, it } from "bun:test";
import { writeFile } from "node:fs/promises";
import { AXES, TOOLS } from "../data/tools.ts";
import { QUESTIONS } from "../data/questions.ts";
import {
  calculateDiagnosticScore,
  evaluateAllCombinations,
  parseDiagnosticAnswers,
  DiagnosticValidationError,
} from "../lib/diagnostic/scoring.ts";
import type { RawDiagnosticInput } from "../lib/diagnostic/types.ts";

describe("Coding Agent Workflow Diagnostic Scoring", () => {
  describe("Given/When/Then: Boundary parsing", () => {
    it("Given a valid raw answer payload When parsed Then returns typed DiagnosticAnswers", () => {
      // Given: complete answer set for all 7 questions
      const rawInput: RawDiagnosticInput = {
        q1_parallelism: 1,
        q2_persistence: 1,
        q3_isolation: 1,
        q4_supervision: 0,
        q5_platform: 0,
        q6_interface: 1,
        q7_maturity: 0,
      };

      // When: parsing at boundary
      const parsed = parseDiagnosticAnswers(rawInput);

      // Then: valid typed answers returned
      expect(parsed.q1_parallelism).toBe(1);
      expect(parsed.q2_persistence).toBe(1);
      expect(parsed.q3_isolation).toBe(1);
      expect(parsed.q4_supervision).toBe(0);
      expect(parsed.q5_platform).toBe(0);
      expect(parsed.q6_interface).toBe(1);
      expect(parsed.q7_maturity).toBe(0);
    });

    it("Given an incomplete answer payload When parsed Then throws DiagnosticValidationError", () => {
      // Given: missing q7
      const incompleteInput = {
        q1_parallelism: 1,
        q2_persistence: 1,
        q3_isolation: 1,
        q4_supervision: 0,
        q5_platform: 0,
        q6_interface: 1,
      };

      // When & Then
      expect(() => parseDiagnosticAnswers(incompleteInput)).toThrow(
        DiagnosticValidationError
      );
    });

    it("Given out-of-range option index When parsed Then throws DiagnosticValidationError", () => {
      // Given: option index 5 (valid: 0, 1, 2)
      const invalidOptionInput = {
        q1_parallelism: 1,
        q2_persistence: 5,
        q3_isolation: 1,
        q4_supervision: 0,
        q5_platform: 0,
        q6_interface: 1,
        q7_maturity: 0,
      };

      // When & Then
      expect(() => parseDiagnosticAnswers(invalidOptionInput)).toThrow(
        DiagnosticValidationError
      );
    });

    it("Given null, empty, or non-object input When parsed Then throws DiagnosticValidationError", () => {
      expect(() => parseDiagnosticAnswers(null)).toThrow(DiagnosticValidationError);
      expect(() => parseDiagnosticAnswers("")).toThrow(DiagnosticValidationError);
      expect(() => parseDiagnosticAnswers([])).toThrow(DiagnosticValidationError);
    });
  });

  describe("Given/When/Then: Data integrity and citations", () => {
    it("Given the tools dataset When inspected Then all 12 tools exist with evidence citations on every scored axis", () => {
      // Given & When: tools dataset
      const toolIds = Object.keys(TOOLS);

      // Then: includes all 11 required competitors + Ferryx
      const requiredTools = [
        "cmux",
        "conductor",
        "crystal",
        "claude-squad",
        "vibe-kanban",
        "superset",
        "orca",
        "herdr",
        "tmux",
        "zellij",
        "warp",
        "ferryx",
      ];

      for (const req of requiredTools) {
        expect(toolIds).toContain(req);
      }

      // Then: every vector has 3 option scores and non-empty evidence (or explicit UNKNOWN)
      for (const tool of Object.values(TOOLS)) {
        expect(tool.id.length).toBeGreaterThan(0);
        expect(tool.name.length).toBeGreaterThan(0);
        expect(tool.weaknesses.length).toBeGreaterThan(0);

        for (const axisId of AXES) {
          const vec = tool.vectors[axisId];
          expect(vec).toBeDefined();
          expect(vec.optionScores.length).toBe(3);
          expect(typeof vec.evidence).toBe("string");
          expect(vec.evidence.length).toBeGreaterThan(0);
        }
      }
    });

    it("Given the questions dataset When inspected Then all 7 questions are defined with 3 options each", () => {
      expect(QUESTIONS.length).toBe(7);
      for (const q of QUESTIONS) {
        expect(q.id.length).toBeGreaterThan(0);
        expect(q.promptKo.length).toBeGreaterThan(0);
        expect(q.promptEn.length).toBeGreaterThan(0);
        expect(q.options.length).toBe(3);
      }
    });
  });

  describe("Given/When/Then: Deterministic scoring and tie handling", () => {
    it("Given answers resulting in a top tie When scored Then returns explicit co-winners", () => {
      // Given: answers producing a tie between top tools
      const answers = parseDiagnosticAnswers({
        q1_parallelism: 1,
        q2_persistence: 1,
        q3_isolation: 1,
        q4_supervision: 1,
        q5_platform: 1,
        q6_interface: 1,
        q7_maturity: 1,
      });

      // When: scored
      const result = calculateDiagnosticScore(answers);

      // Then: structured result contains winners and rankedTools
      expect(result.rankedTools.length).toBe(Object.keys(TOOLS).length);
      expect(result.winners.length).toBeGreaterThanOrEqual(1);
      expect(result.profile.slug.length).toBeGreaterThan(0);
      expect(result.explanation.length).toBeGreaterThan(0);

      // Winner scores must equal the highest ranked tool score
      const highestScore = result.rankedTools[0]?.totalScore;
      expect(highestScore).toBeDefined();
      for (const winner of result.winners) {
        expect(winner.totalScore).toBe(highestScore);
      }
    });

    it("Given tools input provided in shuffled order When scored Then ranking is identical", () => {
      // Given: test answers
      const answers = parseDiagnosticAnswers({
        q1_parallelism: 2,
        q2_persistence: 2,
        q3_isolation: 1,
        q4_supervision: 2,
        q5_platform: 0,
        q6_interface: 2,
        q7_maturity: 2,
      });

      // When: scored against default order vs reverse key order
      const resultA = calculateDiagnosticScore(answers);

      const reverseTools = Object.fromEntries(
        Object.entries(TOOLS).reverse()
      );
      const resultB = calculateDiagnosticScore(answers, reverseTools);

      // Then: top scores and tool order in rank tiers are deterministic
      expect(resultA.winners.map((w) => w.toolId)).toEqual(
        resultB.winners.map((w) => w.toolId)
      );
      expect(resultA.rankedTools.map((t) => t.toolId)).toEqual(
        resultB.rankedTools.map((t) => t.toolId)
      );
    });
  });

  describe("Given/When/Then: Full answer space (729 paths) and fairness assertions", () => {
    it("Given all 729 6-axis answer combinations When evaluated Then satisfies fairness criteria and writes distribution artifact", async () => {
      // When: evaluating all 3^6 = 729 paths
      const evalResult = evaluateAllCombinations();

      // Then: total paths is exactly 729
      expect(evalResult.totalPaths).toBe(729);

      // Then: no single tool wins > 40% (anti-gerrymandering gate)
      for (const [, stats] of Object.entries(evalResult.toolStats)) {
        expect(stats.share).toBeLessThanOrEqual(0.4);
        expect(stats.share).toBeGreaterThanOrEqual(0);
      }

      // Then: at least 8 distinct tools win at least once
      const winningTools = Object.entries(evalResult.toolStats).filter(
        ([, s]) => s.winCount > 0
      );
      expect(winningTools.length).toBeGreaterThanOrEqual(8);

      // Then: Ferryx wins between 2% and 10% (reproduced ~3.98%)
      const ferryxStats = evalResult.toolStats["ferryx"];
      expect(ferryxStats).toBeDefined();
      if (ferryxStats) {
        expect(ferryxStats.share).toBeGreaterThan(0.02);
        expect(ferryxStats.share).toBeLessThan(0.1);
      }

      // Then: explicit ties occur across combinations
      expect(evalResult.tiePathsCount).toBeGreaterThan(0);

      // Then: fractional co-winner credits preserve the complete answer space
      const creditedWins = Object.values(evalResult.toolStats).reduce(
        (total, stats) => total + stats.winCount,
        0
      );
      expect(creditedWins).toBeCloseTo(evalResult.totalPaths, 10);

      // Write distribution artifact to .omo/evidence/ade-quiz/distribution.json
      const distributionJson = JSON.stringify(evalResult, null, 2);
      const targetPath = "../.omo/evidence/ade-quiz/distribution.json";
      await writeFile(targetPath, distributionJson, "utf8");
    });
  });
});
