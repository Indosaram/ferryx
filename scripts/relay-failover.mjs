#!/usr/bin/env bun
// Warm-standby relay operations for relay.ferryx.dev: deploy, promote, demote, Cloudflare
// failover and the failover drill.
//
// Every command prints its plan and changes nothing unless `--apply` is given. The plan is a
// pure function of the config file, so `scripts/relay-failover.test.mjs` pins it against a
// fixture. Secrets (object-store keys, Cloudflare token) are never part of a plan: hosts read
// them from private EnvironmentFiles and the Cloudflare step from the environment.
//
// Invariant: exactly one host writes the account database. The standby carries the marker
// `<dataDir>/standby.readonly`, which makes ferryx-relay refuse every account write with
// ACCOUNT_STORE_READ_ONLY, and it runs no Litestream replicator. `promote-standby` fences the
// old primary first and removes the marker last.
//
// Usage:
//   bun scripts/relay-failover.mjs deploy-standby  --config <file> [--apply]
//   bun scripts/relay-failover.mjs promote-standby --config <file> [--apply]
//   bun scripts/relay-failover.mjs demote <primary|standby> --config <file> [--apply]
//   bun scripts/relay-failover.mjs cloudflare      --config <file> [--apply]
//   bun scripts/relay-failover.mjs drill           --config <file> [--apply]
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const TEMPLATES = join(HERE, "relay");
export const RETIRED_HOSTNAME = "relay.checka.cc";
export const STANDBY_MARKER = "standby.readonly";
export const DB_FILENAME = "account-store.sqlite3";
const HOST_PLACEHOLDER = "<standby-ssh-host>";

export class FailoverConfigError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.code = code;
  }
}

function requireString(value, field) {
  if (typeof value !== "string" || value.trim() === "") {
    throw new FailoverConfigError("CONFIG_INVALID", `${field} must be a non-empty string`);
  }
  return value;
}

function requireAbsolute(value, field) {
  requireString(value, field);
  if (!value.startsWith("/")) throw new FailoverConfigError("CONFIG_INVALID", `${field} must be an absolute path`);
  return value;
}

/** Validates a parsed config and fills defaults. Throws FailoverConfigError. */
export function validateConfig(raw) {
  if (raw?.schemaVersion !== 1) throw new FailoverConfigError("CONFIG_INVALID", "schemaVersion must be 1");
  if (JSON.stringify(raw).includes(RETIRED_HOSTNAME)) {
    throw new FailoverConfigError("RETIRED_HOSTNAME", `${RETIRED_HOSTNAME} is retired and must not appear in the config`);
  }
  requireString(raw.publicHostname, "publicHostname");
  if (!["commercial", "selfhost"].includes(raw.deploymentMode)) {
    throw new FailoverConfigError("CONFIG_INVALID", "deploymentMode must be commercial or selfhost");
  }
  const port = raw.port ?? 8787;
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new FailoverConfigError("CONFIG_INVALID", "port must be 1..65535");
  for (const role of ["primary", "standby"]) {
    const host = raw[role];
    if (!host || typeof host !== "object") throw new FailoverConfigError("CONFIG_INVALID", `${role} is required`);
    requireString(host.name, `${role}.name`);
    requireString(host.user, `${role}.user`);
    for (const field of ["binary", "dataDir", "relayDataDir", "uiDistDir"]) requireAbsolute(host[field], `${role}.${field}`);
    requireString(host.directHostname, `${role}.directHostname`);
    if (host.ssh !== null && host.ssh !== undefined) requireString(host.ssh, `${role}.ssh`);
  }
  requireString(raw.primary.ssh, "primary.ssh");
  if (raw.primary.dataDir === raw.standby.dataDir && raw.primary.ssh === raw.standby.ssh) {
    throw new FailoverConfigError("CONFIG_INVALID", "primary and standby must be different hosts");
  }
  const ls = raw.litestream ?? {};
  for (const field of ["version", "bucket", "path", "endpoint"]) requireString(ls[field], `litestream.${field}`);
  const cf = raw.cloudflare ?? {};
  if (cf.mode !== "load-balancer") {
    throw new FailoverConfigError(
      "CLOUDFLARE_MODE_UNSUPPORTED",
      "only cloudflare.mode=load-balancer gives ordered failover; tunnel replicas spread requests across " +
        "both connectors, which would send account writes to the read-only standby",
    );
  }
  requireString(cf.zone, "cloudflare.zone");
  const drill = raw.drill ?? {};
  requireString(drill.probeHost, "drill.probeHost");
  return {
    ...raw,
    port,
    cloudflare: { monitorIntervalSeconds: 60, ...cf },
    drill: { stopSeconds: 120, probeEverySeconds: 5, evidenceDir: ".omo/evidence/relay-failover", ...drill },
  };
}

