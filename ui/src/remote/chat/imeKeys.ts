import type React from "react";

/**
 * Enter that confirms an IME candidate is not a submit. Safari fires that keydown after
 * compositionend with isComposing=false and keyCode 229, so every signal is checked.
 */
export function isImeConfirmingKey(e: React.KeyboardEvent, composing: boolean): boolean {
  return (
    composing ||
    Boolean(e.nativeEvent?.isComposing) ||
    Boolean((e as unknown as { isComposing?: boolean }).isComposing) ||
    e.keyCode === 229 ||
    (e.nativeEvent as KeyboardEvent | undefined)?.keyCode === 229
  );
}
