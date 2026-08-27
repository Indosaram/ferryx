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
