import { expect, test } from "bun:test";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";

// Builds src/ from the added side of the staged patch, so the code under test is exactly what ships.
// Only platform/model/browser modules are replaced by recording seams; state.ts is the unmodified source copy.
const dir = import.meta.dir;
const RESTRICTED = "모집 중\n외주\n쇼핑몰 구축\n예상 금액\n25,000,000원\n\nPRIME·PRO·BOOST 파트너에게\n공개되는 프라이빗 매칭 프로젝트입니다.\nBOOST 파트너 신청하기\n";
const ELIGIBLE = "모집 중\n외주\n재고 관리 웹 도구 구축\n예상 금액\n5,000,000원\n공개 프로젝트입니다.\n";
const OLD_REF = "D1:100.1";
const ITEMS = [
  { id: "wishket_project:900:1", kind: "wishket_project", external_id: "900", url: "https://www.wishket.com/project/900/", title: "restricted", body: RESTRICTED, chat_ref: OLD_REF },
  { id: "wishket_project:901:1", kind: "wishket_project", external_id: "901", url: "https://www.wishket.com/project/901/", title: "eligible", body: ELIGIBLE, chat_ref: "D1:101.1" },
];
const VERDICT = JSON.stringify({ action: "apply", fit: 90, summary: "재고 도구", risk: "없음", price: "5000000", period: "30", proposal: "구현 계획" });
const BID_ITEM = { id: "freelancer_project:42:1", kind: "freelancer_project", external_id: "42",
  url: "https://example.test/42", title: "Inventory export", body: "Public listing",
  chat_ref: OLD_REF, draft: JSON.stringify({ amount: "300", days: "7", body: "Proposal" }) };

async function stage() {
  const root = await mkdtemp(join(tmpdir(), "autopilot-redraft-"));
  const src = join(root, "src");
  await mkdir(src);
  const patch = await readFile(join(dir, "autopilot.patch"), "utf8");
  for (const section of patch.split(/^diff --git /m).slice(1)) {
    const name = section.match(/^a\/src\/(\S+)/)![1]!;
    const added = section.split("\n").filter((l) => l.startsWith("+") && !l.startsWith("+++")).map((l) => l.slice(1));
    await writeFile(join(src, name), `${added.join("\n")}\n`);
  }
  await cp(join(dir, "autopilot-redraft-seams"), src, { recursive: true });
  await cp(join(dir, "adapter-fixture", "state.ts"), join(src, "state.ts"));
  await writeFile(join(root, "token"), "isolated-test-token");
  return { root, src };
}

async function run(root: string, args: string[], extra: Record<string, string> = {}) {
  const traceFile = join(root, "trace.jsonl");
  await writeFile(traceFile, "");
  const child = Bun.spawn([process.execPath, "--preload", join(dir, "autopilot-redraft-preload.ts"), ...args], {
    env: { ...process.env, AUTOPILOT_HOME: join(root, "home"), FERRYX_HUB_CONTROL_SOCKET: join(root, "absent.sock"),
      AUTOPILOT_SLACK_TOKEN_FILE: join(root, "token"), AUTOPILOT_DRY_RUN: "0", REDRAFT_TRACE: traceFile, ...extra },
    stdout: "pipe", stderr: "pipe",
  });
  const [code, out, err] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
  const calls = (await readFile(traceFile, "utf8")).trim().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  return { code, out, err, calls };
}

const snapshot = async (root: string, src: string) => {
  const r = await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", "[]"]);
  expect(r.code, r.err).toBe(0);
  return JSON.parse(r.out);
};

