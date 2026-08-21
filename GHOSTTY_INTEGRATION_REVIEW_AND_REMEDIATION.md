# orca-lite (rorca) Ghostty 연동 코드 리뷰 및 개선 계획

ChatGPT Web 워커의 정밀 코드 리뷰 결과 도출된 핵심 결함과 개선 로드맵입니다.

---

## 1. 핵심 결함 (P1 / High Priority)

| 우선순위 | 영역 | 상세 결함 내용 | 영향 |
|---|---|---|---|
| **P1** | **Ghostty 설정 Acquisition** | 파일 직접 파싱이 우선이고 `+show-config` CLI가 fallback으로 배치됨 | 복수 config 병합(`config.ghostty`), `theme = ...` 참조 등이 해석되지 않아 Ghostty 최종 설정과 불일치 |
| **P1** | **ANSI 256색 팔레트** | Rust/TS IPC 모델에서 0–15번(16색)만 전달되고 16–255번 팔레트 누락 | 256색 컬러를 사용하는 TUI 툴(Starship, p10k, NeoVim 등)에서 색상 불일치 |
| **P1** | **xterm 초기화 Race Condition** | `refreshNativePreferences`가 호출 시점의 `localSettings` 스냅샷만 캡처함 | 비동기 로딩 중 사용자가 설정을 바꾸면 stale 설정으로 xterm이 생성됨 |
| **P1** | **`config.ghostty` 탐색 누락** | 최신 Ghostty 기본 파일명인 `config.ghostty`를 찾지 않고 구형 `config`만 검색 | 최신 Ghostty 사용자의 설정 파일을 전혀 인식하지 못함 |
| **P1** | **빈 값 Reset 문법 오류 처리** | Ghostty의 정상 문법인 `font-family =` (리셋)을 `Malformed` 에러로 처리 | 정상 설정 하나 때문에 전체 설정 파일 import가 무효화됨 |

---

## 2. 세부 결함 (P2 / Medium Priority)

1. **Pane별 중복 IPC 호출**: 각 `TerminalPane`이 마운트될 때마다 Ghostty 설정을 중복 요청하여 프로세스/파일 IO 낭비.
2. **커서 및 색상 매핑 불완전**: `block_hollow` 커서 스타일이 `bar`로 오변환되고, `cursor_accent`, `selection_foreground` 누락.
3. **Rust vs Frontend Fallback 불일치**: Ghostty 미설치 시 Rust는 `#282c34/block`, Frontend는 `#0a0a0a/bar`로 서로 다른 fallback을 적용.
4. **CLI Timeout 부재**: `ghostty +show-config` 실행에 timeout이 없어 프로세스 행 시 터미널 초기화 지연 위험.

---

## 3. 개선 실행 순서 (Action Plan)

1. **Phase 1: Rust 설정 로더 구조 개편 (`preferences.rs`)**
   - 탐색 순서: `Ghostty CLI (+show-config, with timeout)` ➔ `config.ghostty 및 load order 기반 파일 파서` ➔ `단일 Fallback`.
   - `font-family =` 빈 값 리셋 처리 및 `config.ghostty` 지원.
   - ANSI 256색 `extended_ansi` (16~255) 및 `cursor_accent`, `selection_foreground` 추가.

2. **Phase 2: Frontend IPC 및 전역 상태화 (`tauri.ts`, `terminalSettings.ts`)**
   - `TerminalThemeColors`에 `extendedAnsi` 지원 추가.
   - Pane별 중복 호출을 제거하고 상위 레벨에서 단일 캐시된 Native Preferences 제공.
   - Rust/Frontend fallback 기본값 완전 일치.

3. **Phase 3: TerminalPane 초기화 안전성 강화 (`TerminalPane.tsx`)**
   - xterm 인스턴스 생성 직전 최신 `localSettingsRef`와 `latestNativePreferences`를 재계산하여 Race condition 완전 차단.
   - `allowProposedApi: false` 복원 및 `initialize` 예외 발생 시 부분 리소스 cleanup 스택 도입.
