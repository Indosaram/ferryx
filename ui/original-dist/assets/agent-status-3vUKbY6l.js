import { n as tuiAgentToAgentKind } from "./agent-kind-Dfx6MnkP.js";
function getOrcaCliCommandNameForPlatform(platform) {
	if (platform === "linux") return "orca-ide";
	if (platform === "win32") return "orca.cmd";
	return "orca";
}
const TUI_AGENT_CONFIG = {
	claude: {
		detectCmd: "claude",
		launchCmd: "claude",
		expectedProcess: "claude",
		promptInjectionMode: "argv",
		draftPromptFlag: "--prefill"
	},
	"claude-agent-teams": {
		detectCmd: "orca",
		detectCmdAliases: ["orca-dev", "orca-ide"],
		detectRequiredCommands: ["claude"],
		detectUnsupportedRuntimes: ["win32", "wsl"],
		launchCmd: "orca claude-teams",
		launchCmdByPlatform: {
			linux: `${getOrcaCliCommandNameForPlatform("linux")} claude-teams`,
			win32: `${getOrcaCliCommandNameForPlatform("win32")} claude-teams`
		},
		expectedProcess: "claude",
		promptInjectionMode: "stdin-after-start"
	},
	openclaude: {
		detectCmd: "openclaude",
		launchCmd: "openclaude",
		expectedProcess: "openclaude",
		promptInjectionMode: "argv",
		draftPromptFlag: "--prefill"
	},
	codex: {
		detectCmd: "codex",
		launchCmd: "codex",
		expectedProcess: "codex",
		promptInjectionMode: "argv",
		preflightTrust: "codex",
		draftPasteReadySignal: "codex-composer-prompt"
	},
	autohand: {
		detectCmd: "autohand",
		launchCmd: "autohand",
		expectedProcess: "autohand",
		promptInjectionMode: "stdin-after-start"
	},
	ante: {
		detectCmd: "ante",
		launchCmd: "ante",
		expectedProcess: "ante",
		promptInjectionMode: "stdin-after-start"
	},
	trae: {
		detectCmd: "traecli",
		launchCmd: "traecli",
		expectedProcess: "traecli",
		promptInjectionMode: "argv",
		argvPromptSeparator: "--"
	},
	opencode: {
		detectCmd: "opencode",
		launchCmd: "opencode",
		expectedProcess: "opencode",
		promptInjectionMode: "flag-prompt",
		draftPasteReadySignal: "render-cursor-after-bracketed-paste"
	},
	"mimo-code": {
		detectCmd: "mimo",
		launchCmd: "mimo",
		expectedProcess: "mimo",
		promptInjectionMode: "flag-prompt",
		draftPasteReadySignal: "render-cursor-after-bracketed-paste"
	},
	pi: {
		detectCmd: "pi",
		launchCmd: "pi",
		expectedProcess: "pi",
		promptInjectionMode: "argv",
		draftPromptEnvVar: "ORCA_PI_PREFILL",
		windowsShiftEnterEncoding: "csi-u"
	},
	omp: {
		detectCmd: "omp",
		launchCmd: "omp",
		expectedProcess: "omp",
		promptInjectionMode: "argv",
		draftPromptEnvVar: "ORCA_OMP_PREFILL"
	},
	"prime-agent": {
		detectCmd: "prime-agent",
		launchCmd: "prime-agent",
		expectedProcess: "prime-agent",
		promptInjectionMode: "argv",
		argvPromptSeparator: "--",
		windowsShiftEnterEncoding: "csi-u"
	},
	gemini: {
		detectCmd: "gemini",
		launchCmd: "gemini",
		expectedProcess: "gemini",
		promptInjectionMode: "flag-prompt-interactive"
	},
	antigravity: {
		detectCmd: "agy",
		launchCmd: "agy",
		expectedProcess: "agy",
		promptInjectionMode: "flag-prompt-interactive"
	},
	aider: {
		detectCmd: "aider",
		launchCmd: "aider",
		expectedProcess: "aider",
		promptInjectionMode: "stdin-after-start"
	},
	goose: {
		detectCmd: "goose",
		launchCmd: "goose",
		expectedProcess: "goose",
		promptInjectionMode: "stdin-after-start"
	},
	amp: {
		detectCmd: "amp",
		launchCmd: "amp",
		expectedProcess: "amp",
		promptInjectionMode: "stdin-after-start"
	},
	kilo: {
		detectCmd: "kilo",
		launchCmd: "kilo",
		expectedProcess: "kilo",
		promptInjectionMode: "stdin-after-start"
	},
	kiro: {
		detectCmd: "kiro-cli",
		launchCmd: "kiro-cli chat --tui",
		expectedProcess: "kiro-cli",
		promptInjectionMode: "stdin-after-start"
	},
	crush: {
		detectCmd: "crush",
		launchCmd: "crush",
		expectedProcess: "crush",
		promptInjectionMode: "stdin-after-start"
	},
	aug: {
		detectCmd: "auggie",
		launchCmd: "auggie",
		expectedProcess: "auggie",
		promptInjectionMode: "stdin-after-start"
	},
	cline: {
		detectCmd: "cline",
		launchCmd: "cline",
		expectedProcess: "cline",
		promptInjectionMode: "stdin-after-start"
	},
	codebuff: {
		detectCmd: "codebuff",
		launchCmd: "codebuff",
		expectedProcess: "codebuff",
		promptInjectionMode: "stdin-after-start"
	},
	"command-code": {
		detectCmd: "command-code",
		launchCmd: "command-code --trust",
		expectedProcess: "command-code",
		promptInjectionMode: "argv"
	},
	continue: {
		detectCmd: "cn",
		launchCmd: "cn",
		expectedProcess: "cn",
		promptInjectionMode: "stdin-after-start"
	},
	cursor: {
		detectCmd: "cursor-agent",
		launchCmd: "cursor-agent",
		expectedProcess: "cursor-agent",
		promptInjectionMode: "argv",
		preflightTrust: "cursor"
	},
	droid: {
		detectCmd: "droid",
		launchCmd: "droid",
		expectedProcess: "droid",
		promptInjectionMode: "argv",
		windowsShiftEnterEncoding: "csi-u",
		ctrlEnterEncoding: "csi-u"
	},
	kimi: {
		detectCmd: "kimi",
		launchCmd: "kimi",
		expectedProcess: "kimi",
		promptInjectionMode: "stdin-after-start"
	},
	"mistral-vibe": {
		detectCmd: "vibe",
		detectCmdAliases: ["mistral-vibe"],
		launchCmd: "vibe",
		expectedProcess: "vibe",
		promptInjectionMode: "stdin-after-start"
	},
	"qwen-code": {
		detectCmd: "qwen",
		launchCmd: "qwen",
		expectedProcess: "qwen",
		promptInjectionMode: "stdin-after-start"
	},
	rovo: {
		detectCmd: "rovo",
		launchCmd: "rovo",
		expectedProcess: "rovo",
		promptInjectionMode: "stdin-after-start"
	},
	hermes: {
		detectCmd: "hermes",
		launchCmd: "hermes --tui",
		expectedProcess: "hermes",
		promptInjectionMode: "hermes-query"
	},
	openclaw: {
		detectCmd: "openclaw",
		launchCmd: "openclaw",
		expectedProcess: "openclaw",
		promptInjectionMode: "stdin-after-start"
	},
	copilot: {
		detectCmd: "copilot",
		launchCmd: "copilot",
		expectedProcess: "copilot",
		promptInjectionMode: "flag-interactive",
		preflightTrust: "copilot"
	},
	grok: {
		detectCmd: "grok",
		launchCmd: "grok",
		expectedProcess: "grok",
		promptInjectionMode: "argv",
		argvPromptSeparator: "--",
		draftPasteReadySignal: "grok-composer-prompt",
		ctrlEnterEncoding: "csi-u"
	},
	devin: {
		detectCmd: "devin",
		launchCmd: "devin",
		expectedProcess: "devin",
		promptInjectionMode: "stdin-after-start"
	}
};
function isTuiAgent(value) {
	return typeof value === "string" && Object.hasOwn(TUI_AGENT_CONFIG, value);
}
function getTuiAgentDetectCommands(config) {
	return [config.detectCmd, ...config.detectCmdAliases ?? []];
}
function getTuiAgentLaunchCommand(config, platform, opts) {
	if (opts?.isRemote && platform === "linux") return config.launchCmd;
	return config.launchCmdByPlatform?.[platform] ?? config.launchCmd;
}
var JsonTextStructureCapacityError = class extends Error {
	constructor(resource, limit) {
		super(resource === "structuralTokens" ? `JSON structure exceeds ${limit} tokens` : `JSON nesting exceeds ${limit} levels`);
		this.resource = resource;
		this.limit = limit;
		this.name = "JsonTextStructureCapacityError";
	}
};
function assertJsonTextStructureWithinLimits(content, limits) {
	assertLimit(limits.structuralTokens);
	assertLimit(limits.nestingDepth);
	let structuralTokens = 0;
	let depth = 0;
	let inString = false;
	let escaped = false;
	for (let index = 0; index < content.length; index += 1) {
		const character = content[index];
		if (inString) {
			if (escaped) escaped = false;
			else if (character === "\\") escaped = true;
			else if (character === "\"") inString = false;
			continue;
		}
		if (character === "\"") {
			inString = true;
			continue;
		}
		if (!isStructuralToken(character)) continue;
		structuralTokens += 1;
		if (structuralTokens > limits.structuralTokens) throw new JsonTextStructureCapacityError("structuralTokens", limits.structuralTokens);
		if (character === "{" || character === "[") {
			depth += 1;
			if (depth > limits.nestingDepth) throw new JsonTextStructureCapacityError("nestingDepth", limits.nestingDepth);
		} else if (character === "}" || character === "]") depth = Math.max(0, depth - 1);
	}
}
function assertLimit(value) {
	if (!Number.isSafeInteger(value) || value < 0) throw new RangeError("JSON structure limits must be non-negative safe integers");
}
function isStructuralToken(character) {
	return character === "{" || character === "}" || character === "[" || character === "]" || character === "," || character === ":";
}
const LOCAL_EXECUTION_HOST_ID = "local";
const ALL_EXECUTION_HOSTS_SCOPE = "all";
function getCurrentLocalPlatform() {
	const globalNavigator = globalThis.navigator;
	const userAgent = globalNavigator?.userAgent || globalNavigator?.platform || "";
	if (/Windows/i.test(userAgent)) return "win32";
	if (/Mac/i.test(userAgent)) return "darwin";
	if (/Linux|X11/i.test(userAgent)) return "linux";
	return typeof process === "undefined" ? null : process.platform;
}
function getLocalExecutionHostLabel(platform = null) {
	const localPlatform = platform ?? getCurrentLocalPlatform();
	if (localPlatform === "darwin") return "Local Mac";
	if (localPlatform === "win32") return "Local Windows";
	if (localPlatform === "linux") return "Local Linux";
	return "This computer";
}
function normalizeHostPart(value) {
	const trimmed = value?.trim();
	return trimmed ? trimmed : null;
}
function toSshExecutionHostId(targetId) {
	return `ssh:${encodeURIComponent(targetId)}`;
}
function toRuntimeExecutionHostId(environmentId) {
	return `runtime:${encodeURIComponent(environmentId)}`;
}
function isRuntimeOwnedSshTargetId(targetId) {
	return typeof targetId === "string" && targetId.startsWith("runtime-ssh-");
}
function parseExecutionHostId(value) {
	const normalized = normalizeHostPart(value);
	if (!normalized) return null;
	if (normalized === "local") return {
		kind: "local",
		id: LOCAL_EXECUTION_HOST_ID
	};
	if (normalized.startsWith("ssh:")) {
		const encoded = normalized.slice(4);
		if (!encoded) return null;
		try {
			const targetId = decodeURIComponent(encoded);
			return targetId ? {
				kind: "ssh",
				id: `ssh:${encoded}`,
				targetId
			} : null;
		} catch {
			return null;
		}
	}
	if (normalized.startsWith("runtime:")) {
		const encoded = normalized.slice(8);
		if (!encoded) return null;
		try {
			const environmentId = decodeURIComponent(encoded);
			return environmentId ? {
				kind: "runtime",
				id: `runtime:${encoded}`,
				environmentId
			} : null;
		} catch {
			return null;
		}
	}
	return null;
}
function normalizeExecutionHostId(value) {
	return parseExecutionHostId(value)?.id ?? null;
}
function normalizeExecutionHostScope(value) {
	const normalized = normalizeHostPart(value);
	if (!normalized || normalized === "all") return "all";
	return normalizeExecutionHostId(normalized) ?? "all";
}
function normalizeVisibleExecutionHostIds(value) {
	if (!Array.isArray(value)) return null;
	const ids = [];
	const seen = /* @__PURE__ */ new Set();
	for (const raw of value) {
		const id = normalizeExecutionHostId(raw);
		if (!id || seen.has(id)) continue;
		seen.add(id);
		ids.push(id);
	}
	return ids.length > 0 ? ids : null;
}
function normalizeExecutionHostOrder(value) {
	return normalizeVisibleExecutionHostIds(value) ?? [];
}
function getRepoExecutionHostId(repo) {
	const executionHostId = normalizeExecutionHostId(repo.executionHostId);
	if (executionHostId) return executionHostId;
	const connectionId = normalizeHostPart(repo.connectionId);
	return connectionId ? toSshExecutionHostId(connectionId) : LOCAL_EXECUTION_HOST_ID;
}
function getWorktreeExecutionHostId(worktree, repo, defaultHostId = LOCAL_EXECUTION_HOST_ID) {
	return worktree.hostId ?? (repo?.connectionId || repo?.executionHostId ? getRepoExecutionHostId(repo) : defaultHostId);
}
function getSettingsFocusedExecutionHostId(settings) {
	const runtimeEnvironmentId = normalizeHostPart(settings?.activeRuntimeEnvironmentId);
	return runtimeEnvironmentId ? toRuntimeExecutionHostId(runtimeEnvironmentId) : LOCAL_EXECUTION_HOST_ID;
}
function getExecutionHostLabel(id) {
	if (id === "all") return "All hosts";
	const parsed = parseExecutionHostId(id);
	if (!parsed) return "All hosts";
	switch (parsed.kind) {
		case "local": return getLocalExecutionHostLabel();
		case "ssh": return parsed.targetId;
		case "runtime": return parsed.environmentId;
	}
}
const AGENT_NAMES = [
	"claude",
	"openclaude",
	"codex",
	"copilot",
	"cursor",
	"gemini",
	"antigravity",
	"opencode",
	"mimo",
	"openclaw",
	"aider",
	"grok",
	"devin"
];
var WINDOWS_EXECUTABLE_SUFFIX_RE = String.raw`(?:\.(?:exe|cmd|bat|ps1))`;
function buildAgentNameRe(name) {
	return new RegExp(`(?<![\\w./\\\\-])${name}(?:${WINDOWS_EXECUTABLE_SUFFIX_RE})?(?![\\w./\\\\-])`, "i");
}
var AGENT_NAME_RE_BY_NAME = new Map(AGENT_NAMES.map((name) => [name, buildAgentNameRe(name)]));
var ANY_LEGACY_AGENT_NAME_RE = new RegExp(AGENT_NAMES.map((name) => `(?<![\\w./\\\\-])${name}(?:${WINDOWS_EXECUTABLE_SUFFIX_RE})?(?![\\w./\\\\-])`).join("|"), "i");
function titleHasAgentName(title, name) {
	return AGENT_NAME_RE_BY_NAME.get(name)?.test(title) ?? false;
}
function titleHasAnyLegacyAgentName(title) {
	return ANY_LEGACY_AGENT_NAME_RE.test(title);
}
const DROID_AGENT_NAME_RE = /(?<![\w./\\-])droid(?![\w./\\-])/i;
const HERMES_AGENT_NAME_RE = /(?<![\w./\\-])hermes(?![\w./\\-])/i;
const AGY_AGENT_NAME_RE = /(?<![\w./\\-])agy(?![\w./\\-])/i;
var PI_COMPATIBLE_SYNTHETIC_TITLE_RE = /^\s*(?:[\u2800-\u28ff]\s+)?(pi|omp)(?:\s+-\s+action required|\s+(?:ready|idle|done))?\s*$/i;
var LEGACY_PI_COMPATIBLE_TITLE_RE = /^\s*(?:[\u2800-\u28ff]\s+)?π(?:\s*[-:]|\s)\s*.*$/u;
function containsBrailleSpinner$2(title) {
	for (const char of title) {
		const codePoint = char.codePointAt(0);
		if (codePoint !== void 0 && codePoint >= 10240 && codePoint <= 10495) return true;
	}
	return false;
}
function getPiCompatibleSyntheticAgentLabel(title) {
	const match = PI_COMPATIBLE_SYNTHETIC_TITLE_RE.exec(title);
	if (!match) return null;
	return match[1].toLowerCase() === "omp" ? "OMP" : "Pi";
}
function getPiCompatibleSyntheticAgentStatus(title) {
	if (!getPiCompatibleSyntheticAgentLabel(title)) return null;
	if (containsBrailleSpinner$2(title)) return "working";
	const lower = title.toLowerCase();
	if (lower.includes("action required") || lower.includes("permission") || lower.includes("waiting")) return "permission";
	return "idle";
}
function isLegacyPiCompatibleTitle(title) {
	return LEGACY_PI_COMPATIBLE_TITLE_RE.test(title);
}
var CLAUDE_COMMAND_RE = String.raw`(?:.*[\\/])?claude(?:\.(?:exe|cmd|bat|ps1))?`;
const CLAUDE_MANAGEMENT_TITLE_RE$1 = new RegExp(String.raw`^\s*(?:"${CLAUDE_COMMAND_RE}"|'${CLAUDE_COMMAND_RE}'|${CLAUDE_COMMAND_RE})\s+agents\s*$`, "i");
var STRONG_IDLE_KEYWORDS = [
	"ready",
	"idle",
	"done"
];
var STRONG_WORKING_KEYWORDS = [
	"working",
	"thinking",
	"running"
];
const STRONG_IDLE_KEYWORDS_RE = new RegExp(`(?<![\\w./\\\\-])(${STRONG_IDLE_KEYWORDS.join("|")})(?![\\w\\-])`, "i");
const STRONG_WORKING_KEYWORDS_RE = new RegExp(`(?<![\\w./\\\\-])(${STRONG_WORKING_KEYWORDS.join("|")})(?![\\w\\-])`, "i");
const STRONG_WORKING_KEYWORDS_RE_GLOBAL = new RegExp(STRONG_WORKING_KEYWORDS_RE.source, "gi");
const CURSOR_NATIVE_TITLE_LOWER = "cursor agent";
const BRAILLE_SPINNER_RE = /[\u2800-\u28ff]/g;
const QUARTER_CIRCLE_SPINNER_RE = /[\u25d0-\u25d3]/g;
function isGeminiTerminalTitle(title) {
	if (title.includes("✋") || title.includes("✦") || title.includes("⏲") || title.includes("◇")) return true;
	if (isPiAgentTitle(title)) return false;
	return titleHasAgentName(title, "gemini");
}
function isPiTerminalTitle(title) {
	return isLegacyPiCompatibleTitle(title) && !containsBrailleSpinner$1(title);
}
function isPiAgentTitle(title) {
	return isLegacyPiCompatibleTitle(title);
}
function containsBrailleSpinner$1(title) {
	for (const char of title) {
		const codePoint = char.codePointAt(0);
		if (codePoint !== void 0 && codePoint >= 10240 && codePoint <= 10495) return true;
	}
	return false;
}
function containsQuarterCircleSpinner(title) {
	for (const char of title) {
		const codePoint = char.codePointAt(0);
		if (codePoint !== void 0 && codePoint >= 9680 && codePoint <= 9683) return true;
	}
	return false;
}
function containsAgentSpinnerGlyph(title) {
	return containsBrailleSpinner$1(title) || containsQuarterCircleSpinner(title);
}
function containsLegacyAgentName(title) {
	return titleHasAnyLegacyAgentName(title);
}
function containsAgentName(title) {
	return containsLegacyAgentName(title) || AGY_AGENT_NAME_RE.test(title) || DROID_AGENT_NAME_RE.test(title) || HERMES_AGENT_NAME_RE.test(title);
}
function containsAny(title, words) {
	const lower = title.toLowerCase();
	return words.some((word) => lower.includes(word));
}
function isClaudeManagementTitle(title) {
	return CLAUDE_MANAGEMENT_TITLE_RE$1.test(title);
}
function isCursorNativeAgentTitle(title) {
	return title.trim().toLowerCase() === CURSOR_NATIVE_TITLE_LOWER;
}
function isCursorAgentTitle(title) {
	if (typeof title !== "string") return false;
	const trimmed = title.trim();
	const lower = trimmed.toLowerCase();
	if (lower === "cursor agent" || lower === "cursor ready" || lower === "cursor - action required") return true;
	return /^[\u2800-\u28ff] Cursor Agent$/u.test(trimmed);
}
function shouldSuppressCursorNativeTitle(lastEmittedTitle) {
	return lastEmittedTitle !== null && isCursorAgentTitle(lastEmittedTitle);
}
var OPENCODE_NATIVE_TITLE_RE = /^\s*(?:(?![▣\u2800-\u28ff])[^|]+? \| )?(?:[▣\u2800-\u28ff] )?OC \|[ \t]+\S/u;
function isOpenCodeNativeTitle(title) {
	return title ? OPENCODE_NATIVE_TITLE_RE.test(title) : false;
}
function isMeaningfulOpenCodeTerminalTitle(title) {
	return isOpenCodeNativeTitle(title);
}
function isClaudeAgent(title) {
	if (!title || isClaudeManagementTitle(title) || isOpenCodeNativeTitle(title)) return false;
	const lower = title.toLowerCase();
	if (title.startsWith(`✳ `) || title === "✳") return true;
	if (title.startsWith(". ") || title.startsWith("* ")) return true;
	if (containsAgentSpinnerGlyph(title)) return !isCursorAgentTitle(title) && !lower.includes("openclaude");
	const trimmedTitle = title.trimStart();
	return trimmedTitle.toLowerCase().startsWith("claude") && titleHasAgentName(trimmedTitle, "claude");
}
function getAgentLabel(title) {
	if (isClaudeManagementTitle(title)) return null;
	if (isOpenCodeNativeTitle(title)) return "OpenCode";
	if (title.startsWith(`✳ `) || title === "✳" || title.startsWith(". ") || title.startsWith("* ")) return "Claude Code";
	if (isGeminiTerminalTitle(title)) return "Gemini CLI";
	const piCompatibleSyntheticAgentLabel = getPiCompatibleSyntheticAgentLabel(title);
	if (piCompatibleSyntheticAgentLabel) return piCompatibleSyntheticAgentLabel;
	if (isPiAgentTitle(title)) return "Pi";
	if (titleHasAgentName(title, "codex")) return "Codex";
	if (titleHasAgentName(title, "openclaude")) return "OpenClaude";
	if (titleHasAgentName(title, "copilot")) return "GitHub Copilot";
	if (titleHasAgentName(title, "grok")) return "Grok";
	if (titleHasAgentName(title, "devin")) return "Devin";
	if (titleHasAgentName(title, "antigravity") || AGY_AGENT_NAME_RE.test(title)) return "Antigravity";
	if (titleHasAgentName(title, "opencode")) return "OpenCode";
	if (titleHasAgentName(title, "mimo")) return "MiMo Code";
	if (titleHasAgentName(title, "aider")) return "Aider";
	if (isCursorAgentTitle(title)) return "Cursor";
	if (DROID_AGENT_NAME_RE.test(title)) return "Droid";
	if (HERMES_AGENT_NAME_RE.test(title)) return "Hermes";
	if (isClaudeAgent(title)) return "Claude Code";
	return null;
}
var LEADING_AGENT_TITLE_DECORATION_RE = /^(?:[✳✦⏲◇✋⠀-⣿◐-◓]+|[.*]\s)\s*/;
function stripLeadingAgentTitleDecorationOrEmpty(title) {
	return title.replace(LEADING_AGENT_TITLE_DECORATION_RE, "").trimStart();
}
function stripLeadingAgentTitleDecoration(title) {
	const stripped = stripLeadingAgentTitleDecorationOrEmpty(title);
	return stripped.length > 0 ? stripped : title;
}
var WRAPPER_SEPARATOR = " | ";
function getWrapperTitleSegments(title) {
	const segments = [title];
	let separatorIndex = title.indexOf(WRAPPER_SEPARATOR);
	while (separatorIndex >= 0) {
		const wrapped = title.slice(separatorIndex + 3).trim();
		if (wrapped && !segments.includes(wrapped)) segments.push(wrapped);
		separatorIndex = title.indexOf(WRAPPER_SEPARATOR, separatorIndex + 3);
	}
	return segments;
}
var CLAUDE_MANAGEMENT_TITLE_RE = /^\s*(?:"(?:.*[\\/])?claude(?:\.(?:exe|cmd|bat|ps1))?"|'(?:.*[\\/])?claude(?:\.(?:exe|cmd|bat|ps1))?'|(?:.*[\\/])?claude(?:\.(?:exe|cmd|bat|ps1))?)\s+agents\s*$/i;
function containsBrailleSpinner(title) {
	for (const char of title) {
		const codePoint = char.codePointAt(0);
		if (codePoint !== void 0 && codePoint >= 10240 && codePoint <= 10495) return true;
	}
	return false;
}
function isGeminiTerminalTitle$1(title) {
	if (title.includes("✋") || title.includes("✦") || title.includes("⏲") || title.includes("◇")) return true;
	if (isPiAgentTitle$1(title)) return false;
	return titleHasAgentName(title, "gemini");
}
var GROK_ROTATING_FRAME_RE = /^[\u2800-\u28FF]+\s+-\s+[\s\S]+?\s-\s+grok\s*$/i;
var GROK_COLLAPSED_WORKING_TITLE_RE = /^[\u2800-\u28FF]+\s+grok\s*$/i;
function isGrokRotatingWorkingTitle(title) {
	if (!containsBrailleSpinner(title)) return false;
	return GROK_ROTATING_FRAME_RE.test(title) || GROK_COLLAPSED_WORKING_TITLE_RE.test(title);
}
function isPiAgentTitle$1(title) {
	return isLegacyPiCompatibleTitle(title);
}
function isClaudeAgent$1(title) {
	if (!title || isClaudeManagementTitle$1(title) || isOpenCodeNativeTitle(title)) return false;
	const lower = title.toLowerCase();
	if (title.startsWith(`✳ `) || title === "✳") return true;
	if (title.startsWith(". ") || title.startsWith("* ")) return true;
	if (containsAgentSpinnerGlyph(title)) return !isCursorAgentTitle(title) && !lower.includes("openclaude");
	const trimmedTitle = title.trimStart();
	if (trimmedTitle.toLowerCase().startsWith("claude") && titleHasAgentName(trimmedTitle, "claude")) return true;
	return false;
}
function isClaudeManagementTitle$1(title) {
	return CLAUDE_MANAGEMENT_TITLE_RE.test(title);
}
function getAgentLabel$1(title) {
	if (isClaudeManagementTitle$1(title)) return null;
	if (isOpenCodeNativeTitle(title)) return "OpenCode";
	if (title.startsWith(`✳ `) || title === "✳" || title.startsWith(". ") || title.startsWith("* ")) return "Claude Code";
	if (isGeminiTerminalTitle$1(title)) return "Gemini CLI";
	const piCompatibleSyntheticAgentLabel = getPiCompatibleSyntheticAgentLabel(title);
	if (piCompatibleSyntheticAgentLabel) return piCompatibleSyntheticAgentLabel;
	if (isPiAgentTitle$1(title)) return "Pi";
	if (titleHasAgentName(title, "codex")) return "Codex";
	if (titleHasAgentName(title, "openclaude")) return "OpenClaude";
	if (titleHasAgentName(title, "copilot")) return "GitHub Copilot";
	if (titleHasAgentName(title, "grok")) return "Grok";
	if (titleHasAgentName(title, "devin")) return "Devin";
	if (titleHasAgentName(title, "antigravity") || AGY_AGENT_NAME_RE.test(title)) return "Antigravity";
	if (titleHasAgentName(title, "opencode")) return "OpenCode";
	if (titleHasAgentName(title, "mimo")) return "MiMo Code";
	if (titleHasAgentName(title, "aider")) return "Aider";
	if (isCursorAgentTitle(title)) return "Cursor";
	if (DROID_AGENT_NAME_RE.test(title)) return "Droid";
	if (HERMES_AGENT_NAME_RE.test(title)) return "Hermes";
	if (isClaudeAgent$1(title)) return "Claude Code";
	return null;
}
var TITLE_LABEL_TO_AGENT = {
	"Claude Code": "claude",
	OpenClaude: "openclaude",
	Codex: "codex",
	"Gemini CLI": "gemini",
	"GitHub Copilot": "copilot",
	Grok: "grok",
	Devin: "devin",
	Antigravity: "antigravity",
	OpenCode: "opencode",
	"MiMo Code": "mimo-code",
	Aider: "aider",
	Cursor: "cursor",
	Droid: "droid",
	Hermes: "hermes",
	Pi: "pi",
	OMP: "omp"
};
function hasGenericClaudeStatusPrefix(title) {
	return containsAgentSpinnerGlyph(title) || title.startsWith("✳ ") || title === "✳" || title.startsWith(". ") || title.startsWith("* ");
}
var CLAUDE_IDENTITY_FRAME_RE = /^claude(?: code)?(?:\s+(?:ready|idle|done|working|thinking|running))?(?:\s*-\s*action required)?$/;
function isClaudeIdentityFrameTitle(title) {
	return getWrapperTitleSegments(title).some((segment) => CLAUDE_IDENTITY_FRAME_RE.test(stripLeadingAgentTitleDecorationOrEmpty(segment).trim().toLowerCase()));
}
function isGenericClaudeStatusClaim(title, titleAgent) {
	return titleAgent === "claude" && hasGenericClaudeStatusPrefix(title) && !titleHasAgentName(title, "claude");
}
function resolveTerminalTitleAgentType(title) {
	const label = getAgentLabel$1(title);
	return label ? TITLE_LABEL_TO_AGENT[label] ?? null : null;
}
function resolveExplicitTerminalTitleAgentType(title) {
	const titleAgent = resolveTerminalTitleAgentType(title);
	if (isGenericClaudeStatusClaim(title, titleAgent)) return null;
	return titleAgent;
}
function clearWorkingIndicators(title) {
	let cleaned = title;
	cleaned = cleaned.replace("✦", "");
	cleaned = cleaned.replace("⏲", "");
	cleaned = cleaned.replace(BRAILLE_SPINNER_RE, "");
	cleaned = cleaned.replace(QUARTER_CIRCLE_SPINNER_RE, "");
	if (cleaned.startsWith(". ")) cleaned = cleaned.slice(2);
	if (containsAgentName(cleaned)) cleaned = cleaned.replace(STRONG_WORKING_KEYWORDS_RE_GLOBAL, "");
	cleaned = cleaned.replace(/\s{2,}/g, " ").trim();
	return cleaned || title;
}
function createAgentStatusTracker(onBecameIdle, onBecameWorking, onAgentExited, initialTitle) {
	let lastStatus = initialTitle !== void 0 ? detectAgentStatusFromTitle(initialTitle) : null;
	let restorableExitStatus = null;
	return {
		handleTitle(title) {
			const newStatus = detectAgentStatusFromTitle(title);
			if (newStatus !== null) restorableExitStatus = null;
			if (lastStatus === "working" && newStatus !== null && newStatus !== "working") onBecameIdle(title);
			if (lastStatus !== "working" && newStatus === "working") onBecameWorking?.();
			if (lastStatus !== null && lastStatus !== "working" && newStatus === null) {
				restorableExitStatus = lastStatus;
				lastStatus = null;
				onAgentExited?.();
			}
			if (newStatus !== null) lastStatus = newStatus;
		},
		seedTitle(title) {
			lastStatus = detectAgentStatusFromTitle(title);
			restorableExitStatus = null;
		},
		restoreLastExit() {
			const restoredStatus = lastStatus === null ? restorableExitStatus : null;
			if (restoredStatus !== null) lastStatus = restoredStatus;
			restorableExitStatus = null;
			return restoredStatus;
		},
		reset() {
			lastStatus = null;
			restorableExitStatus = null;
		}
	};
}
function normalizeTerminalTitle(title) {
	if (!title) return title;
	if (isGeminiTerminalTitle(title)) {
		const status = detectAgentStatusFromTitle(title);
		if (status === "permission") return `✋ Gemini CLI`;
		if (status === "working") return `✦ Gemini CLI`;
		if (status === "idle") return `◇ Gemini CLI`;
	}
	if (isPiAgentTitle(title)) {
		const status = detectAgentStatusFromTitle(title);
		if (status === "working") return "⠋ Pi";
		if (status === "idle") return "Pi";
	}
	if (isGrokRotatingWorkingTitle(title)) return "⠋ Grok";
	return title;
}
function detectAgentStatusFromTitle(title) {
	if (!title || isClaudeManagementTitle(title)) return null;
	if (title.trim().toLowerCase() === "cursor agent") return null;
	if (isOpenCodeNativeTitle(title)) return containsAgentSpinnerGlyph(title) ? "working" : "idle";
	if (title.includes("✋")) return "permission";
	if (title.includes("✦") || title.includes("⏲")) return "working";
	if (title.includes("◇")) return "idle";
	const piCompatibleSyntheticAgentStatus = getPiCompatibleSyntheticAgentStatus(title);
	if (piCompatibleSyntheticAgentStatus) return piCompatibleSyntheticAgentStatus;
	if (title.startsWith(`✳ `) || title === "✳") return "idle";
	if (isPiTerminalTitle(title)) return "idle";
	if (containsAgentSpinnerGlyph(title)) return "working";
	const hasDroidAgentName = DROID_AGENT_NAME_RE.test(title);
	const hasHermesAgentName = HERMES_AGENT_NAME_RE.test(title);
	const hasAgyAgentName = AGY_AGENT_NAME_RE.test(title);
	const hasLegacyAgentName = containsLegacyAgentName(title);
	if (!hasLegacyAgentName && !hasDroidAgentName && !hasHermesAgentName && !hasAgyAgentName) return null;
	if (containsAny(title, [
		"action required",
		"permission",
		"waiting"
	])) return "permission";
	if (STRONG_IDLE_KEYWORDS_RE.test(title)) return "idle";
	if (STRONG_WORKING_KEYWORDS_RE.test(title)) return "working";
	if (title.startsWith(". ")) return "working";
	if (title.startsWith("* ")) return "idle";
	if (hasDroidAgentName && !hasLegacyAgentName) return null;
	return "idle";
}
var ESC_CODE_UNIT = 27;
var BEL_CODE_UNIT = 7;
var RIGHT_BRACKET_CODE_UNIT = 93;
var BACKSLASH_CODE_UNIT = 92;
var SEMICOLON_CODE_UNIT = 59;
var OSC_TITLE_COMMANDS = new Set([
	48,
	49,
	50
]);
const MAX_OSC_TITLE_CHARS = 1024;
const MAX_OSC_TITLES_PER_CHUNK = 4096;
function isOscIntroducerAt(data, index) {
	return data.charCodeAt(index) === ESC_CODE_UNIT && data.charCodeAt(index + 1) === RIGHT_BRACKET_CODE_UNIT;
}
function parseOscTitleAt(data, index) {
	if (!isOscIntroducerAt(data, index)) return {
		kind: "invalid",
		nextIndex: index + 1
	};
	if (!OSC_TITLE_COMMANDS.has(data.charCodeAt(index + 2)) || data.charCodeAt(index + 3) !== SEMICOLON_CODE_UNIT) return {
		kind: "invalid",
		nextIndex: index + 2
	};
	const titleStart = index + 4;
	for (let cursor = titleStart; cursor < data.length; cursor += 1) {
		const code = data.charCodeAt(cursor);
		if (code === BEL_CODE_UNIT) return {
			kind: "title",
			title: readBoundedOscTitle(data, titleStart, cursor),
			nextIndex: cursor + 1
		};
		if (code !== ESC_CODE_UNIT) continue;
		if (data.charCodeAt(cursor + 1) === BACKSLASH_CODE_UNIT) return {
			kind: "title",
			title: readBoundedOscTitle(data, titleStart, cursor),
			nextIndex: cursor + 2
		};
		return {
			kind: "invalid",
			nextIndex: cursor
		};
	}
	return { kind: "incomplete" };
}
function readBoundedOscTitle(data, titleStart, titleEnd) {
	if (titleEnd - titleStart <= 1024) return data.slice(titleStart, titleEnd);
	const prefixLength = Math.ceil(MAX_OSC_TITLE_CHARS / 2);
	const suffixLength = MAX_OSC_TITLE_CHARS - prefixLength;
	return data.slice(titleStart, titleStart + prefixLength) + data.slice(titleEnd - suffixLength, titleEnd);
}
function extractAllOscTitles(data) {
	if (!data.includes("\x1B]")) return [];
	const titles = [];
	let oldestTitleIndex = 0;
	let searchStart = 0;
	while (searchStart < data.length) {
		const start = data.indexOf("\x1B]", searchStart);
		if (start === -1) break;
		const parsed = parseOscTitleAt(data, start);
		if (parsed.kind === "incomplete") break;
		if (parsed.kind === "title") {
			if (titles.length < 4096) titles.push(parsed.title);
			else {
				titles[oldestTitleIndex] = parsed.title;
				oldestTitleIndex = (oldestTitleIndex + 1) % MAX_OSC_TITLES_PER_CHUNK;
			}
			searchStart = parsed.nextIndex;
			continue;
		}
		searchStart = parsed.nextIndex;
	}
	return oldestTitleIndex === 0 ? titles : [...titles.slice(oldestTitleIndex), ...titles.slice(0, oldestTitleIndex)];
}
var SHELL_NAMES = new Set("|bash|zsh|sh|fish|cmd|cmd.exe|powershell|powershell.exe|pwsh|pwsh.exe|nu".split("|"));
var WINDOWS_PROCESS_EXTENSION_RE = /\.(?:exe|cmd|bat|ps1)$/i;
function isShellProcess(processName) {
	const normalized = processName.trim().replace(/^["']|["']$/g, "").toLowerCase();
	const basename = normalized.split(/[\\/]/).pop() ?? normalized;
	const basenameWithoutWindowsExtension = basename.replace(WINDOWS_PROCESS_EXTENSION_RE, "");
	return SHELL_NAMES.has(normalized) || SHELL_NAMES.has(basename) || SHELL_NAMES.has(basenameWithoutWindowsExtension);
}
function tabHasLivePty(ptyIdsByTabId, tabId) {
	return (ptyIdsByTabId[tabId]?.length ?? 0) > 0;
}
const ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX = "You are working inside Orca, a multi-agent IDE.";
const ORCA_DISPATCH_STATUS_TASK_MARKER = "=== TASK ===";
var ORCA_DISPATCH_STATUS_TASK_ID_MARKER = "Your task ID is:";
var ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT = 24576;
function isOrcaDispatchStatusPrompt(value) {
	const scanEnd = Math.min(value.length, ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT);
	let start = 0;
	while (start < scanEnd && isEcmaTrimWhitespace$1(value.charCodeAt(start))) start++;
	return start + 47 <= scanEnd && value.startsWith("You are working inside Orca, a multi-agent IDE.", start);
}
function compactDispatchPromptForStatus(value, maxLength, normalizeSingleLine) {
	const scanEnd = Math.min(value.length, ORCA_DISPATCH_STATUS_SOURCE_SCAN_LIMIT);
	let start = 0;
	while (start < scanEnd && isEcmaTrimWhitespace$1(value.charCodeAt(start))) start++;
	const scan = value.slice(start, scanEnd);
	let taskId = "";
	const idMarkerIndex = scan.indexOf(ORCA_DISPATCH_STATUS_TASK_ID_MARKER);
	if (idMarkerIndex !== -1) {
		const afterId = scan.slice(idMarkerIndex + 16);
		let idStart = 0;
		while (idStart < afterId.length && isEcmaTrimWhitespace$1(afterId.charCodeAt(idStart))) idStart++;
		const idRest = afterId.slice(idStart);
		const idEnd = idRest.search(/\s/);
		taskId = (idEnd === -1 ? idRest : idRest.slice(0, idEnd)).trim();
	}
	let taskBody = "";
	const taskMarkerIndex = findOrcaDispatchTaskMarkerIndex(scan);
	if (taskMarkerIndex !== -1) {
		const body = scan.slice(taskMarkerIndex + 12);
		for (const line of body.split(/\r?\n/)) {
			const preview = line.trim().replace(/\s+/g, " ");
			if (preview) {
				taskBody = preview;
				break;
			}
		}
	}
	let compact = ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX;
	if (taskId) compact += ` ${ORCA_DISPATCH_STATUS_TASK_ID_MARKER} ${taskId}`;
	if (taskBody) compact += ` ${ORCA_DISPATCH_STATUS_TASK_MARKER} ${taskBody}`;
	return normalizeSingleLine(compact, maxLength);
}
function findOrcaDispatchTaskMarkerIndex(value) {
	let searchFrom = 0;
	while (searchFrom < value.length) {
		const markerIndex = value.indexOf(ORCA_DISPATCH_STATUS_TASK_MARKER, searchFrom);
		if (markerIndex === -1) break;
		const markerEnd = markerIndex + 12;
		const startsLine = markerIndex === 0 || isLineBreak(value.charCodeAt(markerIndex - 1));
		const endsLine = markerEnd === value.length || isLineBreak(value.charCodeAt(markerEnd));
		if (startsLine && endsLine) return markerIndex;
		searchFrom = markerEnd;
	}
	return value.includes("\n") || value.includes("\r") ? -1 : value.indexOf(ORCA_DISPATCH_STATUS_TASK_MARKER);
}
function isLineBreak(code) {
	return code === 10 || code === 13;
}
function isEcmaTrimWhitespace$1(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
var SINGLE_LINE_FIELD_SCAN_OVERHEAD = 64;
var SINGLE_LINE_FIELD_SCAN_MULTIPLIER = 8;
function truncatePreservingSurrogates(value, maxLength) {
	if (value.length < maxLength) return value;
	let truncated = value.length === maxLength ? value : value.slice(0, maxLength);
	const lastCode = truncated.charCodeAt(truncated.length - 1);
	if (lastCode >= 55296 && lastCode <= 56319) truncated = truncated.slice(0, -1);
	return truncated;
}
function normalizeField(value, maxLength = 200) {
	if (typeof value !== "string") return "";
	return normalizeSingleLinePreview(value, maxLength);
}
function normalizePromptField(value) {
	if (typeof value !== "string") return "";
	if (isOrcaDispatchStatusPrompt(value)) return compactDispatchPromptForStatus(value, 200, normalizeSingleLinePreview);
	return normalizeSingleLinePreview(value, 200);
}
function normalizeSingleLinePreview(value, maxLength) {
	const scanEnd = Math.min(value.length, maxLength * SINGLE_LINE_FIELD_SCAN_MULTIPLIER + SINGLE_LINE_FIELD_SCAN_OVERHEAD);
	let index = 0;
	while (index < scanEnd && isEcmaTrimWhitespace(value.charCodeAt(index))) index++;
	let normalized = "";
	let lineSeparatorRun = false;
	while (index < scanEnd && normalized.length < maxLength) {
		const code = value.charCodeAt(index);
		if (isSingleLineSeparator(code)) {
			if (code === 13 && value.charCodeAt(index + 1) === 10) index++;
			if (!lineSeparatorRun) normalized += " ";
			lineSeparatorRun = true;
			index++;
			continue;
		}
		normalized += value[index];
		lineSeparatorRun = false;
		index++;
	}
	if (normalized.length < maxLength) normalized = trimTrailingWhitespace(normalized);
	return truncatePreservingSurrogates(normalized, maxLength);
}
function normalizeMultilineField(value, maxLength) {
	if (typeof value !== "string") return "";
	const { start, end } = getTrimmedStringBounds(value);
	let normalized = "";
	let newlineRun = 0;
	for (let index = start; index < end && normalized.length < maxLength; index++) {
		const code = value.charCodeAt(index);
		if (code === 13 || code === 10 || code === 8232 || code === 8233) {
			if (code === 13 && value.charCodeAt(index + 1) === 10) index++;
			if (newlineRun < 2) normalized += "\n";
			newlineRun++;
			continue;
		}
		normalized += value[index];
		newlineRun = 0;
	}
	return truncatePreservingSurrogates(normalized, maxLength);
}
function getTrimmedStringBounds(value) {
	let start = 0;
	let end = value.length;
	while (start < end && isEcmaTrimWhitespace(value.charCodeAt(start))) start++;
	while (end > start && isEcmaTrimWhitespace(value.charCodeAt(end - 1))) end--;
	return {
		start,
		end
	};
}
function trimTrailingWhitespace(value) {
	let end = value.length;
	while (end > 0 && isEcmaTrimWhitespace(value.charCodeAt(end - 1))) end--;
	return end === value.length ? value : value.slice(0, end);
}
function isSingleLineSeparator(code) {
	return code === 13 || code === 10 || code === 8232 || code === 8233;
}
function isEcmaTrimWhitespace(code) {
	return code === 32 || code >= 9 && code <= 13 || code === 160 || code === 5760 || code >= 8192 && code <= 8202 || code === 8232 || code === 8233 || code === 8239 || code === 8287 || code === 12288 || code === 65279;
}
function normalizeInteractivePromptField(value, maxLength) {
	if (typeof value !== "string" || value.length === 0) return;
	const truncated = truncatePreservingSurrogates(value, maxLength);
	return truncated.length > 0 ? truncated : void 0;
}
function normalizeOptionalField(value, maxLength) {
	if (typeof value !== "string") return;
	const normalized = normalizeField(value, maxLength);
	return normalized.length > 0 ? normalized : void 0;
}
function normalizeOptionalMultilineField(value, maxLength) {
	if (typeof value !== "string") return;
	const normalized = normalizeMultilineField(value, maxLength);
	return normalized.length > 0 ? normalized : void 0;
}
const AGENT_STATUS_STATES = [
	"working",
	"blocked",
	"waiting",
	"done"
];
const AGENT_STATE_HISTORY_MAX = 20;
const AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH = 8e3;
const AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH = 16e3;
const AGENT_STATUS_STALE_AFTER_MS = 1800 * 1e3;
function isFreshNonDoneAgentStatus(entry, now = Date.now(), staleAfterMs = AGENT_STATUS_STALE_AFTER_MS) {
	return Boolean(entry && entry.state !== "done" && entry.restoredUnconfirmed !== true && now - entry.updatedAt <= staleAfterMs);
}
var VALID_STATES = new Set(AGENT_STATUS_STATES);
const AGENT_STATUS_JSON_STRUCTURE_LIMITS = {
	structuralTokens: 4096,
	nestingDepth: 16
};
var AGENT_SUBAGENT_ID_MAX_LENGTH = 64;
function normalizeSubagentSnapshot(value) {
	if (typeof value !== "object" || value === null) return null;
	const obj = value;
	if (typeof obj.id !== "string") return null;
	const id = obj.id.trim();
	if (id.length === 0 || id.length > AGENT_SUBAGENT_ID_MAX_LENGTH) return null;
	if (obj.state !== "working" && obj.state !== "blocked" && obj.state !== "waiting" && obj.state !== "idle") return null;
	return {
		id,
		state: obj.state,
		startedAt: typeof obj.startedAt === "number" && Number.isFinite(obj.startedAt) ? obj.startedAt : 0,
		agentType: normalizeOptionalField(obj.agentType, 40),
		model: normalizeOptionalField(obj.model, 120),
		description: normalizeOptionalField(obj.description, 160)
	};
}
function normalizeSubagentsField(value) {
	if (!Array.isArray(value) || value.length === 0) return;
	const normalized = [];
	for (const item of value) {
		const snapshot = normalizeSubagentSnapshot(item);
		if (snapshot) {
			normalized.push(snapshot);
			if (normalized.length >= 32) break;
		}
	}
	return normalized.length > 0 ? normalized : void 0;
}
function agentSubagentsEqual(a, b) {
	if (a === b) return true;
	if (!a || !b || a.length !== b.length) return !a && !b;
	for (let i = 0; i < a.length; i++) {
		const x = a[i];
		const y = b[i];
		if (x.id !== y.id || x.state !== y.state || x.startedAt !== y.startedAt || x.agentType !== y.agentType || x.model !== y.model || x.description !== y.description) return false;
	}
	return true;
}
function normalizeAgentStatusObject(parsed) {
	if (typeof parsed !== "object" || parsed === null) return null;
	const obj = parsed;
	if (typeof obj.state !== "string") return null;
	const state = obj.state;
	if (!VALID_STATES.has(state)) return null;
	return {
		state,
		prompt: normalizePromptField(obj.prompt),
		agentType: normalizeOptionalField(obj.agentType, 40),
		model: normalizeOptionalField(obj.model, 120),
		toolName: normalizeOptionalField(obj.toolName, 60),
		toolInput: normalizeOptionalField(obj.toolInput, 160),
		interactivePrompt: normalizeInteractivePromptField(obj.interactivePrompt, AGENT_STATUS_INTERACTIVE_PROMPT_MAX_LENGTH),
		lastAssistantMessage: normalizeOptionalMultilineField(obj.lastAssistantMessage, AGENT_STATUS_ASSISTANT_MESSAGE_MAX_LENGTH),
		interrupted: obj.interrupted === true && state === "done" ? true : void 0,
		sessionBoundary: obj.sessionBoundary === true && state === "done" ? true : void 0,
		subagents: normalizeSubagentsField(obj.subagents)
	};
}
function normalizeAgentStatusPayload(payload) {
	return normalizeAgentStatusObject(payload);
}
function parseAgentStatusPayload(json) {
	try {
		assertJsonTextStructureWithinLimits(json, AGENT_STATUS_JSON_STRUCTURE_LIMITS);
		return normalizeAgentStatusObject(JSON.parse(json));
	} catch {
		return null;
	}
}
function isExplicitAgentStatusFresh(entry, now, staleAfterMs) {
	return entry.restoredUnconfirmed !== true && now - entry.updatedAt <= staleAfterMs;
}
function classifyTitleActivity(title) {
	return detectAgentStatusFromTitle(title);
}
function resolveTitleActivityLabel(title) {
	return getAgentLabel(title);
}
function resolveCommittedTitleAgentType(title) {
	return resolveExplicitTerminalTitleAgentType(title);
}
function resolvePaneAgentActivity(input) {
	const freshEntry = input.explicitEntry && isExplicitAgentStatusFresh(input.explicitEntry, input.now, 18e5) ? input.explicitEntry : null;
	const titleStatus = input.liveTitle !== null ? detectAgentStatusFromTitle(input.liveTitle) : null;
	if (freshEntry) return {
		hookState: freshEntry.state,
		hookAgentType: freshEntry.agentType,
		titleStatus,
		source: "hook",
		confidence: "authoritative",
		livePtyRequired: false
	};
	if (titleStatus !== null) return {
		hookState: null,
		hookAgentType: void 0,
		titleStatus,
		source: "title",
		confidence: "fallback",
		livePtyRequired: !input.hasLivePty
	};
	return {
		hookState: null,
		hookAgentType: void 0,
		titleStatus: null,
		source: "none",
		confidence: "authoritative",
		livePtyRequired: false
	};
}
var WELL_KNOWN_LABELS = {
	claude: "Claude",
	openclaude: "OpenClaude",
	codex: "Codex",
	gemini: "Gemini",
	antigravity: "Antigravity",
	amp: "Amp",
	copilot: "GitHub Copilot",
	opencode: "OpenCode",
	"mimo-code": "MiMo Code",
	cursor: "Cursor",
	aider: "Aider",
	pi: "Pi",
	omp: "OMP",
	"prime-agent": "Prime Agent",
	droid: "Droid",
	"command-code": "Command Code",
	grok: "Grok",
	hermes: "Hermes",
	devin: "Devin",
	ante: "Ante",
	trae: "Trae",
	kimi: "Kimi"
};
function formatAgentTypeLabel(agentType) {
	if (!agentType || agentType === "unknown") return "Agent";
	return WELL_KNOWN_LABELS[agentType] ?? agentType;
}
var ICONABLE_AGENT_TYPES = {
	claude: true,
	"claude-agent-teams": true,
	openclaude: true,
	codex: true,
	autohand: true,
	opencode: true,
	"mimo-code": true,
	pi: true,
	omp: true,
	"prime-agent": true,
	gemini: true,
	antigravity: true,
	aider: true,
	goose: true,
	amp: true,
	kilo: true,
	kiro: true,
	crush: true,
	aug: true,
	cline: true,
	codebuff: true,
	"command-code": true,
	continue: true,
	cursor: true,
	droid: true,
	kimi: true,
	"mistral-vibe": true,
	"qwen-code": true,
	rovo: true,
	hermes: true,
	openclaw: true,
	copilot: true,
	grok: true,
	devin: true,
	ante: true,
	trae: true
};
function agentTypeToIconAgent(agentType) {
	if (!agentType || agentType === "unknown") return null;
	return Object.hasOwn(ICONABLE_AGENT_TYPES, agentType) ? agentType : null;
}
function agentKindForAgentType(agentType) {
	const tuiAgent = agentTypeToIconAgent(agentType);
	return tuiAgent ? tuiAgentToAgentKind(tuiAgent) : "other";
}
export { getRepoExecutionHostId as $, stripLeadingAgentTitleDecoration as A, isGeminiTerminalTitle as B, createAgentStatusTracker as C, resolveExplicitTerminalTitleAgentType as D, isClaudeIdentityFrameTitle as E, isOpenCodeNativeTitle as F, DROID_AGENT_NAME_RE as G, shouldSuppressCursorNativeTitle as H, containsAgentSpinnerGlyph as I, titleHasAnyLegacyAgentName as J, HERMES_AGENT_NAME_RE as K, isClaudeManagementTitle as L, getAgentLabel as M, isClaudeAgent as N, resolveTerminalTitleAgentType as O, isMeaningfulOpenCodeTerminalTitle as P, getLocalExecutionHostLabel as Q, isCursorAgentTitle as R, clearWorkingIndicators as S, normalizeTerminalTitle as T, isLegacyPiCompatibleTitle as U, isPiTerminalTitle as V, AGY_AGENT_NAME_RE as W, LOCAL_EXECUTION_HOST_ID as X, ALL_EXECUTION_HOSTS_SCOPE as Y, getExecutionHostLabel as Z, findOrcaDispatchTaskMarkerIndex as _, isExplicitAgentStatusFresh as a, normalizeExecutionHostScope as at, MAX_OSC_TITLE_CHARS as b, resolveTitleActivityLabel as c, toRuntimeExecutionHostId as ct, agentSubagentsEqual as d, TUI_AGENT_CONFIG as dt, getSettingsFocusedExecutionHostId as et, isFreshNonDoneAgentStatus as f, getTuiAgentDetectCommands as ft, ORCA_DISPATCH_STATUS_TASK_MARKER as g, ORCA_DISPATCH_STATUS_PREAMBLE_PREFIX as h, classifyTitleActivity as i, normalizeExecutionHostOrder as it, stripLeadingAgentTitleDecorationOrEmpty as j, getWrapperTitleSegments as k, AGENT_STATE_HISTORY_MAX as l, toSshExecutionHostId as lt, parseAgentStatusPayload as m, isTuiAgent as mt, agentTypeToIconAgent as n, isRuntimeOwnedSshTargetId as nt, resolveCommittedTitleAgentType as o, normalizeVisibleExecutionHostIds as ot, normalizeAgentStatusPayload as p, getTuiAgentLaunchCommand as pt, titleHasAgentName as q, formatAgentTypeLabel as r, normalizeExecutionHostId as rt, resolvePaneAgentActivity as s, parseExecutionHostId as st, agentKindForAgentType as t, getWorktreeExecutionHostId as tt, AGENT_STATUS_STALE_AFTER_MS as u, assertJsonTextStructureWithinLimits as ut, tabHasLivePty as v, detectAgentStatusFromTitle as w, extractAllOscTitles as x, isShellProcess as y, isCursorNativeAgentTitle as z };
