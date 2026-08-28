# 설정 페이지 리뷰 (디자인 + 기능)

> **STATUS: RESOLVED in `f1dbab4`** (2026-08-28) — every P0/P1/P2 defect below is fixed and
> locked by a test verified to fail without its fix. P3 design items (type scale, `SettingsGroup`
> primitive, Workspace id exposure) are also done. Evidence: full ui suite 118 files / 1027 tests
> and production build pass on that commit in isolation; rendered proof for all 9 sections in
> `.omo/plans/evidence/settings-panel/after/`. Plan: `.omo/plans/settings-page-full-remediation-mass-ulw.md`.
>
> Deliberately NOT changed: `scrollback` remains in `terminalSettings.ts` storage (control removed,
> persisted data preserved); `main.tsx` keeps `installSettingsRuntimeBridge()` for pre-React first paint.

리뷰 대상: `ui/src/components/SettingsDialog.tsx`, `ui/src/components/settings/*`,
`ui/src/lib/{appearanceSettings,generalSettings,terminalSettings,browserSettings,notificationSettings,settingsRuntimeBridge}.ts`
기준 커밋 시점: 2026-08-28 작업 트리 상태.

---

## 1. 전체 인상

구조는 좋다. 9개 섹션(General / Appearance / Terminal / Shortcuts / Workspace / Agents /
Browser / Notifications / Remote Access)이 좌측 280px 내비 + 최대 896px 본문으로 나뉜
풀스크린 오버레이이고, 각 섹션이 파일 하나씩으로 분리되어 있어 유지보수 가능한 형태다.
`SettingsHeading` / `SettingRow` 프리미티브도 있고 shadcn 계열 UI 원자(Switch, Select,
Slider, Card, Badge, Button)를 쓰고 있어 시각적으로 일관된 편이다.

문제는 **기능 쪽**이다. 노출된 컨트롤 중 실제로 아무 효과가 없는 것이 최소 3개, 그리고
설정 반영을 담당하는 런타임 브리지가 근본적으로 잘못된 방식(전역 DOM 텍스트 패치)으로
구현되어 있다.

---

## 2. 치명적 결함

### 2.1 `settingsRuntimeBridge.ts` — DOM 텍스트를 직접 덮어쓰는 전역 브리지 (최우선)

`ui/src/lib/settingsRuntimeBridge.ts`는 `main.tsx`에서 설치되어 앱 전역에서 다음을 한다.

- `document.querySelectorAll("div")` 전수 순회 → `textContent === "Color scheme"`인 노드를
  찾아 그 조상의 마지막 자식 `span`의 `textContent`를 직접 대입 (`syncGeneralAppearanceLabels`).
- `h2` 전수 순회 → `"Effective preferences"` 텍스트를 찾아 형제 노드 텍스트를
  `"Local override"`로 대입 (`syncTerminalSourceLabel`).
- 버튼의 `textContent === "Reset to defaults"` 로 클릭 핸들러 분기.
- `window` 캡처 단계 `change` 리스너에서 `target.id`를 문자열 배열과 비교.

문제:

1. **React가 소유한 DOM을 직접 수정한다.** 리렌더 시 되돌아가고, 최악의 경우 React 재조정과
   충돌한다. 상태는 이미 `useAppearanceSettings` / `useTerminalSettings` 훅이 이벤트
   (`ferryx:appearance-settings`) 기반으로 들고 있으므로 DOM 패치가 필요할 이유가 전혀 없다.
2. **찾는 라벨이 현재 UI에 존재하지 않는다.** `"Color scheme"`, `"Density"`(단독),
   `"Effective preferences"`(h2)는 지금 코드 어디에도 없다. Appearance 섹션의 실제 라벨은
   `"Theme mode"` / `"Interface density"`이고, Terminal의 `"Effective preferences"`는 `h2`가
   아니라 `div`다. 즉 이 동기화 코드는 **전부 no-op**이며, 죽은 코드가 매 이벤트마다 전체
   DOM을 스캔하고 있다.
3. **Radix Select는 네이티브 `change` 이벤트를 발생시키지 않는다.**
   `appearance-theme-mode` / `browser-default-zoom` 등은 `SelectTrigger`(button)에 붙은 id다.
   `<select>`/`<input>`이 아니므로 `window`의 `change` 리스너는 절대 트리거되지 않는다.
   → `applyBrowserDefaultZoom()`(줌 변경 시 열려 있는 브라우저 탭에 반영)이 이 경로로는
   호출되지 않는다. 다행히 `BrowserSection.update()`가 자체적으로 `setBrowserZoom`을 호출하고
   있어 실제 기능은 살아 있지만, 브리지 쪽 경로는 완전히 사문화되었다.
