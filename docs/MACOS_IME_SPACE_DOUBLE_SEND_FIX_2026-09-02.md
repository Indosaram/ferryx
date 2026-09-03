# macOS 한글 입력 스페이스가 두 배 폭으로 렌더링되는 현상 — 근본 원인과 수정 (2026-09-02)

## 증상
네이티브 터미널에서 한글 입력 자체는 정상인데, **스페이스를 누를 때만 단어 간격이 두 배 폭**으로
렌더링됨. 에이전트 응답(출력)의 스페이스는 정상 폭.

## 근본 원인 — 스페이스 하나가 PTY로 두 번 전송 (트레이스로 실증)
`/tmp/ferryx-switch-debug.jsonl`의 실제 재현 트레이스(시퀀스 366–402):

1. macOS 한국어 2볼식 IME에서 스페이스가 컴포지션을 종결시키면, WebKit의
   `compositionend` 데이터에 **종결 스페이스가 포함**된 채 전달된다
   (예: ㅇㅏㄴ+space → data="안 ", textLength=2 — 음절 1개+스페이스 1개인데 길이 2로 실증).
   `onCompositionEnd`가 이 문자열 전체를 `sendInput` → **첫 번째 스페이스**.
2. 같은 물리 키에 대해 WebKit이 `compositionend` **7ms 뒤** `isComposing=false`인 keydown(" ")을
   재전달하고, sink textarea의 onKeyDown 평문 문자 분기가 이를 또 전송 → **두 번째 스페이스**.
   (트레이스상 사람이 두 번 누르는 것은 불가능한 2–9ms 간격으로 sent 이벤트 2회.)
3. 빈 preedit에서의 스페이스도 동일: composition " " 커밋 1회 + keydown " " 1회 = 이중 전송.
4. ASCII 타이핑(예: ".")은 컴포지션이 없어 단일 전송 — 한국어 IME 종결 키에서만 발생.
   → 출력 경로와 무관하므로 에이전트 응답은 항상 정상.

참고: 2026-08-31에 수정한 데몬 PTY locale 부재(`docs/MACOS_HANGUL_SPACING_LOCALE_FIX_2026-08-31.md`)
는 별개의 실재 결함이지만 이 증상의 원인은 아니었다. 해당 수정은 유지한다.

## 수정 (ui/src/components/NativeTerminalPane.tsx)
결정론적 중복 억제 — 타이밍 기반 로직 없음:

- `compositionTailCharRef`: 최근 컴포지션 커밋 텍스트의 마지막 문자를 기록
  (`onCompositionEnd`에서 전송 직후 설정).
- sink의 onKeyDown에서 `isComposing=false`인 keydown이 이 문자와 동일하면 **정확히 1회** 삼킴
  (preventDefault + 전송 안 함). 다른 키가 오면 플래그 해제.
- `isComposing=true`인 keydown(새 컴포지션의 첫 자모)은 절대 삼키지 않음 — 야박 커밋
  (꼬리=자모) 직후 같은 자모로 새 컴포지션을 시작하는 경우 보호. 트레이스 근거: WebKit
  재전달 keydown은 항상 isComposing=false(시퀀스 372), 컴포지션 중 자모는 true(367–369).
- `onBlur`/`onPointerDown`에서 플래그 해제.
- 진단용: `terminal.surface.input.sent` 트레이스에 `textCodePoints`(전송 텍스트의 코드포인트 hex)
  추가 — 향후 재현 시 페이로드 수준 검증 가능.

## 검증
- `NativeTerminalPane.test.tsx` 5개 신규 테스트 포함 **108/108 통과**
  (종결 keydown 삼킴 / 이후 독립 스페이스는 정상 전송 / 다른 키로 억제 해제 /
  컴포지션 없는 스페이스 정상 전송 / 자모 꼬리 커밋 후 새 컴포지션 첫 자모 미삼킴).
- `tsc --noEmit` clean, `bun run build` 성공.

## 사용자 확인 절차
1. 앱 재빌드 후 실행 (데몬 완전 종료 후 재기동).
2. 한글+스페이스 타이핑 재현 → 단어 간격이 1칸인지 육안 확인.
3. 남아있는 경우: `tail -c 200000 /tmp/ferryx-switch-debug.jsonl | grep input.sent`에서
   `textCodePoints` 확인 — 정상이면 "안 " 커밋 뒤 별도 `20`(space) 전송이 없어야 함.
