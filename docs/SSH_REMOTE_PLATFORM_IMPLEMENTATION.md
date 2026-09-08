# SSH 원격 플랫폼 지원 구현과 검증

작성일: 2026-09-08
상태: 구현 및 자동 검증 완료. 사용자 데스크톱 수동 확인 항목은 아래에 별도 기재.

## 해결 범위

`maho-win`의 PowerShell에 POSIX 명령 `true`를 보내던 검증을 제거했다.
호스트의 실제 응답으로 Windows/POSIX 실행 환경을 탐지하고, 그 결과를
프로젝트 등록, 원격 셸 시작, Git 조회, 파일 전송에 공통으로 적용한다.
Windows 호스트를 WSL로 바꾸거나 SSH 서버의 기본 셸을 변경하지 않는다.

## 구현

- `src-tauri/src/ssh/runtime.rs`: 원격 환경, 실행기, 홈/임시 디렉터리,
  Git 사용 가능 여부를 탐지한다. 요청별 nonce가 있는 UTF-8 응답을 검증한다.
  Windows PowerShell/pwsh를 먼저 확인해 Windows의 sh/WSL 런처를
  Windows 자체의 실행 환경으로 오인하지 않는다.
- `src-tauri/src/ssh/operations.rs`: 원격 디렉터리 확인, Git 실행,
  대화형 셸 계획, 이미지 업로드와 선택적 확장 설치를 플랫폼별로 처리한다.
  경로와 인수는 데이터로 전달하며 POSIX 인용을 PowerShell에 적용하지 않는다.
- `src-tauri/src/ssh/direct.rs`: 공통 OpenSSH 전송을 유지한다.
  자동화 작업에는 명시적 `-T`를 적용한다. 종료 코드, 오류 단계,
  원문 stderr 및 원시 바이트의 Base64 표현을 보존한다.
- `src-tauri/src/ssh/projects.rs`와 `src-tauri/src/ipc/project_remote.rs`:
  원격 플랫폼을 저장하고 해당 플랫폼의 절대 경로를 검사한다.
  기존 POSIX 레코드와 프로젝트 ID는 유지한다. 새 연결에서 플랫폼이
  달라졌으면 기존 프로젝트에 조용히 연결하지 않고 재등록을 요구한다.
- `src-tauri/src/terminal/service.rs`와 `src-tauri/src/daemon/server.rs`:
  탐지한 실행기로 원격 PTY를 시작한다. Windows 셸 시작의 임시 오류 설정은
  사용자 대화형 셸에 남기지 않는다.
- `src-tauri/src/ssh/state_bridge.rs`: Windows에서는 원격 루프백 TCP로 받은
  상태를 세션 전용 SSH stdout 채널로 전달한다. 세션 ID와 토큰을 확인하고
  토큰은 로컬 상태 이벤트에서 제거한다. 상주 원격 데몬은 설치하지 않는다.
- `src-tauri/resources/agent-extensions/ferryx-agent-state.ts`: 기존 Unix 소켓과
  선택적 인증 TCP 전송을 함께 지원한다.
- `ui/src/components/settings/SshSection.tsx`: 탐지된 플랫폼/실행기/Git 상태와
  실패 단계를 표시한다. 확장 설치는 별도의 `Prepare agent integration`
  버튼으로만 실행한다. Test와 프로젝트 등록, 터미널 시작의 자동 설치를 제거했다.

Windows OpenSSH에서 중첩 PowerShell의 `OpenStandardInput().Read`가
3,788바이트 입력을 읽지 못하고 정지하는 것을 별도 진단으로 확인했다.
Windows 파일 전송은 `-Command -`가 받는 ASCII 스크립트에 Base64 데이터를
담고, 원격에서 명시적으로 바이트 배열로 복원한다. 파일 바이트를 텍스트로
해석하거나 명령행 길이 제한에 맞추기 위해 이미지를 잘라 버리지 않는다.
확장 재설치의 원자적 교체는 Windows PowerShell 5.1에 실제 null 문자열을
전달하도록 `[NullString]::Value`를 사용한다.

원격 상태 프로세스가 SSH 종료 후 남는 문제도 발견했다. 새 구현은 원격
SSH 소유 프로세스의 종료 이벤트를 .NET Task로 받아 루프백 리스너를 닫는다.
진단 중 남은 프로세스는 PID와 생성 시각, 실행 내용으로 테스트 소유가
확인된 것만 정리했다. 사용자 터미널이나 Ferryx 데몬은 종료하지 않았다.

## 검증 기록

- 수정 전 `ssh maho-win true`: PowerShell `CommandNotFoundException`, 종료 코드 1.
- 수정 전 실제 IPC 진입점 회귀 테스트: `reachable` 단언 실패.
- 실제 maho-win에서 연결 및 환경 탐지 성공.
- 공백, 한글, 작은따옴표, 달러, 앰퍼샌드, 괄호가 포함된 Windows 경로에서
  디렉터리 확인, 프로젝트 등록과 저장된 플랫폼 복원 성공.