4. `"Reset to defaults"`라는 **표시 문자열로 동작을 분기**한다. 문구를 바꾸거나 i18n을
   넣는 순간 조용히 깨진다.

**권고: 이 파일에서 남길 것은 `syncAppearance()` 초기 호출 + system 테마 media query 구독뿐이다.**
나머지(라벨 패치, change 리스너, 클릭 텍스트 매칭)는 삭제하고, 앱 루트에서
`useApplyAppearanceSettings()`를 호출하는 방식으로 대체하는 것이 맞다.

### 2.2 `useApplyAppearanceSettings()`가 어디서도 호출되지 않음

`ui/src/lib/appearanceSettings.ts:146`에 정의되어 있으나 참조처가 정의부 하나뿐이다.
정상 경로(훅으로 상태 구독 → `applyAppearanceSettings`)가 통째로 미사용이고, 대신 2.1의
DOM 브리지가 그 자리를 대신하려다 실패한 상태다. Appearance 변경이 즉시 반영되는 건
`saveAppearanceSettings`가 아니라 **`AppearanceSection`이 리렌더될 때가 아니라**
`syncAppearance`가 초기 1회만 도는 구조라서, 실제로는 다음이 성립한다:

- 테마/액센트/밀도 변경 → localStorage와 React state는 갱신됨
- 그러나 `document.documentElement.dataset.{theme,accent,density}` 는 **갱신되지 않음**
  (Radix Select라 `change` 이벤트가 안 나므로 `syncAppearance` 재호출 없음)
- 결과: **Appearance 섹션의 3개 설정이 앱 재시작 전까지 시각적으로 아무 효과가 없다.**

이건 리뷰에서 발견한 가장 큰 사용자 대면 버그다. 수정은 한 줄 수준(앱 루트에서
`useApplyAppearanceSettings()` 호출)이지만 영향은 크다.

### 2.3 Terminal `Scrollback` 설정이 소비되지 않음

`scrollback`은 `terminalSettings.ts`에서 저장·정규화·클램프(1000–100000)까지 되지만,
`syncNativeOverrides`가 네이티브로 보내는 페이로드는 `{ fontFamily, fontSize, macosOptionAsAlt }`
뿐이다(`ui/src/lib/terminalSettings.ts:184`). `src-tauri` 전체에서 scrollback을 받는 커맨드도
없다. 즉 **사용자가 스크롤백을 바꿔도 터미널 동작은 그대로다.** 노출된 컨트롤이 거짓말을
하고 있으므로 (a) 네이티브까지 연결하거나 (b) 연결 전까지 컨트롤을 감추는 것 중 하나가 필요하다.

### 2.4 Interface density가 루트 `font-size`만 바꾼다

`settings-runtime.css:154-160`에서 compact=16px, comfortable=17px. 그런데 UI 전반은
`text-[11px]`, `text-[12px]`, `h-7`, `size-3` 같은 **고정 px Tailwind 유틸**로 작성되어 있어
루트 폰트 크기에 반응하지 않는다. rem 기반이 아니므로 밀도 설정은 사실상 체감 차이가 없다.
설명 문구("Adjust spacing and padding across tabs, sidebars, and dialog chrome")도 실제 구현과
다르다 — 패딩은 전혀 바뀌지 않는다.

---

## 3. 기능적 문제 (중간 심각도)

### 3.1 Terminal 폰트 입력이 매 키 입력마다 네이티브 override를 푸시

`TerminalSection`의 Font family `Input`은 `onChange`마다 `updateSettings({ fontFamily })`를
호출하고, 이는 localStorage 기록 + `syncNativeOverrides` → `applyTerminalOverrides` IPC로
이어진다. "JetBrains Mono"를 타이핑하면 IPC가 16번 나가고, 중간 상태("J", "Je", "JetB"…)가
전부 유효한 override로 저장된다. Home page 필드처럼 `onBlur`/Enter 커밋 방식이 맞다.

### 3.2 Font size / Scrollback 숫자 입력이 빈 문자열에서 NaN → 0으로 클램프

