# DAG 뷰포트 네비게이션 검증 보고서

## 1. 개요 및 현재 판정 상태

본 문서는 DAG 뷰포트 제스처 및 카메라 제어 기능에 대해 현재까지 확보된 증적을 정리한 지속 검증 기록 초안이다.
Task 4의 최종 완료나 제품 인수 승인을 의미하지 않으며, 네이티브 입력 게이트가 차단된 상태에서 현재 후보의 검증 범위를 확정하기 위해 작성되었다.
현재 종합 판정 상태는 PARTIAL이다.

### 후속 네이티브 기동: 준비 완료, 입력 검증 대기

2026-09-13 로컬 시각 18:02부터 Wave 3 작업트리에서 정확히
`bun tauri dev`로 별도 검증용 앱을 기동했다.
GUI PID 41685, 전용 데몬 PID 42625 및 전용 소켓 경로를 확인했다.
설치된 기존 앱과 데몬은 이 실행의 정리 대상이 아니다.
원본 경계 기록의 baseline/pre 응답은 동일한 백엔드 세션
`6a0733d2-a7e7-40d3-81f9-0c5633f227c3`와 데몬 epoch
`1789290215601`을 가리킨다.
카메라에는 초기 Fit의 요청과 반영 결과가 기록되어 있다.

독립 이미지 판독은 실제 DAG 패널과 옆 터미널, 임시
`Native DAG QA (synthetic provider)` 컨트롤 및 앞쪽 권한 안내창을 확인했다.
사용자도 해당 패널과 디버그 표기를 확인했다.
이 실행의 호스트는 `standalone`이며 기존 DAG 팝업을 교체한 것이 아니다.
팝업은 별도 네이티브 검증이 필요하다.

현재까지 분석한 카메라 기록 128행에는 초기 Fit과 버튼을 누르지 않은
포인터 이동만 있다. 실제 드래그, 휠, 물리 핀치, 입력 전후 격리 결과나
터미널 양성 대조군을 통과한 것으로 처리하지 않는다.
사용자에게 `Fresh pre → DAG 드래그 → Fresh post`를 요청했고,
기록 변경 알림을 연결했다. 데스크톱 입력 자동화는 수행하지 않았다.
SVG 좌표 독립 검증은 완료됐으며 계측 불일치를 확인했다.
원본 commit 3에서 c-d 경로의 경계 사각형은 배율 약 0.40776을 반영하지만,
`getScreenCTM()`으로 계산한 끝점은 배율을 반영하지 않고 해당 사각형 밖에 있다.
별도 이미지 판독은 가리지 않은 c-d 연결선이 실제 카드 양쪽에 붙어 있음을
확인했다. 이 프레임은 화면 정렬 불량보다 좌표 계측 불일치를 뒷받침한다.
정확한 WebKit 원인은 미확정이며, 기록된 CTM 끝점은 네이티브 정렬 판정에
사용하지 않는다. 다른 연결선이나 물리 제스처까지 통과한 것으로 확대하지 않는다.
또한 원본 기록은 `siblingTerminalsSelected=false`이므로 빈 형제 터미널 배열을
터미널 위치 불변의 증거로 사용할 수 없다.
독립 분석: `/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/native-live-geometry-audit-gjfNxhtV.md`.

이 기동은 앞선 사전 점검의 미기동 상태만 갱신한다.
C5/S7 및 과거·현재 S8을 해소하지 않는다.
이후 감독 세션 `bash_254`가 시간 초과, 종료 코드 1로 끝났다.
종료 직후 기록된 검증용 런처·프런트엔드·GUI·전용 데몬·셸 PID는 모두
프로세스 목록에서 사라졌고 5173 리스너도 없었다.
기존 설치 앱 680과 데몬 1010은 원래 시작 시각으로 유지됐다.
최종 원본 기록은 여전히 baseline/pre 두 응답과 카메라 128행뿐이다.
정상 Dispose 응답이나 개별 자식 종료 대기 결과를 확보한 것은 아니므로
현재 프로세스 부재를 완전한 S8 통과로 바꾸지 않는다.
죽은 실행의 파일 감시를 해제했으며, 앞선 창의 조작 안내는 더 이상 유효하지 않다.
원본 증거는 보존하고, 다음 실행 전에 좌표 계측 수정과 감독 시간 제한을 해결한다.
현재 실행의 소유권 및 원본 기록 위치:
`/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/native-live-gjfNxhtV.md`.