- 임시 Git 저장소 초기화, 원격 URL 설정 및 루트/원격 URL 조회 성공.
- 격리된 임시 홈에서 확장 설치와 재설치, 배포 원본 바이트 일치 확인 성공.
- pwsh 실행 경로로 동일 디렉터리 확인 성공.
- 65,536바이트 바이너리 업로드 후 원격 파일을 다시 읽어 전체 바이트 일치 확인.
- 실제 Windows SSH PTY에서 입력 에코와 구분되는 응답 및 실제 현재 디렉터리 확인.
- 실제 Linux 호스트 `omarchy`에서 환경 탐지, 디렉터리 확인과 PTY 현재 디렉터리 확인.
- 설정/호스트/상태 확장 관련 Vitest 39개 통과.
- `bun run --cwd ui build` 종료 코드 0.
- SSH 관련 Rust 회귀 50개 통과.
- 최종 maho-win 통합 테스트 3개가 단일 실행에서 모두 통과했다.
  상태 보고 수신과 SSH 종료 후 원격 전달 프로세스 종료 단언을 포함한다.
- 최종 Linux 실호스트 통합 테스트 1개 통과.
- 최종 결합 검증 명령은 종료 코드 0이며 `git diff --check`도 통과했다.

실호스트 테스트는 외부 호스트를 요구하므로 일반 테스트에서 자동 실행하지
않는다. 아래 명령은 해당 테스트를 명시적으로 실행한다.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib ssh -- --nocapture
FERRYX_SSH_WINDOWS_HOST=maho-win cargo test --manifest-path src-tauri/Cargo.toml \
  --test ssh_windows_live -- --ignored --nocapture --test-threads=1
FERRYX_SSH_POSIX_HOST=omarchy cargo test --manifest-path src-tauri/Cargo.toml \
  --test ssh_posix_live -- --ignored --nocapture
```

테스트는 전용 임시 저장소와 임시 홈만 수정하고 정리한다.
실제 사용자의 에이전트 확장 폴더에는 테스트 설치를 하지 않았다.

## 화면 확인

격리된 Vite 페이지에서 실제 `SshSection` 컴포넌트를 렌더링하고 IPC 응답만
대체했다. Test 후 환경 정보 표시와 명시적 확장 준비 동작을 확인했다.
1,100px 및 390px 폭에서 문서의 가로 스크롤 폭이 뷰포트 폭과 일치했다.

- `docs/evidence/ssh-platform-desktop.png`
- `docs/evidence/ssh-platform-mobile.png`

스크린샷을 캡처했지만 현재 모델은 이미지의 시각적 판독을 지원하지 않아
미적 품질 검수 완료로 표현하지 않는다. DOM 동작과 기하학적 폭 검증을 수행했다.
임시 QA HTML과 Vite 서버는 제거/종료했다.

## 제한과 수동 확인

- LSP 데몬이 응답하지 않아 LSP 진단은 실행되지 않았다. Rust 컴파일과 테스트,
  TypeScript 빌드 결과로 검증했다.
- 다른 세션의 공유 작업 때문에 발생한 컴파일/라우팅 회귀는 해당 세션의
  수정 후 재검증했다. 그 변경을 되돌리거나 해당 테스트를 건너뛰지 않았다.
- 기존 원격 워크트리 생성/삭제 IPC는 미지원이다. 이 작업은 이미 미지원인
  기능을 새로 구현한 것이 아니다. 기존 지원 Git 작업의 실행 경계를 추가했다.
- UNC 공유, Windows SSH 기본 셸을 cmd로 설정한 별도 서버, WSL 자체 SSH,
  Windows 로컬 클라이언트는 이번 실호스트 검증 대상이 아니었다.
- 사용자 데스크톱은 자동 조작하지 않았다. `bun tauri dev` 디버그 앱에서
  SSH Settings의 maho-win Test, Windows 경로 프로젝트 등록, 새 탭/분할과
  실제 에이전트 상태 표시를 사용자가 확인해야 한다.
- 릴리스 앱 교체나 배포, 기존 데몬 재시작은 하지 않았다.

## 코드 리뷰 및 독립 커밋 검증

선행 SSH 변경과 폴더 선택기를 별도 커밋으로 나누도록 사용자 승인을 받았다.
독립 리뷰에서 호스트 설정 교체 후 오래된 비동기 결과가 반영되는 문제와
POSIX 통합 파일 복사 실패를 성공으로 보고하는 문제를 발견해 수정했다.
수정 후 코드 리뷰 결과는 APPROVE다.

선택기를 제외한 독립 트리에서 UI 테스트 42개, SSH Rust 테스트 52개,
원격 등록 테스트 4개, UI 빌드와 cargo check를 통과했다. 실제 Windows
통합 테스트 3개와 Linux PTY 테스트 1개도 통과했다.

상세 리뷰 및 검증 근거: `docs/SSH_PLATFORM_PREREQUISITE_CODE_REVIEW.md`.
관련 없는 릴리스·패키징·에이전트 종료 리뷰 변경은 커밋 범위에서 제외한다.