export function loadConfig(path) {
  return validateConfig(JSON.parse(readFileSync(path, "utf8")));
}

function render(template, vars) {
  return template.replace(/@([A-Z_]+)@/g, (match, key) => {
    if (!(key in vars)) throw new Error(`template variable ${key} has no value`);
    return String(vars[key]);
  });
}

function template(name) {
  return readFileSync(join(TEMPLATES, name), "utf8");
}

function q(value) {
  return `'${String(value).replaceAll("'", `'\\''`)}'`;
}

function hostTarget(config, role) {
  return config[role].ssh ?? (role === "standby" ? HOST_PLACEHOLDER : null);
}

function litestreamYml(config, host) {
  const ls = config.litestream;
  return [
    "# Rendered by scripts/relay-failover.mjs. Credentials come from /etc/ferryx/litestream.env",
    "# (LITESTREAM_ACCESS_KEY_ID, LITESTREAM_SECRET_ACCESS_KEY), never from this file.",
    "dbs:",
    `  - path: ${host.dataDir}/${DB_FILENAME}`,
    "    replicas:",
    "      - type: s3",
    `        bucket: ${ls.bucket}`,
    `        path: ${ls.path}`,
    `        endpoint: ${ls.endpoint}`,
    "        region: auto",
    "        force-path-style: true",
    "        sync-interval: 1s",
    "",
  ].join("\n");
}

function unitVars(config, host) {
  return {
    USER: host.user,
    DEPLOYMENT_MODE: config.deploymentMode,
    PUBLIC_HOSTNAME: config.publicHostname,
    DATA_DIR: host.dataDir,
    RELAY_DATA_DIR: host.relayDataDir,
    UI_DIST_DIR: host.uiDistDir,
    BINARY: host.binary,
    PORT: config.port,
  };
}

const run = (host, id, description, command, extra = {}) => ({ id, host, kind: "run", description, command, ...extra });
const file = (host, id, path, content, mode = "0644") => ({ id, host, kind: "file", description: `write ${path}`, path, mode, content });
const local = (id, description, command) => ({ id, host: "local", kind: "run", description, command });

function healthCheck(id, host, config, role) {
  return run(
    host,
    id,
    `local /healthz answers role=${role}`,
    `curl -sf http://127.0.0.1:${config.port}/healthz | grep -q '"role":"${role}"'`,
  );
}

function litestreamInstall(id, host, version) {
  // Release tarball, not a distro package: the primary runs Arch, the standby any Linux.
  const tarball = `https://github.com/benbjohnson/litestream/releases/download/v${version}/litestream-v${version}-linux-amd64.tar.gz`;
  return run(
    host,
    id,
    `install litestream ${version}`,
    `command -v litestream >/dev/null && litestream version | grep -q ${q(`v${version}`)} || ` +
      `{ curl -fsSL ${tarball} | tar -xz -C /tmp litestream && sudo install -m 0755 /tmp/litestream /usr/local/bin/litestream; }`,
  );
}

