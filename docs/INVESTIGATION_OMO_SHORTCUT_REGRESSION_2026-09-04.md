# 오모(omo) 단축키 재발 원인 분석 보고서

**작성일**: 2026-09-04  
**대상**: Ferryx Native Terminal 키 입력 파이프라인 및 Ghostty 키 인코더

---

## 1. 요약 (Executive Summary)

최근 세션(`2026-09-03T07-05-34Z`)에서 오모(omo / senpi TUI) 환경의 `Ctrl+L` 등 제어 단축키가 작동하지 않던 문제를 조사하고 원인을 규명하여 정상 동작하도록 코드를 수정한 바 있습니다.

그러나 **이 수정사항이 git commit으로 영구 반영되지 않고 작업 디렉토리(Working Directory)에 uncommitted 상태로 남아있었습니다.**  
이후 2026-09-04 22:40:55 및 22:55:22 KST에 터미널 포커스 디버깅 과정 중 **`git reset: moving to HEAD`(`git reset --hard`)가 실행되면서 미커밋 상태였던 수정본이 모두 날아가(HEAD로 리셋) 이전의 버그 상태로 되돌아간 것**이 원인입니다.

직후 커밋된 `36a4d49`(`fix(terminal): restore keyboard focus when clicking terminal panes`)는 오직 페인 클릭 포커스 관련 변경점만 스테이징되어 커밋되었기 때문에, 단축키 수정 코드는 완전히 누락되었습니다.

---

## 2. 오모(omo)에서 단축키가 안 되었던 근본 기술 원인

오모(`omo` / `senpi`)는 TUI 프레임워크 초기화 시 터미널 에뮬레이터로 **Kitty 키보드 프로토콜 활성화 시퀀스(`\x1b[>1u`)**를 전송합니다. 이 모드가 활성화되면 터미널 에뮬레이터의 키 인코딩 규칙이 일반 셸(Legacy ASCII 제어문자)과 완전히 달라집니다.

### 원인 A: Ghostty C API의 `utf8` 페이로드 우회 버그 (Rust 백엔드)
- 레거시 모드에서는 `Ctrl+L`을 누르면 단일 바이트 `0x0C`(Form Feed)를 PTY에 전달합니다.
- Kitty 프로토콜 모드에서는 `Ctrl+L`을 확장 시퀀스인 `\x1b[108;5u`(CSI 108;5u)로 인코딩하여 전송해야 omo가 `Ctrl+L` 단축키로 인식합니다.
- 그런데 `src-tauri/src/native_terminal/key_encoder.rs`의 현재 코드:
  ```rust
  } else if let KeyCode::Character(c) = event.key {
      let mut char_buf = [0u8; 4];
      let encoded = c.encode_utf8(&mut char_buf);
      validate_utf8_for_key_event(encoded)?;
      ghostty_key_event_set_utf8(event_guard.0.as_ptr(), encoded.as_ptr(), encoded.len());
  }
  ```
- `Ctrl+L` 입력 시 `event.key`가 `'l'`이므로 `ghostty_key_event_set_utf8`에 문자열 `"l"`을 설정하고 있었습니다.
- Ghostty 내부 인코더(`key_encode.zig`)는 Kitty 모드라 하더라도 `event.utf8`이 non-empty이고 비제어문자이면, **Ctrl 수식키를 무시하고 일반 텍스트 바이트 `[108]`(`"l"`)을 PTY에 기록**합니다.
- 그 결과, omo 내부에서 `Ctrl+L`을 누르면 모델 선택 창이 열리는 대신 **알파벳 소문자 `'l'`이 프롬프트에 타이핑되거나 무시**되었습니다.
- 또한 Kitty 모드에서 비시프트 키 코드포인트를 알려주는 `ghostty_key_event_set_unshifted_codepoint(c as u32)` FFI 호출도 누락되어 있었습니다.