판정의 세부 내역은 다음과 같다.

- 브라우저 카메라 구현 및 영구 수정: 반영 완료. 통합 후보 커밋은 3a826a15988a57ca7f5346be9e3440674d350e01 (브랜치 dag-viewport-integration, 부모 커밋 165ce821d0dd58d091852c85ab60904d494c9591)이다.
- 네이티브 요구사항(C5, S7) 및 이력 자원 회수(S8): 미검증(UNVERIFIED).
- 최종 병렬 감사(F1, F2, F3, F4): 미착수.
- main 브랜치 병합: 차단됨(BLOCKED). 커밋 트리 자체는 충돌 없이 병합 가능하지만, main 작업 트리에 존재하는 타 세션의 미커밋 변경 사항(.omo/plans/dag-viewport-navigation.md의 태스크 1 체크박스 및 ui/src/components/NativeTerminalPane.tsx의 휠 델타 정규화 코드)과의 중첩으로 인해 안전하게 중단되었다.

## 2. 제스처 및 뷰포트 카메라 동작 수명주기

뷰포트 카메라는 화면상의 CSS 픽셀 좌표계(p = t + s * w)를 기준으로 동작한다.
좌표 상태는 dagStore나 워크스페이스 전역 저장소에 영속화되지 않으며 컴포넌트 내부 상태로 격리된다.

드래그 이동(Drag Pan):
마우스 좌클릭 또는 터치/펜 포인터를 사용하여 배경 및 비대화형 노드 카드를 이동시킨다.
포인터 이동 델타는 스케일로 나누지 않고 화면 픽셀 그대로 평행이동 값(x, y)에 직접 더해진다.
버튼, 링크, 입력창, contenteditable, data-no-pan 속성을 가진 대화형 자식 요소 위에서의 포인터 다운은 감지 즉시 무시되어 뷰포트 이동을 유발하지 않는다.
수락된 포인터는 뷰포트 컨테이너에 setPointerCapture를 적용하여 영역 밖으로 벗어난 포인터 이동도 안정적으로 수집한다.
포인터 업 또는 취소 시 캡처는 멱등하게 해제된다.
드래그가 발생한 포인터 세션에서는 합성 클릭 이벤트(onClickCapture)를 차단하여 카드 클릭 오동작을 막는다.
버튼을 누르지 않은 상태에서의 마우스 이동은 카메라 위치를 변화시키지 않는다.

휠 및 Ctrl+휠 확대/축소(Wheel & Ctrl-wheel Zoom):
뷰포트 DOM 엘리먼트에 직접 바인딩된 passive:false 휠 이벤트 리스너가 동작을 제어한다.
브라우저의 기본 페이지 스크롤 동작을 방지하기 위해 preventDefault와 stopPropagation이 즉시 호출된다.
입력 델타는 normalizeWheelDeltaPixels를 통해 정규화된다. 라인 델타는 16을 곱하고, 페이지 델타는 뷰포트 높이를 곱한다.
요청 배율은 requestedScale = currentScale * exp(-0.002 * clamp(deltaPixels, -1000, 1000)) 공식으로 계산된다.
스케일을 먼저 유효 범위(minScale부터 MAX_SCALE 3.0)로 제한한 뒤, 포인터 아래의 월드 좌표가 고정되도록 평행이동 위치(tNew = p - (sNew / s) * (p - t))를 조정한다.
일반 마우스 휠과 Ctrl+휠은 완전히 동일한 줌 로직을 공유하며 별도의 추가 배율을 부여하지 않는다.
가로 휠(deltaY === 0, deltaX !== 0)은 카메라 제스처로 취급하지 않으며 무시된다.
배율 한계에 도달한 상태에서 발생하는 휠 이벤트 역시 이벤트를 소비하되 카메라 위치 표류를 차단한다.

