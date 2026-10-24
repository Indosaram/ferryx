import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

/**
 * Discovers the previous git release tag relative to the given current tag or commit.
 *
 * @param {string} currentTag - The current release tag (e.g. "v2026.09.17.1")
 * @param {Function} [execFn] - Optional command executor for testing
 * @returns {string | null}
 */
export function findPreviousReleaseTag(currentTag, execFn = null) {
  try {
    if (execFn) {
      return execFn(currentTag);
    }
    const stdout = execFileSync("git", ["describe", "--tags", "--abbrev=0", `${currentTag}^`], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    const tag = stdout.trim();
    return tag.length > 0 ? tag : null;
  } catch {
    // If git describe fails (e.g. shallow clone or first tag), fall back to tag listing
    try {
      const allTagsOut = execFileSync("git", ["tag", "--sort=-v:refname"], {
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      });
      const tags = allTagsOut
        .split("\n")
        .map((t) => t.trim())
        .filter((t) => t.startsWith("v"));
      const currentIndex = tags.indexOf(currentTag);
      if (currentIndex >= 0 && currentIndex + 1 < tags.length) {
        return tags[currentIndex + 1];
      }
      if (tags.length > 0 && tags[0] !== currentTag) {
        return tags[0];
      }
      return null;
    } catch {
      return null;
    }
  }
}

/**
 * Parses conventional commit lines (%h\t%s) into categorized buckets.
 *
 * @param {string} gitLogText
 * @returns {Record<string, Array<{ scope: string | null, subject: string, hash: string }>>}
 */
export function parseConventionalCommits(gitLogText) {
  const categories = {
    feat: [],
    fix: [],
    perf: [],
    refactor: [],
    docs: [],
    chore: [],
    other: [],
  };

  if (!gitLogText || typeof gitLogText !== "string") {
    return categories;
  }

  const lines = gitLogText.split("\n").map((l) => l.trim()).filter(Boolean);

  for (const line of lines) {
    const tabIndex = line.indexOf("\t");
    let hash = "";
    let rawSubject = "";

    if (tabIndex !== -1) {
      hash = line.slice(0, tabIndex).trim();
      rawSubject = line.slice(tabIndex + 1).trim();
    } else {
      const spaceIndex = line.indexOf(" ");
      if (spaceIndex !== -1) {
        hash = line.slice(0, spaceIndex).trim();
        rawSubject = line.slice(spaceIndex + 1).trim();
      } else {
        continue;
      }
    }

    // Use only the first line of subject and strip literal escaped newlines
    const subject = rawSubject.split(/\\n|\n/)[0].trim();
    if (!subject) continue;

    const match = subject.match(/^([a-zA-Z0-9_-]+)(?:\(([^)]+)\))?!?:(?:\s+)(.+)$/);
    if (match) {
      const [, type, scope, desc] = match;
      const key = type.toLowerCase();
      const item = {
        scope: scope ? scope.trim() : null,
        subject: desc.trim(),
        hash: hash.slice(0, 8),
      };

      if (key === "feat" || key === "feature") {
        categories.feat.push(item);
      } else if (key === "fix" || key === "bugfix") {
        categories.fix.push(item);
      } else if (key === "perf" || key === "performance") {
        categories.perf.push(item);
      } else if (key === "refactor") {
        categories.refactor.push(item);
      } else if (key === "docs" || key === "doc") {
        categories.docs.push(item);
      } else if (key === "chore" || key === "test" || key === "ci" || key === "build") {
        categories.chore.push(item);
      } else {
        categories.other.push(item);
      }
    } else {
      categories.other.push({
        scope: null,
        subject,
        hash: hash.slice(0, 8),
      });
    }
  }

  return categories;
}

/**
 * Attempts to fetch GitHub-generated release notes (which include merged PRs).
 *
 * @param {object} options
 * @param {string} options.repo
 * @param {string} options.tag
 * @param {string | null} options.prevTag
 * @param {string} [options.ghCommand]
 * @param {Function} [options.execGhFn]
 * @returns {string | null}
 */
