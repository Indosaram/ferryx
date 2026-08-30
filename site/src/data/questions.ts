import type { AxisId, OptionIndex } from "./types.ts";

export type QuestionId =
  | "q1_parallelism"
  | "q2_persistence"
  | "q3_isolation"
  | "q4_supervision"
  | "q5_platform"
  | "q6_interface"
  | "q7_maturity";

export interface QuestionOption {
  readonly optionIndex: OptionIndex;
  readonly labelKo: string;
  readonly labelEn: string;
  readonly descriptionKo: string;
  readonly descriptionEn: string;
}

export interface QuestionDefinition {
  readonly id: QuestionId;
  readonly promptKo: string;
  readonly promptEn: string;
  readonly axisId?: AxisId;
  readonly options: readonly [QuestionOption, QuestionOption, QuestionOption];
}

export const QUESTIONS: readonly QuestionDefinition[] = [
  {
    id: "q1_parallelism",
    promptKo: "에이전트를 동시에 몇 개 굴리나?",
    promptEn: "How many coding agents do you run simultaneously?",
    options: [
      {
        optionIndex: 0,
        labelKo: "1개",
        labelEn: "1 agent",
        descriptionKo: "단일 작업에 집중",
        descriptionEn: "Focused on a single focused task",
      },
      {
        optionIndex: 1,
        labelKo: "2~4개",
        labelEn: "2–4 agents",
        descriptionKo: "다중 기능 병렬 개발",
        descriptionEn: "Multi-branch parallel development",
      },
      {
        optionIndex: 2,
        labelKo: "5개 이상",
        labelEn: "5+ agents",
        descriptionKo: "대규모 에이전트 오케스트레이션",
        descriptionEn: "High-density fleet orchestration",
      },
    ],
  },
  {
    id: "q2_persistence",
    promptKo: "노트북 덮개를 닫으면 그 작업은?",
    promptEn: "When you close your laptop lid, what should happen to the job?",
    axisId: "A2",
    options: [
      {
        optionIndex: 0,
        labelKo: "죽어도 된다",
        labelEn: "Can terminate",
        descriptionKo: "앱 닫으면 프로세스 종료",
        descriptionEn: "Process terminates with app exit",
      },
      {
        optionIndex: 1,
        labelKo: "살아 있어야 한다",
        labelEn: "Must survive app exit",
        descriptionKo: "로컬 백그라운드 데몬에서 지속",
        descriptionEn: "Keeps running in local daemon",
      },
      {
        optionIndex: 2,
        labelKo: "원격 서버에서 계속 돌아야 한다",
        labelEn: "Must run on remote server",
        descriptionKo: "재부팅 및 SSH 분리에도 무중단",
        descriptionEn: "Survives reboots and SSH disconnects",
      },
    ],
  },
  {
    id: "q3_isolation",
    promptKo: "에이전트 5개가 같은 레포를 만질 때?",
    promptEn: "When 5 agents work on the same repo, how do they isolate?",
    axisId: "A3",
    options: [
      {
        optionIndex: 0,
        labelKo: "그냥 같은 트리",
        labelEn: "Same working tree",
        descriptionKo: "단일 디렉토리 공유",
        descriptionEn: "Share single workspace directory",
      },
      {
        optionIndex: 1,
        labelKo: "git worktree로 분리",
        labelEn: "Git worktrees",
        descriptionKo: "브랜치별 독립 작업 트리 격리",
        descriptionEn: "Isolated branch worktree directories",
      },
      {
        optionIndex: 2,
        labelKo: "컨테이너·VM으로 격리",
        labelEn: "Container / Cloud microVM",
        descriptionKo: "완전한 OS / 클라우드 샌드박스",
        descriptionEn: "Full container or cloud sandbox",
      },
    ],
  },
  {
    id: "q4_supervision",
    promptKo: "에이전트가 승인을 기다릴 때 어떻게 알고 싶나?",
    promptEn: "When an agent waits for approval, how do you want to be notified?",
    axisId: "A4",
    options: [
      {
        optionIndex: 0,
        labelKo: "화면을 보고 있다",
        labelEn: "Watch active screen",
        descriptionKo: "터미널 캔버스 직접 관찰",
        descriptionEn: "Direct terminal canvas monitoring",
      },
      {
        optionIndex: 1,
        labelKo: "알림을 받는다",
        labelEn: "Notification alert",
        descriptionKo: "데스크톱/모바일 링 및 푸시",
        descriptionEn: "Desktop or mobile push alerts",
      },
      {
        optionIndex: 2,
        labelKo: "나중에 리뷰 큐에서 본다",
        labelEn: "Review queue / Kanban",
        descriptionKo: "비동기 보드 및 diff 큐 일괄 확인",
        descriptionEn: "Async review queue and Kanban cards",
      },
    ],
  },
  {
    id: "q5_platform",
    promptKo: "어디서 도나?",
    promptEn: "Where does the environment need to run?",
    axisId: "A5",
    options: [
      {
        optionIndex: 0,
        labelKo: "macOS만 지원하면 됨",
        labelEn: "macOS only is fine",
        descriptionKo: "Apple Silicon 최적화 우선",
        descriptionEn: "macOS native polish prioritized",
      },
      {
        optionIndex: 1,
        labelKo: "Windows·Linux도 필요",
        labelEn: "Windows & Linux required",
        descriptionKo: "크로스플랫폼 데스크톱 지원",
        descriptionEn: "Cross-platform desktop availability",
      },
      {
        optionIndex: 2,
        labelKo: "헤드리스 서버에 SSH로",
        labelEn: "Headless server via SSH",
        descriptionKo: "원격 서버 터미널 및 SSH 브리지",
        descriptionEn: "Headless Linux servers via SSH",
      },
    ],
  },
  {
    id: "q6_interface",
    promptKo: "터미널을 바꿀 의향은?",
    promptEn: "What is your preferred interface substrate?",
    axisId: "A1",
    options: [
      {
        optionIndex: 0,
        labelKo: "지금 터미널 그대로 쓰겠다",
        labelEn: "Keep current terminal (TUI)",
        descriptionKo: "순수 TUI / 멀티플렉서",
        descriptionEn: "Pure TUI inside existing terminal",
      },
      {
        optionIndex: 1,
        labelKo: "전용 앱 좋다",
        labelEn: "Dedicated native GUI app",
        descriptionKo: "GPU 가속 네이티브 GUI 앱",
        descriptionEn: "GPU-accelerated native GUI app",
      },
      {
        optionIndex: 2,
        labelKo: "GUI 앱 선호",
        labelEn: "Web / Electron GUI app",
        descriptionKo: "웹 기술 및 Monaco 에디터 기반",
        descriptionEn: "Web-based or Electron workbench",
      },
    ],
  },
  {
    id: "q7_maturity",
    promptKo: "성숙도는 얼마나 중요한가?",
    promptEn: "How important is project maturity?",
    axisId: "A6",
    options: [
      {
        optionIndex: 0,
        labelKo: "실험적이어도 좋다",
        labelEn: "Experimental is fine",
        descriptionKo: "최신 아키텍처와 빠른 피드백",
        descriptionEn: "Cutting-edge architecture",
      },
      {
        optionIndex: 1,
        labelKo: "검증된 커뮤니티 필요",
        labelEn: "Proven community required",
        descriptionKo: "수만 스타 및 활성 오픈소스 생태계",
        descriptionEn: "Tens of thousands of stars and active OSS",
      },
      {
        optionIndex: 2,
        labelKo: "팀·엔터프라이즈 기능 필요",
        labelEn: "Team & enterprise ready",
        descriptionKo: "상용 지원, 클라우드 권한, 팀 협업",
        descriptionEn: "Commercial support and team features",
      },
    ],
  },
];
