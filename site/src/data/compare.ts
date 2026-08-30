import type { ToolId } from "./types.ts";

export interface ComparePairDefinition {
  readonly slug: string;
  readonly toolA: ToolId;
  readonly toolB: ToolId;
  readonly titleKo: string;
  readonly titleEn: string;
  readonly summaryKo: string;
  readonly summaryEn: string;
  readonly keyDifferentiatorKo: string;
  readonly keyDifferentiatorEn: string;
}

export const COMPARE_PAIRS: readonly ComparePairDefinition[] = [
  {
    slug: "tmux-vs-herdr",
    toolA: "tmux",
    toolB: "herdr",
    titleKo: "tmux vs herdr 비교 — 유닉스 데몬 표준과 Rust 에이전트 런타임",
    titleEn: "tmux vs herdr — Unix Daemon Baseline vs Rust Agent Runtime",
    summaryKo:
      "15년 이상 검증된 표준 터미널 멀티플렉서 tmux와 프로세스 FD 핸드오프 및 에이전트 상태 감지를 갖춘 현대적 Rust TUI herdr의 아키텍처 비교입니다.",
    summaryEn:
      "Architectural comparison between the 15-year battle-tested terminal multiplexer and the modern Rust agent runtime with live FD handoff.",
    keyDifferentiatorKo:
      "tmux는 모든 유닉스 환경에서의 절대적 편재성을 제공하며, herdr는 바이너리 업그레이드 중에도 세션 PTY를 살려두는 라이브 FD 핸드오프를 제공합니다.",
    keyDifferentiatorEn:
      "tmux offers absolute ubiquity across all Unix servers, while herdr provides live process FD handoff surviving daemon binary upgrades.",
  },
  {
    slug: "conductor-vs-superset",
    toolA: "conductor",
    toolB: "superset",
    titleKo: "Conductor vs Superset 비교 — 클라우드 마이크로VM과 대규모 데스크톱 오케스트레이션",
    titleEn: "Conductor vs Superset — Cloud MicroVMs vs High-Density Desktop Orchestration",
    summaryKo:
      "macOS 네이티브 클라이언트와 클라우드 마이크로VM 샌드박스를 제공하는 Conductor와 100개 이상의 로컬 worktree 에이전트를 모나코 diff 뷰어로 조율하는 Superset의 비교입니다.",
    summaryEn:
      "Comparison between Conductor's macOS native cloud microVM sandbox and Superset's high-density local worktree orchestration with Monaco diff reviews.",
    keyDifferentiatorKo:
      "Conductor는 클라우드 샌드박스와 macOS 네이티브 인터페이스에 집중하며, Superset은 로컬 독립 PTY 데몬과 비동기 리뷰 큐를 통한 대규모 병렬 처리에 특화되어 있습니다.",
    keyDifferentiatorEn:
      "Conductor focuses on cloud sandboxes and macOS native UI, whereas Superset specializes in high-density local worktrees with a standalone PTY daemon.",
  },
  {
    slug: "cmux-vs-ferryx",
    toolA: "cmux",
    toolB: "ferryx",
    titleKo: "cmux vs Ferryx 비교 — Swift GhosttyKit 네이티브와 Rust WGPU 터미널 캔버스",
    titleEn: "cmux vs Ferryx — Swift GhosttyKit Native vs Rust WGPU Terminal Canvas",
    summaryKo:
      "GhosttyKit 기반의 정교한 macOS 네이티브 멀티플렉서 cmux와 무중단 백그라운드 데몬 및 PIN 게이트웨이를 결합한 실험적 Rust WGPU 워크스페이스 Ferryx의 비교입니다.",
    summaryEn:
      "Comparison between cmux's refined Swift/GhosttyKit macOS native multiplexer and Ferryx's experimental Rust WGPU canvas with persistent background daemon.",
    keyDifferentiatorKo:
      "cmux는 macOS 플랫폼에 최적화된 최고 수준의 네이티브 UI 마감을 제공하며, Ferryx는 GUI 종료 후에도 지속되는 독립 Rust 데몬과 6자리 PIN 원격 게이트웨이를 실험합니다.",
    keyDifferentiatorEn:
      "cmux provides top-tier macOS native polish, while Ferryx experiments with a headless Rust daemon that survives GUI exit and zero-cloud PIN pairing.",
  },
  {
    slug: "orca-vs-herdr",
    toolA: "orca",
    toolB: "herdr",
    titleKo: "Orca vs herdr 비교 — Electron 멀티 에이전트 워크스페이스와 초경량 Rust TUI",
    titleEn: "Orca vs herdr — Electron Agent Workbench vs Pure Rust TUI",
    summaryKo:
      "자동 git worktree 분리와 모바일 원격 푸시를 갖춘 Electron GUI 워크스페이스 Orca와 최소한의 리소스로 원격 SSH와 로컬을 넘나드는 Rust TUI herdr의 비교입니다.",
    summaryEn:
      "Comparison between Orca's multi-worktree Electron workbench with mobile push and herdr's ultra-lightweight pure Rust TUI runtime.",
    keyDifferentiatorKo:
      "Orca는 시각적 diff 주석과 React Native 모바일 알림 생태계를 제공하고, herdr는 터미널 안에서 0ms 수준의 반응성과 헤드리스 무중단 지속성을 보장합니다.",
    keyDifferentiatorEn:
      "Orca provides visual diff annotations and mobile push notifications, while herdr guarantees pure terminal responsiveness and headless persistence.",
  },
];
