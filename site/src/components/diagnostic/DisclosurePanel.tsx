import { AlertTriangle, ExternalLink } from "lucide-react";

export interface DisclosurePanelProps {
  readonly lang?: "ko" | "en";
  readonly basePath?: string;
}

export function DisclosurePanel({
  lang = "ko",
  basePath = "/",
}: DisclosurePanelProps) {
  const isKo = lang === "ko";

  return (
    <section
      aria-labelledby="ferryx-disclosure-title"
      className="p-5 rounded-xl border border-amber-500/40 bg-amber-500/5 text-ink flex flex-col gap-3.5 my-4"
    >
      <div className="flex items-center gap-2.5">
        <span className="p-1 rounded bg-amber-500/20 text-amber-600 dark:text-amber-400">
          <AlertTriangle className="w-4 h-4" />
        </span>
        <h3
          id="ferryx-disclosure-title"
          className="text-xs sm:text-sm font-semibold tracking-tight text-ink"
        >
          {isKo
            ? "Ferryx 제작자 이해관계 및 신생 도구 공개 (Mandatory Disclosure)"
            : "Ferryx Ownership & Experimental Status Disclosure"}
        </h3>
      </div>

      <div className="text-xs text-ink-soft leading-relaxed flex flex-col gap-2">
        <p>
          {isKo ? (
            <>
              <strong>Ferryx</strong>는 2026년 8월 생성된 실험적 초기 단계(GitHub 1★)
              소프트웨어이며, <strong>이 진단 도구의 제작자가 개발 중인 프로젝트</strong>
              입니다. 제작자 편향 가능성이 존재하므로, 채점 기준과 1차 출처를 100% 공개하고
              있습니다.
            </>
          ) : (
            <>
              <strong>Ferryx</strong> is an experimental early-stage (GitHub 1★) project
              created in August 2026, developed by the author of this diagnostic. All
              scoring vectors, formulas, and primary citations are fully public to ensure
              transparency.
            </>
          )}
        </p>

        <div className="pt-2 border-t border-amber-500/20 text-[11px]">
          <div className="font-semibold text-ink mb-1">
            {isKo ? "왜 다른 도구가 더 적합할 수 있는가:" : "Why alternatives may be better:"}
          </div>
          <ul className="list-disc list-inside space-y-0.5 text-ink-soft">
            <li>
              {isKo
                ? "데몬 무중단 업그레이드 및 라이브 FD 핸드오프: herdr 권장"
                : "Live FD handoff across daemon upgrades: herdr is recommended"}
            </li>
            <li>
              {isKo
                ? "15년간 검증된 절대적 안정성과 원격 편재성: tmux 권장"
                : "15+ years proven stability and ubiquity: tmux is recommended"}
            </li>
            <li>
              {isKo
                ? "100+ 병렬 worktree 및 시각적 Monaco diff 리뷰: Superset 권장"
                : "100+ parallel worktrees with visual Monaco diff: Superset is recommended"}
            </li>
            <li>
              {isKo
                ? "macOS 네이티브 인터페이스 최고 완성도: cmux 권장"
                : "Refined native macOS polish: cmux is recommended"}
            </li>
          </ul>
        </div>

        <div className="pt-2 flex flex-wrap items-center gap-3 text-[11px] font-mono">
          <a
            href={`${basePath}diagnostic/methodology/`}
            className="text-ink hover:underline inline-flex items-center gap-1 font-medium"
          >
            <span>{isKo ? "채점 방법론 및 공정성 데이터" : "Scoring Methodology"}</span>
            <ExternalLink className="w-3 h-3" />
          </a>
          <a
            href={`${basePath}diagnostic/matrix/`}
            className="text-ink hover:underline inline-flex items-center gap-1 font-medium"
          >
            <span>{isKo ? "전체 도구 사실 행렬" : "Full Evidence Matrix"}</span>
            <ExternalLink className="w-3 h-3" />
          </a>
        </div>
      </div>
    </section>
  );
}
