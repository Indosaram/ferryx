import { execFileSync, spawnSync } from "node:child_process";

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