/** Warm-standby deployment plus Litestream replication on the primary. */
export function deployStandbyPlan(config) {
  const s = config.standby;
  const p = config.primary;
  const sh = hostTarget(config, "standby");
  const ph = hostTarget(config, "primary");
  const vars = unitVars(config, s);
  const marker = `${s.dataDir}/${STANDBY_MARKER}`;
  return {
    command: "deploy-standby",
    publicHostname: config.publicHostname,
    blockedOn: config.standby.ssh ? null : "STANDBY_HOST_MISSING: the owner has not provided the standby's SSH host",
    steps: [
      local("build-relay", "release-profile relay binary (todo 33)", "cd src-tauri && cargo build --release --bin ferryx-relay"),
      run(ph, "primary-secrets-present", "primary has the object-store credentials", "sudo test -s /etc/ferryx/litestream.env"),
      litestreamInstall("primary-install-litestream", ph, config.litestream.version),
      file(ph, "primary-litestream-config", "/etc/litestream.yml", litestreamYml(config, p)),
      file(ph, "primary-litestream-unit", "/etc/systemd/system/litestream.service", render(template("litestream.service"), unitVars(config, p))),
      run(ph, "primary-replicate", "start replicating the primary account database", "sudo systemctl daemon-reload && sudo systemctl enable --now litestream.service"),
      run(ph, "primary-replica-exists", "the replica has a generation", `litestream generations -config /etc/litestream.yml ${q(`${p.dataDir}/${DB_FILENAME}`)} | grep -q .`),
      run(sh, "standby-secrets-present", "standby has the object-store credentials and relay env", "sudo test -s /etc/ferryx/litestream.env && sudo test -s /etc/ferryx/relay.env"),
      run(
        sh,
        "standby-user-and-dirs",
        "service user and private data directories",
        `id -u ${q(s.user)} >/dev/null 2>&1 || sudo useradd -r -s /usr/sbin/nologin -d /var/lib/ferryx ${q(s.user)}; ` +
          `sudo install -d -m 0700 -o ${q(s.user)} -g ${q(s.user)} ${q(s.dataDir)} ${q(s.relayDataDir)} && ` +
          `sudo install -d -m 0755 -o ${q(s.user)} -g ${q(s.user)} ${q(s.uiDistDir)}`,
      ),
      run(sh, "standby-marker", "mark the data directory read-only before any database exists", `sudo -u ${q(s.user)} touch ${q(marker)}`),
      run(ph, "copy-ui", "copy the served remote web client from the primary", `rsync -a --delete ${q(`${p.uiDistDir}/`)} ${q(`${sh}:${s.uiDistDir}/`)}`),
      run(ph, "copy-signing-key", "same account signing key, so grants verify on both hosts", `rsync -a ${q(`${p.dataDir}/account-signing-key.json`)} ${q(`${sh}:${s.dataDir}/account-signing-key.json`)}`),
      run(ph, "copy-relay-keys", "same durable machine key registry", `rsync -a ${q(`${p.relayDataDir}/machine_keys.json`)} ${q(`${sh}:${s.relayDataDir}/machine_keys.json`)}`),
      local("copy-binary", "ship the release binary", `scp src-tauri/target/release/ferryx-relay ${q(`${sh}:/tmp/ferryx-relay`)}`),
      run(sh, "install-binary", "install the relay binary", `sudo install -m 0755 /tmp/ferryx-relay ${q(s.binary)}`),
      litestreamInstall("standby-install-litestream", sh, config.litestream.version),
      file(sh, "standby-litestream-config", "/etc/litestream.yml", litestreamYml(config, s)),
      file(sh, "standby-restore-script", "/usr/local/bin/ferryx-standby-restore.sh", template("ferryx-standby-restore.sh"), "0755"),
      file(sh, "standby-relay-unit", "/etc/systemd/system/ferryx-relay.service", render(template("ferryx-relay.service"), vars)),
      file(sh, "standby-restore-unit", "/etc/systemd/system/ferryx-standby-restore.service", render(template("ferryx-standby-restore.service"), vars)),
      file(sh, "standby-restore-timer", "/etc/systemd/system/ferryx-standby-restore.timer", template("ferryx-standby-restore.timer")),
      file(sh, "standby-litestream-unit", "/etc/systemd/system/litestream.service", render(template("litestream.service"), vars)),
      run(sh, "standby-first-restore", "restore the primary's database into the read-only data dir", "sudo systemctl daemon-reload && sudo systemctl start ferryx-standby-restore.service"),
      run(
        sh,
        "standby-start",
        "start the relay and the restore timer; the standby never replicates",
        "sudo systemctl disable --now litestream.service && " +
          "sudo systemctl enable --now ferryx-relay.service ferryx-standby-restore.timer",
      ),
      healthCheck("standby-health", sh, config, "standby"),
      run(
        sh,
        "standby-refuses-writes",
        "an enrollment against the standby gets the typed read-only error",
        `curl -s -X POST -H 'content-type: application/json' -d '{}' http://127.0.0.1:${config.port}/api/account/v1/machines/enroll | grep -q ACCOUNT_STORE_READ_ONLY`,
      ),
    ],
  };
}

