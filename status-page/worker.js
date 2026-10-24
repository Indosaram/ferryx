// status.ferryx.dev: probes /healthz on every relay host once a minute and shows 90-day uptime.
//
// Results are aggregated per host per UTC day in D1 (one upsert per host per minute), because
// per-minute writes exceed the Workers KV free write quota. The page states measured uptime
// only; it promises nothing.

export const DAYS = 90;
const PROBE_TIMEOUT_MS = 10_000;

export const SCHEMA = `CREATE TABLE IF NOT EXISTS checks_daily (
  host TEXT NOT NULL,
  day TEXT NOT NULL,
  ok INTEGER NOT NULL DEFAULT 0,
  total INTEGER NOT NULL DEFAULT 0,
  last_checked_at INTEGER,
  last_ok INTEGER,
  last_role TEXT,
  PRIMARY KEY (host, day)
)`;

/** `[{ name, url }]` from the HOSTS variable (JSON). */
export function parseHosts(env) {
  const hosts = JSON.parse(env.HOSTS ?? "[]");
  if (!Array.isArray(hosts) || hosts.length === 0) throw new Error("HOSTS must list at least one host");
  for (const host of hosts) {
    if (typeof host?.name !== "string" || typeof host?.url !== "string") throw new Error("each host needs name and url");
    if (host.url.includes("relay.checka.cc")) throw new Error("relay.checka.cc is retired");
  }
  return hosts;
}

export function utcDay(ms) {
  return new Date(ms).toISOString().slice(0, 10);
}

/** One probe: up only when /healthz answers 200 with `"ok":true`. */
export async function probe(host, fetchImpl = fetch) {
  try {
    const response = await fetchImpl(new URL("/healthz", host.url).toString(), {
      headers: { "user-agent": "ferryx-status/1" },
      cf: { cacheTtl: 0 },
      signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
    });
    if (response.status !== 200) return { ok: false, role: null };
    const body = await response.json().catch(() => null);
    return { ok: body?.ok === true, role: typeof body?.role === "string" ? body.role : null };
  } catch {
    return { ok: false, role: null };
  }
}

export async function recordChecks(env, now, fetchImpl = fetch) {
  const hosts = parseHosts(env);
  const results = await Promise.all(hosts.map(async (host) => ({ host, ...(await probe(host, fetchImpl)) })));
  await env.DB.exec(SCHEMA.replace(/\n\s*/g, " "));
  const day = utcDay(now);
  await env.DB.batch(
    results.map(({ host, ok, role }) =>
      env.DB.prepare(
        `INSERT INTO checks_daily (host, day, ok, total, last_checked_at, last_ok, last_role)
         VALUES (?1, ?2, ?3, 1, ?4, ?3, ?5)
         ON CONFLICT (host, day) DO UPDATE SET
           ok = ok + excluded.ok, total = total + 1,
           last_checked_at = excluded.last_checked_at, last_ok = excluded.last_ok, last_role = excluded.last_role`,
      ).bind(host.name, day, ok ? 1 : 0, now, role),
    ),
  );
  return results;
}

/** Per-host summary over the last `DAYS` UTC days ending today. */
export async function loadStatus(env, now) {
  const hosts = parseHosts(env);
  const days = Array.from({ length: DAYS }, (_, i) => utcDay(now - (DAYS - 1 - i) * 86_400_000));
  const { results } = await env.DB.prepare(
    "SELECT host, day, ok, total, last_checked_at, last_ok, last_role FROM checks_daily WHERE day >= ?1 ORDER BY day",
  )
    .bind(days[0])
    .all();
  return hosts.map((host) => {
    const rows = new Map(results.filter((row) => row.host === host.name).map((row) => [row.day, row]));
    const series = days.map((day) => {
      const row = rows.get(day);
      return { day, ok: row?.ok ?? 0, total: row?.total ?? 0 };
    });
    const ok = series.reduce((sum, d) => sum + d.ok, 0);
    const total = series.reduce((sum, d) => sum + d.total, 0);
    const latest = [...rows.values()].sort((a, b) => (b.last_checked_at ?? 0) - (a.last_checked_at ?? 0))[0];
    return {
      name: host.name,
      url: host.url,
      uptime: total === 0 ? null : ok / total,
      checks: total,
      current: latest ? { up: latest.last_ok === 1, role: latest.last_role, checkedAt: latest.last_checked_at } : null,
      days: series,
    };
  });
}

const escape = (value) =>
  String(value).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

function percent(value) {
  return value === null ? "no data" : `${(value * 100).toFixed(2)}%`;
}

function barColor(day) {
  if (day.total === 0) return "#3a3a40";
  const ratio = day.ok / day.total;
  if (ratio >= 0.999) return "#2ea043";
  if (ratio >= 0.95) return "#d29922";
  return "#da3633";
}

export function renderPage(status, now) {
  const rows = status
    .map((host) => {
      const state = host.current === null ? "unknown" : host.current.up ? "up" : "down";
      const role = host.current?.role ? ` (${escape(host.current.role)})` : "";
      const bars = host.days
        .map((d) => `<span class="bar" style="background:${barColor(d)}" title="${d.day}: ${d.total ? `${d.ok}/${d.total} checks up` : "no data"}"></span>`)
        .join("");
      return `<section class="host" data-host="${escape(host.name)}">
  <h2>${escape(host.name)} <span class="state ${state}">${state}${role}</span></h2>
  <p class="meta">${escape(host.url)} · ${DAYS}-day uptime <strong>${percent(host.uptime)}</strong> over ${host.checks} checks</p>
  <div class="bars">${bars}</div>
</section>`;
    })
    .join("\n");
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Ferryx relay status</title>
<style>
body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#0e0e10;color:#e8e8ee;max-width:860px;margin:0 auto;padding:32px 16px}
a{color:#8ab4ff}.meta{color:#a0a0aa;font-size:14px}.host{margin:28px 0}
.bars{display:flex;gap:2px}.bar{flex:1;height:32px;border-radius:2px}
.state{font-size:14px;padding:2px 8px;border-radius:10px;margin-left:8px}
.up{background:#1b4d2a}.down{background:#5c1d1b}.unknown{background:#3a3a40}
footer{color:#a0a0aa;font-size:13px;margin-top:40px}
</style>
</head>
<body>
<h1>Ferryx relay status</h1>
<p class="meta">Each host's <code>/healthz</code> is checked once a minute. The bars show the last ${DAYS} days (UTC). This page reports measurements; the hosted relay has no SLA.</p>
${rows}
<footer>Updated ${new Date(now).toISOString()} · <a href="/api/status">JSON</a> · <a href="https://ferryx.dev/docs/self-hosted-relay/">Self-host your own relay</a></footer>
</body>
</html>`;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const now = Date.now();
    if (url.pathname === "/api/status") {
      const status = await loadStatus(env, now);
      return Response.json({ generatedAt: now, days: DAYS, hosts: status }, { headers: { "cache-control": "public, max-age=30" } });
    }
    if (url.pathname !== "/") return new Response("not found", { status: 404 });
    const status = await loadStatus(env, now);
    return new Response(renderPage(status, now), {
      headers: { "content-type": "text/html; charset=utf-8", "cache-control": "public, max-age=30" },
    });
  },
  async scheduled(controller, env, ctx) {
    ctx.waitUntil(recordChecks(env, controller.scheduledTime ?? Date.now()));
  },
};