터치 핀치(Touch Pinch):
두 개의 터치 포인터가 감지되면 두 점의 중점과 초기 거리(d0), 월드 기준점((m0 - t0) / s0)을 즉시 기록한다.
두 접점 사이의 거리가 1px 이상일 때만 유효한 핀치 기준선으로 인정한다.
거리 변화 비율(d1 / d0)에 따라 배율을 갱신하며 중점 위치를 기준으로 화면을 확대하거나 축소한다.
단일 터치에서 2-터치로 넘어가거나 다시 1-터치로 복귀할 때 활성 포인터 맵을 재색인(rebase)하여 카메라 위치가 급격하게 튀는 현상을 방지한다.
현재 배율이 새로운 최소값보다 낮으면 추가 축소 요청은 현재 배율을 유지한다. 확대 요청만 최소값을 향해 점프 없이 진행한다. 최대값보다 높은 경우에는 반대 방향으로 같은 규칙을 적용한다.

복구 및 배율 조정 버튼:
상단 툴바에는 1.2배 축소(Zoom out), 100% 초기화(Reset zoom to 100%), 1.2배 확대(Zoom in), 전체 맞춤(Fit graph) 버튼이 위치한다.
축소, 100% 초기화, 확대 버튼은 뷰포트 중앙을 기준으로 배율을 변경한다. Fit은 전체 그래프가 들어오는 배율과 중앙 배치를 새로 계산한다.
HTML 표준 disabled 속성 대신 aria-disabled 속성을 사용함으로써 모달의 포커스 트랩이 비활성 버튼에 걸려 갇히는 문제를 원천 방지한다.
배율 한계에 도달했을 때의 버튼 클릭은 카메라 상태를 바꾸지 않는 no-op으로 안전하게 처리된다.

수명주기 및 호스트 환경 적응:
ResizeObserver가 뷰포트 크기를 감시하며, 크기 변화 시 너비와 높이 차이의 절반만큼 평행이동 좌표를 이동시켜 화면 중앙의 월드 좌표를 유지한다.
초기 측정 크기가 0인 상태에서는 줌 계산을 유예하며 양수 크기가 확보되는 즉시 최초 Fit을 적용한다.
새로운 실행(runId 변경)이 들어오거나 빈 그래프에서 첫 노드가 채워지는 시점에는 활성 제스처를 취소하고 Fit 카메라를 자동 설정한다.
반면 동일한 실행 내에서 노드 상태나 연결선이 갱신될 때는 사용자가 이동해 둔 현재 카메라 시점을 유지한다.
창 블러(blur), 문서 숨김(visibilitychange hidden), 컴포넌트 언마운트가 발생하면 모든 활성 포인터와 캡처를 강제 해제한다.
실제 그래프 패널 폭이 400px 미만이면 헤더와 범례를 협소 레이아웃으로 배치한다. 브라우저 전체 폭이 아닌 패널 폭이 기준이다.
모달 포커스 트랩과 닫힘 복원은 브라우저 및 관련 컴포넌트 테스트에서 확인했다. 영구 후보 브라우저 하네스의 배지는 TerminalSplitView 밖에 있으므로, 이 결과만으로 모달 포털에서 터미널 리프로의 포커스 전달이나 실제 네이티브 지연 포커스 복구까지 보장하지 않는다.

## 3. 요구사항 및 시나리오 검증 매핑

요구사항 C1부터 C5, 시나리오 S1부터 S8의 검증 현황은 아래와 같다.
모든 증적은 리포지토리 상대 경로 또는 외부 작업트리 절대 경로로 명시되었다.

