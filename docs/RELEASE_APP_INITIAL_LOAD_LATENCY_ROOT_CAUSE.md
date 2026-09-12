# macOS Clean Install Initial Load Latency Root Cause & Resolution

**Date:** 2026-09-12  
**Target:** macOS Release Bundle Clean Install (`/Applications/Ferryx.app` on a fresh Mac)  
**Investigator:** OmO  

---

## 1. 현상 분리

다른 Mac(클린 머신)에 최신 배포 번들(v2026.09.11.1)을 인터넷에서 다운로드하여 설치하고 처음 실행했을 때 발생하는 초기 로딩 지연은 크게 두 구간으로 분리됩니다:

- **구간 A. 독(Dock) 아이콘 클릭 후 창이 뜨기까지의 프리런치(Pre-launch) 지연 (3초 ~ 8초)**:
  Apple Notarization 미스테이플링으로 인한 Gatekeeper(`syspolicyd`) 온라인 CloudKit/OCSP 서버 실시간 검증 지연.
- **구간 B. 창이 뜬 후 첫 UI/워크스페이스가 준비되기까지의 인앱(In-app) 지연 (1초 ~ 3초)**:
  백엔드 데몬 콜드 스폰 경합, 200ms 인위적 슬립 루프, 시작 워크스페이스 등록 동기화 블로킹.

---

## 2. [구간 B] 창이 뜬 후 1~3초 지연의 3가지 실제 원인

### 원인 1: `DaemonClient::connect_or_spawn`의 동기화 락 부재로 인한 데몬 중복 스폰 및 락 경합 실패
- **발생 위치**: `src-tauri/src/daemon/client.rs`
- **메커니즘**:
  - 앱이 처음 켜질 때 백엔드 Tauri Setup의 `start_remote_event_bridge`와 웹뷰 프론트엔드의 `cmd_project_initial`이 거의 동시에 `DaemonClient::connect_or_spawn()`을 호출합니다.
  - 새 컴퓨터에는 데몬이 실행되어 있지 않으므로 두 작업 모두 소켓이 없음을 감지하고 각각 `ferryx --daemon` 자식 프로세스를 동시에 스폰합니다.
  - 첫 번째 데몬 프로세스가 `daemon.lock`을 획득하고, 두 번째 데몬 프로세스는 파일 락 획득 실패로 즉시 비정상 종료(`Another daemon instance is already holding the lock`)됩니다.
  - 두 번째 자식 프로세스의 stdout 출력을 기다리던 `wait_for_daemon_ready`는 데몬 준비 토큰(`FERRYX_DAEMON_READY`)을 받지 못하고 `Daemon process stdout closed before emitting readiness signal` 에러를 발생시킵니다.
  - 이로 인해 `cmd_project_initial`이 실패하거나 타임아웃되어 프론트엔드가 빈 화면(`Initializing project`) 상태로 장시간 대기하게 되었습니다.

### 원인 2: 소켓 부재 시 불필요한 200ms 인위적 재시도 슬립 루프
- **발생 위치**: `src-tauri/src/daemon/client.rs`
- **메커니즘**:
  - `connect_or_spawn` 시작부에 5회 재시도 루프(`for attempt in 0..5 { tokio::time::sleep(50ms) }`)가 존재했습니다.
  - 이는 롤링 핸드오버(기존 데몬 소켓 언링크 중)를 위한 대기 루프였으나, 데몬이 아예 존재하지 않는 신규 클린 머신에서도 소켓 파일이 없는데 무조건 50ms씩 4번 총 200ms를 잠자며 스폰을 지연시켰습니다.

### 원인 3: `run_daemon_headless`의 소켓 바인딩 전 동기식 Git 실행 블로킹
- **발생 위치**: `src-tauri/src/cli.rs`
- **메커니즘**:
  - `run_daemon_headless`가 시작될 때, 소켓 리스너를 바인딩하고 `ready_tx`를 보내기 전에 `initial_project(server.workspace_registry())`를 동기적으로 실행했습니다.
  - `initial_project`는 내부적으로 `git rev-parse --show-toplevel` 등 서브프로세스를 스폰하므로, 데몬이 소켓을 열고 `FERRYX_DAEMON_READY`를 출력하기까지 수백 밀리초가 추가 지연되었습니다.

### 원인 4: (v2026.09.11.1 한정) Finder/Dock 실행 시 `cwd="/"`로 인한 `cmd_project_initial` 즉시 실패 및 유령 `default` 워크스페이스 고착
- **발생 위치**: `src-tauri/src/ipc/project.rs`
- **메커니즘**:
  - v2026.09.11.1 릴리즈는 `d7e5a02` 이전 빌드로, Finder/Dock에서 실행되어 작업 디렉토리가 `/`일 때 `filesystem root cannot be registered as a startup workspace` 에러를 반환했습니다.
  - 클린 머신은 로컬스토리지도 비어 있어 `default`(`repoRoot: "."`)로 폴백되었고, 등록이 불가능한 플레이스홀더로 인식되어 영구히 워크스페이스 런타임이 게이트(`workspace.runtime.gated`)되었습니다.
  - 이는 오늘 오전 커밋 `d7e5a02`에서 `$HOME`으로 폴백하도록 수정되었습니다.

---

## 3. 적용된 해결책 (Fixes Applied)

1. **`DaemonClient`에 Single-Flight `spawn_lock` 도입 (`src-tauri/src/daemon/client.rs`)**:
   - `spawn_lock: Arc<tokio::sync::Mutex<()>>`를 추가하여 다중 태스크가 동시에 데몬을 스폰하지 않도록 직렬화.
   - 락 획득 전후로 `try_connect_existing_socket()`을 수행하는 더블 체크 락킹 패턴 적용.
   - 첫 번째 태스크가 데몬을 정상 스폰하면, 대기하던 후속 태스크는 중복 스폰 없이 즉시 연결에 성공(0ms).
   - 데몬 중복 스폰 및 `daemon.lock` 충돌 에러 원천 차단.

2. **소켓 부재 시 200ms 슬립 제거 및 Fast-Path 즉시 연결 (`src-tauri/src/daemon/client.rs`)**:
   - 데몬 소켓이 이미 살아있으면 0ms 즉시 연결.
   - 소켓 파일이 디스크에 아예 없을 때는 200ms 대기 루프를 건너뛰고 즉시 `spawn_lock`을 잡고 데몬을 스폰하도록 개선.

3. **`run_daemon_headless`의 소켓 바인딩 비차단화 (`src-tauri/src/cli.rs`)**:
   - 데몬 기동 시 소켓 리스너 바인딩과 준비 완료 신호(`FERRYX_DAEMON_READY`) 방출을 최우선으로 진행.
   - `initial_project`는 `tokio::task::spawn_blocking`으로 백그라운드 스레드에서 비동기 처리하여 데몬 준비 완료 시점을 단축.

---

## 4. 검증 결과

- `cargo check --manifest-path src-tauri/Cargo.toml`: 성공 (종료 코드 0).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::client`: 21개 전수 통과.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib test_daemon_client_has_spawn_lock_for_single_flight`: 단일 비행 락 정상 동작 검증 통과.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib project`: 31개 전수 통과.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib cli`: 91개 전수 통과.