/** Failover: fence the old primary, catch up, then make the standby writable. */
export function promoteStandbyPlan(config) {
  const s = config.standby;
  const sh = hostTarget(config, "standby");
  const ph = hostTarget(config, "primary");
  return {
    command: "promote-standby",
    publicHostname: config.publicHostname,
    blockedOn: config.standby.ssh ? null : "STANDBY_HOST_MISSING: the owner has not provided the standby's SSH host",
    steps: [
      run(
        ph,
        "fence-primary",
        "stop the old primary's relay and replicator; if it is unreachable, power it off in the provider console before continuing",
        "sudo systemctl disable --now ferryx-relay.service litestream.service",
        { allowFailure: true },
      ),
      run(sh, "stop-restore-timer", "stop refreshing from the replica", "sudo systemctl disable --now ferryx-standby-restore.timer"),
      run(sh, "final-restore", "take the newest replicated state", "sudo systemctl start ferryx-standby-restore.service"),
      run(sh, "remove-marker", "the standby becomes the only writable account database", `sudo rm -f ${q(`${s.dataDir}/${STANDBY_MARKER}`)}`),
      run(sh, "start-replication", "the new primary replicates from now on", "sudo systemctl enable --now litestream.service && sudo systemctl restart ferryx-relay.service"),
      healthCheck("promoted-health", sh, config, "primary"),
    ],
  };
}

/** Turns a host back into a read-only standby (after a failback, or the old primary rejoining). */
export function demotePlan(config, role) {
  if (!["primary", "standby"].includes(role)) throw new FailoverConfigError("CONFIG_INVALID", "demote takes primary or standby");
  const host = config[role];
  const target = hostTarget(config, role);
  const vars = unitVars(config, host);
  return {
    command: `demote ${role}`,
    publicHostname: config.publicHostname,
    blockedOn: target === HOST_PLACEHOLDER ? "STANDBY_HOST_MISSING: the owner has not provided the standby's SSH host" : null,
    steps: [
      run(target, "stop-replication", "a standby never replicates", "sudo systemctl disable --now litestream.service"),
      run(target, "add-marker", "refuse account writes from here on", `sudo -u ${q(host.user)} touch ${q(`${host.dataDir}/${STANDBY_MARKER}`)}`),
      file(target, "restore-script", "/usr/local/bin/ferryx-standby-restore.sh", template("ferryx-standby-restore.sh"), "0755"),
      file(target, "restore-unit", "/etc/systemd/system/ferryx-standby-restore.service", render(template("ferryx-standby-restore.service"), vars)),
      file(target, "restore-timer", "/etc/systemd/system/ferryx-standby-restore.timer", template("ferryx-standby-restore.timer")),
      run(target, "start-restore", "follow the new primary's replica", "sudo systemctl daemon-reload && sudo systemctl enable --now ferryx-standby-restore.timer && sudo systemctl restart ferryx-relay.service"),
      healthCheck("demoted-health", target, config, "standby"),
    ],
  };
}

