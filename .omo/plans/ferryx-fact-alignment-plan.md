# Plan: Ferryx Landing Page & README Real Truth Alignment

## 1. 개요 및 목적
랜딩페이지(`site/`)와 `README.md`에 들어간 임의의 뇌피셜 수치(115ms, 82MB, 33x Faster, 42 tests 0.28s 등) 및 가짜 시뮬레이션 데이터를 전면 삭제하고, **Ferryx의 실제 아키텍처/기능/커맨드**에 맞게 100% 진실된 기술 콘텐츠로 재구성한다.

---

## 2. 실제 제품 진실 (Fact Sheet)
- **런타임 아키텍처**: Tauri v2 (Rust 백엔드 `src-tauri` + React 18 프론트엔드 `ui`)
- **터미널 엔진**: xterm.js (WebGL 렌더러 + 유니코드 11 지원) + Rust `portable-pty` 네이티브 데몬
- **Git Worktree 통합**: 프로젝트별 Git Worktree 격리 생성/삭제 및 브랜치 작업 지원
- **스플릿 뷰**: 수직/수평 터미널 분할 및 드래그 리사이즈, 세션 분할
- **임베디드 브라우저**: Child Webview (macOS WebKit / Windows Webview2 / Linux WebKitGTK) 기반의 인앱 브라우저 탭, 127.0.0.1:5173 루프백 HMR 연동
- **원격 연결**: QR / 6자리 PIN 페어링 기반 WebSocket 터미널 원격 미러링 (`/remote`)
- **정확한 빌드/실행 명령어**:
  - 개발: `bun install` ➔ `bun run --cwd ui dev` ➔ `cargo tauri dev`
  - 랜딩: `bun run --cwd site dev`

---

## 3. 섹션별 상세 변경 계획

### Task 1. Hero 컴포넌트 리팩토링 (`site/src/components/Hero.tsx`)
- **수정 사항**:
  - 임의 수치 뱃지(`<150ms Cold Launch`, `~82MB Base RAM`) 삭제 ➔ 실제 아키텍처 태그(`Rust PTY Host`, `Git Worktree Isolation`, `WebGL Terminal`, `QR Mobile Remote`)로 교체.
  - Quick Install 카피 스니펫: 실제 빌드 명령어(`git clone https://github.com/stablyai/orca && cd orca && bun install && cargo tauri dev`)로 수정.
  - 터미널 데모 UI: 가짜 테스트 로그 대신 **실제 Ferryx 터미널 환경 (Git Worktree 기반 브랜치 작업, xterm 출력, 백엔드 세션 연결 상태)**을 보여주는 현실적인 개발 환경 UI로 교체.
  - 모바일 탭 데모: 실제 Ferryx의 페어링 PIN 코드 및 Tailscale/LAN 원격 스트리밍 인터페이스 묘사.

### Task 2. Features 컴포넌트 검증 및 정제 (`site/src/components/Features.tsx`)
- **수정 사항**:
  - 모호한 수치 마케팅 문구 제거.
  - 6대 실제 핵심 기능 명확화:
    1. **Rust Native PTY Host**: `portable-pty` 기반 OS 직결 터미널 세션 관리
    2. **Git Worktree Workspace**: 브랜치별 독립 작업 디렉토리 및 터미널 자동 바인딩
    3. **WebGL Accelerated Terminal**: xterm.js WebGL 애드온을 통한 빠른 렌더링
    4. **In-App Browser Tabs**: Tauri Child Webview를 활용한 인앱 웹 뷰어
    5. **Encrypted Remote Pairing**: 토큰/PIN 기반 모바일 웹 원격 터미널 컨트롤
    6. **Session Layout Persistence**: 브라우저 로컬 스토리지 + 데몬 상태 복구

### Task 3. Benchmarks 섹션을 "Architecture & Comparison"으로 전환 (`site/src/components/Benchmarks.tsx`)
- **수정 사항**:
  - 근거 없는 정량적 숫자 카드(115ms vs 3800ms, 82MB vs 840MB 등) 전면 제거.
  - 아키텍처적 차이점을 설명하는 정성적/기술적 비교로 개편:
    - **Ferryx vs Electron IDEs 기술 구조 비교**:
      - UI 런타임: OS 내장 웹뷰 (WebKit/Webview2) vs 번들된 크로미움
      - 백엔드 프로세스: 경량 네이티브 Rust 바이너리 vs 무거운 Node.js 메인 프로세스
      - 터미널 I/O: Rust 직접 PTY 스트리밍 vs Node-PTY IPC 브릿지
      - 원격 제어: 내장 암호화 WebSocket 서버 vs 별도 확장 프로그램 필요

### Task 4. Shortcuts & CTA 검토 (`site/src/components/Shortcuts.tsx`, `site/src/components/CTA.tsx`)
- **수정 사항**:
  - `Shortcuts.tsx`에 기재된 키바인딩이 `ui/src/lib/shortcuts.ts`에 정의된 실제 단축키(`⌘T`, `⌘⇧B`, `⌘W`, `⌘D`, `⌘⇧D`, `⌘[`, `⌘]`, `⌘K` 등)와 100% 일치하는지 전수 대조.
  - `CTA.tsx`의 설치 커맨드도 실제 커맨드로 통일.

### Task 5. 루트 `README.md` 동기화
- **수정 사항**:
  - `README.md` 내의 벤치마크 표와 커맨드를 위 계획과 완벽히 동일하게 수정.

---

## 4. 검증 계획
1. `bun run --cwd site build` (Vite 정적 빌드 및 타입체크 0 에러)
2. `bun run --cwd ui test` (기존 데스크톱 UI 테스트 307개 올그린 유지)
3. 브라우저 렌더링 및 링크/단축키 매핑 수동 점검