요구사항 C1 (드래그 내비게이션):
- 상태: ACCEPTED (브라우저 및 단위 테스트 기준).
- 충족 내역: 좌클릭 드래그를 통한 월드 이동, 픽셀 1:1 일치, 헤더 고정, 스크롤바 미표시, 경계 밖 포인터 해제 및 클릭 합성 방지 검증 완료.
- 증적 경로:
  - 베이스라인 실패 기록(RED): /Users/indo/code/project/orca-lite-wt/dag-viewport-wave1/.omo/evidence/dag-viewport-navigation/baseline/adversarial-verify.md
  - 브라우저 통과 기록(GREEN): /Users/indo/code/project/orca-lite-wt/dag-viewport-wave2/.omo/evidence/dag-viewport-navigation/green/repair3/adversarial-verify.md
  - 단위 테스트: ui/src/components/dag/DagGraphView.viewport.test.tsx
- 한계: 네이티브 데스크톱 앱 내 실제 입력 동작은 미검증.

요구사항 C2 (줌 및 핀치):
- 상태: PARTIAL (브라우저 휠 및 터치 핀치 ACCEPTED, 네이티브 물리 트랙패드 UNVERIFIED).
- 충족 내역: 휠 델타 정규화, 지수 스케일링, 포인터 앵커 유지, 한계 클램핑, DevTools 터치 핀치 및 1-2-1 접점 전환 검증 완료.
- 증적 경로:
  - 브라우저 통과 기록: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave2/.omo/evidence/dag-viewport-navigation/green/repair3/adversarial-verify.md
  - 영구 후보 브라우저 보완 판정: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/permanent-browser/StrippedBrowserAcceptedVerdict.md
- 명시적 공백: macOS 상의 실제 물리 트랙패드 제스처 전달은 검증되지 않았다. 에뮬레이션 입력이나 Chrome 프로필 기반 터치 제스처는 네이티브 검증 대체물로 인정되지 않는다.

요구사항 C3 (복구 제어 및 수명주기):
- 상태: ACCEPTED (브라우저 및 단위 테스트 기준).
- 충족 내역: Fit 버튼, 100% 리셋 버튼, Plus/Minus 버튼의 한계 안전성, 390x844 모바일 뷰포트 내 컨트롤 가시성, 0 크기 지연 처리, 창 리사이즈 중심 보존, 대규모 노드 그래프 Fit 계산 검증 완료.
- 증적 경로:
  - 뷰포트 컨트롤 시각 검증: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave2/.omo/evidence/dag-viewport-navigation/green/repair2/visual-verdict.md
  - 협소 레이아웃 수락 기록: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/narrow-layout-accepted.md
  - 영구 후보 보완 판정: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/permanent-browser/StrippedBrowserAcceptedVerdict.md
- 한계: 후속 네이티브 런타임은 기동했지만 실제 창 리사이즈 동작과 중심 보존은 미검증.

요구사항 C4 (인접 영역 회귀 방지):
- 상태: PARTIAL (브라우저 호스트 간섭 배제 ACCEPTED, 네이티브 터미널 격리 UNVERIFIED).
- 충족 내역: DagPaneBadge 모달 열림 상태에서의 Tab 키 가둠, Escape 닫기 및 포커스 복원, TerminalSplitView 내 DAG 리프 분할 경계 드래그 유지 및 형제 노드 격리 확인.
- 증적 경로:
  - 모달 상호작용 검증: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave2/.omo/evidence/dag-viewport-navigation/green/repair3/modal-complete/independent-verdict.md
  - 모달 포커스 격리 테스트: ui/src/components/TerminalSplitView.modalFocus.test.tsx
  - 네이티브 터미널 포커스 소유권 테스트: ui/src/components/NativeTerminalPane.focusOwnership.test.tsx
- 명시적 공백: 실제 실행 중인 백엔드 PTY 세션으로의 휠 스크롤 또는 키 누출 여부는 네이티브 환경에서 실측되지 않았다.

