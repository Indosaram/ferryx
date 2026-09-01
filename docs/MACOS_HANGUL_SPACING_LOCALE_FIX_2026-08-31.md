# macOS 한글 입력 시 간격이 점점 벌어지는 현상 — 근본 원인과 수정 (2026-08-31)

## 증상
Release Ferryx.app(macOS, Dock 실행)에서 터미널에 한글을 입력하면 글자 간/단어 간 간격이
입력할수록 점점 크게 벌어진다. dev(`bun tauri dev`)에서는 재현되지 않는다.

## 근본 원인 (전 단계 실증 검증 완료)
렌더러/폰트 문제가 아니라 **데몬 PTY 환경에 locale 변수가 없어서 zsh가 C locale로 실행되는 것**.

1. Dock/Finder로 띄운 GUI는 launchd 환경을 상속하며 `LANG`/`LC_*`가 전혀 없음
   (`ps eww <gui-pid>`로 확인. 셸에서 띄운 프로세스는 `LANG=en_US.UTF-8` 보유).
2. 데몬(`ferryx --daemon`)은 GUI의 자식으로 이 환경을 그대로 상속
   (`ps eww <daemon-pid>` — 15개 변수 중 locale 제로).
3. PTY spawn(`src-tauri/src/terminal/pty.rs`)은 `TERM`/`COLORTERM`만 주입하고 locale은 없음.
   사용자 `~/.zshrc` 등에도 LANG 설정 없음 → zsh가 **C locale**로 기동.
4. C locale의 zsh ZLE는 한글 음절을 **바이트 수(3) 컬럼**으로 계산하고,
   libghostty 그리드는 올바르게 **2셀(wide)**로 진행 → 글자당 1셀씩 누적 어긋남.
   ZLE가 재출력할 때마다 오차만큼 패딩을 밀어 넣어 간격이 **점점 커짐**. bash도 동일.

### 실증 증거
- 데몬과 동일 env로 zsh PTY에 "한글 테스트" 타이핑: LANG 없음 → 에코 바이트 단위로 파손·폭
  계산 어긋남 / `LANG=ko_KR.UTF-8` → 완벽한 에코.
- 렌더러 배제: 그리드는 고정 셀 폭, wide 2셀 + Spacer 스킵, 글리프는 자연 advance로
  2셀 박스 중앙 배치라 여백 2~3px 수준 — "엄청 큰 간격" 불가능.
- dev에서 재현 안 되는 이유: dev 데몬은 개발 터미널(env에 LANG 있음)에서 띄워지므로.

## 수정 (src-tauri/src/terminal/pty.rs 단일 지점)
모든 PTY spawn이 지나는 공용 관문 `PtyManager::spawn_with_id_and_worktree`에서,
**상속받은 환경이 locale을 전혀 선택하지 않을 때만**(`LC_ALL`/`LC_CTYPE`/비어있지 않은 `LANG`
모두 부재) UTF-8 locale을 주입한다:

- `pub fn utf8_locale_override(get_env) -> Option<("LANG", locale)>` 순수 함수
  - macOS: `en_US.UTF-8` (항상 존재), 기타: `C.UTF-8`
  - 명시적 locale 설정은 절대 덮어쓰지 않는다 (빈 값은 unset 취급)
- 호출부는 `TERM=dumb` 보정 바로 뒤(같은 클래스의 GUI-launchd env 결핍 보정 위치).

의도적으로 프로세스 전역 `env::set_var`를 쓰지 않는다: 멀티스레드 런타임에서 data race 위험이
있고, CommandBuilder 단위 주입이 더 정확한 경계(PTY 자식에만 적용)이기 때문.

## 검증
- 단위 테스트 3종 신설: 미설정 시 주입 / 명시 설정 시 억제 / 빈 값은 unset 취급.
  `cargo test --lib terminal::pty` → 5 passed (신규 포함), `cargo check` 경고 0.
- 라이브 경계 증명(임시 테스트, 검증 후 제거): 러너 env에서 LANG을 완전 제거한 채
  (`env -i cargo test`) 실제 `spawn_shell`로 로그인 zsh를 띄워 `$LANG`을 프린트 →
  `FERRYX_LANG=[en_US.UTF-8]` 확인. 즉 launchd 상황이 재현된 상태에서 자식이 locale을 받음.
- 메커니즘 증명(파이썬 PTY 하네스): LANG 있음/없음 zsh 에코 대비는 상기 실증 증거와 동일.

## 사용자 확인 필요
재빌드된 release 앱을 Dock에서 실행해 한글 타이핑 간격이 정상화되는지 육안 확인 요청.
(데몬이 이미 실행 중이면 완전 종료 후 새 데몬이 뜨도록 해야 수정본이 적용된다.)