`onChange={(e) => onFontSize(Number(e.target.value))}`. 필드를 비우면 `Number("") === 0`이
전달된다. `normalizeTerminalSettings`가 `Number.isFinite(0)`을 통과시키므로 fallback이 아니라
**클램프 하한(FONT_SIZE_MIN)으로 강제 점프**한다. 편집 중 커서가 튀는 전형적인 UX 버그다.
`min`/`max` 속성만으로는 막히지 않는다(HTML min/max는 입력을 막지 않음).

### 3.3 `Use imported` 버튼이 scrollback을 되돌리지 않음

`onUseImported`는 `{ fontFamily: null, macosOptionAsAlt: null, fontSize: null }`만 초기화한다.
scrollback은 로컬 전용 값이라 의도된 것일 수 있으나, 버튼 라벨상 "임포트된 값 사용"인데
일부만 되돌아가는 것은 혼란스럽다. Appearance/Notifications/Browser는 "Reset to defaults"인데
Terminal만 "Use imported"인 것도 일관성이 없다.

### 3.4 Agents: Command/Args 입력이 `onBlur` + `defaultValue`

`defaultValue={override.command ?? ""}` + `onBlur` 커밋. 값을 지우고 blur하면 빈 문자열이
override로 저장된다(`{ command: "" }`). `mergeDetections`가 빈 문자열을 어떻게 취급하는지에
따라 에이전트 실행이 깨질 수 있다. 최소한 빈 값은 override 제거로 처리해야 한다.
또한 저장 피드백이 전혀 없어서 blur만으로 저장됐는지 사용자가 알 수 없다.

### 3.5 Notifications: 마스터 토글이 꺼져도 Sound/Volume 하위가 부분적으로만 비활성

`Enable Notifications`가 off일 때 `Agent Task Complete`, `Terminal Bell`, Sound Select,
Volume Slider는 disabled 되지만, **`Send Test Notification` 버튼은 여전히 활성**이다.
알림이 꺼진 상태에서 테스트를 보내는 건 모순이다. `Browse...` / `Preview` 버튼도 마찬가지.

### 3.6 Notifications: 커스텀 사운드 선택 시 파일 미선택 상태 처리

`customSoundId === "custom"`이지만 `customSoundPath`가 null이면 실제 알림에서 무음이 된다.
경고 표시가 없다. `Preview`만 disabled 되고 저장은 그대로 허용된다.

### 3.7 Remote Access: 페어링 코드가 자동 생성되고 화면에 상시 노출

`useEffect`에서 게이트웨이가 enabled면 **설정 화면 진입만으로 새 페어링 코드를 생성**한다
(`createPairingCode("control")`). 즉 Remote 탭을 열 때마다 새 control 권한 코드가 발급된다.
화면 공유/스크린샷 상황에서 PIN과 QR이 그대로 노출되고, 코드의 만료/무효화 정책이 UI에
드러나지 않는다. 최소한 "Generate"를 사용자 액션으로 두거나, 코드 표시를 클릭-투-리빌로
가리는 편이 안전하다.

### 3.8 Remote / Notifications / Agents: 실패가 조용히 삼켜짐

- `RemoteAccessSection.handleToggle`의 `catch {}` — 게이트웨이 활성화 실패 시 스위치가 원래
  상태로 돌아가기만 하고 사용자에게 이유가 표시되지 않는다.
- `handleRevoke`의 `catch {}` — 확인 UI만 열린 채 아무 일도 안 일어난다.
- `AgentsSection.runDetection`의 `catch {}` — 감지 실패와 "설치된 게 없음"이 구분되지 않는다.
- `NotificationsSection`의 실패는 전부 `console.error`로만 간다.

`GeneralSection.CliLauncherCard`는 `Alert`로 에러를 제대로 노출하고 있으니, 그 패턴을 다른
섹션에도 맞추면 된다.

### 3.9 Workspace 섹션이 프로젝트 식별자를 그대로 노출

`{project.workspaceId}`를 제목으로 렌더한다. 사이드바에서는 브랜치/워크트리 슬러그를 쓰기로
한 규칙과 어긋난다. 또 `Active` 상태일 때 Badge와 disabled `Active` 버튼이 **같은 정보를
두 번** 표시한다. 버튼은 제거해야 한다.

### 3.10 `SettingsDialog`의 접근성 계약이 어긋남

