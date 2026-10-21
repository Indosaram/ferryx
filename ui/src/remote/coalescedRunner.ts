/**
 * Runs `task` for a stream of triggers with at most one run in flight. Triggers that land
 * while a run is delayed or in flight collapse into exactly one trailing run, so a burst of
 * N triggers costs at most two runs instead of N.
 */
export function createCoalescedRunner(
  task: () => Promise<unknown>,
  delayMs: number,
  onSettled?: () => void,
): { trigger: () => void; cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let inFlight = false;
  let queued = false;
  let cancelled = false;

  const trigger = () => {
    if (cancelled) return;
    if (inFlight || timer !== null) {
      queued = true;
      return;
    }
    timer = setTimeout(() => {
      timer = null;
      queued = false;
      inFlight = true;
      void task()
        .catch(() => undefined)
        .finally(() => {
          inFlight = false;
          if (cancelled) return;
          onSettled?.();
          if (queued) trigger();
        });
    }, delayMs);
  };

  const cancel = () => {
    cancelled = true;
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  return { trigger, cancel };
}
