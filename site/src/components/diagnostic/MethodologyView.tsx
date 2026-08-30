import { CheckCircle } from "lucide-react";
import { evaluateAllCombinations } from "../../lib/diagnostic/scoring.ts";
import { TOOLS } from "../../data/tools.ts";

export interface MethodologyViewProps {
  readonly lang?: "ko" | "en";
}

export function MethodologyView({
  lang = "ko",
}: MethodologyViewProps) {
  const isKo = lang === "ko";
  const distribution = evaluateAllCombinations();

  const sortedStats = Object.values(distribution.toolStats).sort(
    (a, b) => b.share - a.share
  );

  return (
    <div className="flex flex-col gap-8 w-full max-w-4xl mx-auto py-2">
      <header className="flex flex-col gap-3 pb-6 border-b border-line">
        <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-ink">
          {isKo ? "진단 채점 방법론 및 공정성 증명" : "Scoring Methodology & Fairness Proof"}
        </h1>
        <p className="text-sm sm:text-base text-ink-soft leading-relaxed">
          {isKo
            ? "Ferryx 진단은 블랙박스 알고리즘이나 마케팅 가중치 없이, 6개 축의 1차 검증 사실과 가산 매칭 수식으로만 도구를 평가합니다."
            : "The diagnostic uses no black-box AI or marketing weights—only 6 verifiable axes and additive matching formulas."}
        </p>
      </header>

      <section className="flex flex-col gap-4">
        <h2 className="text-lg font-bold text-ink tracking-tight">
          {isKo ? "1. 채점 수식 및 동점 처리" : "1. Scoring Formula & Tie Handling"}
        </h2>
        <div className="p-4 rounded-xl bg-code-bg border border-code-border text-code-ink font-mono text-xs overflow-x-auto leading-relaxed">
          {`// Total score awarded to tool t:
Score(tool_t) = Sum_{axis in {A1..A6}} vectors[axis][user_option]
MatchPercentage(tool_t) = round((Score(tool_t) / 60) * 100)

// Tie Resolution:
if (top_scores_count > 1) {
  result = CoWinnerTie(winners) // Split win credit equally, declare both
}`}
        </div>
        <p className="text-xs sm:text-sm text-ink-soft leading-relaxed">
          {isKo
            ? "각 축은 0~10점의 명시적 가중치를 부여받으며, 전체 6축 만점은 60점입니다. 1위가 복수 도구일 경우 임의로 하나를 고르지 않고 공동 1위로 표기합니다."
            : "Each axis awards 0–10 points with a maximum total of 60. Co-winners are explicitly preserved rather than arbitrarily tie-broken."}
        </p>
      </section>

      <section className="flex flex-col gap-4">
        <h2 className="text-lg font-bold text-ink tracking-tight">
          {isKo ? "2. 전체 729개 경로 전수 시뮬레이션 결과" : "2. Full 729-Path Simulation Distribution"}
        </h2>
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
          <div className="p-4 rounded-xl border border-line bg-surface flex flex-col gap-1">
            <span className="text-xs font-mono text-ink-soft">{isKo ? "총 답변 경로" : "Total Combinations"}</span>
            <span className="text-2xl font-bold font-mono text-ink">{distribution.totalPaths}</span>
            <span className="text-[11px] text-ink-faint">3^6 = 729 paths</span>
          </div>
          <div className="p-4 rounded-xl border border-line bg-surface flex flex-col gap-1">
            <span className="text-xs font-mono text-ink-soft">{isKo ? "공동 1위 동점 경로" : "Co-Winner Tie Paths"}</span>
            <span className="text-2xl font-bold font-mono text-ink">{distribution.tiePathsCount}</span>
            <span className="text-[11px] text-ink-faint">10.4% tie rate</span>
          </div>
          <div className="p-4 rounded-xl border border-line bg-surface flex flex-col gap-1">
            <span className="text-xs font-mono text-ink-soft">{isKo ? "Ferryx 추천 점유율" : "Ferryx Recommendation Share"}</span>
            <span className="text-2xl font-bold font-mono text-ink">
              {((distribution.toolStats["ferryx"]?.share ?? 0) * 100).toFixed(2)}%
            </span>
            <span className="text-[11px] text-ink-faint">43.8 wins / 729</span>
          </div>
        </div>

        <div className="overflow-x-auto rounded-xl border border-line">
          <table className="w-full text-xs text-left">
            <thead className="bg-page border-b border-line text-ink-soft font-mono">
              <tr>
                <th className="p-3">도구 (Tool)</th>
                <th className="p-3">기저 (Substrate)</th>
                <th className="p-3">가중 승수 (Wins)</th>
                <th className="p-3">승률 점유율 (Win Share)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line bg-surface font-mono">
              {sortedStats.map((stat) => {
                const tool = TOOLS[stat.toolId];
                const pct = (stat.share * 100).toFixed(2);
                return (
                  <tr key={stat.toolId} className="hover:bg-page-raised/50">
                    <td className="p-3 font-semibold text-ink">{tool.name}</td>
                    <td className="p-3 text-ink-soft text-[11px]">{tool.substrate}</td>
                    <td className="p-3">{stat.winCount.toFixed(2)}</td>
                    <td className="p-3 font-semibold text-ink">{pct}%</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <section className="flex flex-col gap-4">
        <h2 className="text-lg font-bold text-ink tracking-tight">
          {isKo ? "3. 공정성 및 편향 방지 원칙" : "3. Anti-Gerrymandering Principles"}
        </h2>
        <ul className="space-y-2 text-xs sm:text-sm text-ink-soft">
          <li className="flex items-start gap-2">
            <CheckCircle className="w-4 h-4 text-status-success shrink-0 mt-0.5" />
            <span>
              {isKo
                ? "전체 답변 공간에서 Ferryx의 측정 승률은 6.01%이며, 가장 높은 도구도 20.64%였습니다. 이 분포는 채점 데이터가 바뀔 때마다 테스트에서 다시 계산합니다."
                : "Across the complete answer space, Ferryx measures 6.01% and the largest tool share is 20.64%. Tests recalculate this distribution whenever the scoring data changes."}
            </span>
          </li>
          <li className="flex items-start gap-2">
            <CheckCircle className="w-4 h-4 text-status-success shrink-0 mt-0.5" />
            <span>
              {isKo
                ? "모든 벡터의 점수에는 소스 코드 라인, 공식 문서 또는 GitHub API 실측 근거가 주석되어 있습니다."
                : "Every vector score is backed by source code line references or GitHub metrics."}
            </span>
          </li>
          <li className="flex items-start gap-2">
            <CheckCircle className="w-4 h-4 text-status-success shrink-0 mt-0.5" />
            <span>
              {isKo
                ? "이 데이터는 GitHub PR을 통해 누구나 정정 및 기여를 제안할 수 있습니다."
                : "Anyone can audit or propose vector corrections via GitHub pull requests."}
            </span>
          </li>
        </ul>
      </section>
    </div>
  );
}
