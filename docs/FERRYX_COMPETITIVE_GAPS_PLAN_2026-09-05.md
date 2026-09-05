# Ferryx 기능 도입 계획 - 기존 기반·Design Mode·모바일

조사 및 개정일: 2026-09-05
상태: 범위 정정 / 제품 코드 구현 전. 구현 대상은 **기존 기반 완성, 브라우저 Design Mode, 모바일 작업 완성**이다. 최신 사용자 요청에 따라 PR·checks·실패 로그·GitHub/Linear 이슈 연계를 제외한다. 이전 번호 해석 오류로 추가했던 전체 Git GUI·파일 편집기·음성·E2EE relay도 구현 대상으로 취급하지 않는다.

## 1. 개요 및 범위 결정

원래 다섯 가지 우선순위에서 사용자가 지목한 1번은 기존 기반 완성, 3번은 모바일 작업 완성이다. 두 범위와 앞서 남긴 Design Mode를 구현한다. 제외 기능을 의존성이나 후속 과제로 다시 추가하지 않는다.

### 구현할 세 범위와 우선순위

1. **기존 기반 완성**: 전체 프로젝트의 대기 작업 모아보기, SSH 설정과 Run on, 과거 대화 검색·재개.
2. **브라우저 Design Mode**: 요소·영역 선택 → 스크린샷·DOM/CSS 확인 → 메모 작성 → 선택한 에이전트에게 전달.
3. **모바일 작업 완성**: 공개 에이전트 제어 API, 채팅·승인, 독립 원격 관리, 푸시. 기존 모바일 터미널을 없애지 않고 읽기 쉬운 제어 화면을 추가한다.

아래 단계는 제품 우선순위다. 공통 API 계약처럼 다른 기능의 기반이 되는 부분은 먼저 설계하고, 독립적인 구현은 병렬로 진행할 수 있다.

### 계획에서 제거된 기능 (Out of Scope)

사용자 후속 의견에 따라 아래 항목은 도입 대상에서 제외한다. 비용이나 운영 책임을 정한 뒤 추진할 대기 과제가 아니며, 포함 기능의 선행 조건에도 넣지 않는다.

- **4. 반복 업무 지원**: 예약 실행, 개발 서비스 수명주기·포트 관리, 사용량·계정 추적, 에이전트 휴면, 워크플로 플러그인.
- **5. 별도 제품 결정**: 음성, Cloud VM recipe, E2EE relay.
- **PR·checks·실패 로그·GitHub/Linear 이슈 연계**: 외부 계정 연결, 이슈 기반 작업 생성, PR 생성·조회, CI 상태·로그·에이전트 전달을 모두 제외한다.
- **전체 리뷰 도구로의 확대**: 파일 탐색기·편집기, diff·stage/commit GUI, diff 리뷰 주석.

초기 조사에 있던 별도의 artifact 공유 서비스나 단축키 재지정 등도 이 정정을 이유로 자동 추가하지 않는다. 현재 실행 범위는 위 세 흐름과 아래 태스크로 정한다.

---

## 2. 브라우저 Design Mode (요소/영역 스크린샷·DOM·메모 전달)
- **개념 및 목적**:
  - 프론트엔드 UI/스타일 작업 시, 임베디드 브라우저 화면에서 수정이 필요한 요소를 직접 클릭하거나 사각형 영역으로 드래그하여, 시각적 스크린샷과 정확한 DOM/CSS 메타데이터를 에이전트에 직관적으로 전달하는 인계 파이프라인.
- **주요 기능 요건**:
  - **Design Mode 토글 및 Inspect 오버레이**: 브라우저 상단 툴바에 "Design Mode" 토글 버튼 제공. 활성화 시 마우스 커서 호버에 따른 DOM 하이라이트 박스 및 치수 표시.
  - **요소 선택 (Inspect Element)**: 요소를 클릭하면 태그명, ID/Class, CSS Selector 경로, 계산된 주요 스타일(Computed CSS: color, margin, padding, display, font 등) 및 해당 요소 영역의 스크린샷 자동 캡처.
  - **영역 드래그 캡처 (Area Crop)**: 직사각형 드래그로 복수 요소나 화면 레이아웃 일부를 캡처(PNG DataURL).
  - **사용자 메모 입력 다이얼로그**: 캡처 후 팝오버에서 사용자가 요청사항(예: "이 버튼 좌측 마진 8px 추가하고 모바일에서 숨겨줘")을 작성.
  - **터미널/에이전트 인계 파이프라인**: 캡처된 스크린샷(임시 파일 저장 경로 또는 인라인 지원 형식)과 DOM/CSS 정보, 사용자 메모를 취합하여 현재 활성 터미널(PTY)의 입력으로 주입하거나 에이전트 프롬프트로 전달.
