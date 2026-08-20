import { Io as getRemoteRuntimePtyEnvironmentId, Ja as isTerminalInputTooLargeWithDeferredMeasurement, Kc as makePaneKey, Lo as getRemoteRuntimeTerminalHandle, lp as RuntimeRpcCallError, na as getFirstCommandToken, op as getActiveRuntimeTarget, rp as callRuntimeRpc, t as useAppStore } from "./store-CgXrfmaH.js";
import { dt as TUI_AGENT_CONFIG, ft as getTuiAgentDetectCommands } from "./agent-status-3vUKbY6l.js";
var REMOTE_PTY_ID_PREFIX = "remote:";
var DESKTOP_RUNTIME_CLIENT = {
	id: "orca-desktop",
	type: "desktop"
};
function isRuntimePtyInputTooLarge(data) {
	return isTerminalInputTooLargeWithDeferredMeasurement(data);
}
function isRemoteRuntimePtyId(ptyId) {
	return ptyId.startsWith(REMOTE_PTY_ID_PREFIX);
}
function isTerminalGoneError(error) {
	const message = error instanceof Error ? error.message : String(error);
	const code = error instanceof RuntimeRpcCallError ? error.code : error && typeof error === "object" && "code" in error ? String(error.code) : "";
	return code === "no_connected_pty" || code === "terminal_handle_stale" || code === "terminal_exited" || code === "terminal_gone" || message.includes("terminal_handle_stale") || message.includes("terminal_exited") || message.includes("terminal_gone") || message.includes("no_connected_pty");
}
function recordRuntimeTerminalInputForPtyId(ptyId, timestamp = Date.now()) {
	const state = useAppStore.getState();
	for (const [tabId, layout] of Object.entries(state.terminalLayoutsByTabId)) for (const [leafId, leafPtyId] of Object.entries(layout?.ptyIdsByLeafId ?? {})) {
		if (leafPtyId !== ptyId) continue;
		try {
			state.recordTerminalInput(makePaneKey(tabId, leafId), timestamp);
		} catch {}
		return;
	}
}
async function inspectRuntimeTerminalProcess(settings, ptyId) {
	const ownerEnvironmentId = getRemoteRuntimePtyEnvironmentId(ptyId);
	const target = ownerEnvironmentId ? {
		kind: "environment",
		environmentId: ownerEnvironmentId
	} : getActiveRuntimeTarget(settings);
	const terminal = getRemoteRuntimeTerminalHandle(ptyId);
	if (target.kind !== "environment" || !terminal) return window.api.pty.inspectProcess(ptyId);
	try {
		return (await callRuntimeRpc(target, "terminal.inspectProcess", { terminal }, { timeoutMs: 15e3 })).process;
	} catch (error) {
		if (isTerminalGoneError(error)) return {
			foregroundProcess: null,
			hasChildProcesses: false,
			unavailable: true
		};
		throw error;
	}
}
async function confirmRuntimeTerminalForegroundProcess(settings, ptyId) {
	const ownerEnvironmentId = getRemoteRuntimePtyEnvironmentId(ptyId);
	if ((ownerEnvironmentId ? {
		kind: "environment",
		environmentId: ownerEnvironmentId
	} : getActiveRuntimeTarget(settings)).kind === "environment" && getRemoteRuntimeTerminalHandle(ptyId)) return null;
	const confirmForegroundProcess = window.api.pty.confirmForegroundProcess;
	if (typeof confirmForegroundProcess !== "function") return null;
	return confirmForegroundProcess(ptyId).catch(() => null);
}
function sendRuntimePtyInput(settings, ptyId, data) {
	const tooLarge = isRuntimePtyInputTooLarge(data);
	if (tooLarge === true) return false;
	if (tooLarge !== false) {
		tooLarge.then((resolvedTooLarge) => {
			if (!resolvedTooLarge) sendRuntimePtyInputWithinLimit(settings, ptyId, data);
		}).catch(() => {});
		return true;
	}
	return sendRuntimePtyInputWithinLimit(settings, ptyId, data);
}
function sendRuntimePtyInputWithinLimit(settings, ptyId, data) {
	const ownerEnvironmentId = getRemoteRuntimePtyEnvironmentId(ptyId);
	const target = ownerEnvironmentId ? {
		kind: "environment",
		environmentId: ownerEnvironmentId
	} : getActiveRuntimeTarget(settings);
	const terminal = getRemoteRuntimeTerminalHandle(ptyId);
	if (target.kind !== "environment" || !terminal) {
		window.api.pty.write(ptyId, data);
		recordRuntimeTerminalInputForPtyId(ptyId);
		return true;
	}
	callRuntimeRpc(target, "terminal.send", {
		terminal,
		text: data,
		client: DESKTOP_RUNTIME_CLIENT
	}, { timeoutMs: 15e3 }).then((result) => {
		if (result.send.accepted === true) recordRuntimeTerminalInputForPtyId(ptyId);
	}).catch(() => {});
	return true;
}
async function sendRuntimePtyInputVerified(settings, ptyId, data) {
	const tooLarge = isRuntimePtyInputTooLarge(data);
	if (typeof tooLarge === "boolean" ? tooLarge : await tooLarge) return false;
	const ownerEnvironmentId = getRemoteRuntimePtyEnvironmentId(ptyId);
	const target = ownerEnvironmentId ? {
		kind: "environment",
		environmentId: ownerEnvironmentId
	} : getActiveRuntimeTarget(settings);
	const terminal = getRemoteRuntimeTerminalHandle(ptyId);
	if (target.kind !== "environment" || !terminal) {
		const accepted = await window.api.pty.writeAccepted(ptyId, data);
		if (!accepted) {
			window.api.pty.write(ptyId, data);
			recordRuntimeTerminalInputForPtyId(ptyId);
			return true;
		}
		recordRuntimeTerminalInputForPtyId(ptyId);
		return accepted;
	}
	try {
		if ((await callRuntimeRpc(target, "terminal.send", {
			terminal,
			text: data,
			client: DESKTOP_RUNTIME_CLIENT
		}, { timeoutMs: 15e3 })).send.accepted === true) {
			recordRuntimeTerminalInputForPtyId(ptyId);
			return true;
		}
		return false;
	} catch (error) {
		if (isTerminalGoneError(error)) return false;
		throw error;
	}
}
const EXACT_NODE_ENTRYPOINT_IDENTITIES = [
	{
		pattern: /(?:^|\/)cursor-agent\/versions\/[^/]+\/index\.js$/,
		agent: "cursor",
		processName: "cursor-agent"
	},
	{
		pattern: /(?:^|\/)node_modules\/@(?:earendil-works|mariozechner)\/pi-coding-agent\/dist\/cli\.js$/,
		agent: "pi",
		processName: "pi"
	},
	{
		pattern: /(?:^|\/)node_modules\/prime-agent\/dist\/bundle\/cli\.js$/,
		agent: "prime-agent",
		processName: "prime-agent"
	}
];
var PRINT_MODE_FLAGS = new Set(["--print", "-p"]);
var HEADLESS_OUTPUT_FORMATS = new Set(["json", "stream-json"]);
function optionName$1(token) {
	const eq = token.indexOf("=");
	return eq === -1 ? token : token.slice(0, eq);
}
function optionValue(tokens, index) {
	const token = tokens[index];
	const eq = token.indexOf("=");
	if (eq !== -1) return token.slice(eq + 1);
	return tokens[index + 1] ?? null;
}
function isPrintModeHeadlessOneShotCommand(tokens) {
	for (let index = 1; index < tokens.length; index += 1) {
		if (tokens[index] === "--") return false;
		const name = optionName$1(tokens[index]);
		if (PRINT_MODE_FLAGS.has(name)) return true;
		if (name === "--output-format") {
			const value = optionValue(tokens, index)?.toLowerCase();
			if (value && HEADLESS_OUTPUT_FORMATS.has(value)) return true;
		}
	}
	return false;
}
var ANTE_HEADLESS_PROMPT_FLAGS = new Set(["--prompt", "-p"]);
function isAnteHeadlessPromptFlag(token) {
	const name = optionName$1(token);
	return ANTE_HEADLESS_PROMPT_FLAGS.has(name) || /^-p[^-]/.test(name);
}
function isAnteHeadlessOneShotCommand(tokens) {
	for (let index = 1; index < tokens.length; index += 1) if (isAnteHeadlessPromptFlag(tokens[index])) return true;
	return false;
}
var NON_INTERACTIVE_MODES = new Set([
	"json",
	"rpc",
	"acp",
	"daemon"
]);
function isPrimeAgentHeadlessOneShotCommand(tokens) {
	if (isPrintModeHeadlessOneShotCommand(tokens)) return true;
	for (let index = 1; index < tokens.length; index += 1) {
		if (tokens[index] === "--") return false;
		if (tokens[index] === "--mode" && NON_INTERACTIVE_MODES.has(tokens[index + 1] ?? "")) return true;
	}
	return false;
}
var HEADLESS_ONE_SHOT_MATCHERS = {
	claude: isPrintModeHeadlessOneShotCommand,
	trae: isPrintModeHeadlessOneShotCommand,
	"prime-agent": isPrimeAgentHeadlessOneShotCommand,
	ante: isAnteHeadlessOneShotCommand
};
function isHeadlessOneShotAgentCommand(agent, tokens) {
	return HEADLESS_ONE_SHOT_MATCHERS[agent]?.(tokens) ?? false;
}
function filterHeadlessOneShotAgentCommand(recognition, tokens) {
	if (recognition && isHeadlessOneShotAgentCommand(recognition.agent, tokens)) return null;
	return recognition;
}
var PROCESS_EXTENSION_RE = /\.(?:exe|cmd|bat|ps1)$/i;
var INTERPRETER_SCRIPT_EXTENSION_RE = /\.(?:js|mjs|cjs)$/i;
var PYTHON_SCRIPT_EXTENSION_RE = /\.(?:py|pyw)$/i;
function normalizeProcessName(processName, options = {}) {
	if (!processName) return "";
	const unquoted = processName.trim().replace(/^["']|["']$/g, "");
	const withoutProcessExtension = (unquoted.split(/[\\/]/).pop() ?? unquoted).toLowerCase().replace(PROCESS_EXTENSION_RE, "");
	if (options.stripInterpreterScriptExtension === true) return withoutProcessExtension.replace(INTERPRETER_SCRIPT_EXTENSION_RE, "");
	return withoutProcessExtension;
}
var STATIC_INTERPRETER_PROCESS_NAMES = new Set([
	"node",
	"python",
	"python3",
	"bash",
	"zsh",
	"sh",
	"fish",
	"pwsh",
	"powershell"
]);
var FOREGROUND_AGENT_WRAPPER_PROCESS_NAMES = new Set([
	"node",
	"python",
	"python3"
]);
var PYTHON_PROCESS_RE = /^python(?:\d+(?:\.\d+)*)?$/;
var INTERPRETER_OPTIONS_WITH_VALUE = new Set([
	"-r",
	"--require",
	"--import",
	"--loader",
	"--experimental-loader"
]);
var INTERPRETER_OPTIONS_WITH_INLINE_SOURCE = new Set([
	"-e",
	"--eval",
	"-p",
	"--print",
	"--check"
]);
var NODE_PACKAGE_SCRIPT_ENTRYPOINTS = {
	codex: ["node_modules/@openai/codex/"],
	gemini: ["node_modules/@google/gemini-cli/"]
};
var PYTHON_SCRIPT_ENTRYPOINT_DIRECTORIES = [
	"/bin/",
	"/scripts/",
	"/site-packages/"
];
var PROCESS_TO_AGENT = /* @__PURE__ */ new Map();
var AGENT_TYPE_IDS = /* @__PURE__ */ new Set();
for (const [agent, config] of Object.entries(TUI_AGENT_CONFIG)) {
	AGENT_TYPE_IDS.add(agent);
	for (const candidate of [
		config.expectedProcess,
		...getTuiAgentDetectCommands(config),
		getFirstCommandToken(config.launchCmd)
	]) {
		const normalized = normalizeProcessName(candidate);
		if (normalized) {
			if (!PROCESS_TO_AGENT.has(normalized)) PROCESS_TO_AGENT.set(normalized, agent);
		}
	}
}
function agentForNormalizedProcess(normalized) {
	const exact = PROCESS_TO_AGENT.get(normalized);
	if (exact) return exact;
	if (normalized.startsWith("codex-")) return PROCESS_TO_AGENT.get("codex");
	if (normalized.startsWith("grok-")) return PROCESS_TO_AGENT.get("grok");
}
function recognizedAgentForProcess(normalized) {
	const agent = agentForNormalizedProcess(normalized);
	return agent ? {
		agent,
		processName: normalized
	} : null;
}
function tokenizeCommandLine(commandLine) {
	const tokens = [];
	let current = "";
	let quote = null;
	let escaped = false;
	for (let index = 0; index < commandLine.length; index += 1) {
		const char = commandLine[index];
		if (escaped) {
			current += char;
			escaped = false;
			continue;
		}
		if (char === "\\" && quote !== "'") {
			const next = commandLine[index + 1];
			if (next && (/\s/.test(next) || next === "\"" || next === "'" || next === "\\")) {
				escaped = true;
				continue;
			}
		}
		if ((char === "\"" || char === "'") && quote === null) {
			quote = char;
			continue;
		}
		if (quote === char) {
			quote = null;
			continue;
		}
		if (/\s/.test(char) && quote === null) {
			if (current) {
				tokens.push(current);
				current = "";
			}
			continue;
		}
		current += char;
	}
	if (current) tokens.push(current);
	return tokens;
}
function tokenLooksExecutable(token, index, firstNormalized) {
	if (index === 0) return true;
	if (!isInterpreterProcessName(firstNormalized)) return false;
	return token.includes("/") || token.includes("\\") || PROCESS_EXTENSION_RE.test(token);
}
function isInterpreterProcessName(normalized) {
	return STATIC_INTERPRETER_PROCESS_NAMES.has(normalized) || PYTHON_PROCESS_RE.test(normalized);
}
var isPythonProcessName = (normalized) => PYTHON_PROCESS_RE.test(normalized);
var optionName = (token) => token.split("=", 1)[0] ?? "";
function findInterpreterEntrypointToken(tokens, firstNormalized) {
	if (!isInterpreterProcessName(firstNormalized)) return null;
	for (let index = 1; index < tokens.length; index += 1) {
		const token = tokens[index];
		if (token === "--") continue;
		if (isPythonProcessName(firstNormalized) && token === "-m") return tokens[index + 1] ?? null;
		if (token.startsWith("-")) {
			const name = optionName(token);
			if (INTERPRETER_OPTIONS_WITH_INLINE_SOURCE.has(name)) return null;
			if (INTERPRETER_OPTIONS_WITH_VALUE.has(name) && name === token) index += 1;
			continue;
		}
		if (tokenLooksExecutable(token, index, firstNormalized)) return token;
	}
	return null;
}
function comparablePath(token) {
	return token.trim().replace(/^["']|["']$/g, "").replace(/\\/g, "/").toLowerCase();
}
function recognizeNodeScriptEntrypoint(token) {
	const path = comparablePath(token);
	for (const identity of EXACT_NODE_ENTRYPOINT_IDENTITIES) if (identity.pattern.test(path)) return {
		agent: identity.agent,
		processName: identity.processName
	};
	const normalized = normalizeProcessName(token, { stripInterpreterScriptExtension: true });
	const markers = NODE_PACKAGE_SCRIPT_ENTRYPOINTS[normalized];
	if (!markers) return null;
	if (!markers.some((marker) => path.includes(marker))) return null;
	return recognizedAgentForProcess(normalized);
}
function recognizePythonModule(moduleName) {
	if (!moduleName || moduleName.startsWith("-")) return null;
	return recognizedAgentForProcess(moduleName.split(".", 1)[0]?.toLowerCase() ?? "");
}
function recognizePythonScriptEntrypoint(token) {
	const path = comparablePath(token);
	if (!PYTHON_SCRIPT_EXTENSION_RE.test(path)) return null;
	if (!PYTHON_SCRIPT_ENTRYPOINT_DIRECTORIES.some((marker) => path.includes(marker))) return null;
	return recognizedAgentForProcess((path.split("/").pop() ?? "").replace(PYTHON_SCRIPT_EXTENSION_RE, ""));
}
function recognizePythonEntrypoint(tokens, entrypoint) {
	const moduleFlagIndex = tokens.indexOf("-m");
	if (moduleFlagIndex > 0) return recognizePythonModule(tokens[moduleFlagIndex + 1]);
	return recognizeAgentProcess(entrypoint) ?? recognizePythonScriptEntrypoint(entrypoint);
}
function isExpectedAgentProcess(processName, expectedProcess) {
	const normalizedProcess = normalizeProcessName(processName);
	const normalizedExpected = normalizeProcessName(expectedProcess);
	if (!normalizedProcess || !normalizedExpected) return false;
	return normalizedProcess === normalizedExpected || normalizedProcess.startsWith(`${normalizedExpected}.`);
}
function recognizeAgentProcess(processName) {
	return recognizedAgentForProcess(normalizeProcessName(processName));
}
function recognizeAgentProcessFromCommandLine(commandLine, options) {
	if (!commandLine) return null;
	const keep = options?.includeHeadlessOneShot === true;
	const tokens = tokenizeCommandLine(commandLine);
	const firstNormalized = normalizeProcessName(tokens[0]);
	let direct = recognizeAgentProcess(tokens[0]);
	if (direct?.agent === "claude-agent-teams" && tokens[1]?.toLowerCase() !== "claude-teams") direct = null;
	const directRecognition = keep ? direct : filterHeadlessOneShotAgentCommand(direct, tokens);
	if (directRecognition) return directRecognition;
	const entrypoint = findInterpreterEntrypointToken(tokens, firstNormalized);
	if (!entrypoint) return null;
	const viaEntrypoint = isPythonProcessName(firstNormalized) ? recognizePythonEntrypoint(tokens, entrypoint) : recognizeAgentProcess(entrypoint) ?? recognizeNodeScriptEntrypoint(entrypoint);
	if (viaEntrypoint?.agent === "claude-agent-teams" && tokens[tokens.indexOf(entrypoint, 1) + 1]?.toLowerCase() !== "claude-teams") return null;
	return keep ? viaEntrypoint : filterHeadlessOneShotAgentCommand(viaEntrypoint, tokens);
}
function isAgentForegroundWrapperProcess(processName) {
	const normalized = normalizeProcessName(processName);
	return FOREGROUND_AGENT_WRAPPER_PROCESS_NAMES.has(normalized) || PYTHON_PROCESS_RE.test(normalized);
}
function isRecognizedAgentType(agentType) {
	if (typeof agentType !== "string") return false;
	return AGENT_TYPE_IDS.has(agentType) || agentForNormalizedProcess(normalizeProcessName(agentType)) !== void 0;
}
export { recognizeAgentProcessFromCommandLine as a, isRemoteRuntimePtyId as c, recognizeAgentProcess as i, sendRuntimePtyInput as l, isExpectedAgentProcess as n, confirmRuntimeTerminalForegroundProcess as o, isRecognizedAgentType as r, inspectRuntimeTerminalProcess as s, isAgentForegroundWrapperProcess as t, sendRuntimePtyInputVerified as u };
