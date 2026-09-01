# Ghostty 색상 테마 import 감사 (2026-08-31)

## 결론 요약

1. **import는 정상 동작** — 라이브 프로브로 확인. `sourcePath="ghostty"`(`+show-config` 경로), `status=Imported`.
2. **현재 테마값은 Ghostty와 바이트 단위로 동일** — 사용자 ghostty config에 색상 설정이 하나도 없어
   양쪽 다 Ghostty 기본 One Dark 테마. 차이가 "보인다"면 import가 아니라 렌더링/경로 문제 (아래 3장).
3. **구조적 결함 존재** — import된 팔레트가 libghostty 에뮬레이터에 주입되지 않음.
   지금은 우연히 세 레이어 기본값이 일치해 티가 안 나지만, theme/palette를 설정하는 순간 색상이 갈라진다.

## 1. import 상태 (라이브 프로브, `load_terminal_preferences()`)

| 항목 | 값 |
|---|---|
| source_path | `ghostty` (CLI `+show-config` 성공 — cmux 번들 1.3.2-HEAD, PATH 경유) |
| status | `Imported` |
| font_family | `MesloLGS NF, Noto Sans KR, monospace` |
| font_size | 13.0 |
| macos_option_as_alt | true |
| cursor_style | block |
| background / foreground | `#282c34` / `#ffffff` |
| 16색 팔레트 | One Dark (`#1d1f21 #cc6666 #b5bd68 #f0c674 #81a2be #b294bb #8abeb7 #c5c8c6 #666666 #d54e53 #b9ca4a #e7c547 #7aa6da #c397d8 #70c0b1 #eaeaea`) |

대조 기준 (같은 날 측정):
- `~/Library/Application Support/com.mitchellh.ghostty/config`: font-family, keybind, macos-option-as-alt 등만 있고 **색상 키 전무** (theme/background/foreground/palette/cursor-color/selection-* 없음).
- `Ghostty.app 1.3.1 +show-config --default`: bg `#282c34`, fg `#ffffff`, 동일 One Dark 16색.
- `ghostty(cmux) 1.3.2-HEAD +show-config --default`: 동일.
- 벤더 libghostty-vt (tip `6a508fd5`) 소스: `Config.zig` bg `0x28,0x2C,0x34`, `terminal/color.zig` Name.default = 동일 One Dark.

프레젠테이션 경로도 왜곡 없음: `preferred_terminal_surface_format`이 non-sRGB `Bgra8Unorm` 우선
(sRGB 부착 시 bg가 밝게 뜨는 감차 리프트 방지 — gpu_context.rs 주석 참조), alpha는 `Opaque` 고정.

## 2. 구조적 결함: import 팔레트가 에뮬레이터에 주입되지 않음 (split-brain)

색상 흐름이 둘로 갈라진다:

- **렌더러 기본색**(기본 속성 셀의 bg/fg, 커서, 클리어 컬러): `surface_host.rs`가
  `RendererTheme::from(cached_terminal_preferences())`로 **import 반영**.
- **컬러 출력**(SGR 30-37/90-97, 256색 셀): libghostty-vt가 **자체 내부 config로 resolve**.
  `NativeTerminal::set_default_foreground/background/cursor_color/set_palette`이 존재하지만
  프로덕션 호출처 0곳 (`tests/native_terminal_capability_contract.rs`에서만 호출).

오늘 기준 libghostty 내부 기본 = Ghostty 앱 기본 = import 기본 (전부 One Dark)이라 겉으로 일치.
그러나 다음 순간 갈라진다:
- ghostty config에 `theme = <name>` 또는 palette/색상 설정 시 → Ferryx는 배경/전경만 바뀌고
  ANSI 색상은 계속 One Dark. (theme 파일 resolve는 `resolve_theme()`이 해주지만 어디까지나
  렌더러 기본색에만 반영)
- Ghostty 업그레이드가 기본 팔레트를 바꾸는 순간 → 자동 불일치.
- 2026-08-26 메모의 "기본값 일치는 우연" 경고가 여전히 유효.

기타 관측:
- `TerminalThemeColors.extended_ansi`는 소비자 없음 (컬러 셀은 libghostty가 이미 resolve해서 전달).
- UI `ferryx.terminal.background` 저장 키는 읽는 곳이 없는 죽은 API.
- 원격 웹 클라이언트(`getTerminalPreferences` non-Tauri 폴백)는 자체 하드코딩 팔레트(#0a0a0a,
  Tailwind 계열 16색) — 데스크톱 import와 무관하게 완전히 다른 테마.

## 3. "다르게 보이는" 실제 후보 (테마값이 아닌 것들)

1. **선택 영역 색**: Ferryx 고정 `#52525299`(gray 60% alpha) vs Ghostty 자체 기본 selection 처리.
   드래그하면 확실히 다름.
2. **폰트 렌더링 밀도**: Ferryx CoreText 스택(명시적 wght=400) vs Ghostty 자체 렌더링 —
   글자 잉크 밀도 차이가 "색이 옅다"로 지각될 수 있음.
3. **import 캐시**: `PREFERENCE_CACHE`는 프로세스당 1회. ghostty config 수정은 앱 재시작 또는
   설정 대화상자 재import(`cmd_terminal_preferences`) 전까지 반영 안 됨.

## 권고 후속 조치 (미착수 — 별도 작업 필요)

