// F1 agent link: the pure pieces shared by f1-acceptance.mjs (runner side) and f1-host-agent.mjs
// (target side), kept free of side effects so self-test-units.mjs can drive them with simulated
// drops and a fake clock.
//
// - readAppends: pane-log delivery by byte offset. Only complete lines are returned, the offset
//   advances past the last newline only, so a line is never lost, split or delivered twice
//   whether the caller runs on a watch event, a poll tick, or both.
// - AgentChannel: one logical RPC channel to a host agent over a sequence of links (ssh
//   sessions). Every agent output line carries `oseq`; a reconnecting link asks the agent to
//   replay everything after the last oseq it delivered, lines at or below it are dropped, and
//   requests still unanswered are re-sent (the agent ignores a request id it already received).
//   Drops, attempts, reconnects and a lost channel are recorded with timestamps; once the
//   reconnect budget is spent the channel is lost and every pending and later request fails.

export const RECONNECT_DELAYS_MS = Object.freeze([1_000, 2_000, 5_000, 10_000, 20_000, 30_000]);
export const RECONNECT_BUDGET_MS = 10 * 60_000;

export const backoffDelay = (attempt, delays = RECONNECT_DELAYS_MS) => delays[Math.min(Math.max(attempt, 0), delays.length - 1)];

// `bytes` is the whole file (Buffer/Uint8Array), `offset` the byte offset already delivered.
export function readAppends(bytes, offset) {
  const buffer = Buffer.from(bytes.buffer ?? bytes, bytes.byteOffset ?? 0, bytes.byteLength ?? bytes.length);
  if (buffer.length < offset) return { lines: [], end: offset, truncated: true };
  const last = buffer.lastIndexOf(0x0a);
  if (last + 1 <= offset) return { lines: [], end: offset, truncated: false };
  const lines = [];
  let start = offset;
  for (let index = buffer.indexOf(0x0a, start); index >= 0 && index <= last; index = buffer.indexOf(0x0a, start)) {
    const text = buffer.subarray(start, index).toString('utf8');
    start = index + 1;
    if (text !== '') lines.push({ text, end: start });
  }
  return { lines, end: last + 1, truncated: false };
}

export class AgentChannel {
  // connect({ after, attempt, onLine }) -> Promise<{ link: { send(line), closed: Promise, close() }, attached }>
  //   attempt 0 is the first link; onLine(json, raw) must be called for every agent stdout line.
  // deliver(json, raw): each new line once, in oseq order (lines without oseq pass through).
  // record(entry): evidence of every drop / attempt / reconnect / loss.
  // sleep(ms): the backoff wait (real timers in a run, a fake clock in the unit checks).
  constructor({ id, connect, deliver, record = () => {}, now = Date.now, sleep, budgetMs = RECONNECT_BUDGET_MS, delaysMs = RECONNECT_DELAYS_MS, reconnect = true }) {
    Object.assign(this, { id, connectLink: connect, deliver, record, now, sleep, budgetMs, delaysMs, reconnect });
    this.lastOseq = 0;
    this.pending = new Map();
    this.links = 0;
    this.link = null;
    this.state = 'idle';
    this.closing = false;
    this.events = [];
    this.lostInfo = null;
    this.lostWaiters = new Set();
  }

  #note(entry) {
    const full = { channel: this.id, atMs: this.now(), at: new Date(this.now()).toISOString(), ...entry };
    this.events.push(full);
    this.record(full);
    return full;
  }

  onLine(json, raw) {
    if (json && Number.isSafeInteger(json.oseq)) {
      if (json.oseq <= this.lastOseq) return;
      this.lastOseq = json.oseq;
    }
    if (json && json.id !== undefined && json.id !== null && 'ok' in json) this.pending.delete(json.id);
    this.deliver(json, raw);
  }

  async #attach(attempt) {
    const index = this.links;
    this.links += 1;
    const { link, attached } = await this.connectLink({ after: this.lastOseq, attempt: index, onLine: (json, raw) => this.onLine(json, raw) });
    this.link = link;
    this.state = 'up';
    if (index > 0) this.#note({ kind: 'reconnected', attempt, link: index, after: this.lastOseq, attached: attached ?? null, resent: [...this.pending.keys()] });
    for (const line of this.pending.values()) link.send(line);
    link.closed.then((info) => this.#dropped(link, info));
    return attached;
  }

  open() { return this.#attach(0); }

  async #dropped(link, info) {
    if (link !== this.link) return;
    this.link = null;
    if (this.closing) { this.state = 'closed'; return; }
    const droppedAtMs = this.now();
    this.#note({ kind: 'dropped', info: info ?? null, lastOseq: this.lastOseq, pending: [...this.pending.keys()] });
    if (!this.reconnect) { this.#lose(droppedAtMs, 0, 'reconnect disabled for this agent'); return; }
    this.state = 'reconnecting';
    for (let attempt = 1; ; attempt += 1) {
      const delay = backoffDelay(attempt - 1, this.delaysMs);
      if (this.now() + delay - droppedAtMs > this.budgetMs) { this.#lose(droppedAtMs, attempt - 1, 'reconnect budget spent'); return; }
      await this.sleep(delay);
      if (this.closing) { this.state = 'closed'; return; }
      this.#note({ kind: 'reconnect-attempt', attempt, afterMs: this.now() - droppedAtMs, after: this.lastOseq });
      try { await this.#attach(attempt); return; } catch (error) {
        this.#note({ kind: 'reconnect-failed', attempt, error: String(error?.message ?? error) });
      }
    }
  }

  #lose(droppedAtMs, attempts, reason) {
    this.state = 'lost';
    this.lostInfo = this.#note({ kind: 'lost', reason, attempts, sinceDropMs: this.now() - droppedAtMs, budgetMs: this.budgetMs, unanswered: [...this.pending.keys()] });
    for (const waiter of this.lostWaiters) waiter(this.lostInfo);
    this.lostWaiters.clear();
  }

  // Resolves when the channel is lost (never, if it is not).
  whenLost() {
    if (this.lostInfo) return Promise.resolve(this.lostInfo);
    return new Promise((resolveLost) => this.lostWaiters.add(resolveLost));
  }

  send(id, line) {
    if (this.state === 'lost') return false;
    this.pending.set(id, line);
    this.link?.send(line);
    return true;
  }

  close() {
    this.closing = true;
    try { this.link?.close(); } catch { /* gone */ }
  }
}
