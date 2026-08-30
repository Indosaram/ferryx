import { useState } from "react";
import { ChevronDown, ExternalLink } from "lucide-react";
import { AXES, TOOLS } from "../../data/tools.ts";
import type { ToolId, AxisId } from "../../data/types.ts";
import { Badge } from "../ui/Badge.tsx";

export interface MatrixTableProps {
  readonly lang?: "ko" | "en";
}

const AXIS_NAMES: Record<AxisId, { ko: string; en: string }> = {
  A1: { ko: "A1: 인터페이스", en: "A1: Interface" },
  A2: { ko: "A2: 프로세스 지속성", en: "A2: Persistence" },
  A3: { ko: "A3: 작업 격리", en: "A3: Isolation" },
  A4: { ko: "A4: 감독/리뷰", en: "A4: Supervision" },
  A5: { ko: "A5: 호스트 플랫폼", en: "A5: Platform" },
  A6: { ko: "A6: 성숙도/라이선스", en: "A6: Maturity" },
};

export function MatrixTable({ lang = "ko" }: MatrixTableProps) {
  const isKo = lang === "ko";
  const toolList = Object.values(TOOLS);
  const [selectedTool, setSelectedTool] = useState<ToolId | null>(null);

  return (
    <div className="flex flex-col gap-6 w-full">
      <div className="hidden lg:block overflow-x-auto rounded-2xl border border-line bg-surface shadow-card">
        <table className="w-full text-xs text-left border-collapse min-w-[900px]">
          <caption className="sr-only">
            {isKo ? "도구별 6축 채점 및 1차 근거 행렬" : "Competitor Tools 6-Axis Scored Matrix"}
          </caption>
          <thead>
            <tr className="bg-page border-b border-line text-ink-soft font-mono">
              <th scope="col" className="sticky left-0 z-10 bg-page p-3.5 border-r border-line font-bold text-ink w-44">
                {isKo ? "도구 / 기저" : "Tool / Substrate"}
              </th>
              {AXES.map((axis) => (
                <th key={axis} scope="col" className="p-3.5 border-r border-line font-semibold">
                  <div>{axis}</div>
                  <div className="text-[10px] text-ink-faint font-normal mt-0.5">
                    {isKo ? AXIS_NAMES[axis].ko : AXIS_NAMES[axis].en}
                  </div>
                </th>
              ))}
              <th scope="col" className="p-3.5 font-semibold text-ink">
                {isKo ? "라이선스 · 스타" : "License · Stars"}
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {toolList.map((tool) => (
              <tr key={tool.id} className="hover:bg-page-raised/60 transition-colors">
                <th scope="row" className="sticky left-0 z-10 bg-surface group-hover:bg-page-raised/60 p-3.5 border-r border-line align-top">
                  <div className="flex flex-col gap-1">
                    <div className="flex items-center gap-1.5 font-bold text-ink font-mono text-sm">
                      <a href={tool.url} target="_blank" rel="noreferrer" className="hover:underline">
                        {tool.name}
                      </a>
                    </div>
                    <span className="text-[11px] text-ink-soft leading-snug">{tool.substrate}</span>
                    {tool.experimentalLabel && (
                      <Badge variant="secondary" className="w-fit text-[9px] px-1 py-0 bg-amber-500/10 text-amber-600 border-amber-500/30">
                        {isKo ? "실험적" : "Experimental"}
                      </Badge>
                    )}
                  </div>
                </th>

                {AXES.map((axis) => {
                  const vec = tool.vectors[axis];
                  const scores = vec.optionScores.join("/");
                  return (
                    <td key={axis} className="p-3.5 border-r border-line align-top font-mono">
                      <div className="font-semibold text-ink text-xs mb-1">[{scores}]</div>
                      <div className="text-[11px] text-ink-soft leading-tight">{vec.evidence}</div>
                    </td>
                  );
                })}

                <td className="p-3.5 align-top font-mono text-xs">
                  <div className="font-semibold text-ink">{tool.license}</div>
                  <div className="text-ink-soft text-[11px] mt-0.5">
                    {tool.stars !== null ? `★ ${tool.stars.toLocaleString()}` : "Proprietary"}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="lg:hidden flex flex-col gap-3">
        {toolList.map((tool) => {
          const isExpanded = selectedTool === tool.id;
          return (
            <div key={tool.id} className="rounded-xl border border-line bg-surface overflow-hidden">
              <button
                type="button"
                onClick={() => setSelectedTool(isExpanded ? null : tool.id)}
                className="w-full flex items-center justify-between p-4 text-left hover:bg-page-raised/60 transition-colors"
                aria-expanded={isExpanded}
              >
                <div className="flex flex-col gap-0.5">
                  <div className="flex items-center gap-2">
                    <span className="font-bold text-ink font-mono">{tool.name}</span>
                    {tool.experimentalLabel && (
                      <Badge variant="secondary" className="text-[9px] bg-amber-500/10 text-amber-600 border-amber-500/30">
                        {isKo ? "실험적" : "Experimental"}
                      </Badge>
                    )}
                  </div>
                  <span className="text-xs text-ink-soft">{tool.substrate}</span>
                </div>
                <ChevronDown className={`w-4 h-4 text-ink-soft transition-transform ${isExpanded ? "rotate-180" : ""}`} />
              </button>

              {isExpanded && (
                <div className="p-4 border-t border-line bg-page-raised/40 flex flex-col gap-3 text-xs">
                  <div className="font-mono text-ink-faint pb-2 border-b border-line">
                    라이선스: {tool.license} {tool.stars !== null ? `· ★ ${tool.stars.toLocaleString()}` : ""}
                  </div>

                  <div className="flex flex-col gap-2.5">
                    {AXES.map((axis) => {
                      const vec = tool.vectors[axis];
                      return (
                        <div key={axis} className="flex flex-col gap-0.5 font-mono">
                          <div className="flex justify-between font-semibold text-ink text-[11px]">
                            <span>{isKo ? AXIS_NAMES[axis].ko : AXIS_NAMES[axis].en}</span>
                            <span>[{vec.optionScores.join("/")}]</span>
                          </div>
                          <span className="text-[11px] text-ink-soft">{vec.evidence}</span>
                        </div>
                      );
                    })}
                  </div>

                  <div className="pt-2 border-t border-line">
                    <a
                      href={tool.url}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-flex items-center gap-1 text-ink hover:underline font-mono text-xs"
                    >
                      <span>공식 링크</span>
                      <ExternalLink className="w-3 h-3" />
                    </a>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
