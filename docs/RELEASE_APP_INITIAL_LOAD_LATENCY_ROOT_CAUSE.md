# Ferryx Release App Initial Page Load Latency Root Cause Analysis

**Date:** 2026-09-12  
**Target:** `/Applications/Ferryx.app` (macOS Release Bundle)  
**Investigator:** OmO  

---

## 1. Executive Summary

Ferryx 릴리즈 빌드(`/Applications/Ferryx.app`) 실행 시 초기 페이지 로딩이 심각하게 지연되는 원인을 런타임 로그(`/tmp/rorca-501/boot-trace.log`, `/tmp/ferryx-switch-debug.jsonl`), macOS 시스템 로그(syspolicyd/launchservicesd), 프로세스 트레이스 및 소스 코드를 교차 검증하여 분석했습니다.

측정 결과, **순수 웹뷰 JS 엔진 파싱 및 React 렌더링 자체는 ~30ms 수준으로 매우 빠릅니다.**
그러나 첫 페이지(터미널 워크스페이스/메인 뷰)가 사용자에게 온전히 노출되기까지 **최소 5초에서 최대 11초 이상**의 블로킹 지연이 발생하는 핵심 원인 4가지가 확인되었습니다.

---

## 2. Core Root Causes (핵심 원인 분석)

### 원인 1 (가장 치명적): 활성 프로젝트가 Remote SSH 타깃일 때의 동기식 원격 탐색 블로킹 (8.6초 ~ 11초 지연)
- **증상**: 앱 실행 직후 터미널 화면이 완전히 멈추고 빈 상태로 지속됨 (`workspace.restore.gated`).
- **원인 코드**:
  - `ui/src/App.tsx`: `useWorkspaceRestore`의 활성화 조건이 `enabled: registeredProjectId === activeProject.workspaceId`로 게이트되어 있음.
  - 앱 시작 시 복원된 활성 프로젝트가 원격 SSH 워크스페이스(예: Windows/Linux SSH 머신)인 경우, `registeredProjectId`는 처음에 `null`로 시작함.
  - `registerRemoteProject` -> `cmd_project_register` -> `crate::ssh::runtime::detect(&host)`가 원격 머신으로 SSH 핸드셰이크를 맺고 파워쉘/Bash 환경 및 Git 경로를 네트워크를 통해 탐지함.
  - `src-tauri/src/ssh/runtime.rs`의 탐지 데드라인은 **최대 12초**(`Duration::from_secs(12)`)로 설정되어 있음.
- **실측 증거 (`/tmp/ferryx-switch-debug.jsonl`)**:
  - `+465ms`: `project.register.start` (SSH 프로젝트 등록 시작)
  - `+466ms`: `workspace.restore.gated` (SSH 등록 대기로 복원 멈춤)
  - `+9067ms`: `project.register.success` (**8.6초 동안 SSH 원격 네트워크 블로킹**)
  - `+11113ms`: `worktree.refresh.listed` (원격 worktree 조회로 **추가 2초 소요**)
  - `+11134ms`: `workspace.restore.preloaded` (**시작 후 11.1초가 지나서야 비로소 터미널 복원 시작**)

---

### 원인 2: 마운트 즉시 모든 비활성 프로젝트(16개) 및 모든 SSH 호스트로 동시 네트워크/Git 프로브 폭주
- **증상**: 앱이 켜지자마자 백엔드 Tokio 스레드풀과 시스템 CPU/네트워크가 포화됨.
- **원인 코드**:
  - `ui/src/state/inactiveProjectWorktrees.ts`:
    ```tsx
    const resolved = await Promise.all(
      targets.map(async (project) => {
        if (project.target?.kind === "ssh") {
          ...
          const listed = await services.listWorktrees(project.workspaceId);
        }
        const registered = await services.registerProject(...);
        const listed = await services.listWorktrees(project.workspaceId);
      })
    );
    ```
  - 현재 사용자의 등록 프로젝트 17개 중 16개의 비활성 프로젝트(SSH 6개, 외장 드라이브 `/Volumes/T9-Mac` 4개 포함)에 대해 마운트 직후 `Promise.all`로 일제히 `listWorktrees` 및 `registerProject`를 호출함.
  - SSH 프로젝트 6개가 동시에 원격 호스트(`ssh-maho-win`, `ssh-omarchy`)에 SSH 세션을 개설하려 시도하면서 IPC 큐와 네트워크 소켓 리소스가 경합함.

---

