/**
 * Published exited-agent tab. Placeholders are the only variable fields.
 * An empty substitution is omitted, so a standby or missing id is not a sessionId.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const PLACEHOLDER = /\{\{([A-Za-z0-9_]+)\}\}/g;

export function loadExitedAgentTabFixture() {
  const fixturePath = join(dirname(fileURLToPath(import.meta.url)), "exited-agent-tab.json");
  return JSON.parse(readFileSync(fixturePath, "utf8"));
}

export function fillExitedAgentTab(values) {
  const filled = substitute(loadExitedAgentTabFixture(), values ?? {});
  const cleaned = dropEmpty(filled);
  if (cleaned.providerSession && !cleaned.providerSession.id) delete cleaned.providerSession;
  return cleaned;
}

function substitute(value, values) {
  if (typeof value === "string") {
    return value.replace(PLACEHOLDER, (_, key) => {
      const replacement = values[key];
      return typeof replacement === "string" ? replacement : "";
    });
  }
  if (Array.isArray(value)) return value.map((item) => substitute(item, values));
  if (value && typeof value === "object") {
    const next = {};
    for (const [key, child] of Object.entries(value)) next[key] = substitute(child, values);
    return next;
  }
  return value;
}

function dropEmpty(value) {
  if (Array.isArray(value)) return value.map(dropEmpty);
  if (!value || typeof value !== "object") return value;
  const next = {};
  for (const [key, child] of Object.entries(value)) {
    if (child === "" || child == null) continue;
    const cleaned = dropEmpty(child);
    if (cleaned && typeof cleaned === "object" && !Array.isArray(cleaned) && Object.keys(cleaned).length === 0) continue;
    next[key] = cleaned;
  }
  return next;
}
