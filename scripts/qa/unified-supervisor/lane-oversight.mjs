// Task 7 QA: an isolated hub lane whose fixture worker reports through the real `ferryx-cli mcp`
// `lane_report` tool, twice with the same file bytes and then after changing the file.
// Progress time must move only on the change. The worker is the lane's own pane
// (FERRYX_SESSION_ID); a different session is refused. The quiet-clock ladder
// (10/20 reminders, one 40-minute alert) needs an injected clock and is proven by the
// `lanes::oversight::tests` unit tests, not here.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { HarnessError, POSIX } from './harness.mjs';

export const LANE_OVERSIGHT_REQUIRED_ASSERTIONS = Object.freeze([
  'first-report-with-a-file-is-new-progress',
  'restated-report-with-same-bytes-does-not-move-progress',
  'changed-file-moves-progress',
  'report-from-another-session-is-refused',
]);

const LANE = 'qa-oversight';
const PANE = 'qa-oversight-pane';

export async function runLaneOversight(ctx) {
  if (!POSIX) throw new HarnessError('SCENARIO_PLATFORM_UNSUPPORTED', 'lane-oversight runs a POSIX fixture worker directory');
  let Database;
  try { ({ Database } = await import('bun:sqlite')); } catch {
    throw new HarnessError('SQLITE_UNAVAILABLE', 'lane-oversight seeds the isolated hub database with bun:sqlite; run the runner with bun');
  }
  const root = join(ctx.paths.data, 'lane-oversight');
  const work = join(root, 'work');
  const db = join(root, 'hub', 'hub.sqlite');
  mkdirSync(work, { recursive: true });
  mkdirSync(join(root, 'hub'), { recursive: true });

  const mcp = async (label, session, frames) => {
    const input = frames.map((frame) => `${JSON.stringify(frame)}\n`).join('');
    const result = await ctx.cli(['mcp'], { label, input, deadlineMs: 60_000, env: ctx.productEnv({ FERRYX_HUB_DB: db, FERRYX_SESSION_ID: session }) });
    return result.stdout.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
  };
  const call = (id, name, args) => ({ jsonrpc: '2.0', id, method: 'tools/call', params: { name, arguments: args } });
  const report = (id, text) => call(id, 'lane_report', { key: LANE, kind: 'progress', text, files: ['out.txt'] });
  const parsed = (reply) => {
    if (reply?.error) return { error: reply.error.message };
    return JSON.parse(reply?.result?.content?.[0]?.text ?? 'null');
  };

  // First open migrates the isolated database; then the fixture lane is seeded as a working lane on PANE.
  await mcp('migrate', PANE, [call(1, 'lane.list', {})]);
  const seed = new Database(db);
  try {
    seed.query(`INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, omo_thread_id, status, done_signals, last_activity_at)
                VALUES(?, 'QA oversight', 'lead', ?, ?, ?, 'working', '[]', datetime('now'))`).run(LANE, work, PANE, PANE);
  } finally { seed.close(); }

  writeFileSync(join(work, 'out.txt'), 'first bytes\n');
  const [first] = (await mcp('report-1', PANE, [report(1, 'wrote out.txt')])).map(parsed);
  const [restated] = (await mcp('report-2', PANE, [report(2, 'made a lot of progress on out.txt')])).map(parsed);
  writeFileSync(join(work, 'out.txt'), 'second bytes\n');
  const [changed] = (await mcp('report-3', PANE, [report(3, 'wrote out.txt')])).map(parsed);
  const [stranger] = (await mcp('report-stranger', 'some-other-pane', [report(4, 'not my lane')])).map(parsed);

  const t1 = first?.report?.lastProgressAtMs;
  const t2 = restated?.report?.lastProgressAtMs;
  const t3 = changed?.report?.lastProgressAtMs;
  ctx.assert('first-report-with-a-file-is-new-progress', first?.report?.newItems?.length === 1 && Number.isFinite(t1), { first });
  ctx.assert('restated-report-with-same-bytes-does-not-move-progress', restated?.report?.newItems?.length === 0 && t2 === t1, { restated });
  ctx.assert('changed-file-moves-progress', changed?.report?.newItems?.length === 1 && t3 > t1, { changed });
  ctx.assert('report-from-another-session-is-refused', typeof stranger?.error === 'string' && stranger.error.startsWith('NOT_LANE_WORKER'), { stranger });
}
