#!/usr/bin/env bun
/**
 * scripts/release-workflow-policy.mjs
 *
 * Source-enforced release policy regression guard for GitHub Actions workflows.
 * Parses workflow YAML using Bun.YAML and rejects hosted release producer
 * workflows (build, sign, publish steps, credential references, contents-write),
 * while permitting existing PR check builds and Pages deployment permissions.
 */

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_WORKFLOWS_DIR = join(REPO_ROOT, ".github/workflows");

const FORBIDDEN_SECRETS = [
  "TAURI_SIGNING_PRIVATE_KEY",
  "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
  "APPLE_CERTIFICATE",
  "APPLE_CERTIFICATE_PASSWORD",
  "APPLE_API_KEY",
  "APPLE_API_ISSUER",
  "APPLE_API_KEY_CONTENT",
  "KEYCHAIN_PASSWORD",
  "APPLE_SIGNING_IDENTITY",
];

const FORBIDDEN_BUILD_PATTERNS = [
  { pattern: /\b(?:bunx\s+|bun\s+(?:run\s+)?)?(?:@tauri-apps\/cli|cargo\s+tauri|tauri)\s+build\b/, name: "tauri build", reason: "Tauri release bundle build" },
  { pattern: /\bbuild-msix\.ps1\b/, name: "build-msix.ps1", reason: "Windows MSIX packaging script" },
  { pattern: /\bsecurity\s+import\b/, name: "security import", reason: "macOS codesigning certificate import" },
  { pattern: /\bsecurity\s+create-keychain\b/, name: "security create-keychain", reason: "macOS signing keychain creation" },
  { pattern: /\bcodesign\b/, name: "codesign", reason: "macOS binary codesigning" },
  { pattern: /\bnotarytool\b/, name: "notarytool", reason: "Apple notarization tool" },
  { pattern: /\bstapler\b/, name: "stapler", reason: "Apple notarization stapler" },
  { pattern: /\bassert-updater-archive-layout\.mjs\b/, name: "assert-updater-archive-layout.mjs", reason: "Release updater archive layout validation" },
  { pattern: /\brelease-local\.mjs\b/, name: "release-local.mjs", reason: "Local release coordinator is forbidden in hosted workflows" },
];

const FORBIDDEN_ACTIONS = [
  { pattern: /tauri-apps\/tauri-action/i, name: "tauri-action", reason: "Tauri release action (tauri-apps/tauri-action)" },
  { pattern: /softprops\/action-gh-release/i, name: "action-gh-release", reason: "GitHub release publication action (softprops/action-gh-release)" },
  { pattern: /actions\/create-release/i, name: "create-release", reason: "GitHub release creation action (actions/create-release)" },
  { pattern: /ncipollo\/release-action/i, name: "release-action", reason: "GitHub release publication action (ncipollo/release-action)" },
];

const FORBIDDEN_PUBLISH_COMMANDS = [
  { pattern: /\bgh\s+release\s+(?:create|upload|edit)\b/, name: "gh release", reason: "GitHub CLI release command" },
  { pattern: /\bbuild-latest-json\.mjs\b/, name: "build-latest-json.mjs", reason: "Release updater manifest assembly (build-latest-json.mjs)" },
];

function isScopedFrontendBuild(runCmd, step, job) {
  if (/--cwd\s+(?:ui|site)\b/.test(runCmd)) return true;
  const stepWd = step && (step["working-directory"] || step.workingDirectory);
  if (typeof stepWd === "string" && /(?:ui|site)/.test(stepWd)) return true;
  const jobWd = job?.defaults?.run?.["working-directory"];
  if (typeof jobWd === "string" && /(?:ui|site)/.test(jobWd)) return true;
  return false;
}

export function validateWorkflow(workflow, filePath = "<inline>") {
  const violations = [];
  const fileName = basename(filePath);

  if (fileName.toLowerCase() === "release.yml" || fileName.toLowerCase() === "release.yaml") {
    violations.push(`File name "${fileName}" is forbidden: hosted release producer workflows are retired`);
  }

  if (!workflow || typeof workflow !== "object") {
    violations.push("Invalid workflow document: root must be a YAML mapping");
    return { valid: violations.length === 0, violations };
  }

  const onTrigger = workflow.on ?? workflow["on"];
  if (onTrigger && typeof onTrigger === "object" && onTrigger.push?.tags) {
    violations.push("Trigger 'push.tags' is forbidden: hosted tag-triggered release workflows are retired");
  }

  if (workflow.permissions) {
    checkPermissions(workflow.permissions, "workflow-level permissions", violations);
  }

  const jobs = workflow.jobs;
  if (jobs && typeof jobs === "object") {
    for (const [jobId, job] of Object.entries(jobs)) {
      if (!job || typeof job !== "object") continue;

      if (job.permissions) {
        checkPermissions(job.permissions, `job "${jobId}" permissions`, violations);
      }

      // Check reusable workflow call on job level
      if (typeof job.uses === "string") {
        if (/release/i.test(job.uses) || /tauri-action/i.test(job.uses)) {
          violations.push(`Job "${jobId}" uses forbidden release workflow: ${job.uses}`);
        }
      }
      if (job.secrets === "inherit" && (/release/i.test(jobId) || /release/i.test(job.uses || ""))) {
        violations.push(`Job "${jobId}" reintroduces release producer via secrets: inherit`);
      }

      const steps = job.steps;
      if (Array.isArray(steps)) {
        for (let i = 0; i < steps.length; i++) {
          const step = steps[i];
          if (!step || typeof step !== "object") continue;
          const stepLabel = step.name ? `step "${step.name}"` : `step #${i + 1} in job "${jobId}"`;

          if (typeof step.uses === "string") {
            for (const { pattern, name, reason } of FORBIDDEN_ACTIONS) {
              if (pattern.test(step.uses)) {
                violations.push(`${stepLabel} uses forbidden action ${name} (${reason}): ${step.uses}`);
              }
            }
          }

          if (typeof step.run === "string") {
            for (const { pattern, name, reason } of FORBIDDEN_BUILD_PATTERNS) {
              if (pattern.test(step.run)) {
                violations.push(`${stepLabel} contains forbidden release build command ${name} (${reason})`);
              }
            }
            for (const { pattern, name, reason } of FORBIDDEN_PUBLISH_COMMANDS) {
              if (pattern.test(step.run)) {
                violations.push(`${stepLabel} contains forbidden release publish command ${name} (${reason})`);
              }
            }
            if (/\b(?:bun|npm|pnpm|yarn)\s+(?:run\s+)?build\b/.test(step.run)) {
              if (!isScopedFrontendBuild(step.run, step, job)) {
                violations.push(`${stepLabel} contains forbidden root build delegation (package.json build invokes cargo tauri build)`);
              }
            }
          }
        }
      }
    }
  }

  // Recursive scan covering all env vars, step/job settings, and mapping keys
  scanAllNodesForSecrets(workflow, violations);

  return {
    valid: violations.length === 0,
    violations: Array.from(new Set(violations)),
  };
}

