import { ExternalLink, RotateCcw, Award, AlertCircle } from "lucide-react";
import type { DiagnosticResult } from "../../lib/diagnostic/types.ts";
import { TOOLS } from "../../data/tools.ts";
import { DisclosurePanel } from "./DisclosurePanel.tsx";
import { Button } from "../ui/Button.tsx";
import { Badge } from "../ui/Badge.tsx";

export interface ResultViewProps {
  readonly result: DiagnosticResult;
  readonly onRestart?: () => void;
  readonly lang?: "ko" | "en";
  readonly basePath?: string;
}

export function ResultView({
  result,
  onRestart,
  lang = "ko",
  basePath = "/",
}: ResultViewProps) {
  const isKo = lang === "ko";
  const { winners, isTie, profile, rankedTools } = result;
  const topThree = rankedTools.slice(0, 3);
  const firstScore = topThree[0]?.totalScore ?? 0;
  const secondScore = topThree[1]?.totalScore ?? 0;
  const runnerUpDelta = Math.max(0, firstScore - secondScore);

  const containsFerryx = topThree.some((t) => t.toolId === "ferryx");

  return (
    <div className="flex flex-col gap-8 w-full max-w-3xl mx-auto py-2">
      <header className="flex flex-col gap-3 pb-6 border-b border-line">
        <div className="flex items-center gap-2 text-xs font-mono text-ink-soft">
          <Badge variant="secondary" className="font-mono text-[11px] px-2 py-0.5">
            {isKo ? "진단 프로필 결과" : "Diagnostic Profile Result"}
          </Badge>
          <span>•</span>
          <span>{isTie ? (isKo ? "공동 1위 동점" : "Co-Winner Tie") : (isKo ? "단일 매치" : "Top Match")}</span>
        </div>

        <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-ink">
          {isKo ? profile.titleKo : profile.titleEn}
        </h1>
        <p className="text-sm sm:text-base text-ink-soft leading-relaxed">
          {isKo ? profile.summaryKo : profile.summaryEn}
        </p>

        {isTie ? (
          <div className="mt-2 p-3.5 rounded-xl border border-line bg-page-raised text-xs text-ink leading-relaxed">
            <span className="font-semibold text-ink font-mono mr-1.5">[TIE DETECTED]</span>
            {isKo
              ? `당신의 요구사항에서는 ${winners.map((w) => TOOLS[w.toolId].name).join("와 ")}가 동일한 점수(${firstScore}/60점)로 공동 1위를 기록했습니다.`
              : `A co-winner tie occurred between ${winners.map((w) => TOOLS[w.toolId].name).join(" and ")} with equal score (${firstScore}/60).`}
          </div>
        ) : (
          runnerUpDelta > 0 && topThree[1] && (
            <div className="mt-1 text-xs font-mono text-ink-faint">
              {isKo
                ? `1위 ${TOOLS[topThree[0].toolId].name} vs 차점자 ${TOOLS[topThree[1].toolId].name} 점수 차이: +${runnerUpDelta}점 (${topThree[0].matchPercentage}% vs ${topThree[1].matchPercentage}%)`
                : `Delta to runner-up ${TOOLS[topThree[1].toolId].name}: +${runnerUpDelta} pts (${topThree[0].matchPercentage}% vs ${topThree[1].matchPercentage}%)`}
            </div>
          )
        )}
      </header>

      {containsFerryx && <DisclosurePanel lang={lang} basePath={basePath} />}

      <section aria-labelledby="top-ranked-heading" className="flex flex-col gap-4">
        <h2 id="top-ranked-heading" className="text-sm font-mono uppercase tracking-wider text-ink-soft">
          {isKo ? "추천 도구 순위 (Top Ranked Tools)" : "Top Ranked Tools"}
        </h2>

        <div className="flex flex-col gap-4">
          {topThree.map((scored, index) => {
            const tool = TOOLS[scored.toolId];
            const isWinner = winners.some((w) => w.toolId === scored.toolId);

            return (
              <article
                key={scored.toolId}
                className={`p-5 sm:p-6 rounded-2xl border transition-all ${
                  isWinner ? "border-ink bg-surface shadow-card" : "border-line bg-surface/70"
                }`}
              >
                <div className="flex flex-wrap items-start justify-between gap-3 pb-4 border-b border-line">
                  <div className="flex flex-col gap-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-ink-faint">#{index + 1}</span>
                      <h3 className="text-lg font-bold text-ink">{tool.name}</h3>
                      {tool.experimentalLabel && (
                        <Badge variant="secondary" className="text-[10px] bg-amber-500/10 text-amber-600 border-amber-500/30">
                          {isKo ? "실험적 신생" : "Experimental"}
                        </Badge>
                      )}
                      {isWinner && (
                        <span className="inline-flex items-center gap-1 text-[11px] font-mono px-2 py-0.5 rounded bg-ink text-page font-medium">
                          <Award className="w-3 h-3" />
                          {isKo ? "매칭 1위" : "Top Match"}
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-ink-soft">{tool.tagline}</p>
                  </div>

                  <div className="flex flex-col items-end">
                    <span className="text-xl font-bold font-mono text-ink">{scored.matchPercentage}%</span>
                    <span className="text-[11px] font-mono text-ink-faint">{scored.totalScore} / 60 pts</span>
                  </div>
                </div>

                <div className="py-3 flex flex-wrap gap-2 text-xs font-mono text-ink-soft">
                  <span className="px-2 py-1 rounded bg-page border border-line">
                    기저: {tool.substrate}
                  </span>
                  <span className="px-2 py-1 rounded bg-page border border-line">
                    라이선스: {tool.license}
                  </span>
                  {tool.stars !== null && (
                    <span className="px-2 py-1 rounded bg-page border border-line">
                      GitHub: ★ {tool.stars.toLocaleString()}
                    </span>
                  )}
                </div>

                <div className="mt-2 p-3 rounded-xl bg-red-500/[0.04] border border-red-500/20 text-xs">
                  <div className="flex items-center gap-1.5 font-semibold text-red-600 dark:text-red-400 mb-1">
                    <AlertCircle className="w-3.5 h-3.5" />
                    <span>{isKo ? "이 도구를 쓰지 말아야 할 때 (Weaknesses):" : "When NOT to use this tool:"}</span>
                  </div>
                  <ul className="list-disc list-inside space-y-0.5 text-ink-soft">
                    {tool.weaknesses.map((w, idx) => (
                      <li key={idx}>{w}</li>
                    ))}
                  </ul>
                </div>

                <div className="mt-4 flex items-center justify-between pt-3 border-t border-line text-xs font-mono">
                  <a
                    href={tool.url}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1.5 text-ink hover:underline font-medium"
                  >
                    <span>{tool.name} 공식 링크</span>
                    <ExternalLink className="w-3 h-3" />
                  </a>
                  {tool.repo && (
                    <a
                      href={`https://github.com/${tool.repo}`}
                      target="_blank"
                      rel="noreferrer"
                      className="text-ink-soft hover:text-ink hover:underline"
                    >
                      {tool.repo}
                    </a>
                  )}
                </div>
              </article>
            );
          })}
        </div>
      </section>

      <section aria-labelledby="axis-breakdown-heading" className="flex flex-col gap-3 pt-4 border-t border-line">
        <h2 id="axis-breakdown-heading" className="text-sm font-mono uppercase tracking-wider text-ink-soft">
          {isKo ? "선택 축별 점수 기여도 (Axis Contributions)" : "Axis Contributions"}
        </h2>
        <div className="overflow-x-auto rounded-xl border border-line">
          <table className="w-full text-xs text-left">
            <thead className="bg-page border-b border-line text-ink-soft font-mono">
              <tr>
                <th className="p-3">축 (Axis)</th>
                <th className="p-3">선택값</th>
                <th className="p-3">1위 득점</th>
                <th className="p-3">1차 검증 근거 (Primary Evidence)</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line bg-surface">
              {topThree[0]?.axisContributions.map((c) => (
                <tr key={c.axisId} className="hover:bg-page-raised/50">
                  <td className="p-3 font-mono font-semibold text-ink">{c.axisId}</td>
                  <td className="p-3 font-mono">Option {c.chosenOption}</td>
                  <td className="p-3 font-mono font-semibold text-ink">{c.scoreAwarded} / 10</td>
                  <td className="p-3 font-mono text-[11px] text-ink-soft">{c.evidence}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <div className="flex flex-wrap items-center justify-between gap-4 pt-6 border-t border-line">
        {onRestart ? (
          <Button type="button" variant="outline" onClick={onRestart} className="gap-2 text-xs">
            <RotateCcw className="w-3.5 h-3.5" />
            <span>{isKo ? "진단 다시하기" : "Retake Diagnostic"}</span>
          </Button>
        ) : (
          <a href={`${basePath}diagnostic/`}>
            <Button type="button" variant="outline" className="gap-2 text-xs">
              <RotateCcw className="w-3.5 h-3.5" />
              <span>{isKo ? "진단 시작하기" : "Take Diagnostic"}</span>
            </Button>
          </a>
        )}

        <div className="flex items-center gap-3">
          <a href={`${basePath}diagnostic/matrix/`}>
            <Button type="button" variant="ghost" className="text-xs">
              {isKo ? "전체 비교 행렬" : "Full Matrix"}
            </Button>
          </a>
          <a href={`${basePath}diagnostic/methodology/`}>
            <Button type="button" variant="ghost" className="text-xs">
              {isKo ? "채점 방법론" : "Methodology"}
            </Button>
          </a>
        </div>
      </div>
    </div>
  );
}
