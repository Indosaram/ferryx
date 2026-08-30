import { Info } from "lucide-react";
import type { QuestionDefinition } from "../../data/questions.ts";
import type { OptionIndex } from "../../data/types.ts";

export interface EvidenceRailProps {
  readonly question: QuestionDefinition;
  readonly selectedOption: OptionIndex | undefined;
  readonly lang?: "ko" | "en";
}

const AXIS_DETAILS: Record<
  string,
  {
    nameKo: string;
    nameEn: string;
    descriptionKo: string;
    descriptionEn: string;
    citation: string;
  }
> = {
  A1: {
    nameKo: "인터페이스 기저 (A1: Interface Substrate)",
    nameEn: "Interface Substrate (A1)",
    descriptionKo: "순수 터미널 TUI부터 네이티브 GPU 가속 GUI, 웹·Monaco 에디터 기반까지 인터페이스 반응성을 가릅니다.",
    descriptionEn: "Ranges from pure terminal TUI to native GPU GUI and Monaco-based web workbench.",
    citation: "O12 (cmux), O18 (herdr), O25 (superset), O27 (orca), O55 (ferryx)",
  },
  A2: {
    nameKo: "프로세스 지속성 (A2: Session Persistence)",
    nameEn: "Session Persistence (A2)",
    descriptionKo: "앱 종료 후에도 PTY 데몬이 생존하는지, 바이너리 업그레이드 시 라이브 FD를 핸드오프하는지 평가합니다.",
    descriptionEn: "Evaluates whether background PTY daemons survive UI quit and upgrade via live FD handoff.",
    citation: "O8 (tmux), O16 (herdr handoff.rs), O56 (ferryx daemon)",
  },
  A3: {
    nameKo: "작업 공간 격리 (A3: Workspace Isolation)",
    nameEn: "Workspace Isolation (A3)",
    descriptionKo: "단일 디렉토리 공유 vs git worktree 자동 분리 vs 클라우드 microVM 완전 격리 방식을 비교합니다.",
    descriptionEn: "Compares same directory vs branch worktrees vs cloud microVM sandboxes.",
    citation: "O18 (herdr), O27 (orca), O30 (conductor), O55 (ferryx)",
  },
  A4: {
    nameKo: "감독 및 리뷰 (A4: Supervision & Review)",
    nameEn: "Supervision & Review (A4)",
    descriptionKo: "터미널 다중 패널 직접 관찰부터 데스크톱/모바일 푸시 알림, 비동기 칸반 diff 큐까지 감독 구조를 결정합니다.",
    descriptionEn: "Determines supervision from direct terminal canvas monitoring to async Kanban diff queues.",
    citation: "O23 (vibe-kanban), O27 (orca), O31 (conductor), O47 (cmux)",
  },
  A5: {
    nameKo: "호스트 플랫폼 (A5: Host Platform)",
    nameEn: "Host Platform (A5)",
    descriptionKo: "macOS 전용 네이티브 폴리시부터 Windows/Linux 크로스플랫폼, SSH 헤드리스 지원 범위를 평가합니다.",
    descriptionEn: "Evaluates macOS native focus vs cross-platform desktop and SSH remote bridge.",
    citation: "O12 (cmux macOS), O30 (conductor macOS), O18 (herdr Windows/SSH)",
  },
  A6: {
    nameKo: "프로젝트 성숙도 (A6: Project Maturity)",
    nameEn: "Project Maturity (A6)",
    descriptionKo: "15년 유닉스 표준(tmux 48k★)부터 7일 된 실험적 신생 도구(Ferryx 1★)까지 성숙도 위험을 투명하게 반영합니다.",
    descriptionEn: "Balances 15-year battle-tested standards against experimental early projects.",
    citation: "O1 (Ferryx 1★), O2 (herdr 33k★), O8 (tmux 48k★), O32 (conductor Pro)",
  },
};

export function EvidenceRail({
  question,
  selectedOption,
  lang = "ko",
}: EvidenceRailProps) {
  const axis = question.axisId !== undefined ? AXIS_DETAILS[question.axisId] : undefined;
  const isKo = lang === "ko";

  return (
    <aside
      className="flex flex-col gap-4 p-5 rounded-2xl border border-line bg-surface/80 text-ink"
      aria-label={isKo ? "선택 근거 및 영향 레일" : "Evidence & Impact Rail"}
    >
      <div className="flex items-center gap-2 text-xs font-mono text-ink-soft uppercase tracking-wider">
        <Info className="w-3.5 h-3.5" />
        <span>{isKo ? "선택 영향 및 1차 출처" : "Impact & Primary Evidence"}</span>
      </div>

      {axis !== undefined ? (
        <div className="flex flex-col gap-3">
          <div>
            <div className="text-xs font-mono text-ink font-semibold">
              {isKo ? axis.nameKo : axis.nameEn}
            </div>
            <p className="text-xs text-ink-soft mt-1 leading-relaxed">
              {isKo ? axis.descriptionKo : axis.descriptionEn}
            </p>
          </div>

          <div className="pt-2 border-t border-line text-[11px] font-mono text-ink-faint">
            <span className="text-ink-soft font-medium">
              {isKo ? "검증 출처: " : "Citations: "}
            </span>
            <span>{axis.citation}</span>
          </div>
        </div>
      ) : (
        <div className="text-xs text-ink-soft leading-relaxed">
          {isKo
            ? "에이전트 병렬 운용 수량은 워크스페이스의 권장 뷰포트 밀도와 CPU/메모리 요구사항을 결정합니다."
            : "Agent concurrency determines viewport density and workspace memory layout."}
        </div>
      )}

      {selectedOption !== undefined && (
        <div className="p-3 rounded-lg bg-page border border-line text-xs">
          <div className="font-mono text-[11px] text-ink-soft mb-1">
            {isKo ? "현재 선택 옵션" : "Selected Option"}:
          </div>
          <div className="font-semibold text-ink">
            {isKo
              ? question.options[selectedOption].labelKo
              : question.options[selectedOption].labelEn}
          </div>
          <div className="text-ink-soft text-[12px] mt-0.5">
            {isKo
              ? question.options[selectedOption].descriptionKo
              : question.options[selectedOption].descriptionEn}
          </div>
        </div>
      )}
    </aside>
  );
}