- `role="dialog"` + `aria-modal={false}` + `fixed inset-0`: 화면 전체를 덮으면서 modal이
  아니라고 선언한다. 실제로는 뒤 콘텐츠로 포커스가 빠져나갈 수 있다(포커스 트랩 없음).
- 열릴 때 초기 포커스 이동이 없다. 키보드 사용자는 Tab을 여러 번 눌러야 내비에 도달한다.
- 각 섹션이 `SettingsHeading`으로 시각적 제목(`div`, semantic heading 아님)을 그리고,
  별도로 `<h2 className="sr-only">`를 둔다. 같은 텍스트가 두 번(시각/스크린리더) 존재하는
  이중 구조로, `SettingsHeading`의 title을 `h2`로 만들면 sr-only는 전부 삭제 가능하다.
- Escape는 캡처 단계에서 무조건 `preventDefault` + close. 열린 Radix Select나 검색 입력
  안에서 Escape를 눌러도 팝업만 닫히는 게 아니라 **설정 화면 전체가 닫힌다.**

---

## 4. 디자인 일관성 문제

### 4.1 폰트 크기 스케일이 난립

한 화면에서 `text-[9px]`, `[10px]`, `[11px]`, `[12px]`, `text-xs`(12px), `[13px]`, `[15px]`가
섞여 쓰인다. `text-xs`와 `text-[12px]`가 같은 값인데 병행되고 있는 것도 문제다.
`SettingRow` 제목은 `text-xs`, 설명은 `text-[11px]`인 반면 `ShortcutsSection` 항목 제목은
`text-[12px]`, 그룹은 `text-[10px]`이다. 토큰 3단계(제목 13 / 본문 12 / 보조 11) 정도로
정리해야 한다. `9px`(브라우저 프로필 id, 활성 탭 URL)은 가독 한계 이하다.

### 4.2 섹션 레이아웃 패턴이 4가지로 갈린다

| 섹션 | 패턴 |
|---|---|
| General | 구분선 리스트 + `Card` 2개 |
| Appearance / Notifications / Browser | 서브헤딩 + `Reset to defaults` 우측 정렬 |
| Terminal | 자체 "Effective preferences" 헤더 + `Use imported` |
| Workspace / Remote | `Card` 기반 리스트 |
| Agents | 아코디언(ChevronDown) |
| Shortcuts | 검색 + 필터칩 |

각 섹션이 자기만의 헤더 구조를 새로 그린다. `SettingsHeading`/`SettingRow` 외에
`SettingsGroup(title, action)` 프리미티브 하나만 추가하면 Appearance/Notifications/Browser/
Terminal/Workspace의 헤더 중복이 모두 제거된다.

### 4.3 `Reset to defaults`가 있는 섹션과 없는 섹션

Appearance, Notifications, Browser에는 있고 General, Terminal(다른 이름), Agents, Workspace,
Remote에는 없다. 전역 리셋 하나 + 섹션별 리셋 규칙을 정해야 한다.

### 4.4 Button variant prop과 className이 서로 싸운다

거의 모든 버튼이 `variant="outline"` 등을 주면서 동시에 `className`으로 색/높이/패딩을
전부 덮어쓴다. 예: `SoftwareUpdateCard`의 Install 버튼은 `variant`를 조건부로 바꾸면서
동일한 조건으로 className도 다시 계산한다. variant 시스템이 사실상 무력화되어 있어
디자인 토큰 변경이 UI에 전파되지 않는다. `size="sm"`도 항상 `h-7`로 덮어쓴다 →
`size="xs"` 변형을 정의하는 편이 낫다.

### 4.5 값 정렬 규칙이 섞임

`SettingRow`의 우측 슬롯 폭이 제각각이다. Select는 `w-[180px]`, Home page는 `w-[330px]`,
Volume은 슬라이더+`w-8` 텍스트, Workspace의 Active worktree는 폭 제한 없는 mono 텍스트라
긴 경로가 들어오면 레이아웃이 밀린다(`truncate` 없음).

### 4.6 Remote QR 카드가 `SettingRow` 우측 슬롯 안에 들어가 있음

160×160 QR + PIN + 버튼이라는 큰 블록이 "라벨 | 컨트롤" 행 구조의 컨트롤 자리에 박혀 있어
행 높이가 튄다. 별도 카드 블록으로 승격해야 한다.

### 4.7 상태 색상 사용이 불균일