- **Ferryx 현재 상태 및 직접 근거**:
  - Ferryx에는 Tauri 자식 웹뷰 기반 `BrowserPane.tsx` 및 `ui/src/lib/browserTauri.ts`가 구현되어 기본적인 웹 페이지 탐색, 검색, 줌, 다운로드가 가능하다.
  - 그러나 브라우저 웹뷰 내부의 DOM 선택 오버레이, 요소 하이라이트 인젝션 스크립트, 뷰포트 영역 스크린샷 캡처 및 터미널 PTY로의 피드백 전달 파이프라인은 부재하다.
- **참고 출처**:
  - `https://www.onorca.dev/docs/browser/design-mode`

---

## 3. 구현 로드맵 및 태스크

### Phase 1. 기존 기반 완성

- [ ] A1. 전체 프로젝트의 대기 작업 모아보기
  - **기반/대상**: `src-tauri/src/remote/server.rs`, remote protocol, daemon session inventory, `ui/src/remote/RemoteApp.tsx`, Sidebar. 기존 waiting 탐지와 모바일 이동 UI를 재사용한다.
  - **구현 내용**: 현재 desktop 선택과 전체 작업 상태 집계를 분리한다. host/workspace/tab/backend session으로 이동 대상을 식별하고 unread와 waiting을 구분한다.
  - **완료/검증**: 서로 다른 두 프로젝트의 실제 PTY를 waiting 상태로 만들면 현재 선택과 무관하게 모두 표시된다. 항목 선택 시 정확한 pane으로 이동하며 종료된 대상에 대한 입력은 다른 세션으로 전달되지 않는다. 상태 전이 테스트와 실제 remote API를 확인한다.

- [ ] A2. SSH 설정과 Run on
  - **기반/대상**: `src-tauri/src/ipc/ssh.rs`의 import/list/update/test/remote worktree 명령, Settings, `ProjectDialogs.tsx`.
  - **구현 내용**: SSH host 등록·설정 가져오기·연결 테스트, 프로젝트/새 작업의 Local 또는 SSH 실행 위치 선택, 원격 worktree와 PTY 수명주기를 연결한다. 기존 SSH backend를 중복 작성하지 않는다.
  - **완료/검증**: 격리된 SSH host에서 작업 생성→원격 cwd 확인→실제 명령 출력→연결 중단/재접속까지 검증한다. host key 변경·인증 실패·권한 없음·daemon 부재를 구분한다. 단순 socket 연결 성공을 원격 실행 완료로 세지 않는다.

- [ ] A3. 과거 대화 검색·재개
  - **기반/대상**: `ui/src/lib/agentResumeAffordance.ts`, provider session 계약, 기존 command palette와 별도의 대화 기록 화면.
  - **구현 내용**: 프로젝트·에이전트·검색어로 과거 기록을 찾고, 페이지 단위로 대화를 읽고 선택 재개한다. 열린 탭 검색이나 터미널 find와 구분한다.
  - **완료/검증**: provider별 fixture와 격리된 실제 agent에서 종료→검색→대화 열람→원래 cwd/session ID로 재개를 확인한다. 대형/손상/삭제 로그를 처리하며 Ferryx가 agent session ID를 생성·주입하지 않는다.

### Phase 2. 브라우저 Design Mode 및 시각적 피드백

기존 기반과 독립적으로 구현할 수 있으며 C1의 내부 대상 선택·전달 계약을 재사용한다. 공개 API 전체 출시를 기다릴 필요는 없다. 아래 1→2→3 순서로 연결한다.

- [ ] 1. 브라우저 인스펙트 오버레이 및 선택 엔진 구현
  - **대상 파일**: `ui/src/components/BrowserPane.tsx`, `ui/src/lib/browserTauri.ts`, `src-tauri/src/` (웹뷰 IPC)
  - **구현 내용**:
    - 브라우저 툴바에 Design Mode 활성화 토글 추가.
    - 웹뷰 내부에 삽입할 인스펙트 스크립트 작성 (마우스 호버 하이라이트, 클릭 시 태그/클래스/selector/computed style 수집).
    - 사각형 드래그 영역 선택 UI 추가.
  - **완료 조건**: Design Mode 활성화 시 브라우저 요소에 하이라이트가 반응하고, 클릭 시 해당 요소의 DOM/CSS 정보가 프론트엔드로 전달된다.