요구사항 C5 (실제 앱 검증 및 자원 정리):
- 상태: UNVERIFIED 및 BLOCKED.
- 사유: macOS Accessibility 권한이 거부되어 OS 차원의 자동화 입력 주입이 차단되었다(2026-09-13T02:37:03Z 확인). 화면 기록(Screen Recording) 권한만으로는 입력 주입이 불가능하다. 물리 트랙패드 핀치 장치 역시 에이전트 환경에서 지원되지 않는다.
- 증적 경로:
  - 네이티브 입력 게이트 상태: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/native/preparation/input-gate-current.md
  - 네이티브 차단 및 다음 조치 보고서: /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/native-next-action.md
  - 격리 기동 사전 점검(HOLD): /Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/native/preparation/isolated-launch-preflight.md

시나리오 S1 (양쪽 호스트 팬):
- 브라우저: PASS. 모달 및 독립 분할 뷰에서 (100, 100)에서 (220, 180)으로 드래그 시 월드 엘리먼트와 연결선 좌표가 정확히 (120, 80) 이동함을 확인.
- 네이티브: UNVERIFIED.

시나리오 S2 (양쪽 호스트 줌):
- 브라우저: PASS. 중앙 기준 휠 인/아웃 시 스케일 축소 및 원복 확인, 앵커 좌표 오차 1px 이내, 수평 휠 시 무반응, 한계 배율 시 스케일 및 이동 고정 확인.
- 네이티브: UNVERIFIED.

시나리오 S3 (컨트롤 및 Fit):
- 브라우저: PASS. 화면 밖으로 이동한 후 Fit 클릭 시 전체 그래프가 뷰포트 내로 복귀, 100% 리셋 및 확대/축소 버튼 정상 동작, 390x844 모바일 뷰포트에서 컨트롤 잘림 없음 확인.
- 네이티브: UNVERIFIED.

시나리오 S4 (제스처 수명주기):
- 브라우저: PASS. 모달 경계 밖으로 포인터를 드래그하여 놓았을 때 캡처가 정상 해제되고 추가 이동이 발생하지 않음. 모달을 2회 닫고 다시 열었을 때 이벤트 리스너 누수 없이 단일 휠 동작 배율이 일정하게 유지됨.
- 네이티브: UNVERIFIED. 모달 세 번째 열림의 최초 캡처 구현은 임시 계측 코드에서 독립 검증되었으나, 실제 네이티브 S4 실행 증거는 없다. 구현 근거는 외부 작업트리의 native-next-action.md에 연결되어 있다.

시나리오 S5 (터치 핀치):
- 브라우저: PASS. Chrome DevTools 프로토콜을 통한 2개 접점 핀치 입력 시 스케일 1.6배 갱신 및 앵커 불변 확인. 단일 터치에서 다중 터치 전환 시 위치 튐 없음 확인.
- 네이티브 물리 트랙패드: UNVERIFIED.

시나리오 S6 (실시간 갱신 및 호스트 상호작용):
- 브라우저: PASS. 그래프가 이동/확대된 상태에서 dagStore 상태 갱신 발생 시 카메라 유지. 다른 run 탭 선택 시 새로운 Fit 적용. 창 리사이즈 시 중심점 유지. Tab 키 네비게이션 시 모달 내부 순환 및 Escape 키로 모달 닫힘 확인.
- 네이티브: UNVERIFIED.

시나리오 S7 (네이티브 전달 및 PTY 격리):
- 상태: UNVERIFIED 및 BLOCKED.
- 세부 내용: bun tauri dev 디버그 앱 상에서 모달 및 독립 패널의 실제 클릭/휠 전달 증적이 확보되지 않았다. 형제 터미널 PTY 세션으로의 휠 이벤트 누출 차단 센티넬 테스트가 미수행 상태로 남아 있다.

시나리오 S8 (자원 회수 및 정리):
- 상태: PARTIAL.
- 세부 내용: 영구 후보 브라우저 러너 프로세스(Chrome 컨텍스트, Vite 서버 PID 47578/47579, 포트 5193)는 완전히 종료 및 회수되었다. Wave 3 작업트리 내 15개 임시 계측 파일 삭제 및 아카이브 무결성 검증은 통과했다(/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/post-cleanup-accepted.md). 그러나 과거 이력 차수의 네이티브 자원 완전 종료 영수증은 최종 완료 상태가 아니다.

