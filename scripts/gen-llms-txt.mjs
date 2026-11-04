#!/usr/bin/env bun
// Generates docs/llms.txt (https://llmstxt.org) from the ferryx-cli command catalogue, so the
// agent-facing command list can never drift from the binary: the CLI section is rendered from
// `ferryx-cli api list --json` and `ferryx-cli api describe <command> --json`, never written by
// hand. The Agents and Remote sections link the site's docs pages.
//
//   bun scripts/gen-llms-txt.mjs             regenerate docs/llms.txt
//   bun scripts/gen-llms-txt.mjs --check     exit 1 with a DRIFT message when docs/llms.txt is stale
//   bun scripts/gen-llms-txt.mjs --publish   copy docs/llms.txt to site/public/llms.txt (site build)
//
// The catalogue comes from `--cli <ferryx-cli>`, else $FERRYX_CLI, else a ferryx-cli built here
// with `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin ferryx-cli`.
// `--catalog <file>` reads a `{"commands":[<describe entry>...]}` file instead (tests), and
// `--out <file>` writes or checks another file than docs/llms.txt.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const LLMS_TXT = join(REPO_ROOT, "docs/llms.txt");
export const SITE_COPY = join(REPO_ROOT, "site/public/llms.txt");
export const API_CATALOG_SOURCE = join(REPO_ROOT, "src-tauri/src/cli/api_catalog.rs");

const SITE = "https://ferryx.dev";
const SKILL_URL =
  "https://github.com/Indosaram/ferryx/blob/main/src-tauri/resources/agent-skills/ferryx/SKILL.md";

export const CLI_LINKS = [
  ["Owner Luau scripting", "https://github.com/Indosaram/ferryx/blob/main/docs/scripting.md", "luauApiV1 functions, key bindings, sandbox, settings and handover behavior"],
  ["User guide", `${SITE}/docs/guide/`, "installing Ferryx, workspaces, panes, remote access and agents"],
  ["Technical architecture", `${SITE}/docs/architecture/`, "the daemon, the desktop app and how ferryx-cli reaches them"],
];
export const AGENT_LINKS = [
  ["Ferryx agent skill", SKILL_URL, "the skill Ferryx installs into coding agents: supervising panes, integrations and status reporting"],
  ["Agent support matrix", `${SITE}/blog/agent-support-matrix/`, "which agents get status detection, hooks, resume and remote history"],
  ["Running coding agents in parallel", `${SITE}/use-cases/parallel-ai-agents/`, "one agent per pane and worktree"],
  ["When an agent looks stuck", `${SITE}/blog/agent-appears-stuck/`, "diagnosing a pane whose agent seems frozen"],
];
export const REMOTE_LINKS = [
  ["Pair Ferryx with your phone", `${SITE}/blog/pair-ferryx-with-phone/`, "reach your panes from a phone or another browser"],
  ["Pairing troubleshooting", `${SITE}/blog/pairing-troubleshooting/`, "when pairing a device fails"],
  ["Remote terminal access", `${SITE}/use-cases/remote-terminal-access/`, "terminals on other machines through the relay or SSH"],
  ["Self-hosted relay", `${SITE}/docs/self-hosted-relay/`, "running your own relay"],
  ["Security and operations", `${SITE}/docs/security/`, "the threat model, pairing keys and what the relay can see"],
];

/** The group names in `CLI_GROUPS` of src-tauri/src/cli/api_catalog.rs. */
export function readCliGroups(source = readFileSync(API_CATALOG_SOURCE, "utf8")) {
  const block = /pub const CLI_GROUPS: &\[&str\] = &\[([\s\S]*?)\];/.exec(source);
  if (!block) throw new Error("CLI_GROUPS not found in src-tauri/src/cli/api_catalog.rs");
  const groups = [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  if (groups.length === 0) throw new Error("CLI_GROUPS is empty");
  return groups;
}

function isString(value) {
  return typeof value === "string";
}

/** Rejects a catalogue that is not the shape `api describe` prints, or that lacks a CLI group. */
export function validateCatalog(commands, groups = readCliGroups()) {
  if (!Array.isArray(commands) || commands.length === 0) throw new Error("catalogue has no commands");
  for (const command of commands) {
    const ok =
      command &&
      isString(command.name) &&
      isString(command.summary) &&
      isString(command.output) &&
      Array.isArray(command.subcommands) &&
      command.subcommands.every((sub) => sub && isString(sub.name));
    if (!ok) throw new Error(`malformed catalogue entry: ${JSON.stringify(command)}`);
  }
  const names = commands.map((command) => command.name);
  const missing = groups.filter((group) => !names.includes(group));
  if (missing.length > 0) {
    throw new Error(`catalogue lacks CLI_GROUPS entries ${missing.join(", ")} (stale ferryx-cli binary?)`);
  }
  return commands;
}

function linkLine([title, url, note]) {
  return `- [${title}](${url}): ${note}`;
}

function commandLine(command) {
  const verbs = command.subcommands.map((sub) => sub.name).filter((name) => name !== "");
  const verbText = verbs.length > 0 ? ` Verbs: ${verbs.map((verb) => `\`${verb}\``).join(", ")}.` : "";
  return `- \`ferryx-cli ${command.name}\`: ${command.summary}.${verbText} Output: ${command.output}.`;
}

