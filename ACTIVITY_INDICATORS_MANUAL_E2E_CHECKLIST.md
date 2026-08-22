# Activity Indicators & Notification - Manual E2E Checklist

> 자동화(단위/빌드) 검증만으로는 실기동 파이프라인이 확인되지 않으므로,
> 아래 절차를 사람이 직접 실행해 확인한다. (2026-08-21 기준)

## 검증된 것 vs 검증되지 않은 것

| 항목 | 방법 | 상태 |
|---|---|---|
| 타이틀 → activity 판정 로직 (`agentTitle.ts`) | Vitest 단위 테스트 | 자동 통과 |
| 상태 변경 시 UI 렌더 (`workspaceStore`, `TabBar`, `TerminalSplitView`) | Vitest 단위 테스트 | 자동 통과 |
| Rust 백엔드 전체 | `cargo test` | 자동 통과 |
| PTY → xterm `onTitleChange`/`onBell` → 스피너 라이브 체인 | 실기동 E2E | **수동 확인 필요** |

## 사전 준비

- 앱 실행: `bun run tauri dev` 또는 빌드된 Ferryx 앱
- 테스트 대상 CLI 에이전트: `agy`(antigravity), `omo`, `claude` 중 최소 1개

## 수동 확인 절차

1. 워크트리 탭에서 터미널을 열고 `agy`를 실행한다.
2. **작업 중**: 해당 탭에 🔄 회전 스피너가 뜨는지 확인한다.
3. **입력 대기**: 에이전트가 질문/권한 요청으로 멈추면 🟡 노란 점으로 바뀌는지 확인한다.
4. 워크트리 리스트의 StatusDot이 탭과 동일한 상태를 보여주는지 확인한다.
5. 사이드바 프로젝트 헤더에 `N running` 뱃지가 갱신되는지 확인한다.
6. 일반 쉘(`zsh`, `bash`)에서는 인디케이터가 뜨지 않는지 확인한다 (오탐 방지).
7. 알림: 작업 완료 시 OS 알림이 오는지 확인한다.

## 실패 시 추적 지점

탭 인디케이터가 전혀 안 뜨면 아래 체인 중 실기동에서 끊긴 곳을 순서대로 확인:

```
Tauri PTY OSC title
  → terminalHostManager.ts 의 terminal.onTitleChange / onBell 바인딩
    → TerminalPane onTitleChange 프롭
      → App.tsx updateSessionTitleActivity 연결
        → workspaceStore classifyTerminalTitleActivity 판정
          → TabBar / WorktreeList / Sidebar 렌더링
```

관련 구현 계획: `ACTIVITY_INDICATORS_IMPLEMENTATION_PLAN.md`, `NOTIFICATION_IMPLEMENTATION_PLAN.md`
