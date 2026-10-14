import type { FilePreviewResolved } from "./filePreviewCommands";

export interface PathExistenceCacheOptions {
  ttlMs?: number;
  now?: () => number;
  maxEntries?: number;
}

export interface PathExistenceCache {
  check(path: string, backendSessionId: string, workspaceId?: string | null): Promise<FilePreviewResolved | null>;
  clear(): void;
}

interface CacheEntry {
  value: FilePreviewResolved;
  expiresAt: number;
}

export function createPathExistenceCache(
  resolve: (path: string, backendSessionId: string, workspaceId: string | null) => Promise<FilePreviewResolved>,
  options?: PathExistenceCacheOptions,
): PathExistenceCache {
  const ttlMs = options?.ttlMs ?? 5000;
  const now = options?.now ?? Date.now;
  const maxEntries = options?.maxEntries ?? 256;

  const entries = new Map<string, CacheEntry>();
  const inFlight = new Map<string, Promise<FilePreviewResolved | null>>();

  function evictOldest(): void {
    while (entries.size > maxEntries && maxEntries >= 0) {
      const oldestKey = entries.keys().next().value;
      if (oldestKey !== undefined) {
        entries.delete(oldestKey);
      } else {
        break;
      }
    }
  }

  return {
    async check(path: string, backendSessionId: string, workspaceId: string | null = null): Promise<FilePreviewResolved | null> {
      const key = `${workspaceId ?? ""}\u0000${backendSessionId}\u0000${path}`;
      const currentTime = now();

      const cached = entries.get(key);
      if (cached !== undefined) {
        if (currentTime < cached.expiresAt) {
          return cached.value;
        }
        entries.delete(key);
      }

      const pending = inFlight.get(key);
      if (pending !== undefined) {
        return pending;
      }

      const promise = (async () => {
        try {
          const resolved = await resolve(path, backendSessionId, workspaceId);
          const expiresAt = now() + ttlMs;
          if (entries.has(key)) {
            entries.delete(key);
          }
          entries.set(key, { value: resolved, expiresAt });
          evictOldest();
          return resolved;
        } catch {
          return null;
        } finally {
          inFlight.delete(key);
        }
      })();

      inFlight.set(key, promise);
      return promise;
    },

    clear(): void {
      entries.clear();
      inFlight.clear();
    },
  };
}
