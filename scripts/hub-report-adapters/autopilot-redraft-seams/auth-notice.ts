// A source that needs the owner to sign in again is not a bug the cron's repair agent can fix.
// Such a source is skipped, the owner gets one Korean line per site at most once a day, and the
// run fails only for other errors.
import { existsSync, readFileSync, writeFileSync } from "node:fs";

export const NOTICE_INTERVAL_MS = 24 * 3600_000;

export class AuthRequiredError extends Error {}

const SITES: Record<string, { key: string; notice: string }> = {
  "kmong-messages": { key: "kmong", notice: "크몽 로그인이 만료되어 메시지·주문 수집을 건너뜁니다. Aside(u0)에서 크몽에 다시 로그인해 주세요." },
  "kmong-orders": { key: "kmong", notice: "크몽 로그인이 만료되어 메시지·주문 수집을 건너뜁니다. Aside(u0)에서 크몽에 다시 로그인해 주세요." },
  freemoa: { key: "freemoa", notice: "프리모아 로그인이 필요해 수집을 건너뜁니다. Aside(u0)에서 프리모아 Google 로그인(패스키 확인 포함)을 해 주세요." },
  fiverr: { key: "fiverr", notice: "Fiverr 로그인이 만료되어 안 읽은 메시지 확인을 건너뜁니다. Aside(u0)에서 Fiverr에 다시 로그인해 주세요." },
};

const AUTH_MESSAGE = /크몽 재로그인 필요|HTTP 401 \/api\/|FIVERR_LOGIN_REQUIRED|LOGIN_REQUIRED|PASSKEY_REQUIRED/;

export function isAuthRequired(step: string, error: unknown): boolean {
  if (!SITES[step]) return false;
  if (error instanceof AuthRequiredError) return true;
  return AUTH_MESSAGE.test(error instanceof Error ? error.message : String(error));
}

export type StepFailure = { step: string; error: unknown };
export type NoticeState = Record<string, string>;

export interface Outcome {
  notices: string[];
  errors: StepFailure[];
  state: NoticeState;
  exitCode: 0 | 1;
}

export function decide(failures: StepFailure[], state: NoticeState, now = Date.now()): Outcome {
  const next = { ...state };
  const notices: string[] = [];
  const errors: StepFailure[] = [];
  for (const f of failures) {
    if (!isAuthRequired(f.step, f.error)) { errors.push(f); continue; }
    const site = SITES[f.step]!;
    const last = Date.parse(next[site.key] ?? "");
    if (!Number.isNaN(last) && now - last < NOTICE_INTERVAL_MS) continue;
    next[site.key] = new Date(now).toISOString();
    notices.push(site.notice);
  }
  return { notices, errors, state: next, exitCode: errors.length ? 1 : 0 };
}

export function loadNoticeState(path: string): NoticeState {
  if (!existsSync(path)) return {};
  try { return JSON.parse(readFileSync(path, "utf8")) as NoticeState; } catch { return {}; }
}

export function saveNoticeState(path: string, state: NoticeState) {
  writeFileSync(path, JSON.stringify(state, null, 2));
}