- [ ] 2. 스크린샷 캡처 및 피드백 작성 모달
  - **대상 파일**: `ui/src/components/BrowserDesignFeedbackModal.tsx` (신규), `ui/src/lib/browserTauri.ts`
  - **구현 내용**:
    - 선택된 요소 또는 드래그 영역의 뷰포트 캡처 (웹뷰 스크린샷 API 또는 캔버스 추출).
    - 캡처 썸네일, 요소 셀렉터 요약, 텍스트 메모 입력란을 갖춘 팝오버/모달 제공.
    - 대상 터미널 세션 선택기(현재 활성 pane 기본값).
  - **완료 조건**: 요소 선택 즉시 미리보기와 함께 메모를 작성할 수 있는 팝업이 뜨고 취소/전송이 정상 동작한다.

- [ ] 3. 터미널/에이전트 PTY 피드백 주입 브리지
  - **대상 파일**: `ui/src/lib/tauri.ts`, `src-tauri/src/ipc/`
  - **구현 내용**:
    - 스크린샷 이미지를 임시 파일(`.ferryx/design-feedback/<id>.png`)로 저장.
    - DOM 계층, 계산된 스타일, 이미지 경로, 사용자 메모를 마크다운 형태의 프롬프트 템플릿으로 패키징.
    - 활성 터미널의 PTY stdin으로 주입(`sendInput`)하여 실행 중인 에이전트(Claude, Codex 등)가 즉시 인식할 수 있도록 처리.
  - **완료 조건**: 사용자가 확인한 대상 세션에 스크린샷과 DOM 정보가 포함된 메시지를 전달한다. 원격 agent에는 로컬 경로만 보내지 않고 대상 host에서 읽을 수 있는 첨부 경로를 사용한다.
  - **검증**: 실제 fixture 페이지의 요소·영역 캡처와 agent의 이미지 읽기를 확인한다. 화면 이동·stale browser generation·iframe·캡처 실패·대상 종료·포커스 전환을 다룬다. OS별 실제 캡처 API를 확인하며 DOM을 canvas로 그리는 것만으로 원본 화면과 동일하다고 가정하지 않는다.

---

### Phase 3. 모바일 작업 완성

휴대폰에서 작업을 시작하고, 대화를 읽고, 승인에 응답하고, 알림에서 돌아오는 흐름을 완성한다. 공개 제어 API는 서버 전체를 무인증 공개한다는 의미가 아니라 문서화된 인증·권한 계약을 제공한다는 의미다.

- [ ] C1. 공개 에이전트 제어 API
  - **기반/대상**: daemon command/event 계약, remote gateway, `main.rs`의 기존 browser CLI 옆 workspace/pane/agent 제어 명령. raw PTY 입력과 의미 있는 agent 제어를 구분한다.
  - **구현 내용**: create/list/start/prompt/read/wait/stop, 안정된 JSON 결과·오류 코드·권한 검사·event subscription을 제공한다. UI와 외부 client가 동일한 서비스를 사용한다.
  - **완료/검증**: 앱 내부 함수를 직접 호출하지 않고 실제 CLI/API client로 agent 시작→prompt→working→응답 대기/완료를 관측한다. 취소·timeout·unknown·view-only·폐기된 token·종료된 대상·중복 전송을 검증한다. 완료 상태를 추측하거나 고정 sleep에 의존하지 않는다.

- [ ] C2. 모바일 채팅·승인·질문 (C1 의존)
  - **기반/대상**: provider capability adapter, transcript/composer/permission card, 기존 `RemoteTerminal.tsx`와의 전환.
  - **구현 내용**: 읽기 쉬운 대화·도구 실행 결과, 명시적인 승인/거절·질문 선택지, 사진/파일 첨부와 미전송 draft 유지. 요청 ID와 session identity로 응답을 묶는다.
  - **완료/검증**: 지원 provider의 실제 질문·승인·거절·중단을 휴대폰에서 처리하고 terminal과 이어진다. 두 client의 동시 응답·오래된 요청·reconnect·첨부 실패·IME 입력을 확인한다. 구조화된 계약이 없는 agent는 terminal로 남기며 출력 문자열만 보고 승인 버튼을 추측하지 않는다.