/** Cloudflare Load Balancing: one monitor on /healthz, a primary pool and a standby pool. */
export function cloudflarePlan(config) {
  const cf = config.cloudflare;
  const origin = (role) => ({
    name: config[role].name,
    address: config[role].originAddress ?? config[role].directHostname,
    enabled: true,
    header: { Host: [config.publicHostname] },
  });
  return {
    command: "cloudflare",
    mode: "load-balancer",
    publicHostname: config.publicHostname,
    steps: [
      {
        id: "monitor",
        kind: "api",
        method: "POST",
        path: "/accounts/{account_id}/load_balancers/monitors",
        body: {
          type: "https",
          description: "ferryx-relay /healthz",
          method: "GET",
          path: "/healthz",
          expected_codes: "200",
          expected_body: '"ok":true',
          interval: cf.monitorIntervalSeconds,
          retries: 1,
          timeout: 5,
          follow_redirects: false,
          header: { Host: [config.publicHostname] },
        },
      },
      ...["primary", "standby"].map((role) => ({
        id: `pool-${role}`,
        kind: "api",
        method: "POST",
        path: "/accounts/{account_id}/load_balancers/pools",
        body: {
          name: `ferryx-relay-${role}`,
          monitor: "{monitor.id}",
          minimum_origins: 1,
          origins: [origin(role)],
        },
      })),
      {
        id: "load-balancer",
        kind: "api",
        method: "POST",
        path: "/zones/{zone_id}/load_balancers",
        body: {
          name: config.publicHostname,
          proxied: true,
          steering_policy: "off",
          default_pools: ["{pool-primary.id}", "{pool-standby.id}"],
          fallback_pool: "{pool-standby.id}",
          session_affinity: "none",
        },
      },
    ],
  };
}

/** Stops the primary relay for `stopSeconds` while `probeHost` polls the public /healthz. */
export function drillPlan(config) {
  const d = config.drill;
  const url = `https://${config.publicHostname}/healthz`;
  const window = d.stopSeconds + 60;
  return {
    command: "drill",
    publicHostname: config.publicHostname,
    probeHost: d.probeHost,
    url,
    stopSeconds: d.stopSeconds,
    evidenceDir: d.evidenceDir,
    steps: [
      run(
        d.probeHost,
        "probe",
        `poll ${url} every ${d.probeEverySeconds}s for ${window}s and print "<unix ms> <http code>" lines`,
        `powershell -NoProfile -Command "$end=(Get-Date).AddSeconds(${window}); while((Get-Date) -lt $end){ ` +
          `$t=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); $c=(curl.exe -s -o NUL -w '%{http_code}' --max-time 4 ${url}); ` +
          `Write-Output \\"$t $c\\"; Start-Sleep -Seconds ${d.probeEverySeconds} }"`,
        { background: true },
      ),
      run(
        hostTarget(config, "primary"),
        "stop-primary",
        `stop the primary relay for ${d.stopSeconds}s, then start it again`,
        `date +%s%3N; sudo systemctl stop ferryx-relay.service; sleep ${d.stopSeconds}; sudo systemctl start ferryx-relay.service; date +%s%3N`,
      ),
    ],
  };
}

/** Turns probe output plus the stop/start timestamps into the drill result. */
export function summarizeDrill(probeOutput, stoppedAt, startedAt) {
  const samples = probeOutput
    .split(/\r?\n/)
    .map((line) => line.trim().match(/^(\d+)\s+(\d{3})$/))
    .filter(Boolean)
    .map(([, at, code]) => ({ at: Number(at), code: Number(code) }));
  const during = samples.filter((s) => s.at >= stoppedAt && s.at <= startedAt);
  const failures = samples.filter((s) => s.code !== 200);
  return {
    samples: samples.length,
    samplesWhilePrimaryStopped: during.length,
    failures: failures.length,
    failedAt: failures.map((s) => s.at),
    longestOutageMs: longestRun(samples),
    passed: samples.length > 0 && during.length > 0 && failures.length === 0,
  };
}

function longestRun(samples) {
  let longest = 0;
  let start = null;
  for (const s of samples) {
    if (s.code !== 200) {
      start ??= s.at;
      longest = Math.max(longest, s.at - start);
    } else start = null;
  }
  return longest;
}

export function planFor(command, config, arg) {
  switch (command) {
    case "deploy-standby":
      return deployStandbyPlan(config);
    case "promote-standby":
      return promoteStandbyPlan(config);
    case "demote":
      return demotePlan(config, arg);
    case "cloudflare":
      return cloudflarePlan(config);
    case "drill":
      return drillPlan(config);
    default:
      throw new FailoverConfigError("USAGE", `unknown command ${command}`);
  }
}

