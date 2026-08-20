import { Cp as getRepoIdFromWorktreeId, Eu as getExecutionHostIdForWorktree, Hp as parseWslUncPath, Ia as getFolderWorkspaceCandidateRepos, Ou as getRuntimeEnvironmentIdForWorktree, Vp as isWslUncPath, _h as resolveTuiAgentLaunchEnv, bf as isResumableTuiAgent, bo as reconcileTabOrder, cf as commandSeparator, da as getLocalProjectExecutionRuntimeContext, df as quoteStartupArg, ef as buildAgentResumeStartupPlan, fc as resolveLocalWindowsTerminalShellOverrideForTab, ff as resolveLoginShellStartupDialect, gh as resolveTuiAgentLaunchArgs, lc as resolveWindowsShellStartupFamily, lf as isPosixStartupShell, pf as resolveStartupShell, qt as getIndexedWorktreeMap, sf as clearEnvCommand, t as useAppStore, to as measureClipboardTextByteLength, vd as parseWorkspaceKey } from "./store-CgXrfmaH.js";
import { $ as getRepoExecutionHostId, X as LOCAL_EXECUTION_HOST_ID, dt as TUI_AGENT_CONFIG, lt as toSshExecutionHostId, rt as normalizeExecutionHostId, st as parseExecutionHostId } from "./agent-status-3vUKbY6l.js";
import { n as tuiAgentToAgentKind } from "./agent-kind-Dfx6MnkP.js";
import { d as isWebRuntimeSessionActive, u as createWebRuntimeSessionTerminal } from "./web-runtime-session-CN2syA39.js";
import { m as CLIENT_PLATFORM } from "./native-chat-session-option-cache-DGE3h47U.js";
function getAiVaultResumeRepoTargetStatus(repo) {
	if (!repo) return "unknown";
	return getAiVaultResumeExecutionHostTargetStatus(getRepoExecutionHostId(repo));
}
function isSupportedAiVaultResumeTargetStatus(status) {
	return status === "local" || status === "ssh" || status === "runtime";
}
function isWslStoredAiVaultSessionFile(sessionFilePath) {
	return Boolean(sessionFilePath && isWslUncPath(sessionFilePath));
}
function canResumeAiVaultSessionOnTarget(args) {
	const sessionExecutionHostId = normalizeExecutionHostId(args.sessionExecutionHostId);
	const targetExecutionHostId = normalizeExecutionHostId(args.targetExecutionHostId);
	if (args.targetStatus === "runtime") return Boolean(sessionExecutionHostId && targetExecutionHostId && sessionExecutionHostId === targetExecutionHostId);
	if (!isSupportedAiVaultResumeTargetStatus(args.targetStatus)) return false;
	if (sessionExecutionHostId) {
		if (targetExecutionHostId) {
			if (sessionExecutionHostId === targetExecutionHostId) return true;
			return sessionExecutionHostId === "local" && args.targetStatus === "ssh" && isWslStoredAiVaultSessionFile(args.sessionFilePath);
		}
		if (sessionExecutionHostId !== "local") return false;
	}
	if (args.targetStatus === "ssh") return isWslStoredAiVaultSessionFile(args.sessionFilePath);
	return true;
}
function getAiVaultResumeWorkspaceExecutionHostId(state, workspaceId) {
	if (!workspaceId) return null;
	const workspaceKey = parseWorkspaceKey(workspaceId);
	if (workspaceKey?.type === "folder") return getAiVaultResumeFolderExecutionHostId(state, workspaceKey.folderWorkspaceId);
	const worktreeId = workspaceKey?.type === "worktree" ? workspaceKey.worktreeId : workspaceId;
	const worktree = getIndexedWorktreeMap(state.worktreesByRepo ?? {}).get(worktreeId);
	const worktreeHostId = normalizeExecutionHostId(worktree?.hostId);
	if (worktreeHostId) return worktreeHostId;
	const repoId = worktree?.repoId ?? getRepoIdFromWorktreeId(worktreeId);
	const repo = state.repos.find((candidate) => candidate.id === repoId);
	return repo ? getRepoExecutionHostId(repo) : null;
}
function getAiVaultResumeWorkspaceTargetStatus(state, workspaceId) {
	if (!workspaceId) return "unknown";
	const workspaceKey = parseWorkspaceKey(workspaceId);
	if (workspaceKey?.type === "folder") return getAiVaultResumeFolderTargetStatus(state, workspaceKey.folderWorkspaceId);
	const worktreeId = workspaceKey?.type === "worktree" ? workspaceKey.worktreeId : workspaceId;
	const worktree = getIndexedWorktreeMap(state.worktreesByRepo ?? {}).get(worktreeId);
	const worktreeHost = getAiVaultResumeExecutionHostTargetStatus(worktree?.hostId);
	if (worktreeHost !== "unknown") return worktreeHost;
	const repoId = worktree?.repoId ?? getRepoIdFromWorktreeId(worktreeId);
	return getAiVaultResumeRepoTargetStatus(state.repos.find((repo) => repo.id === repoId));
}
function getAiVaultResumeFolderTargetStatus(state, folderWorkspaceId) {
	const workspace = state.folderWorkspaces.find((entry) => entry.id === folderWorkspaceId);
	if (!workspace) return "unknown";
	const group = state.projectGroups.find((entry) => entry.id === workspace.projectGroupId);
	const groupHostId = normalizeExecutionHostId(workspace.executionHostId ?? group?.executionHostId);
	if (groupHostId) return getAiVaultResumeExecutionHostTargetStatus(groupHostId);
	const explicitConnectionId = (workspace.connectionId ?? group?.connectionId ?? "").trim();
	if (explicitConnectionId) return getAiVaultResumeExecutionHostTargetStatus(toSshExecutionHostId(explicitConnectionId));
	return mergeAiVaultResumeExecutionHostTargetStatuses(getFolderWorkspaceCandidateRepos(state, folderWorkspaceId).map(getRepoExecutionHostId));
}
function getAiVaultResumeFolderExecutionHostId(state, folderWorkspaceId) {
	const workspace = state.folderWorkspaces.find((entry) => entry.id === folderWorkspaceId);
	if (!workspace) return null;
	const group = state.projectGroups.find((entry) => entry.id === workspace.projectGroupId);
	const groupHostId = normalizeExecutionHostId(workspace.executionHostId ?? group?.executionHostId);
	if (groupHostId) return groupHostId;
	const explicitConnectionId = (workspace.connectionId ?? group?.connectionId ?? "").trim();
	if (explicitConnectionId) return toSshExecutionHostId(explicitConnectionId);
	return mergeAiVaultResumeExecutionHostIds(getFolderWorkspaceCandidateRepos(state, folderWorkspaceId).map(getRepoExecutionHostId));
}
function getAiVaultResumeExecutionHostTargetStatus(hostId) {
	const parsed = parseExecutionHostId(hostId);
	if (!parsed) return "unknown";
	if (parsed.kind === "local") return "local";
	return parsed.kind;
}
function mergeAiVaultResumeExecutionHostTargetStatuses(hostIds) {
	if (hostIds.length === 0) return "local";
	const statuses = hostIds.map(getAiVaultResumeExecutionHostTargetStatus);
	if (new Set(statuses).has("runtime")) return "runtime";
	return new Set(hostIds).size === 1 ? statuses[0] ?? "unknown" : "unknown";
}
function mergeAiVaultResumeExecutionHostIds(hostIds) {
	if (hostIds.length === 0) return LOCAL_EXECUTION_HOST_ID;
	return new Set(hostIds).size === 1 ? hostIds[0] ?? null : null;
}
const AI_VAULT_AGENTS = [
	"claude",
	"codex",
	"hermes",
	"pi",
	"omp",
	"prime-agent",
	"cursor",
	"gemini",
	"antigravity",
	"rovo",
	"copilot",
	"opencode",
	"grok",
	"openclaw",
	"devin",
	"droid",
	"kimi"
];
function isAiVaultScanCancelledError(error) {
	return error instanceof Error && (error.name === "AbortError" || error.message.includes("Agent Session History scan was cancelled"));
}
const AI_VAULT_AGENT_LABELS = {
	claude: "Claude",
	codex: "Codex",
	hermes: "Hermes",
	pi: "Pi",
	omp: "OMP",
	"prime-agent": "Prime Agent",
	cursor: "Cursor",
	gemini: "Gemini",
	antigravity: "Antigravity",
	rovo: "Rovo Dev",
	copilot: "GitHub Copilot",
	opencode: "OpenCode",
	grok: "Grok",
	openclaw: "OpenClaw",
	devin: "Devin",
	droid: "Droid",
	kimi: "Kimi"
};
function isAiVaultSessionResumableContent(session) {
	return session.messageCount > 0 || session.previewMessages.some((message) => message.role === "user" || message.role === "assistant");
}
function aiVaultSessionRecoverableSignalCount(session) {
	return Math.max(0, session.queuedMessageCount) + Math.max(0, session.subagentTranscriptCount);
}
function isAiVaultSessionRecoverableEmpty(session) {
	return !isAiVaultSessionResumableContent(session) && aiVaultSessionRecoverableSignalCount(session) > 0;
}
function aiVaultAgentLabel(agent) {
	return AI_VAULT_AGENT_LABELS[agent];
}
const AI_VAULT_SESSION_DRAG_TYPE = "application/x-orca-ai-vault-session";
const AI_VAULT_SESSION_DRAG_START_EVENT = "orca-ai-vault-session-drag-start";
const AI_VAULT_SESSION_DRAG_END_EVENT = "orca-ai-vault-session-drag-end";
var activeAiVaultSessionDragPayload = null;
function isAiVaultAgent(value) {
	return typeof value === "string" && AI_VAULT_AGENTS.includes(value);
}
function isNonEmptyString(value) {
	return typeof value === "string" && value.trim().length > 0;
}
function isStringRecord(value) {
	if (!value || typeof value !== "object" || Array.isArray(value)) return false;
	return Object.values(value).every((entry) => typeof entry === "string");
}
function isEnvDeletionList(value) {
	return Array.isArray(value) && value.length <= 32 && value.every((entry) => typeof entry === "string" && entry.length > 0 && entry.length <= 256);
}
function isLaunchConfig(value) {
	if (!value || typeof value !== "object") return false;
	const config = value;
	return (config.agentCommand === void 0 || typeof config.agentCommand === "string") && typeof config.agentArgs === "string" && isStringRecord(config.agentEnv) && (config.ompResumeFilePath === void 0 || isNonEmptyString(config.ompResumeFilePath));
}
function isSerializedPayload(value) {
	if (!value || typeof value !== "object") return false;
	const payload = value;
	return payload.kind === "ai-vault-session" && payload.version === 1 && isAiVaultAgent(payload.agent) && isNonEmptyString(payload.sessionId) && isNonEmptyString(payload.title) && isNonEmptyString(payload.command) && (payload.sessionFilePath === void 0 || isNonEmptyString(payload.sessionFilePath)) && (payload.sessionExecutionHostId === void 0 || Boolean(normalizeExecutionHostId(payload.sessionExecutionHostId))) && (payload.codexHome === void 0 || payload.codexHome === null || isNonEmptyString(payload.codexHome)) && (payload.sessionCwd === void 0 || payload.sessionCwd === null || isNonEmptyString(payload.sessionCwd)) && (payload.env === void 0 || isStringRecord(payload.env)) && (payload.envToDelete === void 0 || isEnvDeletionList(payload.envToDelete)) && (payload.launchConfig === void 0 || isLaunchConfig(payload.launchConfig)) && (payload.realHomeStartup === void 0 || isResumeStartup(payload.realHomeStartup));
}
function isResumeStartup(value) {
	if (!value || typeof value !== "object") return false;
	const startup = value;
	return isNonEmptyString(startup.command) && (startup.env === void 0 || isStringRecord(startup.env)) && (startup.envToDelete === void 0 || isEnvDeletionList(startup.envToDelete)) && (startup.launchConfig === void 0 || isLaunchConfig(startup.launchConfig));
}
function writeAiVaultSessionDragData(dataTransfer, payload) {
	const serialized = JSON.stringify({
		kind: "ai-vault-session",
		version: 1,
		...payload
	});
	if (isAiVaultSessionDragPayloadTooLarge(serialized)) {
		activeAiVaultSessionDragPayload = null;
		dataTransfer.effectAllowed = "copy";
		dataTransfer.setData(AI_VAULT_SESSION_DRAG_TYPE, "");
		return;
	}
	activeAiVaultSessionDragPayload = { ...payload };
	dataTransfer.effectAllowed = "copy";
	dataTransfer.setData(AI_VAULT_SESSION_DRAG_TYPE, serialized);
}
function hasAiVaultSessionDragData(dataTransfer) {
	return Array.from(dataTransfer.types).includes(AI_VAULT_SESSION_DRAG_TYPE);
}
function clearAiVaultSessionDragData() {
	activeAiVaultSessionDragPayload = null;
}
function readAiVaultSessionDragData(dataTransfer) {
	const raw = dataTransfer.getData(AI_VAULT_SESSION_DRAG_TYPE);
	if (!raw) return hasAiVaultSessionDragData(dataTransfer) ? activeAiVaultSessionDragPayload : null;
	if (isAiVaultSessionDragPayloadTooLarge(raw)) return null;
	try {
		const parsed = JSON.parse(raw);
		if (!isSerializedPayload(parsed)) return null;
		const { agent, sessionId, title, command, sessionFilePath, sessionExecutionHostId, codexHome, sessionCwd, env, envToDelete, launchConfig, realHomeStartup } = parsed;
		return {
			agent,
			sessionId,
			title,
			command,
			...sessionFilePath ? { sessionFilePath } : {},
			...sessionExecutionHostId ? { sessionExecutionHostId } : {},
			...codexHome !== void 0 ? { codexHome } : {},
			...sessionCwd !== void 0 ? { sessionCwd } : {},
			...env ? { env } : {},
			...envToDelete ? { envToDelete } : {},
			...launchConfig ? { launchConfig } : {},
			...realHomeStartup ? { realHomeStartup } : {}
		};
	} catch {
		return null;
	}
}
function isAiVaultSessionDragPayloadTooLarge(raw) {
	return raw.length > 16384 || measureClipboardTextByteLength(raw, { stopAfterBytes: 16384 }).exceededLimit;
}
function buildAiVaultResumeCommand(args) {
	const { agent, sessionId, cwd, platform, commandOverride, codexHome, resumeFilePath, shell } = args;
	const baseCommand = commandOverride?.trim() || defaultAiVaultResumeCommandBase(agent);
	const resumeTarget = (agent === "omp" || agent === "prime-agent") && resumeFilePath?.trim() ? resumeFilePath.trim() : sessionId;
	return buildAiVaultResumeShellCommand({
		resumeCommand: buildAgentResumeInvocation(agent, baseCommand, shell === "cmd" ? quoteWindowsCmdArg(resumeTarget) : shell ? quoteStartupArg(resumeTarget, shell) : quoteShellArg(resumeTarget, platform)),
		cwd,
		platform,
		codexHome,
		shell
	});
}
function buildAiVaultResumeShellCommand(args) {
	const { cwd, platform, codexHome, shell } = args;
	if (platform === "win32" && shell && shell !== "cmd") return buildResumeShellCommandForShell({
		resumeCommand: args.resumeCommand,
		cwd,
		codexHome: codexHome?.trim() || null,
		shell
	});
	const resumeCommand = `${codexHomeEnvPrefix(codexHome?.trim() || null, platform)}${args.resumeCommand}`;
	if (platform === "win32" && shell === "cmd") return cwd ? `cd /d ${quoteWindowsCmdArg(cwd)} && ${resumeCommand}` : resumeCommand;
	if (!cwd) return resumeCommand;
	if (platform === "win32") return `cmd /d /s /c ${quoteWindowsCmdArg(`cd /d ${quoteWindowsCmdArg(cwd)} && ${resumeCommand}`)}`;
	return `cd ${quoteShellArg(cwd, platform)} && ${resumeCommand}`;
}
function buildResumeShellCommandForShell(args) {
	const { cwd, codexHome, shell } = args;
	if (isPosixStartupShell(shell)) {
		const command = `${codexHome ? `CODEX_HOME=${quoteStartupArg(codexHome, shell)} ` : ""}${args.resumeCommand}`;
		return cwd ? `cd ${quoteStartupArg(cwd, shell)} && ${command}` : command;
	}
	const separator = commandSeparator(shell);
	const segments = [];
	if (cwd) segments.push(`Set-Location -LiteralPath ${quoteStartupArg(cwd, shell)}`);
	if (codexHome) segments.push(`$env:CODEX_HOME=${quoteStartupArg(codexHome, shell)}`);
	segments.push(args.resumeCommand);
	return segments.join(separator);
}
function realHomeCodexResumeEnvDeletion(session) {
	if (session.agent !== "codex" || session.codexHome !== null) return {};
	return { envToDelete: ["CODEX_HOME", "ORCA_CODEX_HOME"] };
}
function defaultAiVaultResumeCommandBase(agent) {
	if (agent === "cursor") return "cursor-agent";
	if (agent === "hermes") return "hermes";
	if (agent === "rovo") return "acli";
	return TUI_AGENT_CONFIG[agent].detectCmd;
}
function buildAgentResumeInvocation(agent, baseCommand, sessionArg) {
	switch (agent) {
		case "codex": return `${baseCommand} resume ${sessionArg}`;
		case "rovo": return `${baseCommand} rovodev run --restore ${sessionArg}`;
		case "opencode":
		case "pi":
		case "kimi": return `${baseCommand} --session ${sessionArg}`;
		case "copilot": return `${baseCommand} --resume=${sessionArg}`;
		case "claude":
		case "cursor":
		case "gemini":
		case "grok":
		case "hermes":
		case "devin":
		case "openclaw":
		case "droid":
		case "omp":
		case "prime-agent": return `${baseCommand} --resume ${sessionArg}`;
		case "antigravity": return `${baseCommand} --conversation ${sessionArg}`;
	}
}
function codexHomeEnvPrefix(codexHome, platform) {
	if (!codexHome) return "";
	if (platform === "win32") return `set ${quoteWindowsCmdArg(`CODEX_HOME=${codexHome}`)} && `;
	return `CODEX_HOME=${quoteShellArg(codexHome, platform)} `;
}
function quoteShellArg(value, platform) {
	if (platform === "win32") return quoteWindowsCmdArg(value);
	return `'${value.replace(/'/g, `'\\''`)}'`;
}
function quoteWindowsCmdArg(value) {
	return `"${value.replace(/"/g, "\"\"")}"`;
}
function normalizeAiVaultResumeFilePath(filePath, platform) {
	if (!filePath || platform !== "linux") return filePath;
	return parseWslUncPath(filePath)?.linuxPath ?? filePath;
}
function getClientLoginShell() {
	try {
		return window.api?.platform?.get?.().shell ?? "";
	} catch {
		return "";
	}
}
function resolveAiVaultResumeStartupShell(args) {
	if (args.platform !== "win32") return args.parsedByClientLoginShell ? resolveLoginShellStartupDialect(getClientLoginShell()) : "posix";
	const projectRuntime = args.isLocalSession ? getLocalProjectExecutionRuntimeContext(args.state, args.worktreeId, CLIENT_PLATFORM) : void 0;
	const workspacePath = getAiVaultResumeWorkspacePath(args.state, args.worktreeId ?? args.state.activeWorktreeId);
	const shellOverride = args.isLocalSession ? resolveLocalWindowsTerminalShellOverrideForTab({
		explicitShellOverride: void 0,
		defaultWindowsShell: args.state.settings?.terminalWindowsShell,
		isWslWorktree: Boolean(workspacePath && parseWslUncPath(workspacePath)),
		projectRuntime
	}) : void 0;
	const shell = shellOverride ? resolveWindowsShellStartupFamily(shellOverride) : void 0;
	return resolveStartupShell(args.platform, shell);
}
function getAiVaultResumeWorkspacePath(state, worktreeId) {
	if (!worktreeId) return null;
	const workspaceScope = parseWorkspaceKey(worktreeId);
	if (workspaceScope?.type === "folder") return state.folderWorkspaces.find((workspace) => workspace.id === workspaceScope.folderWorkspaceId)?.folderPath ?? null;
	const targetWorktreeId = workspaceScope?.type === "worktree" ? workspaceScope.worktreeId : worktreeId;
	return Object.values(state.worktreesByRepo ?? {}).flat().find((candidate) => candidate.id === targetWorktreeId)?.path ?? null;
}
function buildAiVaultResumeCopyCommandForWorktree(args) {
	const command = buildAiVaultResumeForWorktree(args, true).command;
	if (args.session.agent !== "codex" || args.session.codexHome !== null) return command;
	const shell = resolveAiVaultResumeShell(args);
	const separator = commandSeparator(shell);
	return `${["CODEX_HOME", "ORCA_CODEX_HOME"].map((name) => clearEnvCommand(name, shell)).join(separator)}${separator}${command}`;
}
function buildAiVaultResumeStartupForWorktree(args) {
	return buildAiVaultResumeForWorktree(args, false);
}
function buildAiVaultDropRepinStartup(args) {
	if (args.payload.sessionCwd === void 0 || !args.payload.sessionFilePath) return null;
	return buildAiVaultResumeStartupForWorktree({
		state: args.state,
		worktreeId: args.worktreeId,
		session: {
			agent: args.payload.agent,
			sessionId: args.payload.sessionId,
			cwd: args.payload.sessionCwd,
			codexHome: args.substituteCodexHome,
			executionHostId: args.payload.sessionExecutionHostId,
			filePath: args.payload.sessionFilePath
		},
		commandOverride: args.state.settings?.agentCmdOverrides?.[args.payload.agent]
	});
}
function buildAiVaultResumeForWorktree(args, embedCwd) {
	const providerSession = getAiVaultAgentProviderSession(args.session);
	if (args.session.executionHostId && args.session.executionHostId !== "local" && args.session.resumeCommand && args.session.agent !== "omp" && !(args.session.agent === "codex" && args.session.codexHome === null) && !args.commandOverride?.trim()) return {
		command: args.session.resumeCommand,
		...realHomeCodexResumeEnvDeletion(args.session),
		...providerSession ? { providerSession } : {}
	};
	const platform = args.session.executionHostId && args.session.executionHostId !== "local" && args.session.executionHostPlatform ? args.session.executionHostPlatform : getAiVaultResumePlatform(args.state, args.worktreeId);
	const codexHome = getAiVaultResumeCodexHome(args.session.codexHome, platform);
	const isLocalSession = !args.session.executionHostId || args.session.executionHostId === "local";
	const resumeFilePath = normalizeAiVaultResumeFilePath(args.session.filePath, platform);
	const liveShell = platform === "win32" ? isLocalSession ? resolveAiVaultResumeShell(args) : "powershell" : void 0;
	const cwd = embedCwd ? args.session.cwd : null;
	const startupCwd = !embedCwd && args.session.cwd ? { cwd: args.session.cwd } : {};
	if (providerSession && isResumableTuiAgent(args.session.agent)) {
		const startupPlan = buildAgentResumeStartupPlan({
			agent: args.session.agent,
			providerSession,
			cmdOverrides: {
				...args.state.settings?.agentCmdOverrides,
				...args.commandOverride?.trim() ? { [args.session.agent]: args.commandOverride } : {}
			},
			platform,
			shell: liveShell,
			agentArgs: resolveTuiAgentLaunchArgs(args.session.agent, args.state.settings?.agentDefaultArgs),
			agentEnv: resolveTuiAgentLaunchEnv(args.session.agent, args.state.settings?.agentDefaultEnv),
			...args.session.agent === "omp" && resumeFilePath ? { ompResumeFilePath: resumeFilePath } : {}
		});
		if (startupPlan) return {
			command: args.session.agent === "omp" ? buildAiVaultResumeCommand({
				agent: args.session.agent,
				sessionId: args.session.sessionId,
				resumeFilePath,
				cwd,
				platform,
				commandOverride: startupPlan.launchConfig.agentCommand,
				codexHome,
				shell: liveShell
			}) : buildAiVaultResumeShellCommand({
				resumeCommand: startupPlan.launchCommand,
				cwd,
				platform,
				codexHome,
				shell: liveShell
			}),
			...startupPlan.env ? { env: startupPlan.env } : {},
			...realHomeCodexResumeEnvDeletion(args.session),
			...startupCwd,
			launchConfig: startupPlan.launchConfig,
			providerSession
		};
	}
	return {
		command: buildAiVaultResumeCommand({
			agent: args.session.agent,
			sessionId: args.session.sessionId,
			resumeFilePath,
			cwd,
			platform,
			commandOverride: args.commandOverride,
			codexHome,
			shell: liveShell
		}),
		...startupCwd,
		...realHomeCodexResumeEnvDeletion(args.session)
	};
}
function resolveAiVaultResumeShell(args) {
	const platform = args.session.executionHostId && args.session.executionHostId !== "local" && args.session.executionHostPlatform ? args.session.executionHostPlatform : getAiVaultResumePlatform(args.state, args.worktreeId);
	const isLocalSession = !args.session.executionHostId || args.session.executionHostId === "local";
	return resolveAiVaultResumeStartupShell({
		state: args.state,
		worktreeId: args.worktreeId,
		platform,
		isLocalSession,
		parsedByClientLoginShell: isLocalSession && runsOnClientLoginShell(args, platform)
	});
}
function runsOnClientLoginShell(args, platform) {
	const executionHost = parseExecutionHostId(getExecutionHostIdForWorktree(args.state, args.worktreeId ?? args.state.activeWorktreeId));
	if (executionHost?.kind === "ssh" || executionHost?.kind === "runtime") return false;
	return platform === CLIENT_PLATFORM;
}
function getAiVaultAgentProviderSession(session) {
	if (!isResumableTuiAgent(session.agent)) return null;
	if (session.agent === "antigravity") return {
		key: "conversation_id",
		id: session.sessionId
	};
	if (session.agent === "pi" || session.agent === "prime-agent") return session.filePath ? {
		key: "session_id",
		id: session.sessionId,
		transcriptPath: session.filePath
	} : null;
	return {
		key: "session_id",
		id: session.sessionId
	};
}
function getAiVaultResumeCodexHome(codexHome, platform) {
	if (!codexHome || platform !== "linux") return codexHome;
	return parseWslUncPath(codexHome)?.linuxPath ?? codexHome;
}
function getAiVaultResumePlatform(state, worktreeId) {
	const targetWorktreeId = worktreeId ?? state.activeWorktreeId;
	const executionHost = parseExecutionHostId(getExecutionHostIdForWorktree(state, targetWorktreeId));
	if (executionHost?.kind === "ssh" || executionHost?.kind === "runtime") return "linux";
	const projectRuntime = getLocalProjectExecutionRuntimeContext(state, worktreeId, CLIENT_PLATFORM);
	if (projectRuntime?.status === "repair-required") return projectRuntime.repair.preferredRuntime.kind === "wsl" ? "linux" : CLIENT_PLATFORM;
	if (projectRuntime?.status === "resolved" && projectRuntime.runtime.kind === "wsl") return "linux";
	const workspacePath = getAiVaultResumeWorkspacePath(state, targetWorktreeId);
	return workspacePath && parseWslUncPath(workspacePath) ? "linux" : CLIENT_PLATFORM;
}
function launchAiVaultSessionInNewTab(args) {
	const store = useAppStore.getState();
	let targetGroupId = args.targetGroupId;
	const runtimeEnvironmentId = getRuntimeEnvironmentIdForWorktree(store, args.worktreeId);
	if (isWebRuntimeSessionActive(runtimeEnvironmentId)) {
		const observedRuntimeLaunch = createWebRuntimeSessionTerminal({
			worktreeId: args.worktreeId,
			environmentId: runtimeEnvironmentId,
			...targetGroupId ? { targetGroupId } : {},
			agentSessionKind: "resume",
			launchAgent: args.agent,
			command: args.command,
			...args.cwd ? { cwd: args.cwd } : {},
			...args.env ? { env: args.env } : {},
			...args.envToDelete ? { envToDelete: args.envToDelete } : {},
			...args.launchConfig ? { launchConfig: args.launchConfig } : {},
			...args.providerSession ? { providerSession: args.providerSession } : {},
			...args.launchConfig ? { agentArgs: args.launchConfig.agentArgs } : {},
			activate: true
		}).then((outcome) => {
			if (outcome.status === "created") useAppStore.getState().setActiveTabType("terminal");
			return outcome;
		});
		return {
			tabId: null,
			...targetGroupId ? { groupId: targetGroupId } : {},
			runtimeLaunch: observedRuntimeLaunch
		};
	}
	if (args.splitDirection && targetGroupId) targetGroupId = store.createEmptySplitGroup(args.worktreeId, targetGroupId, args.splitDirection) ?? targetGroupId;
	const tab = args.cwd ? store.createTab(args.worktreeId, targetGroupId, void 0, { startupCwd: args.cwd }) : store.createTab(args.worktreeId, targetGroupId);
	store.queueTabStartupCommand(tab.id, {
		command: args.command,
		...args.env ? { env: args.env } : {},
		...args.envToDelete ? { envToDelete: args.envToDelete } : {},
		...args.launchConfig ? {
			launchConfig: args.launchConfig,
			launchAgent: args.agent
		} : {},
		...args.providerSession ? { resumeProviderSession: args.providerSession } : {},
		telemetry: {
			agent_kind: tuiAgentToAgentKind(args.agent),
			launch_source: "sidebar",
			request_kind: "resume"
		}
	});
	store.setActiveTabType("terminal");
	const fresh = useAppStore.getState();
	const termIds = (fresh.tabsByWorktree[args.worktreeId] ?? []).map((t) => t.id);
	const editorIds = fresh.openFiles.filter((f) => f.worktreeId === args.worktreeId).map((f) => f.id);
	const browserIds = (fresh.browserTabsByWorktree?.[args.worktreeId] ?? []).map((t) => t.id);
	const order = reconcileTabOrder(fresh.tabBarOrderByWorktree[args.worktreeId], termIds, editorIds, browserIds).filter((id) => id !== tab.id);
	order.push(tab.id);
	fresh.setTabBarOrder(args.worktreeId, order);
	return {
		tabId: tab.id,
		groupId: targetGroupId
	};
}
function isLegacySharedCodexHome(codexHome) {
	if (!codexHome) return false;
	const segments = codexHome.split(/[\\/]/).filter(Boolean);
	return segments.at(-2) === "codex-runtime-home" && segments.at(-1) === "home";
}
function isPerAccountManagedCodexHome(codexHome) {
	if (!codexHome) return false;
	const segments = codexHome.split(/[\\/]/).filter(Boolean);
	return segments.at(-3) === "codex-accounts" && segments.at(-1) === "home";
}
async function prepareAiVaultSessionForResume(session) {
	if (!aiVaultSessionNeedsResumePreparation(session)) return session;
	const result = await window.api.aiVault.prepareSessionResume({
		agent: session.agent,
		filePath: session.filePath,
		codexHome: session.codexHome,
		executionHostId: session.executionHostId
	});
	if (result.useRealCodexHome) return {
		...session,
		codexHome: null
	};
	if (result.substituteCodexHome) return {
		...session,
		codexHome: result.substituteCodexHome
	};
	return session;
}
function aiVaultSessionNeedsResumePreparation(session) {
	if (session.agent !== "codex") return false;
	if (isLegacySharedCodexHome(session.codexHome)) return true;
	return isPerAccountManagedCodexHome(session.codexHome) && (!session.executionHostId || session.executionHostId === "local");
}
export { getAiVaultResumeWorkspaceTargetStatus as S, isAiVaultScanCancelledError as _, buildAiVaultResumeCopyCommandForWorktree as a, canResumeAiVaultSessionOnTarget as b, AI_VAULT_SESSION_DRAG_END_EVENT as c, hasAiVaultSessionDragData as d, readAiVaultSessionDragData as f, aiVaultSessionRecoverableSignalCount as g, aiVaultAgentLabel as h, buildAiVaultDropRepinStartup as i, AI_VAULT_SESSION_DRAG_START_EVENT as l, AI_VAULT_AGENTS as m, prepareAiVaultSessionForResume as n, buildAiVaultResumeStartupForWorktree as o, writeAiVaultSessionDragData as p, launchAiVaultSessionInNewTab as r, getAiVaultAgentProviderSession as s, aiVaultSessionNeedsResumePreparation as t, clearAiVaultSessionDragData as u, isAiVaultSessionRecoverableEmpty as v, getAiVaultResumeWorkspaceExecutionHostId as x, isAiVaultSessionResumableContent as y };
