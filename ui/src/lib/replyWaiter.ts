export interface ReplyWaiter {
  arm(baselineId: string): Promise<string>;
  observe(id: string, assistantText: string): void;
  disarm(): void;
}

export function createReplyWaiter(timeoutMs = 90_000): ReplyWaiter {
  let baseline = "";
  let settled = true;
  let resolve: ((text: string) => void) | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const finish = (text: string) => {
    if (settled) return;
    settled = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
    const done = resolve;
    resolve = null;
    done?.(text);
  };

  return {
    arm(baselineId: string) {
      baseline = baselineId;
      settled = false;
      if (timer !== null) clearTimeout(timer);
      return new Promise<string>((next) => {
        resolve = next;
        timer = setTimeout(() => finish(""), timeoutMs);
      });
    },
    observe(id: string, assistantText: string) {
      const text = assistantText.trim();
      if (settled || text === "" || id === baseline) return;
      finish(text);
    },
    disarm() {
      finish("");
    },
  };
}