function checkPermissions(permissions, context, violations) {
  if (typeof permissions === "string" && permissions === "write-all") {
    violations.push(`Disallowed permission in ${context}: write-all`);
  } else if (typeof permissions === "object" && permissions !== null && permissions.contents === "write") {
    violations.push(`Disallowed permission in ${context}: contents: write`);
  }
}

function scanAllNodesForSecrets(node, violations) {
  if (typeof node === "string") {
    for (const secret of FORBIDDEN_SECRETS) {
      if (node.includes(secret)) {
        violations.push(`Disallowed release secret reference: ${secret}`);
      }
    }
  } else if (Array.isArray(node)) {
    for (const item of node) {
      scanAllNodesForSecrets(item, violations);
    }
  } else if (node !== null && typeof node === "object") {
    for (const [key, val] of Object.entries(node)) {
      for (const secret of FORBIDDEN_SECRETS) {
        if (key.includes(secret)) {
          violations.push(`Disallowed release secret reference: ${secret}`);
        }
      }
      scanAllNodesForSecrets(val, violations);
    }
  }
}

export function validateWorkflowYaml(yamlContent, filePath = "<inline>") {
  let parsed;
  try {
    parsed = Bun.YAML.parse(yamlContent);
  } catch (err) {
    return {
      valid: false,
      violations: [`YAML parsing error in ${filePath}: ${err.message}`],
    };
  }
  return validateWorkflow(parsed, filePath);
}

export function validateWorkflowFile(filePath) {
  const resolved = resolve(filePath);
  if (!existsSync(resolved)) {
    return { filePath, valid: false, violations: [`Workflow file not found: ${filePath}`] };
  }
  return { filePath, ...validateWorkflowYaml(readFileSync(resolved, "utf8"), resolved) };
}

export function validateWorkflowsDir(dirPath) {
  const resolved = resolve(dirPath);
  if (!existsSync(resolved) || !statSync(resolved).isDirectory()) {
    return { dirPath, valid: false, violations: [`Workflows directory not found: ${dirPath}`], results: [] };
  }

  const entries = readdirSync(resolved);
  const results = [];
  const allViolations = [];

  for (const entry of entries) {
    if (entry.toLowerCase() === "release.yml" || entry.toLowerCase() === "release.yaml") {
      allViolations.push(`Hosted release producer workflow "${entry}" is forbidden in ${dirPath}; releases are source-enforced local-only`);
    }
    if (entry.endsWith(".yml") || entry.endsWith(".yaml")) {
      const res = validateWorkflowFile(join(resolved, entry));
      results.push(res);
      if (!res.valid) {
        for (const v of res.violations) allViolations.push(`[${entry}] ${v}`);
      }
    }
  }

  return {
    dirPath,
    valid: allViolations.length === 0,
    violations: Array.from(new Set(allViolations)),
    results,
  };
}

export function runCli(argv = process.argv.slice(2)) {
  const jsonMode = argv.includes("--json");
  const targets = argv.filter(arg => arg !== "--json");
  if (targets.length === 0) targets.push(DEFAULT_WORKFLOWS_DIR);

  let overallValid = true;
  const allViolations = [];
  const report = [];

  for (const target of targets) {
    const resolved = resolve(target);
    if (!existsSync(resolved)) {
      overallValid = false;
      const v = `Target not found: ${target}`;
      allViolations.push(v);
      report.push({ target, valid: false, violations: [v] });
      continue;
    }

    const res = statSync(resolved).isDirectory() ? validateWorkflowsDir(resolved) : validateWorkflowFile(resolved);
    if (!res.valid) {
      overallValid = false;
      allViolations.push(...res.violations);
    }
    report.push(res);
  }

  if (jsonMode) {
    console.log(JSON.stringify({ valid: overallValid, violations: allViolations, report }, null, 2));
  } else if (overallValid) {
    console.log(`[release-workflow-policy] PASS: All evaluated workflows comply with local-release-only policy.`);
    for (const item of report) {
      console.log(`  - ${basename(item.dirPath || item.filePath)}: valid`);
    }
  } else {
    console.error(`[release-workflow-policy] FAIL: Release policy violations detected!`);
    for (const v of allViolations) console.error(`  - ${v}`);
  }

  return overallValid ? 0 : 1;
}

if (import.meta.main) {
  process.exit(runCli());
}