### 원인 3: 릴리즈 앱 실행 시 `cwd = "/"`로 인한 `cmd_project_initial` 즉시 실패 및 Fallback 복원 경로 강제 진입
- **증상**: 모든 릴리즈 앱 실행마다 `initial.error` 발생 후 세션 복구 경로로 꺾임.
- **원인 코드**:
  - `src-tauri/src/ipc/project.rs`:
    ```rust
    let cwd = std::env::current_dir()?;
    let canonical = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    if canonical.parent().is_none() {
        return Err(IpcError::from(WorktreeError::InvalidPath {
            path: canonical,
            reason: "filesystem root cannot be registered as a startup workspace".to_string(),
        }));
    }
    ```
  - macOS GUI 앱 번들(`/Applications/Ferryx.app`)을 Finder/Spotlight/Dock에서 실행하면 OS에 의해 기본 `current_dir`이 항상 `/`(루트)로 설정됨.
  - `canonical.parent().is_none()` 조건에 걸려 `cmd_project_initial`이 무조건 실패(`INVALID_PATH`)함.
- **실측 증거 (`/tmp/rorca-501/boot-trace.log`)**:
  - 릴리즈 앱의 모든 실행 기록에서 `initial.start` 직후 10ms 만에:
    `"filesystem root cannot be registered as a startup workspace"` 에러가 발생하며 `.catch` 블록으로 넘어가 디스크 `loadSession()` 및 `loadProjectBootstrap()` 복구 경로를 거침.

---

### 원인 4: macOS Gatekeeper(syspolicyd) 온라인 공증 티켓 검증 지연
- **증상**: Dock에서 앱 아이콘을 클릭했을 때 창이 즉시 뜨지 않고 수 초간 바운스되거나 반응이 늦음.
- **실측 상태 (`spctl -a -vvv -t install /Applications/Ferryx.app`)**:
  - `rejected, source=Unnotarized Developer ID`
  - `Ferryx.app does not have a ticket stapled to it.`
  - Apple Developer ID 인증서로 서명되었으나 Notarization Ticket이 번들에 스테이플링(`staple`)되어 있지 않은 상태입니다.
  - macOS Gatekeeper(`syspolicyd`, `trustd`)가 앱 실행 시점에 Apple CloudKit/OCSP 서버로 온라인 네트워크 유효성 검사를 수행하므로, 네트워크 상태에 따라 프로세스 생성 및 메인 윈도우 표시까지 2~4초의 프리런치(Pre-launch) 지연이 발생할 수 있습니다.

---

### 원인 5: Native Terminal View의 초기 다중 Attach/Detach 사이클 (플리커 및 렌더링 랙)
- **증상**: 메인 창이 뜬 뒤에도 터미널 서피스가 깜빡거리거나 다시 그려짐.
- **원인 로그 (`/tmp/ferryx-switch-debug.jsonl`)**:
  - `+445ms`: `terminal.surface.attach.start` (초기 WGPU 서피스 마운트)
  - `+515ms`: `terminal.surface.detach.scheduled` (초기 등록 상태 변화로 언마운트 예약)
  - `+526ms`: `terminal.surface.attach.start` (재마운트)
  - `+7785ms`: `terminal.surface.detach.scheduled` (SSH 및 프로젝트 갱신 이벤트로 다시 디태치)
  - `+11148ms`: `terminal.surface.attach.start` (최종 복원 후 다시 어태치)
  - 짧은 시간에 네이티브 AppKit 뷰/WGPU 서피스가 3회 이상 붙었다 떨어지면서 불필요한 GPU 리소스 재생성과 블랭크 화면이 유발됨.

---

## 3. Recommended Remediation Plan (개선 방안 제안)

1. **원격 SSH 프로젝트 비동기/낙관적 복원 (Optimistic Restore)**:
   - `useWorkspaceRestore`가 `registerRemoteProject`의 네트워크 응답을 기다리지 않고, 캐시된 로컬 스냅샷(`session_state.json`)의 터미널 레이아웃과 탭을 즉시 화면에 렌더링하도록 분리.
   - 원격 SSH 연결은 백그라운드에서 진행되고, 연결이 완료되면 터미널 I/O를 붙이도록 개선.
2. **비활성 프로젝트 워크트리 지연 로딩 (Lazy Inactive Worktrees)**:
   - 앱 켜자마자 16개 프로젝트 전체의 worktree를 `Promise.all`로 한 번에 조회하지 않고, 사이드바에서 해당 프로젝트 아코디언을 펼칠 때(on-demand) 조회하도록 전환. 특히 SSH 타깃은 필수적으로 지연 로딩.
3. **`cmd_project_initial` 루트 경로 방어**:
   - `cwd == "/"`인 경우 즉시 에러를 뱉는 대신, 마지막으로 사용했던 활성 프로젝트(`session_state.json`의 `activeWorkspaceId`)를 기본 프로젝트로 간주하여 정상 시작하도록 처리.
4. **macOS 배포 시 Notarization & Stapling 필수 적용**:
   - `xcrun notarytool` 및 `xcrun stapler staple` 파이프라인을 완전히 거친 번들로 설치하여 Gatekeeper 온라인 조회 지연 제거.