### 원인 B: 한글 입력기(IME) 상태에서의 물리 키 매핑 누락 (프론트엔드)
- macOS/Linux/Windows에서 한글 입력기 상태로 `Ctrl+L`을 누르면 브라우저 이벤트는 `event.key = "ㅣ"`, `event.code = "KeyL"`을 발생시킵니다.
- `ui/src/components/NativeTerminalPane.tsx`는 Alt 키 조합에만 `physicalKeyForAltChord`를 적용하고 있었으며, 일반 키 포워딩 시에는:
  ```ts
  key: physicalKeyForAltChord(forwardable)
  ```
  를 호출하여 `event.altKey`가 false이면 `event.key`인 `"ㅣ"`를 그대로 Rust 백엔드로 전송했습니다.
- Rust 백엔드의 `map_key_code_to_c`는 `'ㅣ'`를 인식하지 못해 `GHOSTTY_KEY_UNIDENTIFIED`로 처리되어 단축키 인코딩이 완전히 실패했습니다.

---

## 3. 이전 세션(`2026-09-03T07-05-34Z`)에서 검증 완료되었던 해결책

이전 세션에서는 아래 세 가지 핵심 수정으로 문제를 완전히 해결하고 단위/통합 테스트를 통과시켰습니다:

1. **`src-tauri/src/native_terminal/sys/ffi.rs`**:
   - `ghostty_key_event_set_unshifted_codepoint(event: GhosttyKeyEvent, codepoint: u32)` 바인딩 추가.
2. **`src-tauri/src/native_terminal/key_encoder.rs`**:
   - `KeyCode::Character(c)`일 때 `ghostty_key_event_set_unshifted_codepoint(event_guard.0.as_ptr(), c as u32)` 호출.
   - `event.modifiers.ctrl` 또는 `event.modifiers.super_key`가 활성화된 경우 `ghostty_key_event_set_utf8`에 일반 문자를 넘기지 않도록 가드 추가 (Ghostty가 Kitty 인코딩 경로를 정상적으로 타도록 보장).
3. **`ui/src/components/NativeTerminalPane.tsx`**:
   - `physicalKeyForModifierChord`를 구현하여 `ctrlKey`, `altKey`, `metaKey` 조합일 때 `event.code`(`Key[A-Z]`, `Digit[0-9]`)로부터 물리적 영문/숫자 키를 추출하도록 통일.

---

## 4. 재발 원인 상세 타임라인 (왜 다시 안 되기 시작했는가?)

1. **2026-09-03 ~17:30 KST**:
   - 단축키 수정 작업 완료 및 테스트 통과.
   - **그러나 git commit 명령어가 실행되지 않아 파일들이 워킹 트리에 uncommitted 상태로 남음.**
   - 사용자가 `bun tauri dev`를 실행했을 때는 로컬 디스크의 수정 파일들이 빌드되어 정상 작동을 확인.
2. **2026-09-04 20:21:45 KST (`7e08fe3`)**:
   - 사이드바 드래그 기능 커밋(`feat(sidebar): allow dragging workspace rows...`). 이때 단축키 수정 파일은 포함되지 않음.
3. **2026-09-04 22:40:55 및 22:55:22 KST**:
   - 터미널 포커스 문제 조사 과정 중 `git reflog` 기록:
     ```
     7e08fe3 HEAD@{2026-09-04 22:55:22 +0900}: reset: moving to HEAD
     7e08fe3 HEAD@{2026-09-04 22:40:55 +0900}: reset: moving to HEAD
     ```
   - **`git reset --hard HEAD`가 실행됨.**
   - 워킹 트리에 남아있던 `src-tauri/src/native_terminal/key_encoder.rs`, `src-tauri/src/native_terminal/sys/ffi.rs` 등의 수정 내역이 완전히 삭제되어 HEAD(`7e08fe3`) 상태로 복구됨.
4. **2026-09-04 23:36:59 KST (`36a4d49`)**:
   - 페인 클릭 포커스 복원 커밋(`fix(terminal): restore keyboard focus when clicking terminal panes`) 생성.
   - 이 커밋에는 포커스 수정사항만 포함되었고, 리셋으로 날아간 단축키 인코더 수정은 반영되지 않음.

---

## 5. 복구 계획 (Restoration Plan)

문제를 영구적으로 해결하기 위해 다음 복구 작업을 적용하고 커밋해야 합니다:

