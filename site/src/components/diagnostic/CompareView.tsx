import { ExternalLink } from "lucide-react";
import type { ComparePairDefinition } from "../../data/compare.ts";
import { TOOLS, AXES } from "../../data/tools.ts";
import { Badge } from "../ui/Badge.tsx";

export interface CompareViewProps {
  readonly pair: ComparePairDefinition;
  readonly lang?: "ko" | "en";
  readonly basePath?: string;
}

const AXIS_LABELS: Record<string, { ko: string; en: string }> = {
  A1: { ko: "A1: 인터페이스 기저", en: "A1: Interface Substrate" },
  A2: { ko: "A2: 세션/프로세스 지속성", en: "A2: Session Persistence" },
  A3: { ko: "A3: 작업 공간 격리", en: "A3: Workspace Isolation" },
  A4: { ko: "A4: 감독 및 리뷰 큐", en: "A4: Supervision & Review" },
  A5: { ko: "A5: 호스트 플랫폼 지원", en: "A5: Host Platform" },
  A6: { ko: "A6: 프로젝트 성숙도", en: "A6: Project Maturity" },
};

export function CompareView({
  pair,
  lang = "ko",
  basePath = "/",
}: CompareViewProps) {
  const isKo = lang === "ko";
  const toolA = TOOLS[pair.toolA];
  const toolB = TOOLS[pair.toolB];

  return (
    <div className="flex flex-col gap-8 w-full max-w-4xl mx-auto py-2">
      <header className="flex flex-col gap-3 pb-6 border-b border-line">
        <div className="flex items-center gap-2 text-xs font-mono text-ink-soft">
          <Badge variant="secondary" className="font-mono text-[11px] px-2 py-0.5">
            {isKo ? "중립 아키텍처 비교" : "Neutral Architectural Comparison"}
          </Badge>
          <span>•</span>
          <span>{pair.slug}</span>
        </div>

        <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-ink">
          {isKo ? pair.titleKo : pair.titleEn}
        </h1>
        <p className="text-sm sm:text-base text-ink-soft leading-relaxed">
          {isKo ? pair.summaryKo : pair.summaryEn}
        </p>

        <div className="mt-2 p-4 rounded-xl border border-line bg-surface flex flex-col gap-1.5 text-xs">
          <span className="font-semibold font-mono text-ink">
            {isKo ? "핵심 차이점 (Key Differentiator):" : "Key Differentiator:"}
          </span>
          <p className="text-ink-soft leading-relaxed">
            {isKo ? pair.keyDifferentiatorKo : pair.keyDifferentiatorEn}
          </p>
        </div>
      </header>

      <section className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {[toolA, toolB].map((tool) => (
          <div
            key={tool.id}
            className="p-5 rounded-2xl border border-line bg-surface flex flex-col justify-between gap-4"
          >
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <h2 className="text-xl font-bold text-ink font-mono">{tool.name}</h2>
                {tool.experimentalLabel && (
                  <Badge variant="secondary" className="text-[10px] bg-amber-500/10 text-amber-600 border-amber-500/30">
                    {isKo ? "실험적" : "Experimental"}
                  </Badge>
                )}
              </div>
              <p className="text-xs text-ink-soft">{tool.tagline}</p>

              <div className="mt-2 flex flex-wrap gap-1.5 text-[11px] font-mono text-ink-soft">
                <span className="px-2 py-0.5 rounded bg-page border border-line">기저: {tool.substrate}</span>
                <span className="px-2 py-0.5 rounded bg-page border border-line">라이선스: {tool.license}</span>
                {tool.stars !== null && (
                  <span className="px-2 py-0.5 rounded bg-page border border-line">★ {tool.stars.toLocaleString()}</span>
                )}
              </div>
            </div>

            <div className="p-3 rounded-lg bg-page border border-line text-xs">
              <span className="font-semibold text-ink block mb-1">
                {isKo ? "주요 제약 (Weaknesses):" : "Key Weaknesses:"}
              </span>
              <ul className="list-disc list-inside space-y-0.5 text-ink-soft">
                {tool.weaknesses.map((w, idx) => (
                  <li key={idx}>{w}</li>
                ))}
              </ul>
            </div>

            <div className="pt-2 border-t border-line">
              <a
                href={tool.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 text-xs font-mono text-ink hover:underline font-medium"
              >
                <span>{tool.name} 공식 링크</span>
                <ExternalLink className="w-3 h-3" />
              </a>
            </div>
          </div>
        ))}
      </section>

      <section className="flex flex-col gap-3 pt-4 border-t border-line">
        <h2 className="text-sm font-mono uppercase tracking-wider text-ink-soft">
          {isKo ? "6개 축별 1차 검증 근거 비교" : "6-Axis Detailed Evidence Comparison"}
        </h2>

        <div className="overflow-x-auto rounded-xl border border-line">
          <table className="w-full text-xs text-left">
            <thead className="bg-page border-b border-line text-ink-soft font-mono">
              <tr>
                <th className="p-3 w-1/4">평가 축</th>
                <th className="p-3 w-3/8 font-bold text-ink">{toolA.name}</th>
                <th className="p-3 w-3/8 font-bold text-ink">{toolB.name}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line bg-surface font-mono">
              {AXES.map((axis) => {
                const vecA = toolA.vectors[axis];
                const vecB = toolB.vectors[axis];
                const label = isKo ? AXIS_LABELS[axis].ko : AXIS_LABELS[axis].en;

                return (
                  <tr key={axis} className="hover:bg-page-raised/40">
                    <td className="p-3 font-semibold text-ink align-top">{label}</td>
                    <td className="p-3 align-top border-l border-line">
                      <div className="font-semibold text-ink">[{vecA.optionScores.join("/")}]</div>
                      <div className="text-[11px] text-ink-soft mt-1 leading-snug">{vecA.evidence}</div>
                    </td>
                    <td className="p-3 align-top border-l border-line">
                      <div className="font-semibold text-ink">[{vecB.optionScores.join("/")}]</div>
                      <div className="text-[11px] text-ink-soft mt-1 leading-snug">{vecB.evidence}</div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <div className="flex items-center justify-between pt-6 border-t border-line text-xs font-mono">
        <a href={`${basePath}diagnostic/`} className="text-ink hover:underline">
          ← {isKo ? "진단 시작하기" : "Take Diagnostic"}
        </a>
        <a href={`${basePath}diagnostic/matrix/`} className="text-ink hover:underline">
          {isKo ? "전체 비교 행렬 보기" : "View Full Matrix"} →
        </a>
      </div>
    </div>
  );
}
