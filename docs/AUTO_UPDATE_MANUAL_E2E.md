# 자동 업데이트 수동 E2E 절차

자동 검증은 실제 `tauri-plugin-updater`가 로컬 HTTP 매니페스트를 조회하고, 새 버전을 판별하고,
업데이트 아티팩트를 내려받아 서명을 검증하는 경로까지 통과했다. 이 문서는 설치된 데스크톱 앱이
GitHub Release를 상대로 동일한 경로를 한 번 더 검증하는 절차다.

## 사전 조건

1. GitHub Release에 `latest.json`, 해당 플랫폼 updater 아티팩트, 그리고 같은 이름의 `.sig`가 있다.
2. 설치할 앱의 버전이 `latest.json.version`보다 낮다. 날짜 태그는 `vYYYY.MM.DD[.N]`이고, 앱과
   매니페스트가 쓰는 버전은 `YYYY.MMDD.revision`이다. 예: `v2026.08.26.1` -> `2026.826.1`.
3. 설치된 앱의 `src-tauri/tauri.conf.json` 공개키와 Release `.sig`를 만든 개인키가 같은 키쌍이다.

## 절차

1. 이전 버전 Ferryx를 실행한다. 데스크톱 앱은 시작할 때 서명된 업데이트를 백그라운드에서 자동 확인한다.
2. Settings > General > Software Update로 이동한다. 발견된 업데이트는 이 화면에 표시된다. 표시되지 않았을 때만
   **Check for Updates**를 누른다.
3. 새 버전 번호와 릴리스 노트가 표시되고 **Install and Relaunch**가 강조되어 활성화되는지 확인한다.
4. **Install and Relaunch**를 누른다. 서명된 업데이트의 다운로드 진행 막대가 0에서 100까지
   증가한 뒤 앱이 자동으로 설치되고 재시작되는지 확인한다.
5. 앱이 재시작된 뒤 Settings > General에서 현재 버전이 Release 버전과 같은지 확인한다.
6. 재시작 직전의 프로젝트, worktree, terminal tab이 다시 보이는지 확인한다. 실행 중이던 agent/terminal PTY는
   daemon에 남아 있으므로 앱 창이 재시작되어도 백그라운드에서 계속 실행된다.

### 프로젝트 목록 복구 확인

업데이트 과정에서 WebView의 `ferryx.projects` 목록이 비어 있거나 시작 폴더 하나로 축소되었더라도,
최신 Ferryx는 native `session_state.json`에 저장된 workspace 목록으로 프로젝트 카탈로그와 활성 프로젝트를
복구한다. 따라서 hotfix 설치 후에는 Settings를 다시 열기 전에 Sidebar에서 이전 프로젝트와 worktree가 돌아왔는지
확인한다.

## 실패 판정

- 업데이트가 없다고 나오면 설치본과 `latest.json`의 `version`을 비교하고, Settings를 한 번 열어 update 상태를 확인한다.
- Install and Relaunch 뒤 진행 막대가 나타나지 않거나 재시작되지 않으면 `.sig` 파일 이름,
  `latest.json.platforms`의 URL과 signature, 그리고 공개키/개인키 쌍을 확인한다. 서명 오류가
  원인일 때는 앱을 강제로 재시작하거나 서명 검증을 우회하지 않는다.
- `failed to unpack '._Ferryx.app'` 오류가 나오면 updater 아카이브에 AppleDouble 메타데이터가
  포함된 것이다. 해당 릴리스를 재시도하지 말고, `scripts/assert-updater-archive-layout.mjs`를
  통과한 새로 서명된 `.app.tar.gz`와 `.sig`가 있는 후속 릴리스로 교체한다.
- 자동 재시작이 동작하지 않으면 `process:default` capability가 포함된 최신 앱 번들을 설치했는지
  확인한다.

수동 실행 결과(설치 전/후 버전, Release URL, 복구된 프로젝트/worktree 이름, agent/session 상태, 화면 캡처)는
`docs/evidence/auto-update/`에 추가한다.
