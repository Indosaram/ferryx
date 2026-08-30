import type { DiagnosticAnswers, ResultProfile } from "./types.ts";
import type { ToolId } from "../../data/types.ts";

export const DEFAULT_PROFILE: ResultProfile = {
  slug: "native-craft",
  titleKo: "네이티브 GPU 장인파",
  titleEn: "Native GPU Craftsman",
  summaryKo: "하드웨어 가속 렌더링과 낮은 지연시간, 직관적인 데스크톱 GUI 인터페이스를 선호합니다.",
  summaryEn: "Prefers hardware-accelerated rendering, ultra-low latency, and native desktop UX.",
};

export const RESULT_PROFILES: Readonly<Record<string, ResultProfile>> = {
  "headless-persistence": {
    slug: "headless-persistence",
    titleKo: "헤드리스 지속성파",
    titleEn: "Headless Persistence Purist",
    summaryKo: "노트북을 닫아도 세션이 살아 있고 원격 SSH 환경과 결합된 무중단 지속성을 중시합니다.",
    summaryEn: "Prioritizes uninterrupted session lifecycle across lid closes and remote SSH connections.",
  },
  "worktree-isolationist": {
    slug: "worktree-isolationist",
    titleKo: "worktree 격리주의자",
    titleEn: "Git Worktree Isolationist",
    summaryKo: "다중 에이전트 간 충돌을 방지하기 위해 브랜치별 독립 git worktree 환경을 필수화합니다.",
    summaryEn: "Mandates branch-isolated git worktrees to avoid concurrent agent workspace collisions.",
  },
  "terminal-purist": {
    slug: "terminal-purist",
    titleKo: "터미널 순수주의자",
    titleEn: "Terminal Multiplexer Purist",
    summaryKo: "새로운 무거운 GUI 앱 대신 기존 셸과 TUI 멀티플렉서의 빠른 속도와 편재성을 신뢰합니다.",
    summaryEn: "Relies on the speed, ergonomics, and ubiquity of terminal multiplexers and pure TUIs.",
  },
  "native-craft": DEFAULT_PROFILE,
  "kanban-coordinator": {
    slug: "kanban-coordinator",
    titleKo: "비동기 칸반 조율자",
    titleEn: "Async Kanban Coordinator",
    summaryKo: "에이전트 작업을 태스크 보드와 비동기 리뷰 큐로 구조화하여 대규모 병렬 작업을 관리합니다.",
    summaryEn: "Structures agent runs into Kanban columns and async review queues for high concurrency.",
  },
  "cloud-orchestrator": {
    slug: "cloud-orchestrator",
    titleKo: "클라우드 오케스트레이터",
    titleEn: "Cloud Fleet Orchestrator",
    summaryKo: "클라우드 샌드박스와 마이크로VM 격리를 통해 엔터프라이즈급 안정성과 협업을 추구합니다.",
    summaryEn: "Leverages cloud sandboxes and microVMs for enterprise-grade isolation and teamwork.",
  },
  "web-diff-pilot": {
    slug: "web-diff-pilot",
    titleKo: "웹·diff 네비게이터",
    titleEn: "Web Diff Navigator",
    summaryKo: "시각적 Monaco diff 리뷰와 풍부한 웹 기반 작업 공간을 통해 에이전트 변경사항을 감독합니다.",
    summaryEn: "Supervises agent modifications through visual Monaco diff reviews and rich web workspaces.",
  },
  "experimental-pioneer": {
    slug: "experimental-pioneer",
    titleKo: "실험적 개척자",
    titleEn: "Experimental Pioneer",
    summaryKo: "성숙도나 스타 수보다 혁신적인 차세대 아키텍처와 새로운 오케스트레이션 패러다임을 환영합니다.",
    summaryEn: "Welcomes bleeding-edge architecture and new orchestration paradigms over legacy maturity.",
  },
};

function getProfile(slug: string): ResultProfile {
  const profile = RESULT_PROFILES[slug];
  if (profile !== undefined) {
    return profile;
  }
  return DEFAULT_PROFILE;
}

export function resolveResultProfile(
  answers: DiagnosticAnswers,
  primaryWinnerId: ToolId
): ResultProfile {
  if (answers.q7_maturity === 0 && primaryWinnerId === "ferryx") {
    return getProfile("experimental-pioneer");
  }
  if (answers.q4_supervision === 2 || primaryWinnerId === "vibe-kanban") {
    return getProfile("kanban-coordinator");
  }
  if (answers.q3_isolation === 2 || primaryWinnerId === "conductor") {
    return getProfile("cloud-orchestrator");
  }
  if (answers.q2_persistence === 2 || primaryWinnerId === "herdr") {
    return getProfile("headless-persistence");
  }
  if (answers.q6_interface === 0 || primaryWinnerId === "tmux" || primaryWinnerId === "claude-squad") {
    return getProfile("terminal-purist");
  }
  if (answers.q6_interface === 2 || primaryWinnerId === "orca" || primaryWinnerId === "superset" || primaryWinnerId === "crystal") {
    return getProfile("web-diff-pilot");
  }
  if (answers.q3_isolation === 1) {
    return getProfile("worktree-isolationist");
  }
  return getProfile("native-craft");
}