- [ ] C3. 독립 원격 관리 (A1, A2, C1 의존)
  - **기반/대상**: `RemoteApp.tsx`, remote session 목록, 기존 worktree 생성 REST handler와 `RemoteClient.createWorktree`. desktop 선택과 remote client 선택을 분리한다.
  - **구현 내용**: host/workspace 목록·연결 상태, 원격 작업 생성·agent 실행·중지, host 전환·재접속. 기존 worktree API는 DTO와 실제 동작을 검증해 연결하며 backend spawn 없는 임시 session 문자열을 실행 성공으로 세지 않는다.
  - **완료/검증**: desktop 창 없이 phone/web에서 목록 조회→작업 생성→실제 PTY/agent 실행→재접속한다. 두 격리 host에서 같은 local session ID가 충돌하지 않고 desktop과 phone이 독립적으로 탐색한다. control 충돌·device revoke·작업 삭제를 검증한다.
  - **경계**: C2 전체 UI는 이 관리 기능의 선행 조건이 아니다. LAN/Tailscale 등 도달 가능한 host 경로로 구현하며 E2EE relay나 Cloud VM provisioner를 요구하지 않는다.

- [ ] C4. 모바일 푸시와 정확한 작업 복귀 (A1, C3 의존)
  - **기반/대상**: agent notification event, device subscription, service worker 또는 native notification bridge, host/workspace/session deep link.
  - **구현 내용**: 작업 완료·응답/승인 대기를 background push로 알리고, 알림을 누르면 해당 작업으로 이동한다. 권한 요청·알림 설정·구독 해제·token 폐기·본문 노출 설정을 제공한다.
  - **완료/검증**: iOS/Android 실기기의 잠금/백그라운드 상태에서 알림을 받고 정확한 작업으로 복귀한다. 종료된 작업·권한 거부·만료 구독·기기 폐기·중복 알림을 처리한다. 열린 페이지의 waiting 배너나 desktop 알림만으로 완료 처리하지 않는다.
  - **구현 전 선택**: HTTPS PWA Web Push 또는 native delivery 중 지원 기기에 맞는 경로와 자격 증명 보관 방식을 정한다. 일반 HTTP LAN에서 PWA push가 동작한다고 가정하지 않는다. 푸시 delivery는 제외한 E2EE relay와 별개로 설계한다.

## 4. 검증 및 품질 기준

- **비침습성**:
  - 새 제어 화면을 사용하지 않아도 기존 로컬 worktree 생성 및 터미널 사용이 정상 동작해야 한다.
  - 브라우저 Design Mode가 꺼져 있을 때는 웹뷰 성능이나 마우스 이벤트 처리에 오버헤드가 없어야 한다.
- **크로스 플랫폼**:
  - macOS, Windows, Linux 전 타겟에서 웹뷰 스크린샷 캡처 및 터미널 주입이 동일하게 작동해야 한다.
- **안전한 인증 관리**:
  - 외부 API 토큰은 일반 텍스트 로그나 커밋에 노출되지 않아야 하며 안전하게 저장/파기되어야 한다.
- **기존 기능 보존**:
  - daemon PTY 생존·재개, 탭/분할/DnD, browser identity, 모바일 terminal/대기 이동을 유지한다. 화면용 DAG ownership은 backend process ownership으로 사용하지 않는다.
- **검증 방식**:
  - 동작 변경은 해당 경계에서 실패하는 회귀 테스트부터 작성한다. 비동기 검증은 정확한 event를 먼저 구독하고 bounded timeout으로 기다린다.
  - 실제 worktree·PTY·SSH host·remote client·지원 agent로 완료 조건을 검증한다. desktop 수동 QA는 debug 앱을 정확히 `bun tauri dev`로 실행하며 사용자의 직접 조작을 요청한다. 모바일 푸시는 실기기 QA 전에는 완료라고 하지 않는다.
- **범위와 진행 상태**:
  - 이 문서는 범위와 검증 조건의 복원이다. 제품 코드는 구현하지 않았고 위 checkbox는 모두 미완료다. 조사 시점 소스·공식 문서 근거이며 구현 착수 전 현재 코드를 다시 확인한다.
  - 문서의 LSP 진단은 daemon socket 연결 실패로 사용할 수 없었다. 제품 동작 변경이 없으므로 이번 범위 정정에서 빌드/제품 테스트는 실행하지 않는다.

## 5. 비교 근거

- Orca: [대화 기록](https://www.onorca.dev/docs/agents/session-history), [Design Mode](https://www.onorca.dev/docs/browser/design-mode).
- Paseo: [공식 저장소](https://github.com/getpaseo/paseo), [연결 방식](https://paseo.sh/docs/connectivity). native mobile push와 web background push의 제공 범위를 동일시하지 않는다.
- Herdr: [에이전트 자동화](https://herdr.dev/docs/agent-automation/). 공식 모바일은 SSH TUI이며 별도 제품인 Moshi의 기능을 Herdr 자체 기능으로 계산하지 않는다.