- `NativeTerminal` 생성 시 import 테마를 libghostty에 주입:
  bg/fg/cursor + 256 전체 팔레트 배열 구성 후 `set_palette` 호출.
  (현재 `extended_ansi`는 config에 있는 인덱스만 담으므로 256 전체 배열로 확장 필요 —
  미설정 인덱스는 libghostty 기본값 또는 xterm-256으로 채움)
- 테마/팔레트 오버라이드가 생기면 위 주입과 함께 재적재.

## 4. 해결: "완전 블랙 ghostty"의 정체 (같은 날 추적)

사용자 관찰: Ferryx = 차콜, "ghostty" = 완전 블랙. 원인 확정:

- **Ghostty.app는 실행 중이 아님** (`ps` — 터미널 호스트는 cmux뿐).
  사용자가 비교하는 블랙 터미널은 **cmux에 임베디드된 ghostty 터미널**.
- cmux는 ghostty fork의 기본값(#282c34)과 무관하게 **앱 자체 TerminalTheme를 런타임에
  surface별로 주입**한다. 증거: cmux 바이너리 심볼 `TerminalTheme`, `terminalThemesBySurfaceID`,
  `cachedTerminalTheme`, `hasLoadedTerminalTheme`, `terminalThemeRevision`,
  `_sidebarMatchTerminalBackground`.
- cmux 설정 파일(`~/.config/cmux/settings.json`, `cmux.json`) 활성 값에는 theme 키가 없음
  (schema 주석에 `matchTerminalBackground`, `tintColor` 문서만 존재) → 블랙은 **cmux 컴파일 타임
  기본 테마**로, 어떤 config 파일에도 존재하지 않음.
- 결론: Ferryx import는 설정을 정확히 반영하며, 블랙은 "설정"이 아니라 cmux 앱 테마라서
  어떤 importer로도 얻을 수 없는 색이다.

### 사용자 선택지
- A. Ferryx를 블랙에 맞춤: ghostty config에 `background = #000000` 추가 → Ferryx 배경 반영.
  단, 팔레트 미주입 갭 때문에 ANSI 16색은 One Dark 유지(split-brain) — 위 주입 수정이 선행/병행 필요.
- B. Ghostty 공식 기본(차콜)이 기준: 현상 유지, cmux 테마가 원인임을 인지.
- C. cmux 쪽 테마 설정으로 cmux 터미널을 차콜에 맞춤.

## 5. 팔레트 주입 구현 완료 (2026-08-31, 사용자 결정: "ghostty 설정 100% 반영 렌더링")

사용자는 블랙 매칭이 아니라 **ghostty 설정 resolve 결과의 100% 렌더링 동등성**을 요구.
섹션 2의 split-brain 갭을 수정함 (구현: Gemini 3.7 Flash 위임, 검증 완료):

- `TerminalThemeColors.palette_overrides: Vec<(u8, String)>` 추가 (skip-serialized, wire 불변).
  `imported()`가 config.palette(0-255 전체)를 정렬해 채움 — theme 병합 후이므로
  config 명시값 > theme > 기본 우선순위 유지.
- `NativeTerminal::apply_theme_preferences(&TerminalThemeColors)`:
  fg/bg/cursor set + `full_palette()` 256 배열 set. 파싱 실패 엔트리는 스킵(기본값 유지).
- `full_palette()`: 0-15 = 테마 16색(파싱 실패 시 One Dark 폴백), 16-231 = 큐브 `ch*40+55`,
  232-255 = 그레이 `(i-232)*10+8`, 마지막에 overrides 덮어쓰기 — vendor `terminal/color.zig`와 동일.
- 주입 지점: `prepare_session_layout` 신규 터미널 + attach 경로 (둘 다 best-effort,
  실패 시 tracing::warn 후 세션 생성 계속). `input.rs` 인코딩용 터미널은 제외.
- 재적용: `rerender_native_sessions` 선행 `state.reapply_theme_to_sessions()` —
  설정 재import(cmd_terminal_preferences)와 오버라이드 적용 시 라이브 세션 전부 반영.
- 테마 파일 탐색에 PATH 상 ghostty 바이너리 기준 상대 경로 추가
  (`<bin>/../ghostty/themes`, `<bin>/../share/ghostty/themes`) — cmux 번들 테마도 resolve.
- 주입이 히스토리 리플레이보다 먼저라 앱의 OSC 4 동적 팔레트가 config보다 우선 — Ghostty와 동일.

검증: `cargo check` 경고 증분 0, `--lib` 470 passed, `native_terminal_capability_contract` 6 passed,
`native_terminal_surface_host_contract` 17 passed. FFI 왕복 테스트로 주입값 확인
(`terminal.default_foreground()/palette()` getter로 set→get 검증).

알려진 잔여 편차(문서화):
- selection-background 미설정 시 기본 선택색: ghostty는 셰이더 내부 계산, Ferryx 고정 #52525299.
  설정에 명시하면 양쪽 동일.
- cursor-text(cursor_accent)는 libghostty 옵션이 없어 미주입 — ghostty 기본(bg)과 동일해 무편차.
- **선존재 플래키(무관)**: `terminal::pty::tests::close_escalates_term_...`가 병렬 전체 실행 시
  간헐 실패(단독 3/3 통과) — pty.rs는 이번 태스크 미변경 파일, 부하 의존 reap 레이스. 별도 수정 필요.