`text-status-success`, `bg-status-success/10`, `text-status-warning`, `text-destructive`가
섹션마다 다른 조합(테두리 유무, 배경 투명도)으로 쓰인다. Notifications의 permission Badge만
`border-*/20 bg-*/10 text-*` 3종 세트를 쓰고, Agents의 Detected Badge는 `border-transparent
bg-*/10`, CLI의 Installed Badge는 배경조차 없다.

### 4.8 좌측 내비에 검색이 없다

섹션 9개 + 각 섹션 내 설정 수십 개인데 설정 전역 검색이 없다. Shortcuts 섹션에만 검색이
있다. 이 규모면 상단에 설정 검색을 두는 게 표준(VS Code, Chrome, Warp 모두 그렇다).

### 4.9 `NavButton`에 `aria-current`가 없음

활성 상태가 배경색으로만 표현된다. `aria-current="page"` 또는 `aria-selected`가 필요하다.

---

## 5. 사소한 것들

- `SettingsDialog.tsx` 하단의 대량 re-export(`GeneralSection as GeneralSettings` 등)는 테스트
  호환용 별칭으로 보인다. 프로덕션 번들에 남길 이유가 없고, 이름이 3중(`BrowserSection`,
  `BrowserSettings`, `BrowserSettingsPanel`)인 것도 정리 대상.
- `TerminalSection`의 안내문 "Leave the local override reset to follow Ghostty."는 문장이
  어색하고, 실제로 어떻게 리셋하는지(=`Use imported`) 연결이 없다.
- `AgentsSection`의 Default Agent 설명이 5문장이다. `SettingRow` 설명치고 지나치게 길어
  행 높이가 크게 밀린다. 툴팁이나 접힘 처리 대상.
- `BrowserSection`의 프로필 이름 인풋이 `<input>` 원시 요소다(다른 곳은 `Input` 컴포넌트).
  포커스 링/디스에이블 스타일이 다르다.
- `GeneralSection`의 `p` 태그 안에 `CheckCircle2` 아이콘 + `span`을 넣고 `flex`를 조건부로
  주는 구조라, 조건이 false일 때는 `flex`가 없어 아이콘 정렬 규칙이 달라진다.
- `Show sidebar on startup`이 `useGeneralSettings`가 아니라 별도 `useState` +
  `saveSidebarOpenStartup`으로 관리되어, App 쪽 사이드바 상태와 이벤트 동기화가 없다.
  "Changes apply on next app start" 문구로 회피하고 있지만 실제로는 App이 같은 키를 쓰므로
  현재 세션에서 사이드바를 토글하면 이 설정값이 덮어써진다.

---

## 6. 우선순위 제안

**P0 (기능이 거짓말하고 있음)**
1. `useApplyAppearanceSettings()`를 앱 루트에서 호출 → 테마/액센트/밀도 즉시 반영 (2.2)
2. `settingsRuntimeBridge.ts`의 DOM 텍스트 패치·`change` 리스너·텍스트 매칭 클릭 핸들러 제거,
   초기 apply + system 테마 구독만 남김 (2.1)
3. Scrollback을 네이티브까지 연결하거나 컨트롤 숨김 (2.3)

**P1 (입력 신뢰성)**
4. Terminal font family를 blur/Enter 커밋으로 변경 (3.1)
5. 숫자 입력 빈 문자열 → NaN → 0 클램프 방지 (3.2)
6. Agents override 빈 문자열을 override 제거로 처리 (3.4)
7. Escape 키 캡처가 Select/검색을 삼키지 않도록 처리, 포커스 트랩·초기 포커스 추가 (3.10)

**P2 (안전/피드백)**
8. Remote 페어링 코드 자동 생성 제거 또는 리빌 방식으로 변경 (3.7)
9. 조용한 `catch {}`들을 `Alert` 기반 에러 표시로 (3.8)
10. 알림 마스터 off 시 Test/Browse/Preview도 비활성 (3.5)

**P3 (디자인 정리)**
11. 폰트 스케일 3단계로 정규화, `text-xs`/`text-[12px]` 혼용 제거 (4.1)
12. `SettingsGroup` 프리미티브 추가로 섹션 헤더 4패턴 통합 (4.2)
13. Button variant 오버라이드 정리, `size="xs"` 도입 (4.4)
14. 설정 전역 검색 추가 (4.8)
15. Interface density를 실제 spacing 토큰에 연결하거나 제거 (2.4)
