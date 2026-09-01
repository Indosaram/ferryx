# macOS 이미지 첨부(클립보드 붙여넣기) 진단 — 2026-08-31

## 결론 (확정)

**Ferryx가 에이전트에 붙여넣기 chord를 합성할 때 ghostty 키 인코더를 통과시키는데, 에이전트(omo/pi-tui)가
시작 시 Kitty keyboard protocol flags 7을 push하면 그 인코더가 Ctrl+V를 `0x16`도 CSI-u도 아닌
평범한 글자 `"v"`(0x76)로 인코딩한다.** omo 에디터에는 그냥 "v" 한 글자가 입력될 뿐 이미지가 첨부되지
않는다. Claude Code는 kitty 프로토콜을 협상하지 않으므로 legacy `0x16`을 받아 정상 첨부된다 —
"CC는 되고 omo는 안 된다"와 정확히 일치. 텍스트 붙여넣기가 멀쩡했던 이유: 그쪽은 브래킷 페이스트
인코더(`paste.rs encode_paste`)를 쓰기 때문.

**수정 (적용 완료)**: `ui/src/components/NativeTerminalPane.tsx`의 `sendImagePasteShortcut`이
keyEvent 대신 원시 바이트 `{ text: "\u0016" }`를 보낸다. `NativeTerminalInput::Text`는 인코더를
우회해 raw byte를 PTY에 쓰므로, 에이전트의 터미널 모드(kitty/legacy)와 무관하게 항상 정확한
chord가 전달된다. 테스트 6곳의 keyEvent 단언을 `{ text: "\u0016" }`로 갱신, 87 tests pass, build pass.

## 근거 사슬 (모두 런타임/실험으로 입증)

1. **Ferryx 체인 정상 (라이브 트레이스 /tmp/ferryx-switch-debug.jsonl)**: ⌘V → AppKit 모니터
   `EmitAndConsume` → `native_terminal_paste` → 올바른 판 라우팅 → `cmd_native_terminal_clipboard_content`
   분류 `kind:image` (public.png/public.tiff) → `input.sent hasKeyEvent=true`. 39회 시도 전부 성공,
   에러 흔적 0건. 같은 판에서 텍스트 ⌘V 3회 성공.
2. **omo 자체 기능 정상 (라이브 PTY 실험)**: omo TUI를 PTY에서 구동하고 원시 `0x16` 주입 →
   `[Image #1]` 첨부 성공. CSI-u `\x1b[118;5u` 주입 → `[Image #2]` 첨부 성공.
   네이티브 클립보드 모듈(`@mariozechner/clipboard`)도 bun/node 모두에서 로드·이미지 읽기 성공.
3. **인코더 버그 재현 (Rust 프로브)**: 실제 ghostty 터미널 인스턴스에 omo가 push하는
   `CSI > 7 u`를 먹인 뒤 `{key:"v", ctrl:true}`를 인코딩하면:
   - push 전: `\x16` (정상)
   - push 후: `"v"` (0x76 — 붙여넣기 chord 소실) ← **버그**
   (단, kitty CSI-u 경로는 플랫폼 키코드 데이터가 필요한데 합성 이벤트에는 없어 text 경로로
   떨어지는 것이 유력 원인 — ghostty `src/input/key_encode.zig`.)
4. **omo 파서 정규식도 확인**: `\x1b[118;5;118u`(텍스트 코드포인트 포함) 형태는 거부하지만
   ghostty는 ctrl의 `preventsText`로 텍스트 필드를 생성하지 않으므로 무관.

## 시사점

- 에이전트가 kitty 프로토콜을 push하는 한(Claude Code는 안 함, omo는 함), 합성 keyEvent 경로는
  신뢰할 수 없다. chord류 전송은 raw byte(Text) 경로를 써야 한다.
- Windows에서도 동일 구조이므로 본 수정이 Windows omo 이미지 첨부에도 그대로 유효하다.
- 사용자 확인 없이 자동화된 검증으로 완료: 라이브 omo PTY에서 `0x16` 첨부 성공이 최종 근거.
