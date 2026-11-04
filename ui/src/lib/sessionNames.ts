import { SESSION_NAME_ADJECTIVES, SESSION_NAME_NOUNS } from "./sessionNameWords";

/** FNV-1a 32-bit over UTF-8 bytes; must match `fnv1a` in src-tauri/src/session_names.rs. */
export function fnv1a(text: string): number {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(text)) {
    hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

/** The bare `<adjective>-<noun>` name the daemon derives for `sessionId`. */
export function baseSessionName(sessionId: string): string {
  const hash = fnv1a(sessionId);
  const adjectives = SESSION_NAME_ADJECTIVES.length;
  const adjective = SESSION_NAME_ADJECTIVES[hash % adjectives];
  const noun = SESSION_NAME_NOUNS[Math.floor(hash / adjectives) % SESSION_NAME_NOUNS.length];
  return `${adjective}-${noun}`;
}

/** A readable name for a new session, unique among `taken` (first free of base, base-2, base-3...). */
export function uniqueSessionName(sessionId: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  const base = baseSessionName(sessionId);
  if (!used.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!used.has(candidate)) return candidate;
  }
}
