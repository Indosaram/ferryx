# Microsoft Store 제출용 전체 메타데이터 및 패키지 팩 (Submission Pack)

**작성일:** 2026-09-05  
**게시자 계정:** `Project Maho` (`CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`)  
**제출 대상 앱:** Ferryx (`ProjectMaho.Ferryx`)

---

## 1. 빌드 완료 산출물 위치

파트너 센터의 **패키지(Packages)** 업로드 단계에서 아래 파일을 그대로 드래그 앤 드롭하시면 됩니다.

- **MSIX 패키지 파일**:
  `dist/msix/Ferryx_2026.902.2_x64.msix` (약 15.3 MB)
  *(식별자 `ProjectMaho.Ferryx` 및 `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36` 검증 완료)*
- **스토어 등록용 스크린샷**:
  `dist/msix/Screenshot_1.png` (2176 x 1344 고해상도 데스크톱 캡처)

---

## 2. 파트너 센터 단계별 복사-붙여넣기 입력값

파트너 센터 대시보드 (`https://partner.microsoft.com/dashboard/apps-and-games/overview`)에서 **Ferryx**를 선택하고 **제출 시작(Start your submission)** 클릭 후 각 단계에 입력할 내용입니다.

### 2.1 가격 및 가용성 (Pricing and availability)
- **가격 (Price)**: `무료 (Free - $0.00)`
- **시장 (Markets)**: `모든 시장 (All markets)` 또는 기본값 유지
- **표시 여부 (Visibility)**: `스토어에 앱 표시 및 검색 가능 (Public)`

### 2.2 앱 속성 (Properties)
- **카테고리 (Category)**: `Developer Tools` (개발자 도구)
- **하위 카테고리 (Subcategory)**: `Development Utilities` (개발 유틸리티)
- **개인정보처리방침 URL (Privacy Policy URL)**:
  ```text
  https://ferryx.app/privacy
  ```
- **웹사이트 URL (Website URL)**:
  ```text
  https://ferryx.app
  ```
- **지원 연락처 URL (Support Contact)**:
  ```text
  https://github.com/Indosaram/ferryx/issues
  ```
- **제한된 기능 (Restricted Capabilities) - `runFullTrust` 사유 입력란**:
  ```text
  Ferryx is a local developer terminal and workspace manager that launches local shell PTY sessions, runs Git subprocesses, and communicates with a local background daemon over Unix domain sockets.
  ```

### 2.3 연령 등급 (Age ratings - IARC 설문)
- **앱 유형**: `유틸리티 / 생산성 / 개발 도구 (Utility/Productivity)`
- **폭력, 성적 콘텐츠, 공포, 도박, 약물, 거친 언어**: 모두 `아니요 (No)`
- **사용자 간 상호작용 (채팅 등)**: `아니요 (No)`
- **사용자 물리적 위치 공유**: `아니요 (No)`
- **예상 등급**: `PEGI 3 / ESRB Everyone / IARC 3+ (전체 이용가)`

### 2.4 패키지 (Packages)
- `dist/msix/Ferryx_2026.902.2_x64.msix` 파일을 업로드 영역에 드래그 앤 드롭
- 파트너 센터의 자동 유효성 검사기가 `ProjectMaho.Ferryx`, `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`, `x64` 아키텍처, 로고 에셋들을 자동 검증하여 통과합니다.

### 2.5 스토어 등록정보 (Store listings - 영어(미국) 및 한국어)

#### [English (United States)]
- **App title**: `Ferryx`
- **Short description**:
  ```text
  Ultra-lightweight multi-workspace terminal and Git worktree manager powered by Tauri v2 and Rust.
  ```
- **Full description**:
  ```text
  Ferryx is an ultra-lightweight workspace terminal and Git worktree manager built with Rust and Tauri v2.

  Key Features:
  • High-Performance Native Terminal: Smooth GPU-accelerated rendering powered by Ghostty VT and WGPU.
  • Resilient Background Daemon: Terminal sessions survive GUI restarts and disconnects with zero lost state.
  • Git Worktree Integration: Create, switch, and manage isolated worktrees in seconds.
  • Multi-Tab & Flexible Splits: Organize complex dev workflows with flexible horizontal and vertical split panes.
  • AI Agent Companion: Seamlessly coordinates with local coding agents and autonomous CLI workflows.
  • Ultra-Low Footprint: Negligible idle CPU usage and minimal memory overhead compared to traditional Electron wrappers.
  ```
- **What's new in this release (릴리스 정보)**:
  ```text
  • Initial Microsoft Store release.
  • Hardware-accelerated GPU terminal rendering engine.
  • Background headless daemon for persistent terminal sessions.
  • Korean IME (Hangul) preedit and multi-click drag selection fixes.
  • Multi-workspace layout persistence.
  ```
- **Product features (기능 목록)**:
  - `Fast native GPU-rendered terminal`
  - `Persistent background daemon (zero session loss)`
  - `Multi-workspace & Git worktree manager`
  - `Tab and split-pane layout restoration`
  - `Native IME and clipboard integration`
- **Search terms (검색 키워드)**:
  `terminal, workspace, git, worktree, developer tools, rust, console, tauri, ghostty`
- **Copyright and trademark info**:
  ```text
  Copyright © 2026 Project Maho. All rights reserved.
  ```
- **Developer name / Publisher**:
  ```text
  Project Maho
  ```
- **Screenshots (스크린샷)**:
  `dist/msix/Screenshot_1.png` 업로드

---

## 3. 최종 검토 및 제출 (Review and submit)
1. 모든 섹션에 녹색 체크 표시(Complete)가 되었는지 확인합니다.
2. 우측 상단의 **스토어에 제출(Submit to the Store)** 버튼을 클릭합니다.
3. 인증 및 심사는 보통 24~48시간 이내에 완료되며, 승인 즉시 Microsoft Store에 공개 배포됩니다.