function exec(cmd, args, { input, allowFailure = false } = {}) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cmd, args, { stdio: ["pipe", "pipe", "inherit"] });
    let out = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
      process.stdout.write(chunk);
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0 || allowFailure) resolvePromise({ code, out });
      else reject(new Error(`${cmd} ${args.join(" ")} exited ${code}`));
    });
    child.stdin.end(input ?? "");
  });
}

function execStep(step) {
  const allowFailure = Boolean(step.allowFailure);
  if (step.kind === "file") {
    const script = `sudo install -d $(dirname ${q(step.path)}) && sudo tee ${q(step.path)} >/dev/null && sudo chmod ${step.mode} ${q(step.path)}`;
    return exec("ssh", [step.host, script], { input: step.content, allowFailure });
  }
  if (step.host === "local") return exec("bash", ["-c", step.command], { allowFailure });
  return exec("ssh", [step.host, step.command], { allowFailure });
}

async function applyHostPlan(plan) {
  if (plan.blockedOn) throw new FailoverConfigError("STANDBY_HOST_MISSING", plan.blockedOn);
  for (const step of plan.steps) {
    console.error(`[${step.id}] ${step.host}: ${step.description}`);
    await execStep(step);
  }
}

async function applyCloudflare(plan) {
  const token = process.env.CLOUDFLARE_API_TOKEN;
  const accountId = process.env.CLOUDFLARE_ACCOUNT_ID;
  const zoneId = process.env.CLOUDFLARE_ZONE_ID;
  if (!token || !accountId || !zoneId) {
    throw new FailoverConfigError("CLOUDFLARE_ENV_MISSING", "set CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_ZONE_ID");
  }
  const ids = { account_id: accountId, zone_id: zoneId };
  const fill = (value) =>
    JSON.parse(JSON.stringify(value).replace(/\{([a-z_-]+)(?:\.id)?\}/g, (m, key) => {
      if (!(key in ids)) throw new Error(`unresolved reference ${m}`);
      return ids[key];
    }));
  for (const step of plan.steps) {
    const response = await fetch(`https://api.cloudflare.com/client/v4${fill(step.path)}`, {
      method: step.method,
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: JSON.stringify(fill(step.body)),
    });
    const body = await response.json();
    if (!response.ok || !body.success) throw new Error(`${step.id}: ${JSON.stringify(body.errors ?? body)}`);
    ids[step.id] = body.result.id;
    console.error(`[${step.id}] created ${body.result.id}`);
  }
}

async function applyDrill(plan) {
  mkdirSync(plan.evidenceDir, { recursive: true });
  const [probe, stop] = plan.steps;
  const probing = exec("ssh", [probe.host, probe.command]);
  const stopped = await exec("ssh", [stop.host, stop.command]);
  const [stoppedAt, startedAt] = stopped.out.trim().split(/\s+/).map(Number);
  const probed = await probing;
  const summary = { url: plan.url, probeHost: plan.probeHost, stoppedAt, startedAt, ...summarizeDrill(probed.out, stoppedAt, startedAt) };
  writeFileSync(join(plan.evidenceDir, "drill-probe.txt"), probed.out);
  writeFileSync(join(plan.evidenceDir, "drill-summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
  console.log(JSON.stringify(summary, null, 2));
  if (!summary.passed) process.exitCode = 1;
}

export async function main(argv) {
  const args = [...argv];
  const apply = args.includes("--apply");
  const configIndex = args.indexOf("--config");
  if (configIndex < 0 || !args[configIndex + 1]) throw new FailoverConfigError("USAGE", "--config <file> is required");
  const config = loadConfig(resolve(args[configIndex + 1]));
  const positional = args.filter((arg, i) => !arg.startsWith("--") && args[i - 1] !== "--config");
  const [command, arg] = positional;
  const plan = planFor(command, config, arg);
  if (!apply) {
    console.log(JSON.stringify({ dryRun: true, ...plan }, null, 2));
    return;
  }
  if (command === "cloudflare") return applyCloudflare(plan);
  if (command === "drill") return applyDrill(plan);
  return applyHostPlan(plan);
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exit(error instanceof FailoverConfigError ? 2 : 1);
  });
}