## 4. 독립 자동화 검증 상세 (커밋 3a826a15)

영구 통합 후보 커밋 3a826a15988a57ca7f5346be9e3440674d350e01의 실행 기록을 독립 검증 세션(st_01a09905)이 소스 해시, 설정, 원본 출력과 대조했다. 이 검증 세션이 테스트나 빌드를 새로 실행한 것은 아니다.

관련 테스트 스위트 (285개 테스트 통과):
총 10개 관련 테스트 파일, 285개 테스트 케이스가 실패 없이 통과했다(exit 0).
- NativeTerminalPane: 151개 통과
- DagGraphView.viewport: 48개 통과
- DagPaneBadge: 25개 통과
- dagViewUtils: 20개 통과
- DagGraphView: 14개 통과
- paneFocus: 6개 통과
- modalFocus: 5개 통과
- focusOwnership: 3개 통과
- badge ownership: 8개 통과
- DagNodeCard: 5개 통과

빌드 및 타입 검사 등가 실행:
문자 그대로의 bun run build 스크립트는 실행되지 않았다.
심볼릭 링크된 공유 node_modules 디렉토리(.vite-temp) 내부로 번들 임시 파일이 기록되는 오염을 방지하기 위함이었다.
검증은 격리된 설정(vitest.permanent.config.mjs, vite.permanent-build.config.mjs)을 사용하여 tsc(PID 46677, exit 0) 실행 후 Vite 프로덕션 빌드(PID 46705, exit 0)를 순차 수행하는 등가 절차로 진행되었다.
컴파일러 출력은 깨끗하게 종료되었으며 Vite 번들링 결과물(App-DbddyozA.js 514.63 kB)이 정상 생성되었다.

보존된 비정상 출력 및 경고:
- 최초 테스트 설정 로딩은 `ReferenceError: __dirname is not defined`로 종료 1을 반환했고 테스트는 0개 실행되었다. 이는 동작 RED가 아닌 하네스 시작 실패이며, 이후 동등 설정의 통과 기록과 구분해 보존했다.
- TypeScript 힌트 1건: ui/src/components/NativeTerminalPane.tsx 190:20-190:27 위치에서 TS6385 'keyCode' is deprecated 제안 힌트가 보고되었으며, 기존 코드로 보존되었다.
- Vite 청크 크기 권고: App 번들이 514.63 kB로 500 kB 권장 크기를 초과하여 청크 분할 권고 메시지가 stderr에 출력되었다.
- 테스트 mock 에러 출력: NativeTerminalPane.test.tsx 5116행의 마운트 재시도/백오프 테스트에서 두 차례의 의도된 모의 실패 스택(cmd_native_terminal_attach 실패 로그)이 stderr에 기록되었다. 이는 테스트 케이스의 의도된 통과 경로이다.
- 공유 캐시 변경: 빌드 과정 중 /Users/indo/code/project/orca-lite/ui/node_modules/.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json 파일의 SHA-256 해시가 변경되었다. 변경을 발생시킨 프로세스의 작성자(writer)는 식별되지 않았다(UNKNOWN).

독립 검증 증적 문서:
/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/permanent-automated-independent.md

## 5. 브라우저 및 시각 검증 내역

실제 Chromium 환경을 이용한 헤드리스 러너 검증은 메인 테스트와 레이아웃 보완 테스트의 두 단계로 완료되었다.

메인 러너 (Main run 227):
총 398개 검사 항목이 모두 통과했다(398/398 PASS).
데스크톱(1280x900)과 모바일(390x844), 모달 호스트와 독립 분할 뷰 호스트 전반에 걸쳐 S1부터 S6까지의 제스처 및 레이아웃 상태가 검증되었다.
페이지 콘솔 에러는 0건으로 확인되었다.

최종 보완 러너 (Supplement run 235):
총 8개 검사 항목이 모두 통과했다(8/8 PASS).
400px 분기점을 오가는 헤더 크기 변화 시 뷰포트 높이가 16px 증가함에 따라 요구되는 +8px의 cameraY 이동과 복귀가 오차 없이 기록되었다.
모든 컨트롤의 가시성, 히트 영역 클릭 가능 여부, 분할 경계 불변성이 확인되었다.

