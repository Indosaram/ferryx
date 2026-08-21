# rorca (Orca Lite) 내장 브라우저 지원 여부 분석

> **작성일:** 2026-08-21  
> **조사 대상:** `orca-lite` (rorca) 코드베이스 및 원본 `orca` (`ui/original-dist/`)

---

## 1. 결론 요약

**rorca(orca-lite)는 현재 원본 Orca와 같은 내장 브라우저(Embedded Browser) 기능을 지원하지 않습니다.**

원본 Orca에서는 Electron 기반의 브라우저 탭(`createBrowserTab`), 포트 포워딩 프리뷰, `agent-browser` 연동 등이 지원되었으나, rorca에서는 경량화 및 안정성을 위해 **의도적으로 스코프에서 제외(By-Design Gap / Scope OUT)**되어 있습니다.

---

## 2. 세부 분석 및 근거

### (1) 설계 및 아키텍처 스코프 (By-Design Gap)
- `ORCA_LITE_PARITY_AUDIT.md` §4 (by-design gap) 및 `.omo/drafts/orca-ui-recovery.md`:
  > *"Scope OUT: 브라우저 pane · 에디터/Monaco · PR 뷰 · 원격 호스트 관리 · 자동화 · 에이전트 오케스트레이션"*
  > *"현재 IPC = worktree + terminal뿐 (`lib.rs:28-39`)"*
- rorca는 무거운 Electron 런타임 대신 Tauri + Rust PTY 기반의 초경량 Git Worktree + Terminal 워크스페이스 제공에 집중하도록 설계되었습니다.

### (2) 백엔드 IPC 계층 (`src-tauri`)
- `src-tauri/src/ipc/`에 등록된 명령어는 **Worktree 관리**(`project`, `worktree`), **터미널 PTY**(`terminal`), **환경설정**(`preferences`), **원격 접속**(`remote`)뿐입니다.
- 웹뷰 인스턴스 생성, URL 라우팅, 쿠키/세션 격리, 브라우저 탐색 제어 등을 위한 Rust 백엔드 IPC 핸들러가 존재하지 않습니다.

### (3) 프론트엔드 상태 및 UI 계층 (`ui/src`)
- **탭 모델 (`ui/src/lib/types.ts`)**: `TerminalTab`, `LayoutState`, `TerminalPane` 등 모든 레이아웃 상태가 터미널 세션(`sessionId`)에 1:1로 귀속되어 있습니다.
- **테스트 명세 (`ui/src/components/SettingsDialog.test.tsx`)**: 설정 다이얼로그 및 단축키 목록에서 브라우저 관련 텍스트/기능이 완전히 제외되었음을 검증하는 단언(`expect(...).not.toHaveTextContent(/browser/i)`)이 포함되어 있습니다.

---

## 3. 원본 Orca vs rorca 비교

| 항목 | 원본 Orca (Electron) | rorca (Tauri / Rust) |
|---|---|---|
| **브라우저 탭 생성** | 지원 (`store.createBrowserTab`) | **미지원** (터미널 탭 전용) |
| **로컬 포트 프리뷰** | 내장 브라우저로 열기 지원 | **미지원** (외부 브라우저 또는 터미널 중심) |
| **에이전트 브라우징** | `agent-browser` 패키지 통합 | **미지원** |
| **렌더링 엔진** | Chromium (Electron WebContentsView) | OS Native Webview (Tauri 메인 윈도우 UI 전용) |
