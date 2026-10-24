import { expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import worker, { DAYS, loadStatus, parseHosts, probe, recordChecks, utcDay } from "./worker.js";

// A D1 stand-in over bun:sqlite with the subset of the binding API the worker uses.
function fakeD1() {
  const db = new Database(":memory:");
  const statement = (sql, params = []) => ({
    bind: (...values) => statement(sql, values),
    run: () => db.query(sql).run(...params),
    all: () => ({ results: db.query(sql).all(...params) }),
  });
  return {
    exec: async (sql) => db.exec(sql),
    prepare: (sql) => statement(sql.replace(/\?(\d)/g, "?$1")),
    batch: async (statements) => statements.map((s) => s.run()),
  };
}

const HOSTS = JSON.stringify([
  { name: "primary", url: "https://relay-1.example.test" },
  { name: "standby", url: "https://relay-2.example.test" },
]);

function fakeFetch(states) {
  return async (url) => {
    const host = new URL(url).hostname;
    expect(new URL(url).pathname).toBe("/healthz");
    const state = states[host];
    if (state === "throw") throw new Error("connect failed");
    if (state === "down") return new Response("bad gateway", { status: 502 });
    return Response.json({ ok: true, role: state });
  };
}

test("probe is up only for a 200 with ok:true", async () => {
  const host = { name: "h", url: "https://relay-1.example.test" };
  expect(await probe(host, fakeFetch({ "relay-1.example.test": "primary" }))).toEqual({ ok: true, role: "primary" });
  expect(await probe(host, fakeFetch({ "relay-1.example.test": "down" }))).toEqual({ ok: false, role: null });
  expect(await probe(host, fakeFetch({ "relay-1.example.test": "throw" }))).toEqual({ ok: false, role: null });
  expect(await probe(host, async () => new Response("<html>spa</html>", { status: 200 }))).toEqual({ ok: false, role: null });
});

test("checks aggregate per host per day and both hosts appear on the page", async () => {
  const env = { DB: fakeD1(), HOSTS };
  const day0 = Date.UTC(2026, 9, 24, 12, 0, 0);
  const fetchUp = fakeFetch({ "relay-1.example.test": "primary", "relay-2.example.test": "standby" });
  const fetchPrimaryDown = fakeFetch({ "relay-1.example.test": "down", "relay-2.example.test": "standby" });
  await recordChecks(env, day0, fetchUp);
  await recordChecks(env, day0 + 60_000, fetchPrimaryDown);
  await recordChecks(env, day0 + 120_000, fetchUp);

  const status = await loadStatus(env, day0 + 180_000);
  expect(status.map((h) => h.name)).toEqual(["primary", "standby"]);
  const [primary, standby] = status;
  expect(primary.days).toHaveLength(DAYS);
  expect(primary.days.at(-1)).toEqual({ day: utcDay(day0), ok: 2, total: 3 });
  expect(primary.uptime).toBeCloseTo(2 / 3);
  expect(primary.current).toEqual({ up: true, role: "primary", checkedAt: day0 + 120_000 });
  expect(standby.uptime).toBe(1);
  expect(standby.current.role).toBe("standby");

  const realNow = Date.now;
  Date.now = () => day0 + 180_000;
  try {
    const page = await worker.fetch(new Request("https://status.ferryx.dev/"), env);
    const html = await page.text();
    expect(html).toContain('data-host="primary"');
    expect(html).toContain('data-host="standby"');
    expect(html).toContain("66.67%");
    const json = await (await worker.fetch(new Request("https://status.ferryx.dev/api/status"), env)).json();
    expect(json.hosts).toHaveLength(2);
  } finally {
    Date.now = realNow;
  }
});

test("days older than the window are excluded", async () => {
  const env = { DB: fakeD1(), HOSTS };
  const now = Date.UTC(2026, 9, 24);
  const fetchUp = fakeFetch({ "relay-1.example.test": "primary", "relay-2.example.test": "standby" });
  await recordChecks(env, now - DAYS * 86_400_000, fetchUp);
  const [primary] = await loadStatus(env, now);
  expect(primary.checks).toBe(0);
  expect(primary.uptime).toBeNull();
});

test("the shipped wrangler config lists both relay hosts and never the retired one", async () => {
  const text = await Bun.file(new URL("./wrangler.jsonc", import.meta.url)).text();
  const hostsLine = text.match(/"HOSTS":\s*("(?:[^"\\]|\\.)*")/)[1];
  const hosts = parseHosts({ HOSTS: JSON.parse(hostsLine) });
  expect(hosts.map((h) => new URL(h.url).hostname)).toEqual(["relay.ferryx.dev", "relay-1.ferryx.dev", "relay-2.ferryx.dev"]);
  expect(text).toContain('"crons": ["* * * * *"]');
  expect(() => parseHosts({ HOSTS: JSON.stringify([{ name: "x", url: "https://relay.checka.cc" }]) })).toThrow();
});
