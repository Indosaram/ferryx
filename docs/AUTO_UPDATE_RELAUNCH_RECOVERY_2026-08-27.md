# Ferryx 업데이트 재시작 상태 복구 hotfix (2026-08-27)

## 사용자 보고

`v2026.08.26.9` 설치본이 인앱 업데이트 후 재시작됐지만 기존 프로젝트와 worktree가 보이지 않았다.
또한 앱 시작 시 자동 업데이트 확인은 구현돼 있지 않아 Settings의 수동 버튼만 업데이트를 조회했다.

## 확인된 원인

1. `ui/src/lib/updater.ts`의 `relaunchApp()`은 Tauri process plugin의 `relaunch()`를 직접 호출했다.
   `relaunch()`는 renderer를 즉시 종료하므로 `App.tsx`에 등록된 workspace snapshot close guard와 500ms
   저장 debounce를 우회했다.
2. 프로젝트 목록은 WebView `localStorage`의 `ferryx.projects`에만 의존했다. 이 목록이 빈 상태 또는 시작 폴더
   하나만 남은 상태가 되면 native app-data의 `session_state.json`에는 여러 workspace가 남아 있어도 startup이
   그것들을 project sidebar로 복구하지 않았다.
3. `App.tsx`에 `checkForUpdate()` startup 호출이 없었다.

## 수정

- updater relaunch 전에 모든 registered close guard를 await해 최신 workspace snapshot을 native session file에 저장한다.
- 시작 시 조용히 서명된 update manifest를 확인한다. Settings > General > Software Update는 그 상태를 그대로
  보여 주며, 수동 **Check for Updates**는 fallback으로 남는다.
- startup catalog가 uninitialized이거나 update 뒤 시작 프로젝트 하나로 축소됐고 native session에 추가 workspace가
  있을 때, native session의 workspace roots와 active workspace를 복구해 WebView catalog에 다시 저장한다.
- 이미 존재하는 다중 프로젝트 catalog에는 과거 session workspace를 임의로 추가하지 않는다.

## TDD 증적

red 단계에서 다음 두 회귀가 재현됐다.

- `relaunchApp()`은 guard 실행 없이 `relaunch()`만 호출했다.
- WebView project catalog가 비었을 때 saved native session의 `alpha`/`beta` workspace 대신 startup project만 표시했다.

green 단계:

```text
bun run --cwd ui test src/App.test.tsx src/lib/updater.test.ts src/components/SettingsDialog.update.test.tsx
Test Files  3 passed (3)
Tests       95 passed (95)

bun run --cwd ui build
tsc && vite build: success
```

전체 UI suite도 실행했다. updater와 무관한 기존 `NativeTerminalPane` geometry test 두 건만 실패했다.
`src/components/NativeTerminalPane.test.tsx`의 resize coalescing 및 scrollbar thumb expectation이며, 이 hotfix가
수정하지 않은 shared native-terminal worktree 변경과 관련된 실패라 수정하거나 약화하지 않았다.

## 배포 후 수동 확인

`docs/AUTO_UPDATE_MANUAL_E2E.md`의 절차로 직전 버전에서 hotfix를 설치한다. 재시작 뒤 현재 version, update
자동 확인 상태, 복구된 프로젝트/worktree, 기존 agent/session의 지속 여부를 기록한다.

## 실제 설치 E2E 후속 발견 (2026-08-28)

`/Applications/Ferryx.app`에 설치한 공개 `2026.826.9` 앱에서 실제 업데이트를 실행했다.

- Settings가 `Version 2026.826.10 is available.`와 활성화된 **Install and Relaunch**를 표시했다.
- 다운로드 진행률이 화면에 표시된 뒤, `2026.826.9` GUI PID가 종료됐다.
- `/Applications/Ferryx.app` 번들은 `2026.826.10`으로 교체됐고 새 GUI PID가 기동됐다.
- 기존 headless daemon PID는 종료되지 않았다.

그러나 클린 `2026.826.10` 재기동 뒤 Sidebar는 WebView에 남은 단일 프로젝트만 표시했다. native
`session_state.json`에는 `default`, `maho-workspace`, `orca-lite`, `superwiki-mail-otp`,
`orca-lite-release-verify-13768`의 다섯 workspace와 기존 terminal session이 남아 있었으므로, 이는
설치/서명/relaunch 실패가 아니라 project catalog recovery의 실제 회귀다.

원인은 `mergeRecoveredProjectBootstrap()`이 단일 WebView catalog를 복구 대상으로 보려면 그 프로젝트가
현재 process의 `getInitialProject()` 결과와 정확히 같아야 한다는 조건이었다. Finder 또는 `/Applications`에서
기동하면 startup root가 저장된 마지막 프로젝트와 다를 수 있어, 여러 native workspace가 존재해도 recovery를
건너뛰었다.

후속 hotfix는 다음 회귀 테스트로 고정한다.

- native session이 여러 workspace를 포함하고 localStorage catalog가 그중 하나인 단일 프로젝트일 때,
  startup project가 달라도 모든 session workspace를 복구한다.
- 이미 여러 프로젝트를 가진 catalog에는 과거 workspace를 추가하지 않는다.

이 항목은 후속 서명 릴리스에서 실제 설치 E2E를 다시 통과하기 전까지 성공으로 판정하지 않는다.
