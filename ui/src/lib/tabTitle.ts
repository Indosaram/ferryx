import {
  agentDisplayNameForType,
  isBareAgentTitle,
  normalizeTerminalTitle,
  parseAgentTitle,
  stripLeadingActivityGlyphs,
} from "./agentTitle";

const BARE_SHELL_NAMES = new Set([
  "zsh",
  "-zsh",
  "bash",
  "-bash",
  "fish",
  "-fish",
  "sh",
  "-sh",
  "pwsh",
  "-pwsh",
  "powershell",
  "-powershell",
  "cmd",
  "-cmd",
  "cmd.exe",
  "powershell.exe",
  "pwsh.exe",
  "bash.exe",
  "zsh.exe",
  "nu",
  "-nu",
  "login",
]);

function escapeRegex(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function isShellPromptTitle(title: string, worktreeName?: string): boolean {
  const normalized = normalizeTerminalTitle(title).trim();
  if (!normalized) return true;

  const lower = normalized.toLowerCase();

  if (BARE_SHELL_NAMES.has(lower)) {
    return true;
  }

  if (/^[^@\s]+@[^:\s]+(?::.*)?$/.test(normalized)) {
    return true;
  }

  if (
    normalized === "~" ||
    normalized.startsWith("~/") ||
    normalized.startsWith("~\\") ||
    normalized.startsWith("/") ||
    normalized.startsWith("\\") ||
    /^[a-zA-Z]:[\\/]/.test(normalized) ||
    /^[a-zA-Z]:$/.test(normalized) ||
    normalized.startsWith("./") ||
    normalized.startsWith(".\\") ||
    normalized.startsWith("../") ||
    normalized.startsWith("..\\")
  ) {
    return true;
  }

  if (worktreeName) {
    const wtDir = worktreeName.replace(/[/\\]+$/, "").split(/[/\\]/).filter(Boolean).pop();
    if (wtDir) {
      const wtLower = wtDir.toLowerCase();
      if (lower === wtLower || lower === worktreeName.toLowerCase()) {
        return true;
      }
      if (
        (normalized.endsWith("/" + wtDir) ||
          normalized.endsWith("\\" + wtDir) ||
          lower.endsWith("/" + wtLower) ||
          lower.endsWith("\\" + wtLower)) &&
        !normalized.includes(" ")
      ) {
        return true;
      }
    }
  }

  return false;
}

export function isTransientAgentStatusTask(task: string): boolean {
  if (!task) return false;
  const trimmed = task.trim();
  if (/^(?:pretooluse|posttooluse|sessionstart|stop):/i.test(trimmed)) {
    return true;
  }
  if (/^running\s+\S+$/i.test(trimmed)) {
    return true;
  }
  return false;
}

export function cleanAgentTask(
  title: string,
  projectName?: string,
  agentType?: string,
): string | null {
  if (!title) {
    if (agentType) {
      return agentDisplayNameForType(agentType) ?? null;
    }
    return null;
  }

  const normalized = normalizeTerminalTitle(title).trim();
  if (!normalized) return null;

  const withoutSpinner = stripLeadingActivityGlyphs(normalized);

  const piMatch = /^(?:π|\bpi\b)\s*[:\-–—|]?\s*(.*)$/i.exec(withoutSpinner);
  if (piMatch) {
    const effectiveType = "pi";
    let task = (piMatch[1] || "").trim();

    if (!task || task.toLowerCase() === "pi" || task === "π") {
      task = "";
    }

    if (projectName) {
      const normProj = projectName.trim().toLowerCase();
      if (task.toLowerCase() === normProj) {
        task = "";
      } else {
        const trailingProjRegex = new RegExp(`\\s*[-–—|]\\s*${escapeRegex(projectName.trim())}$`, "i");
        if (trailingProjRegex.test(task)) {
          task = task.replace(trailingProjRegex, "").trim();
        }
      }
    }

    if (task) {
      const match = /^(.*?)\s*[-–—]\s*([^-–—]+)$/.exec(task);
      if (match && match[1]?.trim()) {
        if (!projectName || match[2].trim().toLowerCase() === projectName.trim().toLowerCase()) {
          task = match[1].trim();
        }
      }
    }

    if (!task) {
      return agentDisplayNameForType(effectiveType) ?? "Pi";
    }

    return task;
  }

  const parsed = parseAgentTitle(normalized);
  if (!parsed || (!parsed.isAgent && !agentType)) {
    return null;
  }

  const effectiveType = (agentType || parsed.agentType || "").toLowerCase();
  let task = (parsed.task || "").trim();

  if (
    isBareAgentTitle(normalized) ||
    task.toLowerCase() === parsed.name.toLowerCase() ||
    task.toLowerCase() === effectiveType
  ) {
    task = "";
  }

  if (projectName) {
    const normProj = projectName.trim().toLowerCase();
    if (task.toLowerCase() === normProj) {
      task = "";
    } else {
      const trailingProjRegex = new RegExp(`\\s*[-–—|]\\s*${escapeRegex(projectName.trim())}$`, "i");
      if (trailingProjRegex.test(task)) {
        task = task.replace(trailingProjRegex, "").trim();
      }
    }
  }

  if (task && (effectiveType === "omo" || /^omo\b/i.test(normalized))) {
    const match = /^(.*?)\s*[-–—]\s*([^-–—]+)$/.exec(task);
    if (match && match[1]?.trim()) {
      if (!projectName || match[2].trim().toLowerCase() === projectName.trim().toLowerCase()) {
        task = match[1].trim();
      }
    }
  }

  if (!task) {
    return (
      agentDisplayNameForType(effectiveType) ??
      agentDisplayNameForType(parsed.agentType) ??
      parsed.name ??
      "Agent"
    );
  }

  return task;
}

export type ResolveTerminalTabTextInput = {
  customLabel?: string;
  baseLabel: string;
  /** The session's readable default name; replaces the base label, never a rename/agent/title. */
  sessionName?: string;
  activity?: {
    title?: string;
    isAgent?: boolean;
    agentType?: string;
  } | null;
  projectName?: string;
  previousAgentText?: string;
};

export type ResolveTerminalTabTextResult = {
  text: string;
  tooltip?: string;
  source: "custom" | "agent" | "title" | "base";
};

export function resolveTerminalTabText(
  input: ResolveTerminalTabTextInput,
): ResolveTerminalTabTextResult {
  const cleanBase = stripLegacyOrdinal(input.baseLabel).trim() || "Terminal";
  const rawTitle = input.activity?.title ? normalizeTerminalTitle(input.activity.title).trim() : "";

  const custom = input.customLabel?.trim();
  if (custom) {
    let tooltip: string | undefined = undefined;
    if (rawTitle && !isShellPromptTitle(rawTitle, input.projectName) && rawTitle !== custom) {
      tooltip = cleanBase ? `${rawTitle} (${cleanBase})` : rawTitle;
    } else if (custom !== cleanBase) {
      tooltip = cleanBase;
    }
    return {
      text: custom,
      tooltip,
      source: "custom",
    };
  }

  if (input.activity) {
    const withoutSpinner = stripLeadingActivityGlyphs(rawTitle);
    const effectiveAgentType =
      input.activity.agentType || (/^(?:π|\bpi\b)/i.test(withoutSpinner) ? "pi" : undefined);
    const isAgent = Boolean(
      input.activity.isAgent ||
      effectiveAgentType ||
      (rawTitle && parseAgentTitle(rawTitle)?.isAgent),
    );
    if (isAgent) {
      const agentTask = cleanAgentTask(
        rawTitle,
        input.projectName,
        effectiveAgentType,
      );
      if (agentTask) {
        let text = agentTask;
        if (isTransientAgentStatusTask(agentTask)) {
          if (input.previousAgentText?.trim()) {
            text = input.previousAgentText.trim();
          } else {
            text =
              agentDisplayNameForType(effectiveAgentType) ??
              (rawTitle ? parseAgentTitle(rawTitle)?.name : undefined) ??
              "Agent";
          }
        }

        let tooltip: string | undefined = undefined;
        if (rawTitle && rawTitle !== text) {
          tooltip = cleanBase ? `${rawTitle} (${cleanBase})` : rawTitle;
        } else if (text !== cleanBase) {
          tooltip = cleanBase;
        }
        return {
          text,
          tooltip,
          source: "agent",
        };
      }
    }
  }

  if (rawTitle && !isShellPromptTitle(rawTitle, input.projectName)) {
    let tooltip: string | undefined = undefined;
    if (rawTitle !== cleanBase) {
      tooltip = cleanBase;
    }
    return {
      text: rawTitle,
      tooltip,
      source: "title",
    };
  }

  const sessionName = input.sessionName?.trim();
  if (sessionName) {
    return {
      text: sessionName,
      tooltip: sessionName !== cleanBase ? cleanBase : undefined,
      source: "base",
    };
  }

  return {
    text: cleanBase,
    source: "base",
  };
}

export function disambiguateTabTexts(texts: string[]): string[] {
  const counts = new Map<string, number>();
  return texts.map((text) => {
    const count = (counts.get(text) ?? 0) + 1;
    counts.set(text, count);
    return count === 1 ? text : `${text} (${count})`;
  });
}

export function stripLegacyOrdinal(label: string): string {
  if (!label) return "";
  return label.replace(/\s+\(\d+\)$/, "");
}

/**
 * Static name for a tab on surfaces that do not follow the live title (close dialogs, the
 * command palette, the remote mirror): the user's rename wins over the automatic label.
 */
export function staticTabLabel(tab: { label: string; customLabel?: string }): string {
  return tab.customLabel?.trim() || stripLegacyOrdinal(tab.label).trim() || tab.label;
}