/** The llms.txt text for a catalogue of `api describe` entries, in catalogue order. */
export function render(commands) {
  const lines = [
    "# Ferryx",
    "",
    "> Ferryx is a terminal workspace that runs coding agents in panes on this machine and on paired or SSH hosts. `ferryx-cli` talks to the running Ferryx daemon; `ferryx-cli api list` and `ferryx-cli api describe <command>` print the command catalogue below as JSON.",
    "",
    "This file is generated by `bun scripts/gen-llms-txt.mjs` from `ferryx-cli api list --json`; do not edit it by hand. Run `ferryx-cli api describe <command>` for the arguments, flags, exit codes and output of one command instead of parsing usage text.",
    "",
    "## CLI",
    "",
    ...CLI_LINKS.map(linkLine),
    ...commands.map(commandLine),
    "",
    "## Agents",
    "",
    ...AGENT_LINKS.map(linkLine),
    "",
    "## Remote",
    "",
    ...REMOTE_LINKS.map(linkLine),
  ];
  return `${lines.join("\n")}\n`;
}

function runJson(argv) {
  const [program, ...args] = argv;
  const result = spawnSync(program, args, { cwd: REPO_ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (result.error) throw new Error(`${argv.join(" ")}: ${result.error.message}`);
  if (result.status !== 0) {
    throw new Error(`${argv.join(" ")} exited ${result.status}: ${result.stderr.trim()}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`${argv.join(" ")} printed invalid JSON: ${error.message}`);
  }
}

/** Builds ferryx-cli (headless) with cargo and returns the executable cargo reports. */
export function buildCli() {
  const result = spawnSync(
    "cargo",
    [
      "build",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--no-default-features",
      "--bin",
      "ferryx-cli",
      "--message-format=json-render-diagnostics",
    ],
    { cwd: REPO_ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 256 * 1024 * 1024 },
  );
  if (result.error) throw new Error(`cargo build: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`cargo build of ferryx-cli exited ${result.status}`);
  for (const line of result.stdout.split("\n")) {
    if (!line.startsWith("{")) continue;
    const message = JSON.parse(line);
    if (message.reason === "compiler-artifact" && message.target?.name === "ferryx-cli" && message.executable) {
      return message.executable;
    }
  }
  throw new Error("cargo build did not report a ferryx-cli executable");
}

/** Every `api describe` entry, in `api list` order. */
export function catalogFromCli(cli) {
  const list = runJson([cli, "api", "list", "--json"]);
  if (!Array.isArray(list?.commands)) throw new Error(`${cli} api list --json: no commands array`);
  return list.commands.map((command) => runJson([cli, "api", "describe", String(command?.name), "--json"]));
}

export function catalogFromFile(path) {
  return JSON.parse(readFileSync(path, "utf8"))?.commands;
}

/** `null` when `actual` equals `expected`, else the DRIFT message naming the first differing line. */
export function drift(expected, actual, label) {
  if (actual === expected) return null;
  if (actual === null) return `DRIFT: ${label} is missing; run bun scripts/gen-llms-txt.mjs`;
  const want = expected.split("\n");
  const have = actual.split("\n");
  let line = 0;
  while (line < want.length && want[line] === have[line]) line += 1;
  return `DRIFT: ${label} differs from the generated catalogue at line ${line + 1}; run bun scripts/gen-llms-txt.mjs`;
}

function readOrNull(path) {
  try {
    return readFileSync(path, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

export function parseArgs(argv, env = process.env) {
  const options = { check: false, publish: false, cli: env.FERRYX_CLI || null, catalog: null, out: LLMS_TXT };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--check") options.check = true;
    else if (arg === "--publish") options.publish = true;
    else if (arg === "--cli" || arg === "--catalog" || arg === "--out") {
      const value = argv[index + 1];
      if (!value) throw new Error(`${arg} needs a value`);
      options[arg.slice(2)] = resolve(value);
      index += 1;
    } else throw new Error(`unknown argument ${arg}`);
  }
  return options;
}

export function main(argv) {
  const options = parseArgs(argv);
  if (options.publish) {
    mkdirSync(dirname(SITE_COPY), { recursive: true });
    copyFileSync(options.out, SITE_COPY);
    console.log(`gen-llms-txt: copied ${relative(REPO_ROOT, options.out)} to ${relative(REPO_ROOT, SITE_COPY)}`);
    return 0;
  }
  const commands = validateCatalog(
    options.catalog ? catalogFromFile(options.catalog) : catalogFromCli(options.cli ?? buildCli()),
  );
  const text = render(commands);
  const label = relative(REPO_ROOT, options.out) || options.out;
  if (options.check) {
    const message = drift(text, readOrNull(options.out), label);
    if (message) {
      console.error(message);
      return 1;
    }
    console.log(`gen-llms-txt: ${label} is up to date (${commands.length} commands)`);
    return 0;
  }
  mkdirSync(dirname(options.out), { recursive: true });
  writeFileSync(options.out, text);
  console.log(`gen-llms-txt: wrote ${commands.length} commands to ${label}`);
  return 0;
}

if (import.meta.main ?? process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (error) {
    console.error(`gen-llms-txt: ${error.message}`);
    process.exit(1);
  }
}