5개 시각 프레임 검토 (StrippedCandidateVision.md):
메인 4개 프레임(데스크톱/모바일, 모달/독립 분할 뷰)과 모바일 보완 프레임 1개를 대상으로 시각적 정합성 검토가 완료되었다.
모바일 보완 이미지 출처에 대한 명시적 한계가 존재한다.
검토 보고서에 기재된 모바일 보완 캡처 해시(73d12568a73bd36fd9a87927b38376f41121e8a280d1056e9efd9b997f028b8e)는 최종 235 차수의 캡처가 아니라 run 228 시점에 캡처된 second-stripped-run/host-layout/mobile-390.png 파일의 해시이다.
동일한 소스 후보에 대한 정적 픽셀 검토 증적으로서 유효하지만, 235 차수의 산출물에 대해 개별적인 픽셀 감사가 다시 수행된 것은 아니다.

브라우저 수락 증적 문서:
/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/permanent-browser/StrippedBrowserAcceptedVerdict.md

### 실제 App 검증용 컨트롤: 브라우저 한정 인수

영구 후보의 위 결과와 별도로, 임시 계측을 포함한 Wave 3 실제 App에서 모달/독립 패널 및 1280px/390px 조합의 8개 시나리오, 374개 검사가 통과했다(run 187).
리드는 원본 `final-pane-width-host-run/browser-results.json`의 모든 검사 이름과 비통과 항목 집합을 확인했고, 기록된 관련 소스 10개의 해시가 현재 Wave 3 파일과 모두 일치함을 확인했다.
검사는 실제 포털 이동 중 대기 상태 유지, Tab/Shift+Tab, 닫힌 뒤 배지 포커스 복원, 세 번째 열림에서 단일 휠 배율, 잘못된 실행/제공자 제외, 대기 중 Dispose의 순서와 오류 보존을 포함한다.
오류 시나리오의 `boundary-identity-mismatch`와 모달 해제 시 `viewport-detached` 콘솔 기록은 삭제하지 않았다. 이 실행을 콘솔 오류 0건으로 표현하지 않는다.

후속 협소 화면과 399/400/399 전환 검증 및 시각 검토까지 연결하여 계획의 `Verify browser fixture controls and owned resource cleanup` 하위 항목을 완료 처리했다.
원본 브라우저 종료 기록에는 8개 컨텍스트와 브라우저 종료가 있고, 부모의 `narrow-controls/exact-clipping-runtime.md`에는 보완 실행까지 합한 9개 컨텍스트, 2개 브라우저 및 서버 181/PID 24114·24116/포트 5194 정리가 기록되어 있다.
분기점 검증의 컨텍스트·브라우저·서버 199 정리는 별도 `threshold/AcceptedVerdict.md`에 기록되어 있다.

증거 디렉터리:
`/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/native/minimal-fixture/browser/`.
주요 판정은 `FinalPaneWidthHostVerdict.md`, `narrow-controls/ExactClippingVerdict.md`, `narrow-controls/PaneWidthVision.md`, `threshold/AcceptedVerdict.md`다.
초기 `VisualVerdict.md`의 도구 부족 판정은 당시 실행되지 않은 시도의 기록으로 보존한다.
이후 실행 증거가 해소한 범위는 브라우저 하위 항목뿐이다. Tauri 전송·저장·PTY는 모두 모의 처리되었으므로 실제 네이티브 바인딩, 세션 생성, 입력, 프로세스 회수나 C5/S7을 증명하지 않는다.

## 6. 전체 테스트 스위트 상태 및 미해결 게이트

현재 남아 있는 미해결 작업과 게이트는 다음과 같다.