### 1) Rust FFI 바인딩 복원 (`src-tauri/src/native_terminal/sys/ffi.rs`)
```rust
pub fn ghostty_key_event_set_unshifted_codepoint(event: GhosttyKeyEvent, codepoint: u32);
```

### 2) Rust 키 인코더 복원 (`src-tauri/src/native_terminal/key_encoder.rs`)
- `ghostty_key_event_set_unshifted_codepoint` 호출 추가
- `ctrl` / `super_key` 활성화 시 `utf8` 설정 억제:
```rust
if let KeyCode::Character(c) = event.key {
    ghostty_key_event_set_unshifted_codepoint(event_guard.0.as_ptr(), c as u32);
}

if let Some(ref text) = event.utf8 {
    validate_utf8_for_key_event(text)?;
    ghostty_key_event_set_utf8(event_guard.0.as_ptr(), text.as_ptr(), text.len());
} else if let KeyCode::Character(c) = event.key {
    if !event.modifiers.ctrl && !event.modifiers.super_key {
        let mut char_buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut char_buf);
        validate_utf8_for_key_event(encoded)?;
        ghostty_key_event_set_utf8(event_guard.0.as_ptr(), encoded.as_ptr(), encoded.len());
    }
}
```

### 3) 프론트엔드 물리 키 변환 복원 (`ui/src/components/NativeTerminalPane.tsx`)
- `physicalKeyForModifierChord`를 적용하여 `ctrlKey` 활성화 시 한글 자모 대신 물리 키(`KeyL` -> `'l'`) 전달.

### 4) 계약 테스트 복원 및 영구 커밋
- `src-tauri/tests/native_terminal_engine_contract/key_encoding.rs`에 Kitty 프로토콜 활성화 모드에서의 `Ctrl+L`(`\x1b[108;5u`) 검증 테스트 추가.
- 변경사항을 명시적으로 git commit하여 향후 `git reset`에 소실되지 않도록 보존.

---

## 6. 복구 완료 및 검증 결과 (Resolution & Verification Evidence)

위 복구 계획에 따라 Rust 백엔드와 프론트엔드 양쪽 모두 수정을 재적용하고 검증을 마쳤습니다.

### 적용된 파일 변경 내역
1. **`src-tauri/src/native_terminal/sys/ffi.rs`**:
   - `ghostty_key_event_set_unshifted_codepoint(event: GhosttyKeyEvent, codepoint: u32)` FFI 선언 추가.
2. **`src-tauri/src/native_terminal/key_encoder.rs`**:
   - `KeyCode::Character(c)`에 대해 비시프트 코드포인트 세팅 추가.
   - `ctrl` / `super_key` 활성화 시 `utf8` 전달 억제 적용 (Ghostty 내부의 Kitty 인코딩 경로 보장).
3. **`src-tauri/tests/native_terminal_engine_contract/key_encoding.rs`**:
   - 레거시 모드 `Ctrl+L`(`\x0c`) 및 Kitty 프로토콜 활성화 모드 `Ctrl+L`(`\x1b[108;5u`) 검증 계약 테스트 추가.
4. **`ui/src/components/NativeTerminalPane.tsx`**:
   - `physicalKeyForModifierChord` 구현 및 `handleCaptureKeyDown`, `textarea.onKeyDown`의 키 포워딩에 적용 (한글 입력기 상태에서 `KeyL`이 `'l'`로 정상 변환).
5. **`ui/src/components/NativeTerminalPane.test.tsx`**:
   - 한글 입력기 상태 `Ctrl+L`(`key: "ㅣ"`, `code: "KeyL"`) 누름 시 `'l'`이 전송되는지 검증하는 단위 테스트 추가.

### 테스트 검증 결과
- **Rust Native Terminal 계약 테스트**:
  - `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_engine_contract`: **22 passed, 0 failed**.
  - `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract`: **14 passed, 0 failed**.
- **Frontend 단위 테스트**:
  - `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: **124 passed (124 tests), 0 failed**.
- **Frontend 타입체크 및 빌드**:
  - `bun run --cwd ui build`: 성공 (tsc + vite build 정상 완료).