test("processing and restart restoration edit the card without a new report", async () => {
  const { root, src } = await stage();
  try {
    for (const mode of ["processing", "restore"]) {
      const result = await run(root, [join(dir, "autopilot-redraft-case.ts"), src, mode], { REDRAFT_CARD: "1" });
      expect(result.code, result.err).toBe(0);
      expect(JSON.parse(result.out).reports).toHaveLength(1); // the seeded parent, no new report
      expect(result.calls).toEqual([{ kind: "hub", detail: mode === "processing" ? "action_row" : "restore_actions" }]);
    }
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

const CASE = join(dir, "autopilot-redraft-case.ts");
const states = (out: { intents: { state: string }[] }) => out.intents.map((intent) => intent.state);

test("pending card edit keeps one identity and drains in order on restart", async () => {
  const { root, src } = await stage();
  try {
    const pending = { REDRAFT_CARD: "1", REDRAFT_HUB_DELIVERY: "pending" };
    const first = await run(root, [CASE, src, "processing"], pending);
    expect(first.code, first.err).toBe(0);
    const queued = JSON.parse(first.out);
    expect(queued.intents).toHaveLength(1);
    expect(queued.intents[0]).toMatchObject({ card: "report:parent", mutation_id: queued.mutations[0], state: "pending" });
    const repeat = await run(root, [CASE, src, "processing"], pending);
    expect(repeat.code, repeat.err).toBe(0);
    expect(JSON.parse(repeat.out).mutations).toEqual(queued.mutations); // same identity, no second intent
    expect(JSON.parse(repeat.out).intents).toEqual(queued.intents);
    const restart = await run(root, [CASE, src, "restore"], { REDRAFT_CARD: "1" });
    expect(restart.code, restart.err).toBe(0);
    expect(restart.calls).toEqual([{ kind: "hub", detail: "action_row" }, { kind: "hub", detail: "restore_actions" }]);
    const drained = JSON.parse(restart.out);
    expect(drained.mutations[0]).toBe(queued.mutations[0]);
    expect(new Set(drained.mutations).size).toBe(2);
    expect(states(drained)).toEqual(["delivered", "delivered"]);
    const idle = await run(root, [CASE, src, "flush"], { REDRAFT_CARD: "1" });
    expect(idle.code, idle.err).toBe(0);
    expect(idle.calls).toEqual([]);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

test("flushReports retries a pending card edit under its stored identity", async () => {
  const { root, src } = await stage();
  try {
    const first = JSON.parse((await run(root, [CASE, src, "processing"], { REDRAFT_CARD: "1", REDRAFT_HUB_DELIVERY: "pending" })).out);
    const flushed = await run(root, [CASE, src, "flush"], { REDRAFT_CARD: "1" });
    expect(flushed.code, flushed.err).toBe(0);
    expect(JSON.parse(flushed.out).mutations).toEqual(first.mutations);
    expect(states(JSON.parse(flushed.out))).toEqual(["delivered"]);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

for (const terminal of ["uncertain", "suppressed"]) {
  test(`${terminal} card edit is never replayed; only uncertain blocks later edits`, async () => {
    const { root, src } = await stage();
    try {
      const first = await run(root, [CASE, src, "processing"], { REDRAFT_CARD: "1", REDRAFT_HUB_DELIVERY: terminal });
      expect(first.code, first.err).toBe(0);
      expect(states(JSON.parse(first.out))).toEqual([terminal]);
      const later = await run(root, [CASE, src, "restore"], { REDRAFT_CARD: "1" });
      expect(later.code, later.err).toBe(0);
      const flushed = await run(root, [CASE, src, "flush"], { REDRAFT_CARD: "1" });
      expect(flushed.calls).toEqual([]);
      if (terminal === "uncertain") {
        expect(later.calls).toEqual([]);
        expect(states(JSON.parse(flushed.out))).toEqual(["uncertain", "pending"]);
      } else {
        expect(later.calls).toEqual([{ kind: "hub", detail: "restore_actions" }]);
        expect(states(JSON.parse(flushed.out))).toEqual(["suppressed", "delivered"]);
      }
    } finally { await rm(root, { recursive: true }); }
  }, 15_000);
}

test("legacy card edits stay durably held in order without hub or Slack calls", async () => {
  const { root, src } = await stage();
  try {
    expect((await run(root, [CASE, src, "seed", JSON.stringify([BID_ITEM])])).code).toBe(0);
    const legacy = { REDRAFT_CARD: "1", REDRAFT_NO_PARENT: "1" };
    let last = { code: 0, out: "", err: "", calls: [] as unknown[] };
    for (const mode of ["processing", "restore", "flush", "processing"]) {
      last = await run(root, [CASE, src, mode], legacy);
      expect(last.code, last.err).toBe(0);
      expect(last.calls).toEqual([]);
    }
    const out = JSON.parse(last.out);
    expect(out.reports).toEqual([]);
    expect(out.intents.map(({ card, item_id, state }: Record<string, string>) => ({ card, item_id, state }))).toEqual([
      { card: OLD_REF, item_id: BID_ITEM.id, state: "legacy" },
      { card: OLD_REF, item_id: BID_ITEM.id, state: "legacy" },
      { card: OLD_REF, item_id: BID_ITEM.id, state: "legacy" },
    ]);
    expect(new Set(out.intents.map((intent: { mutation_id: string }) => intent.mutation_id)).size).toBe(3);
    const adopted = await run(root, [CASE, src, "flush"], { ...legacy, REDRAFT_ADOPT: "1" });
    expect(adopted.code, adopted.err).toBe(0);
    expect(adopted.calls).toEqual([
      { kind: "hub", detail: "report_adopt" },
      { kind: "hub", detail: "action_row" },
      { kind: "hub", detail: "restore_actions" },
      { kind: "hub", detail: "action_row" },
    ]);
    expect(states(JSON.parse(adopted.out))).toEqual(["delivered", "delivered", "delivered"]);
    const repeated = await run(root, [CASE, src, "flush"], { ...legacy, REDRAFT_ADOPT: "1" });
    expect(repeated.code, repeated.err).toBe(0);
    expect(repeated.calls).toEqual([]);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

for (const code of ["PREFERRED_ONLY", "PROJECT_CLOSED", "BID_FORM_MISSING"]) {
  test(`bid outcome ${code} preserves only actionable retries`, async () => {
    const { root, src } = await stage();
    try {
      expect((await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", JSON.stringify([BID_ITEM])])).code).toBe(0);
      const result = await run(root, [join(src, "action.ts"), "bid", "1"], { REDRAFT_BID_CODE: code });
      const retryable = code === "BID_FORM_MISSING";
      expect(result.code, result.err).toBe(retryable ? 1 : 0);
      expect(result.calls.filter((call: { kind: string }) => call.kind === "bid")).toEqual([{ kind: "bid", detail: code }]);
      const state = await snapshot(root, src);
      expect(state.items[0].status).toBe(retryable ? "failed" : "skipped");
      expect(state.reports).toHaveLength(1);
      expect(JSON.parse(state.reports[0].request).salience).toBe(retryable ? "action" : "result");
      if (!retryable) {
        const repeat = await run(root, [join(src, "action.ts"), "bid", "1"], { REDRAFT_BID_CODE: code });
        expect(repeat.code, repeat.err).toBe(0);
        expect(repeat.calls.filter((call: { kind: string }) => call.kind === "bid")).toEqual([]);
        expect((await snapshot(root, src)).reports).toHaveLength(1);
      }
    } finally { await rm(root, { recursive: true }); }
  }, 15_000);
}

for (const error of ["CDP command timeout", "Aside Browser profile is not connected to the daemon", "Failed to request daemon auth challenge: fetch failed"]) {
test("transient collection failure stays quiet until persistent: " + error, async () => {
  const { root, src } = await stage();
  try {
    const args = [join(src, "collect.ts"), "--only=wishket"];
    const failure = { REDRAFT_TRANSIENT: error };
    const first = await run(root, args, failure);
    expect(first.code, first.err).toBe(0);
    expect((await snapshot(root, src)).reports).toEqual([]);
    await writeFile(join(root, "home", "outage-since"), "2000-01-01T00:00:00.000Z");
    const persistent = await run(root, args, failure);
    expect(persistent.code).toBe(1);
    const reports = (await snapshot(root, src)).reports;
    expect(reports).toHaveLength(1);
    expect(JSON.parse(reports[0].request).topic).toBe("autopilot:collect-error");
    expect((await run(root, args, failure)).code).toBe(1);
    expect((await snapshot(root, src)).reports).toHaveLength(1);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

}

test("restricted saved Wishket body is skipped at the draft entry before judge, network or card", async () => {
  const { root, src } = await stage();
  try {
    expect((await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", JSON.stringify(ITEMS)])).code).toBe(0);
    const r = await run(root, [join(dir, "autopilot-redraft-case.ts"), src, ITEMS[0]!.id], { REDRAFT_VERDICT: VERDICT });
    expect(r.code, r.err).toBe(0);
    expect(r.calls).toEqual([]);
    const state = JSON.parse(r.out);
    expect(state.items[0]).toMatchObject({ status: "skipped", chat_ref: OLD_REF, verdict: null, draft: "old draft" });
    expect(state.reports).toEqual([]);
    expect(state.log).toContainEqual({ item_id: ITEMS[0]!.id, event: "skip", detail: "PRIME/PRO/BOOST only" });
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

test("eligible control still judges and queues exactly one approval card", async () => {
  const { root, src } = await stage();
  try {
    expect((await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", JSON.stringify(ITEMS)])).code).toBe(0);
    const r = await run(root, [join(dir, "autopilot-redraft-case.ts"), src, ITEMS[1]!.id], { REDRAFT_VERDICT: VERDICT });
    expect(r.code, r.err).toBe(0);
    expect(r.calls).toEqual([{ kind: "judge", detail: "" }]);
    const state = JSON.parse(r.out);
    expect(state.reports).toHaveLength(1);
    expect(state.reports[0]).toMatchObject({ item_id: ITEMS[1]!.id, approval: 1 });
    expect(JSON.parse(state.reports[0].request).topic).toBe(`autopilot:item:${ITEMS[1]!.id}`);
    expect(state.items[1].status).toBe("pending_approval");
    expect(state.items[1].chat_ref).toStartWith("report:");
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

test("redo on a restricted item with a stale chat_ref does not claim a new card", async () => {
  const { root, src } = await stage();
  try {
    expect((await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", JSON.stringify(ITEMS)])).code).toBe(0);
    const r = await run(root, [join(src, "action.ts"), "redo", "1"], { REDRAFT_VERDICT: VERDICT });
    expect(r.code, r.err).toBe(0);

    expect(r.calls.filter((c: { kind: string }) => c.kind !== "network")).toEqual([]);
    // Only the old card is touched (processing marker lookup + close); no message is posted.
    expect(r.calls.filter((c: { kind: string; detail: string }) => c.kind === "network" && c.detail !== "conversations.history")).toEqual([]);
    const state = await snapshot(root, src);
    expect(state.items[0]).toMatchObject({ status: "skipped", chat_ref: OLD_REF });
    expect(state.reports.filter((row: { approval: number }) => row.approval)).toEqual([]);
    expect(state.reports.map((row: { request: string }) => JSON.parse(row.request).topic)).toEqual([`autopilot:item:${ITEMS[0]!.id}:result`]);
    expect(JSON.parse(state.reports[0].request).state).toBe(
      createHash("sha256").update(JSON.stringify(["redo", false, "SKIPPED"])).digest("hex"));
  } finally { await rm(root, { recursive: true }); }
}, 15_000);

test("redo control on an eligible item reports the queued replacement card", async () => {
  const { root, src } = await stage();
  try {
    expect((await run(root, [join(dir, "autopilot-redraft-case.ts"), src, "seed", JSON.stringify(ITEMS)])).code).toBe(0);
    const r = await run(root, [join(src, "action.ts"), "redo", "2"], { REDRAFT_VERDICT: VERDICT });
    expect(r.code, r.err).toBe(0);

    expect(r.calls.filter((c: { kind: string }) => c.kind === "judge")).toHaveLength(1);
    const state = await snapshot(root, src);
    expect(state.items[1].status).toBe("pending_approval");
    expect(state.items[1].chat_ref).toStartWith("report:");
    expect(state.reports.filter((row: { approval: number }) => row.approval)).toHaveLength(1);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);


test("shipped card generation regressions run against the staged producer", async () => {
  const { root, src } = await stage();
  try {
    const child = Bun.spawn([process.execPath, "test", "--preload",
      join(dir, "autopilot-redraft-preload.ts"), join(src, "report-card.test.ts")], {
      env: { ...process.env, AUTOPILOT_HOME: join(root, "home"),
        FERRYX_HUB_CONTROL_SOCKET: join(root, "absent.sock"),
        AUTOPILOT_SLACK_TOKEN_FILE: join(root, "token"), AUTOPILOT_DRY_RUN: "0",
        REDRAFT_TRACE: join(root, "trace.jsonl") },
      stdout: "pipe", stderr: "pipe",
    });
    const [code, , stderr] = await Promise.all([child.exited,
      new Response(child.stdout).text(), new Response(child.stderr).text()]);
    expect(code, stderr).toBe(0);
  } finally { await rm(root, { recursive: true }); }
}, 15_000);