전체 테스트 스위트 (Full Suite):
커밋 `3a826a15`에서 필터 없는 전체 Vitest 검사를 한 번 실행한 결과 FAIL이다.
217개 파일 중 205개 통과, 12개 실패이며, 2,334개 테스트 중 2,307개 통과, 27개 실패다. pending/todo는 0개이고 종료 코드는 1이다.
`bun run --cwd ui test`에 검증된 동등 설정, runner 로더, 결과 캐시 비활성화 및 JSON 출력 인자를 사용했다. `--maxWorkers=1`과 기존 시간 제한은 유지했다.
전체 실행 시간은 323.62초이며 재시도나 감독 프로세스 시간 초과는 없었다. 담당자는 추적 파일 1,797개의 전후 해시 일치와 실행 프로세스·소유 의존성 링크 정리를 기록했다.
27개 실패의 정확한 테스트 이름, 원본 출력, JSON, 명령과 정리 기록:
`/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/final/full-suite/report.md`.
독립 비교 작업 `st_01a099d5`가 DAG 구현 전 기준 `f4ab00e2`에서 전체 검사를 한 번 실행했다. 기준 결과는 213개 파일, 2,265개 테스트 중 27개 실패이며 종료 코드는 1이다.
실패한 27개 테스트의 파일·이름과 전체 실패 메시지는 두 실행에서 모두 일치했다. 메시지 비교에서 작업트리 경로와 ANSI 색상만 정규화했으며, 후보에만 발생한 실패와 기준에만 발생한 실패는 모두 0개다. 리드도 양쪽 원본 JSON을 별도로 대조해 같은 결과를 확인했다.
따라서 관찰된 27개 실패는 기존 실패로 분류한다. 전체 스위트가 통과했다는 뜻은 아니며, 특히 9개 시간 초과의 내부 원인이 같거나 타이밍 변동이 없다고 증명한 것은 아니다. 이 실패 집합을 근거로 DAG 제품 코드를 수정하지 않는다.
비교 보고서와 원본 증거:
`/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/evidence/dag-viewport-navigation/final/full-suite/baseline-attribution.md`.

네이티브 입력 및 트랙패드 핀치 게이트 (C5 / S7):
마지막 권한 관찰은 2026-09-13T02:37:03Z의 Accessibility 거부 기록이며, 이후 권한 상태를 새로 확인한 것은 아니다.
에이전트가 사용할 수 있는 물리 트랙패드 입력 경로는 확인되지 않았다. 실제 입력 도구를 가진 실행 주체의 검증 또는 사용자의 명시적인 요구사항 조정이 필요하다.
가상 터치나 브라우저 Ctrl+휠 에뮬레이션을 통한 C5 완화는 허용되지 않는다.

과거 네이티브 자원 종료 증거 (S8):
`st_01a099e4`의 대조 보고서와 원래 실행 주장·기록 복구 보고서를 리드가 확인했다.
앞선 실패 런처 47591의 종료 기록은 이후 `st_01a09673`이 주장한 실행의 기록이 아니다.
후자의 원본 실행 PID·프로세스 그룹·PTY 및 종료 대기 결과는 복구되지 않았다.
확보된 기록에서 현재 종료할 작업 소유 네이티브 프로세스나 PTY도 식별되지 않았지만, 이는 누수가 없다는 증명이 아니다.
현재 PID 부재나 새로운 정상 실행으로 과거 종료 증거를 대신하지 않는다. S8은 미완료다.
대조 보고서:
`/Users/indo/code/project/orca-lite-wt/dag-viewport-wave3/.omo/ulw-execute/historical-s8-reconciliation.md`.

main 브랜치 병합 차단:
main 작업 트리에 존재하는 타 세션 소유의 미커밋 파일로 인해 충돌 방지 차원에서 병합 명령이 차단되어 있다.
외부 수정 소유자의 변경 사항 커밋 또는 분리 선행 커밋 승인이 선행되어야 안전한 병합이 가능하다.

최종 감사 게이트 (F1부터 F4):
계획 준수 감사(F1), 코드 품질 검토(F2), 수동 QA 실측 감사(F3), 스코프 충실도 검토(F4)는 시작되지 않았다.
모든 미해결 게이트가 해소된 후 최종 독립 감사 절차를 진행해야 한다.
