import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  FailoverConfigError,
  STANDBY_MARKER,
  cloudflarePlan,
  deployStandbyPlan,
  drillPlan,
  loadConfig,
  promoteStandbyPlan,
  summarizeDrill,
  validateConfig,
} from "./relay-failover.mjs";

const SCRIPTS = import.meta.dir;
const FIXTURES = join(SCRIPTS, "fixtures", "relay-failover");
const CONFIG = join(FIXTURES, "config.json");
const raw = () => JSON.parse(readFileSync(CONFIG, "utf8"));
const expected = (name) => JSON.parse(readFileSync(join(FIXTURES, name), "utf8"));

function dryRun(...args) {
  const result = spawnSync(process.execPath, [join(SCRIPTS, "relay-failover.mjs"), ...args, "--config", CONFIG], {
    encoding: "utf8",
  });
  expect(result.status).toBe(0);
  return JSON.parse(result.stdout);
}

test.each([
  ["deploy-standby", "deploy-standby.plan.json"],
  ["promote-standby", "promote-standby.plan.json"],
  ["cloudflare", "cloudflare.plan.json"],
  ["drill", "drill.plan.json"],
])("%s dry-run plan matches the fixture", (command, fixture) => {
  expect(dryRun(command)).toEqual(expected(fixture));
});

test("the standby is read-only before it ever holds a database and never replicates", () => {
  const ids = deployStandbyPlan(loadConfig(CONFIG)).steps.map((step) => step.id);
  expect(ids.indexOf("standby-marker")).toBeLessThan(ids.indexOf("standby-first-restore"));
  expect(ids.indexOf("standby-first-restore")).toBeLessThan(ids.indexOf("standby-start"));
  const start = deployStandbyPlan(loadConfig(CONFIG)).steps.find((step) => step.id === "standby-start");
  expect(start.command).toContain("disable --now litestream.service");
  expect(ids.at(-1)).toBe("standby-refuses-writes");
});

test("promotion fences the old primary before the marker is removed", () => {
  const steps = promoteStandbyPlan(loadConfig(CONFIG)).steps;
  const ids = steps.map((step) => step.id);
  expect(ids[0]).toBe("fence-primary");
  expect(ids.indexOf("final-restore")).toBeLessThan(ids.indexOf("remove-marker"));
  expect(steps.find((step) => step.id === "remove-marker").command).toContain(STANDBY_MARKER);
});

test("promotion stops the standby relay before the marker goes and bumps the lease epoch before it starts", () => {
  const steps = dryRun("promote-standby").steps;
  const ids = steps.map((step) => step.id);
  expect(ids.indexOf("stop-relay")).toBeGreaterThan(-1);
  expect(ids.indexOf("stop-relay")).toBeLessThan(ids.indexOf("remove-marker"));
  expect(ids.indexOf("remove-marker")).toBeLessThan(ids.indexOf("bump-lease-epoch"));
  expect(ids.indexOf("bump-lease-epoch")).toBeLessThan(ids.indexOf("start-replication"));
  expect(steps.find((step) => step.id === "bump-lease-epoch").command).toContain("ferryx-relay' bump-lease-epoch");
  const before = ids.slice(0, ids.indexOf("bump-lease-epoch"));
  for (const id of before) {
    expect(steps.find((step) => step.id === id).command).not.toMatch(/(start|restart) ferryx-relay/);
  }
});

test("a missing standby host leaves the plan printable but blocked", () => {
  const config = raw();
  config.standby.ssh = null;
  const plan = deployStandbyPlan(validateConfig(config));
  expect(plan.blockedOn).toStartWith("STANDBY_HOST_MISSING");
  expect(plan.steps.find((step) => step.id === "standby-marker").host).toBe("<standby-ssh-host>");
});

test("the retired relay hostname is rejected anywhere in the config", () => {
  const config = raw();
  config.standby.directHostname = "relay.checka.cc";
  expect(() => validateConfig(config)).toThrow(FailoverConfigError);
  try {
    validateConfig(config);
  } catch (error) {
    expect(error.code).toBe("RETIRED_HOSTNAME");
  }
});

test("tunnel replicas are refused because they would split account writes", () => {
  const config = raw();
  config.cloudflare.mode = "tunnel";
  try {
    validateConfig(config);
    throw new Error("expected a config error");
  } catch (error) {
    expect(error.code).toBe("CLOUDFLARE_MODE_UNSUPPORTED");
  }
});

test("the load balancer prefers the primary pool and health-checks /healthz", () => {
  const plan = cloudflarePlan(loadConfig(CONFIG));
  const monitor = plan.steps.find((step) => step.id === "monitor");
  expect(monitor.body.path).toBe("/healthz");
  const lb = plan.steps.find((step) => step.id === "load-balancer");
  expect(lb.body.steering_policy).toBe("off");
  expect(lb.body.default_pools).toEqual(["{pool-primary.id}", "{pool-standby.id}"]);
});

test("drill summary passes only with all-200 samples inside the stop window", () => {
  const plan = drillPlan(loadConfig(CONFIG));
  expect(plan.probeHost).toBe("maho-win");
  expect(plan.url).toBe("https://relay.ferryx.dev/healthz");
  const ok = summarizeDrill("1000 200\r\n6000 200\n11000 200\n", 2000, 9000);
  expect(ok).toMatchObject({ samples: 3, samplesWhilePrimaryStopped: 1, failures: 0, passed: true });
  const bad = summarizeDrill("1000 200\n6000 000\n11000 502\n16000 200\n", 2000, 12000);
  expect(bad).toMatchObject({ failures: 2, failedAt: [6000, 11000], longestOutageMs: 5000, passed: false });
});

test("the CLI refuses to run without --config", () => {
  const result = spawnSync(process.execPath, [join(SCRIPTS, "relay-failover.mjs"), "deploy-standby"], { encoding: "utf8" });
  expect(result.status).toBe(2);
  expect(result.stderr).toContain("USAGE");
});
