# Microsoft Partner Center 3대 앱 스토어 제출용 종합 가이드 (All Apps Submission Pack)

**작성일:** 2026-09-05  
**공통 게시자(Publisher ID):** `CN=68073D7F-44F8-47BF-8B3E-B17FBDC44F36`  
**공통 게시자 표시명(PublisherDisplayName):** `Project Maho`  
**대시보드 주소:** [Microsoft Partner Center Apps and games](https://partner.microsoft.com/dashboard/apps-and-games/overview)

---

## 앱별 산출물 및 등록 식별자 요약

| 앱 이름 | Package/Identity/Name | Package Family Name (PFN) | 패키지 파일 (`dist/msix/`) | 스크린샷 폴더 |
|---|---|---|---|---|
| **Ferryx** | `ProjectMaho.Ferryx` | `ProjectMaho.Ferryx_s4dtschhe0d3e` | `Ferryx_2026.902.2_x64.msix` (15.3MB) | `dist/msix/Screenshot_1.png` |
| **Noveling** | `ProjectMaho.Noveling` | `ProjectMaho.Noveling_s4dtschhe0d3e` | `Noveling_1.6.0.0_x64.msix` (63.6MB) | `dist/msix/noveling_screenshots/` (5장) |
| **Maho Browser** | `ProjectMaho.MahoBrowser` | `ProjectMaho.MahoBrowser_s4dtschhe0d3e` | `MahoBrowser_1.0.0.0_x64.msix` (219.1MB) | `dist/msix/maho_screenshots/` (5장) |

---

# 1. Ferryx (터미널 & 워크스페이스 매니저)

### [Packages 단계]
* 업로드 파일: `dist/msix/Ferryx_2026.902.2_x64.msix`

### [Properties 단계]
* **Category**: `Developer Tools` -> `Development Utilities`
* **Privacy Policy URL**: `https://ferryx.app/privacy`
* **Website URL**: `https://ferryx.app`
* **Support Contact**: `https://github.com/Indosaram/ferryx/issues`
* **Restricted Capabilities (`runFullTrust` 권한 사유)**:
  ```text
  Ferryx is a local developer terminal and workspace manager that launches local shell PTY sessions, runs Git subprocesses, and communicates with a local background daemon over Unix domain sockets.
  ```

### [Age ratings 단계]
* 유형: `Utility / Productivity`
* 모든 유해 항목(폭력/성/공포/도박/채팅/위치): `No` (전체 이용가)

### [Store listings 단계 (English)]
* **App title**: `Ferryx`
* **Short description**:
  ```text
  Ultra-lightweight multi-workspace terminal and Git worktree manager powered by Tauri v2 and Rust.
  ```
* **Full description**:
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
* **What's new in this release**:
  ```text
  • Initial Microsoft Store release.
  • Hardware-accelerated GPU terminal rendering engine.
  • Persistent background daemon for zero session loss.
  • Korean IME (Hangul) preedit and multi-click drag selection fixes.
  ```
* **Product features**:
  - `Fast native GPU-rendered terminal`
  - `Persistent background daemon (zero session loss)`
  - `Multi-workspace & Git worktree manager`
  - `Tab and split-pane layout restoration`
* **Search terms**: `terminal, workspace, git, worktree, developer tools, rust, console, tauri, ghostty`
* **Copyright**: `Copyright © 2026 Project Maho. All rights reserved.`
* **Screenshots**: `dist/msix/Screenshot_1.png`

---

# 2. Noveling (소설 및 문학 창작 편집기)

### [Packages 단계]
* 업로드 파일: `dist/msix/Noveling_1.6.0.0_x64.msix`

### [Properties 단계]
* **Category**: `Productivity` (생산성) 또는 `Books & Reference`
* **Privacy Policy URL**: `https://noveling.dev/privacy`
* **Website URL**: `https://noveling.dev`
* **Support Contact**: `https://github.com/Indosaram/noveling/issues`
* **Restricted Capabilities (`runFullTrust` 권한 사유)**:
  ```text
  Noveling is a local creative writing desktop application that interacts with local document storage, file export converters (Pandoc/DOCX/PDF), and secure local SQLite storage.
  ```

### [Age ratings 단계]
* 유형: `Productivity / Office`
* 모든 유해 항목: `No` (전체 이용가)

### [Store listings 단계 (English & Korean)]

#### [English]
* **App title**: `Noveling`
* **Short description**:
  ```text
  A literary writing editor designed specifically for novelists and creative authors.
  ```
* **Full description**:
  ```text
  Noveling is a dedicated literary writing environment built for authors, novelists, and creative storytellers.

  Key Features:
  • Focused Writing Environment: Distraction-free manuscript editor designed for deep creative focus.
  • Chapter & Scene Organization: Seamlessly structure your chapters, scenes, and manuscript hierarchy.
  • Canon Tracking: Keep track of characters, plotlines, worldbuilding, and organizations in one unified dashboard.
  • Powerful Exporting: Export to standard publishing formats including DOCX, PDF, and Markdown.
  • Secure Cloud Sync & Offline Support: Write anytime, anywhere, with instant synchronization across your devices.
  ```
* **What's new in this release**:
  ```text
  • Initial Microsoft Store release for Windows.
  • Enhanced scene outline navigation and chapter registry.
  • Cloud sync and offline conflict resolution improvements.
  ```
* **Product features**:
  - `Distraction-free manuscript editor`
  - `Chapter and scene hierarchical outline`
  - `Character, plot, and worldbuilding tracking`
  - `Multi-format export (DOCX, PDF, Markdown)`
* **Search terms**: `novel, writing, editor, author, book, manuscript, fiction, story, creative writing`
* **Copyright**: `Copyright © 2026 Project Maho. All rights reserved.`
* **Screenshots**: `dist/msix/noveling_screenshots/` 내 이미지 5장 업로드 (`01-hero.png`, `02-editor.png`, `03-creative.png`, `04-ai.png`, `05-projects.png`)

#### [Korean (한국어 목록 추가 시)]
* **앱 제목**: `Noveling`
* **간략한 설명**:
  ```text
  소설가와 작가를 위한 전문 문학 창작 문서 편집기
  ```
* **상세 설명**:
  ```text
  Noveling은 소설가와 창작자를 위해 탄생한 전용 문학 집필 솔루션입니다.

  주요 기능:
  • 몰입형 원고 에디터: 집필에만 집중할 수 있는 깔끔한 타이포그래피와 인터페이스.
  • 챕터 및 장면 구조화: 장편 소설의 복잡한 챕터와 장면을 체계적으로 구성.
  • 세계관 및 인물 설정 관리: 캐릭터, 플롯, 세계관, 단체를 한눈에 정리하는 캐논 대시보드.
  • 다양한 포맷 내보내기: DOCX, PDF, 마크다운 등 표준 문서 형식 지원.
  • 안전한 클라우드 동기화: 오프라인 집필 완벽 지원 및 기기간 자동 동기화.
  ```
* **기능 목록**:
  - `집필 몰입형 텍스트 에디터`
  - `챕터 및 장면 계층 구조 관리`
  - `인물, 플롯, 세계관 설정 대시보드`
  - `DOCX, PDF 표준 형식 내보내기`

---

# 3. Maho Browser (AI 통합 차세대 브라우저)

### [Packages 단계]
* 업로드 파일: `dist/msix/MahoBrowser_1.0.0.0_x64.msix`

### [Properties 단계]
* **Category**: `Productivity` -> `Web Browsers` (또는 `Utilities & Tools`)
* **Privacy Policy URL**: `https://mahobrowser.com/privacy`
* **Website URL**: `https://mahobrowser.com`
* **Support Contact**: `https://github.com/Indosaram/maho/issues`
* **Restricted Capabilities (`runFullTrust` 권한 사유)**:
  ```text
  Maho is a standalone desktop web browser built on Chromium that manages multi-process web rendering, network sockets, profile storage, and optional local AI assistance tools.
  ```

### [Age ratings 단계]
* 유형: `Web Browser`
* 브라우저 특성상 "인터넷 웹 콘텐츠 탐색 기능 있음" 체크
* 결과 등급: 일반 브라우저 표준 등급 (12+ 또는 Everyone with internet access)

### [Store listings 단계 (English)]
* **App title**: `Maho`
* **Short description**:
  ```text
  Next-generation Chromium browser with integrated AI assistance, spaces, and vertical tabs.
  ```
* **Full description**:
  ```text
  Maho is a fast, elegant, and intelligent web browser designed for modern productivity.

  Key Features:
  • Spaces & Vertical Sidebar: Organize projects, research, and workflows into dedicated spaces.
  • Deep AI Integration: Summarize web pages, extract context, and draft content with built-in AI tools.
  • Built-in Privacy & Protection: Native ad-blocking and tracking protection for a cleaner, faster web.
  • Integrated Mail & Productivity: Experience unified communication directly in your browser.
  • Chromium Engine: Full compatibility with your favorite web apps, extensions, and modern web standards.
  ```
* **Product features**:
  - `Vertical tabs and customizable workspace spaces`
  - `Built-in AI page assistance and smart tools`
  - `Native ad-blocking and tracker shield`
  - `Full Chrome extension ecosystem compatibility`
* **Search terms**: `browser, chromium, ai browser, web browser, internet, spaces, vertical tabs, fast`
* **Copyright**: `Copyright © 2026 Project Maho. All rights reserved.`
* **Screenshots**: `dist/msix/maho_screenshots/` 내 이미지 5장 업로드 (`maho-browser-hero-full.png`, `maho-feature-spaces.png`, `maho-feature-split-view.png`, `maho-feature-command-bar.png`, `maho-feature-agent.png`)
