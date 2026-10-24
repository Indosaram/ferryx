#!/usr/bin/env bun
// Generates site/src/data/agents.json from the shipped status-detection manifests
// (src-tauri/src/agent_detect/manifests/*.toml), then refreshes every generated region in the
// site's markdown content. The manifests are the single source of truth: the site must never
// hand-write the agent list or the agent count.
//
// Generated markdown regions are delimited by HTML comments:
//   <!-- agents:count -->16<!-- /agents:count -->   number of agents with shipped detection
//   <!-- agents:list -->...<!-- /agents:list -->    "A, B, and C" in display order
//   <!-- agents:matrix -->...<!-- /agents:matrix --> per-agent capability table
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const MANIFEST_DIR = join(REPO_ROOT, "src-tauri/src/agent_detect/manifests");
export const OUTPUT = join(REPO_ROOT, "site/src/data/agents.json");
export const CONTENT_DIR = join(REPO_ROOT, "site/src/content");

// Capabilities the manifests cannot express, keyed by manifest id. Sources:
//   resume       ui/src/lib/agentResume.ts RESUME_BUILDERS
//   hooks        src-tauri/src/ferryx_scope/ssh/agent_hook_core.rs HOOK_AGENTS
//                (asserted by src-tauri/src/cli/agent_hook.rs)
//   history      src-tauri/src/remote/reference_chat/history_*.rs and
//                ReferenceNativeHistoryKind::from_registry_id (codex reads rollouts in history.rs)
//   managedChat  src-tauri/src/ferryx_scope/chat/mod.rs (the supervisor binds Codex threads only)
//   detectNote   the manifest's own header comment
// Every shipped manifest must have an entry, so a new manifest fails here until its row is filled.
export const CAPABILITIES = {
  aider: { resume: false, hooks: false, history: false, managedChat: false },
  antigravity: { resume: true, hooks: true, history: true, managedChat: false },
  claude: { resume: true, hooks: true, history: true, managedChat: false },
  cline: { resume: true, hooks: true, history: true, managedChat: false },
  codex: { resume: true, hooks: true, history: true, managedChat: true },
  copilot: { resume: true, hooks: false, history: true, managedChat: false },
  crush: { resume: true, hooks: true, history: true, managedChat: false },
  cursor: { resume: true, hooks: true, history: true, managedChat: false },
  droid: { resume: true, hooks: false, history: false, managedChat: false, detectNote: "blocked states only" },
  gjc: { resume: true, hooks: false, history: true, managedChat: false },
  grok: { resume: true, hooks: true, history: true, managedChat: false },
  kimi: { resume: true, hooks: false, history: true, managedChat: false },
  omo: { resume: true, hooks: false, history: true, managedChat: false },
  omp: { resume: true, hooks: false, history: true, managedChat: false },
  opencode: { resume: true, hooks: false, history: true, managedChat: false },
  pi: { resume: true, hooks: false, history: true, managedChat: false },
};

// Top-level string keys only: everything before the first [table] / [[array]] header.
function topLevelStrings(source) {
  const keys = {};
  for (const line of source.split("\n")) {
    if (/^\s*\[/.test(line)) break;
    const match = /^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*"((?:[^"\\]|\\.)*)"\s*(?:#.*)?$/.exec(line);
    if (match) keys[match[1]] = JSON.parse(`"${match[2]}"`);
  }
  return keys;
}

export function readAgents(manifestDir = MANIFEST_DIR) {
  const files = readdirSync(manifestDir).filter((file) => file.endsWith(".toml")).sort();
  if (files.length === 0) throw new Error(`no manifests found in ${manifestDir}`);
  const agents = files.map((file) => {
    const path = join(manifestDir, file);
    const keys = topLevelStrings(readFileSync(path, "utf8"));
    for (const key of ["id", "display_name", "homepage"]) {
      if (!keys[key]?.trim()) throw new Error(`${path}: missing top-level ${key}`);
    }
    if (!/^https:\/\//.test(keys.homepage)) throw new Error(`${path}: homepage must be an https URL`);
    return { id: keys.id, displayName: keys.display_name, homepage: keys.homepage };
  });
  agents.sort((a, b) => a.displayName.localeCompare(b.displayName, "en", { sensitivity: "base" }));
  return agents;
}

export function formatList(names) {
  if (names.length < 3) return names.join(" and ");
  return `${names.slice(0, -1).join(", ")}, and ${names.at(-1)}`;
}

export function renderMatrix(agents) {
  const mark = (value) => (value ? "Yes" : "No");
  const rows = agents.map((agent) => {
    const caps = CAPABILITIES[agent.id];
    if (!caps) throw new Error(`no CAPABILITIES entry for manifest id "${agent.id}" in scripts/gen-site-agents.mjs`);
    const detect = caps.detectNote ? `Yes (${caps.detectNote})` : "Yes";
    return `| [${agent.displayName}](${agent.homepage}) | ${detect} | ${mark(caps.resume)} | ${mark(caps.hooks)} | ${mark(caps.history)} | ${mark(caps.managedChat)} |`;
  });
  return [
    "| Agent | Status detection | Resume | Hooks | Remote history | Managed chat |",
    "| --- | --- | --- | --- | --- | --- |",
    ...rows,
  ].join("\n");
}

const REGION = /<!-- agents:(count|list|matrix) -->[\s\S]*?<!-- \/agents:\1 -->/g;

export function fillRegions(text, agents) {
  return text.replace(REGION, (_whole, kind) => {
    const body =
      kind === "count"
        ? String(agents.length)
        : kind === "list"
          ? formatList(agents.map((agent) => agent.displayName))
          : `\n\n${renderMatrix(agents)}\n\n`;
    return `<!-- agents:${kind} -->${body}<!-- /agents:${kind} -->`;
  });
}

function markdownFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return markdownFiles(path);
    return /\.mdx?$/.test(entry.name) ? [path] : [];
  });
}

export function generate({ manifestDir = MANIFEST_DIR, output = OUTPUT, contentDir = CONTENT_DIR } = {}) {
  const agents = readAgents(manifestDir);
  for (const agent of agents) {
    if (!CAPABILITIES[agent.id]) {
      throw new Error(`no CAPABILITIES entry for manifest id "${agent.id}" in scripts/gen-site-agents.mjs`);
    }
  }
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(agents, null, 2)}\n`);
  const updated = [];
  for (const path of markdownFiles(contentDir)) {
    const before = readFileSync(path, "utf8");
    const after = fillRegions(before, agents);
    if (after !== before) {
      writeFileSync(path, after);
      updated.push(path);
    }
  }
  return { agents, updated };
}

if (import.meta.main ?? process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const { agents, updated } = generate();
    console.log(`gen-site-agents: wrote ${agents.length} agents to ${relative(REPO_ROOT, OUTPUT)}`);
    for (const path of updated) console.log(`gen-site-agents: refreshed ${relative(REPO_ROOT, path)}`);
  } catch (error) {
    console.error(`gen-site-agents: ${error.message}`);
    process.exit(1);
  }
}
