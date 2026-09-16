# ERROR TOAST COPY ROOT CAUSE (2026-09-16)

증상: 에러 토스트 메시지가 간헐적으로 복사되지 않는다.

## 결론 (3중 원인)

복사가 실패하는 경로는 두 가지이고, 그중 Copy 버튼 경로가 "간헐적" 실패의 본체다.

### 1. Copy 버튼이 클립보드 거절을 조용히 삼킨다 (간헐적 실패의 직접 원인)

- 클릭 전달은 구조적으로 문제없음이 확인됐다: macOS 네이티브 터미널 뷰(`FerryxNativeTerminalView`)는 WKWebView 아래에 삽입되고 `hitTest:`가 항상 `nil`을 반환해(`src-tauri/src/native_terminal/platform/macos.rs:1-72`) 클릭이 항상 WKWebView/DOM에 도달한다. 토스트 클릭이 네이티브 뷰에 뺏기는 구조가 아니다.
- 핸들러 유래: 커밋 `6d8574aa` (2026-09-02, sonner 통합)에서 `navigator.clipboard.writeText` + 무음 catch로 처음 작성됐다.

- 유일하게 Copy 액션이 붙은 에러 토스트는 `runtimeError` 토스트다.
  - 위치: `ui/src/App.tsx:987-1009` (`useWorkspaceRuntime`의 `reportRuntimeError`가 트리거, `duration: Infinity`).
- 핸들러:
  ```ts
  // App.tsx:999-1007
  onClick: () => {
    void navigator.clipboard
      .writeText(clipboardText)
      .then(() => { toast.success("Copied error to clipboard"); })
      .catch(() => {});          // ← 실패를 완전히 삼킴
  },
  ```
- macOS WKWebView에서 `navigator.clipboard.writeText`는 문서 포커스/사용자 활성(user activation) 상태에 따라 `NotAllowedError`로 거절될 수 있다. 이 앱이 이미 이 제약을 문서로 남기고 있다:
  - `ui/src/components/NativeTerminalPane.tsx:1035-1037` — "On macOS, native cmd_native_terminal_copy_selection writes non-empty selection directly to NSPasteboard on the main thread, **bypassing WebKit user-activation restrictions**."
  - 즉 터미널 복사는 네이티브 NSPasteboard 우회로를 이미 쓰는데, 토스트 Copy 버튼은 이 우회가 없다.
- 클릭 시점의 WKWebView 포커스 상태는 직전 인터랙션(네이티브 터미널 서페이스, 브라우저 차일드 웹뷰, WebKit의 22-24ms 포커스 리셋 이력 등)에 따라 달라지므로 같은 클릭도 성공/거절이 갈린다 → "간헐적".
- 거절 시 `.catch(() => {})`라서 성공 토스트도, 실패 안내도 없다 → 사용자 관점 "아무 일도 안 일어남".

### 2. 토스트 텍스트는 애초에 선택 불가 (결정적)

- `ui/src/index.css:128-131`: `body { user-select: none; -webkit-user-select: none; }` — 선택은 `.selectable` 유틸리티(index.css:140-141) 옵트인 방식.
- Toaster 래퍼(`ui/src/components/ui/sonner.tsx:54`)와 `toastOptions.className("font-sans text-sm")` 어디에도 `.selectable` 없음 → 모든 토스트 텍스트는 드래그 선택이 불가능하다.
- 따라서 "드래그 선택 후 Cmd+C / 우클릭 Copy" 경로는 모든 토스트에서 항상 죽어 있다.

### 3. 선택 시도가 토스트를 스와이프 삭제시킨다 (sonner)

- sonner 2.0.8 (`ui/node_modules/sonner/dist/index.mjs`):
  - `onPointerMove`(794-795행)는 텍스트 선택이 존재하면(`isHighlighted`) 스와이프 추적을 중단하는 가드가 있다.
  - 그러나 원인 2 때문에 선택이 항상 비어 있어 가드가 발동하지 않는다.
  - 드래그 ≥ 45px(`SWIPE_THRESHOLD`) 또는 빠른 플릭(velocity > 0.11 px/ms)이면 `onPointerUp`에서 `deleteToast()` → 메시지를 복사하려던 드래그가 토스트를 날려버린다.

## 증상 재구성

- Copy 버튼 클릭: WKWebView 포커스/활성 상태가 온전하면 성공("Copied error to clipboard" 표시), 아니면 조용히 실패 → 간헐적 복사 실패로 체감.
- 드래그 선택: 항상 실패(원인 2) + 시도하면 토스트 소멸(원인 3).
- 그 외 `toast.error` 호출부(NativeTerminalPane, Sidebar, WorktreeList, PermissionsSection, App.tsx:694/708, updateToast 등)에는 Copy 수단 자체가 없다.

## 수정 방향 (제안, 미구현)

1. `RemoteAccessSection.tsx:71-90`의 `copyTextToClipboard`(Clipboard API → `execCommand("copy")` textarea 폴백)을 `ui/src/lib/` 공용 유틸로 추출해 토스트 Copy 액션에 사용. 실패 시 조용한 `.catch` 대신 실패 토스트 표시.
2. (macOS) 터미널 복사가 쓰는 네이티브 NSPasteboard 기록 경로를 일반 텍스트 복사 커맨드로 노출해 토스트 Copy에서도 우회 사용 — WebKit user-activation 제약의 검증된 해법.
3. Toaster 래퍼에 `.selectable` 적용 → 토스트 텍스트 선택 가능. sonner의 `isHighlighted` 가드가 선택 중 스와이프도 함께 막아준다.
4. (부수) `runtimeError` 토스트에 안정적 `id` 부여 — 반복 에러 시 중복 적층 대신 갱신.

## 확정 방법 (1줄 로그)

`App.tsx:1006`의 `.catch(() => {})`를 임시로 아래처럼 바꾸고 `bun tauri dev`에서 재현한다:

```ts
.catch((e: unknown) => console.error("toast copy rejected", (e as Error)?.name, (e as Error)?.message))
```

거절 시점에 `NotAllowedError`로 찍히면 WebKit 포커스/활성 게이트 확정이다. 수정안(execCommand 폴백 또는 네이티브 NSPasteboard 커맨드)은 어느 트리거든 검사 자체를 우회하므로 동일하게 해결된다.

## 검증 상태

- 위 파일/라인은 전부 본 세션에서 직접 읽어 확인한 코드 사실이다 (sonner 2.0.8 dist 포함).
- WKWebView 거절 시나리오는 저장소 내 주석(NativeTerminalPane.tsx:1035)과 알려진 WebKit 동작에 근거한 것이며, 실행 중인 앱에서 재현하지는 않았다. 라이브 확인이 필요하면 `bun tauri dev`에서 Copy 클릭 → 실패 시 콘솔에 reject 사유가 남도록 `.catch`에 로그를 임시 추가하는 방법을 권장한다.