export function fetchGitHubReleaseNotes({
  repo,
  tag,
  prevTag = null,
  ghCommand = "gh",
  execGhFn = null,
}) {
  try {
    if (execGhFn) {
      return execGhFn({ repo, tag, prevTag });
    }

    const args = [
      "api",
      "-X",
      "POST",
      `/repos/${repo}/releases/generate-notes`,
      "-f",
      `tag_name=${tag}`,
    ];
    if (prevTag) {
      args.push("-f", `previous_tag_name=${prevTag}`);
    }

    const res = spawnSync(ghCommand, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    if (res.status === 0 && res.stdout) {
      const parsed = JSON.parse(res.stdout);
      return typeof parsed.body === "string" ? parsed.body.trim() : null;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Generates structured markdown release notes for a Ferryx release.
 * Integrates both GitHub PR release notes (when PRs exist) and Conventional Commits.
 *
 * @param {object} options
 * @param {string} options.repo - GitHub repository (e.g. "Indosaram/ferryx")
 * @param {string} options.tag - Release tag (e.g. "v2026.09.17.1")
 * @param {string} [options.commitSha] - Tag commit SHA
 * @param {string | null} [options.prevTag] - Override previous tag
 * @param {string} [options.ghCommand] - Path to gh CLI
 * @param {Function} [options.execGitFn] - Git log executor override for testing
 * @param {Function} [options.execGhFn] - GitHub API executor override for testing
 * @returns {string}
 */
export function generateChangelog({
  repo,
  tag,
  commitSha = "HEAD",
  prevTag = null,
  ghCommand = "gh",
  execGitFn = null,
  execGhFn = null,
}) {
  const resolvedPrevTag = prevTag ?? findPreviousReleaseTag(tag);

  // 1. Check if GitHub has PR release notes
  const ghNotes = fetchGitHubReleaseNotes({
    repo,
    tag,
    prevTag: resolvedPrevTag,
    ghCommand,
    execGhFn,
  });

  // Check if GitHub returned actual PR contents (not just full changelog link)
  let prSection = "";
  if (ghNotes) {
    const cleaned = ghNotes.trim();
    // GitHub typically puts "## What's Changed" or bullet points
    if (cleaned.includes("* ") && cleaned.includes(" in https://github.com")) {
      prSection = cleaned;
    }
  }

  // 2. Fetch git log commits
  let gitLogText = "";
  if (execGitFn) {
    gitLogText = execGitFn({ tag, prevTag: resolvedPrevTag, commitSha });
  } else {
    try {
      const range = resolvedPrevTag ? `${resolvedPrevTag}..${commitSha}` : commitSha;
      gitLogText = execFileSync(
        "git",
        ["log", range, "--no-merges", "--pretty=format:%h\t%s"],
        { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
      );
    } catch {
      gitLogText = "";
    }
  }

  const categories = parseConventionalCommits(gitLogText);

  // 3. Assemble Release Notes Markdown
  const sections = [];
  sections.push(`# Ferryx release ${tag}`);

  if (prSection) {
    sections.push(prSection);
  }

  const categoryTitles = [
    { key: "feat", title: "Features" },
    { key: "fix", title: "Bug Fixes" },
    { key: "perf", title: "Performance" },
    { key: "refactor", title: "Refactoring" },
    { key: "docs", title: "Documentation" },
    { key: "chore", title: "Maintenance & Internal" },
    { key: "other", title: "Other Changes" },
  ];

  for (const { key, title } of categoryTitles) {
    const items = categories[key];
    if (items && items.length > 0) {
      const lines = [`## ${title}`, ""];
      for (const item of items) {
        const entry = item.scope
          ? `- **${item.scope}**: ${item.subject} (${item.hash})`
          : `- ${item.subject} (${item.hash})`;
        lines.push(entry);
      }
      sections.push(lines.join("\n"));
    }
  }

  if (resolvedPrevTag && (!prSection || !prSection.includes("Full Changelog"))) {
    sections.push(
      `**Full Changelog**: https://github.com/${repo}/compare/${resolvedPrevTag}...${tag}`,
    );
  }

  return sections.join("\n\n") + "\n";
}

// Current vYYYY.MMDD.N tags plus the legacy vYYYY.MM.DD.N form (scripts/release-tag-policy.mjs).
/**
 * Appends the session survival report (scripts/lib/release-survival.mjs) to release notes, so
 * the GitHub release body carries the scenario x platform table and any owner waiver. Any
 * previously appended survival section is replaced.
 *
 * @param {string} notes - Release notes markdown
 * @param {string} report - survival-report.md content (starts with "## Session survival")
 * @returns {string}
 */
export function appendSurvivalReport(notes, report) {
  const marker = "## Session survival";
  const start = notes.indexOf(marker);
  const base = (start >= 0 ? notes.slice(0, start) : notes).replace(/\s+$/, "");
  return `${base}\n\n${report.trim()}\n`;
}

const RELEASE_TAG_PATTERN = /^v\d{4}\.(?:\d{4}|\d{2}\.\d{2})\.\d+$/;

export const CHANGELOG_HEADER = [
  "# Changelog",
  "",
  "All notable changes to Ferryx, one section per date-based release tag (newest first).",
  "Generated from Conventional Commit subjects by",
  "`bun scripts/lib/release-changelog.mjs --write-changelog`; do not edit sections by hand.",
  "",
].join("\n");

function runGit(args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
}

/**
 * Lists date-based release tags (vYYYY.MMDD.N and legacy vYYYY.MM.DD.N), newest first.
 *
 * @param {Function} [execFn] - Returns raw `git tag` output; for testing
 * @returns {string[]}
 */
export function listReleaseTags(execFn = null) {
  const raw = execFn ? execFn() : runGit(["tag", "--list", "v*", "--sort=-v:refname"]);
  return raw
    .split("\n")
    .map((t) => t.trim())
    .filter((t) => RELEASE_TAG_PATTERN.test(t));
}

/**
 * Renders one CHANGELOG.md section for a release tag from its commit log.
 *
 * @param {object} options
 * @param {string} options.repo
 * @param {string} options.tag
 * @param {string | null} options.prevTag - Older tag; null for the first release
 * @param {Function} [options.execGitFn] - Returns `%h\t%s` log text for { tag, prevTag }
 * @returns {string}
 */
export function renderChangelogSection({ repo, tag, prevTag, execGitFn = null }) {
  const gitLogText = execGitFn
    ? execGitFn({ tag, prevTag })
    : runGit(["log", prevTag ? `${prevTag}..${tag}` : tag, "--no-merges", "--pretty=format:%h\t%s"]);
  const categories = parseConventionalCommits(gitLogText);
  const lines = [`## ${tag}`, ""];
  const titles = [
    ["feat", "Features"],
    ["fix", "Bug Fixes"],
    ["perf", "Performance"],
    ["refactor", "Refactoring"],
    ["docs", "Documentation"],
    ["chore", "Maintenance & Internal"],
    ["other", "Other Changes"],
  ];
  let any = false;
  for (const [key, title] of titles) {
    const items = categories[key];
    if (!items.length) continue;
    any = true;
    lines.push(`### ${title}`, "");
    for (const item of items) {
      lines.push(item.scope ? `- **${item.scope}**: ${item.subject} (${item.hash})` : `- ${item.subject} (${item.hash})`);
    }
    lines.push("");
  }
  if (!any) lines.push("No commits.", "");
  if (prevTag) lines.push(`**Full Changelog**: https://github.com/${repo}/compare/${prevTag}...${tag}`, "");
  return lines.join("\n");
}

/**
 * Builds CHANGELOG.md content: sections for release tags missing from `existing` are
 * prepended above the sections already present, newest first.
 *
 * @param {object} options
 * @param {string} options.repo
 * @param {string[]} options.allTags - Every release tag, newest first (for previous-tag lookup)
 * @param {string[]} [options.tags] - Tags to write (subset of allTags); defaults to allTags
 * @param {string} [options.existing] - Current CHANGELOG.md content
 * @param {Function} [options.execGitFn]
 * @returns {{ content: string, added: string[] }}
 */
export function buildChangelogFile({ repo, allTags, tags = allTags, existing = "", execGitFn = null }) {
  const present = new Set(
    [...existing.matchAll(/^## (v\S+)\s*$/gm)].map((m) => m[1]),
  );
  const added = tags.filter((tag) => !present.has(tag));
  const sections = added.map((tag) => {
    const index = allTags.indexOf(tag);
    const prevTag = index >= 0 && index + 1 < allTags.length ? allTags[index + 1] : null;
    return renderChangelogSection({ repo, tag, prevTag, execGitFn });
  });
  const firstSection = existing.search(/^## /m);
  const body = firstSection >= 0 ? existing.slice(firstSection) : "";
  const content = [CHANGELOG_HEADER, ...sections, body].filter(Boolean).join("\n").replace(/\n+$/, "\n");
  return { content, added };
}

/**
 * Selects the inclusive tag range [from, to] from tags sorted newest first.
 * Throws when a bound is not a known release tag or the range is inverted.
 *
 * @param {string[]} allTags
 * @param {{ from?: string | null, to?: string | null }} range
 * @returns {string[]}
 */
export function selectTagRange(allTags, { from = null, to = null } = {}) {
  if (allTags.length === 0) throw new Error("no release tags (vYYYY.MMDD.N or vYYYY.MM.DD.N) found");
  const indexOf = (tag, flag) => {
    const index = allTags.indexOf(tag);
    if (index < 0) throw new Error(`${flag} ${tag} is not a release tag (vYYYY.MMDD.N or vYYYY.MM.DD.N) in this repository`);
    return index;
  };
  const newest = to ? indexOf(to, "--to") : 0;
  const oldest = from ? indexOf(from, "--from") : allTags.length - 1;
  if (newest > oldest) throw new Error(`empty tag range: --from ${from} is newer than --to ${to}`);
  return allTags.slice(newest, oldest + 1);
}

/**
 * CLI: `--write-changelog [--dry-run] [--from <tag>] [--to <tag>] [--file <path>] [--repo <owner/name>]`.
 * Prepends a section for every release tag in range that CHANGELOG.md lacks. With --dry-run
 * the resulting file is printed instead of written. Returns the process exit code.
 *
 * @param {string[]} argv
 * @param {{ stdout?: (s: string) => void, stderr?: (s: string) => void, listTagsFn?: Function, execGitFn?: Function }} [io]
 * @returns {number}
 */
export function runChangelogCli(argv, io = {}) {
  const stdout = io.stdout ?? ((s) => process.stdout.write(s));
  const stderr = io.stderr ?? ((s) => process.stderr.write(s));
  const opts = { write: false, dryRun: false, from: null, to: null, file: "CHANGELOG.md", repo: "Indosaram/ferryx" };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const value = () => {
      const next = argv[i + 1];
      if (!next || next.startsWith("--")) throw new Error(`${arg} needs a value`);
      i += 1;
      return next;
    };
    try {
      if (arg === "--write-changelog") opts.write = true;
      else if (arg === "--dry-run") opts.dryRun = true;
      else if (arg === "--from") opts.from = value();
      else if (arg === "--to") opts.to = value();
      else if (arg === "--file") opts.file = value();
      else if (arg === "--repo") opts.repo = value();
      else throw new Error(`unknown argument ${arg}`);
    } catch (error) {
      stderr(`release-changelog: ${error.message}\n`);
      return 2;
    }
  }
  if (!opts.write) {
    stderr("usage: bun scripts/lib/release-changelog.mjs --write-changelog [--dry-run] [--from <tag>] [--to <tag>] [--file <path>] [--repo <owner/name>]\n");
    return 2;
  }
  try {
    const allTags = listReleaseTags(io.listTagsFn ?? null);
    const tags = selectTagRange(allTags, { from: opts.from, to: opts.to });
    const existing = existsSync(opts.file) ? readFileSync(opts.file, "utf8") : "";
    const { content, added } = buildChangelogFile({
      repo: opts.repo,
      allTags,
      tags,
      existing,
      execGitFn: io.execGitFn ?? null,
    });
    if (opts.dryRun) {
      stdout(content);
      stderr(`release-changelog: dry run, ${added.length} new section(s) for ${tags.length} tag(s) in range\n`);
    } else {
      writeFileSync(opts.file, content, "utf8");
      stderr(`release-changelog: wrote ${opts.file} (${added.length} new section(s))\n`);
    }
    return 0;
  } catch (error) {
    stderr(`release-changelog: ${error.message}\n`);
    return 1;
  }
}

if (import.meta.main) {
  process.exit(runChangelogCli(process.argv.slice(2)));
}
