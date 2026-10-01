import { screen } from "@testing-library/react";

export interface DomSignal<T> {
  readonly promise: Promise<T>;
}

export function deferred<T>(): { readonly promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolution) => {
    resolve = resolution;
  });
  return { promise, resolve };
}

const DEFAULT_SIGNAL_TIMEOUT_MS = 4000;

export function observeDom<T>(read: () => T | null, timeoutMs = DEFAULT_SIGNAL_TIMEOUT_MS): DomSignal<T> {
  let observer: MutationObserver | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const promise = new Promise<T>((resolve, reject) => {
    const cleanup = () => {
      if (observer) {
        observer.disconnect();
        observer = null;
      }
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
    };

    const check = () => {
      try {
        const value = read();
        if (value !== null && value !== undefined) {
          cleanup();
          resolve(value);
          return true;
        }
      } catch (err) {
        cleanup();
        reject(err);
        return true;
      }
      return false;
    };

    if (check()) return;

    timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out after ${timeoutMs}ms waiting for DOM condition`));
    }, timeoutMs);

    observer = new MutationObserver(() => {
      check();
    });
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      characterData: true,
    });
  });

  return { promise };
}

export async function settle<T>(signal: DomSignal<T> | Promise<T>): Promise<T> {
  const promise = signal instanceof Promise ? signal : signal.promise;
  const value = await promise;
  if (value === undefined) throw new Error("signal resolved without a value");
  return value;
}

export function settledQuery<T>(read: () => T | null, timeoutMs?: number): Promise<T> {
  return settle(observeDom(read, timeoutMs));
}

export function settledTestId(testId: string, timeoutMs?: number): Promise<HTMLElement> {
  return settledQuery(() => screen.queryByTestId(testId), timeoutMs);
}

export function settledAllTestId(testId: string, timeoutMs?: number): Promise<HTMLElement[]> {
  return settledQuery(() => {
    const nodes = screen.queryAllByTestId(testId);
    return nodes.length > 0 ? nodes : null;
  }, timeoutMs);
}

export function settledCount(testId: string, count: number, timeoutMs?: number): Promise<HTMLElement[]> {
  return settledQuery(() => {
    const nodes = screen.queryAllByTestId(testId);
    return nodes.length === count ? nodes : null;
  }, timeoutMs);
}

export function settledRole(
  role: Parameters<typeof screen.queryByRole>[0],
  options?: Parameters<typeof screen.queryByRole>[1],
  timeoutMs?: number,
): Promise<HTMLElement> {
  return settledQuery(() => screen.queryByRole(role, options), timeoutMs);
}

export interface UrlBearing {
  readonly url: string;
}

const requestPumps = new WeakMap<UrlBearing[], Array<() => void>>();

export function pumpRequests(requests: UrlBearing[]): void {
  const list = requestPumps.get(requests);
  if (!list) return;
  for (const check of [...list]) check();
}

export function observeRequests<T extends UrlBearing>(
  requests: T[],
  pathSuffix: string,
  count: number,
  timeoutMs = DEFAULT_SIGNAL_TIMEOUT_MS,
): Promise<T[]> {
  const read = () => {
    const matching = requests.filter((entry) => entry.url.endsWith(pathSuffix));
    return matching.length >= count ? matching : null;
  };
  const immediate = read();
  if (immediate) return Promise.resolve(immediate);

  return new Promise<T[]>((resolve, reject) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let checker: (() => void) | null = null;

    const cleanup = () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      if (checker) {
        const currentList = requestPumps.get(requests);
        if (currentList) {
          const idx = currentList.indexOf(checker);
          if (idx !== -1) currentList.splice(idx, 1);
        }
        checker = null;
      }
    };

    checker = () => {
      const value = read();
      if (value) {
        cleanup();
        resolve(value);
      }
    };

    timer = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out after ${timeoutMs}ms waiting for request matching ${pathSuffix} (#${count})`));
    }, timeoutMs);

    const list = requestPumps.get(requests) ?? [];
    requestPumps.set(requests, list);
    list.push(checker);
  });
}
